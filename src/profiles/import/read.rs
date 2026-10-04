//! Getting raw settings out of a file — whatever shape it comes in.
//!
//! Nothing here interprets a setting. Each format is reduced to the same thing,
//! a list of [`RawPreset`]s holding every `key → value` exactly as written, so
//! the mapping that follows has one input to handle however the file arrived:
//!
//! | File | Where the settings are |
//! | --- | --- |
//! | `.ini` | `key = value` lines, optionally in `[print:…]` / `[filament:…]` / `[printer:…]` sections |
//! | `.json` | one preset object, with `type` saying which kind |
//! | `.3mf` / `.amf` | `Metadata/Slic3r_PE.config` (INI) or `Metadata/project_settings.config` (JSON) |
//! | preset bundles (`.orca_printer`, `.bbscfg`, …) | a zip of JSON presets |
//! | `.gcode` | the `; key = value` configuration block at the end |

use std::io::{Cursor, Read};

use serde_json::{Map, Value};

use super::report::{ImportNote, ImportSource, SourceContainer, SourceSlicer};

/// The kind of profile a foreign preset is, as its own file says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Category {
    Printer,
    Filament,
    Process,
}

impl Category {
    pub(crate) fn noun(self) -> &'static str {
        match self {
            Self::Printer => "printer",
            Self::Filament => "filament",
            Self::Process => "print profile",
        }
    }
}

/// How a preset's values are encoded, which decides how lists and escapes are
/// read back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Encoding {
    /// INI text: C-style escapes, `;`-separated quoted string lists,
    /// `,`-separated number lists.
    Ini,
    /// JSON: real strings, and arrays for anything per extruder.
    Json,
}

/// One preset as the file states it, nothing interpreted yet.
#[derive(Debug, Clone)]
pub(crate) struct RawPreset {
    /// `None` for a file that mixes printer, filament and print settings in one
    /// flat list — an exported configuration, a project, a G-code file.
    pub category: Option<Category>,
    /// The preset's own name, when the file gives one.
    pub name: Option<String>,
    /// Parents it inherits from, nearest last applied.
    pub inherits: Vec<String>,
    /// A bundle's abstract template (`*common*`): there to be inherited from,
    /// never a profile of its own.
    pub is_template: bool,
    pub encoding: Encoding,
    /// Every value, in file order.
    pub values: Map<String, Value>,
}

/// Everything read out of one file.
#[derive(Debug)]
pub(crate) struct RawImport {
    pub source: ImportSource,
    pub presets: Vec<RawPreset>,
    pub notes: Vec<ImportNote>,
}

/// Why a file yielded no settings at all.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ImportError {
    /// The bytes aren't any format the importer reads.
    #[error("{0}")]
    Unreadable(String),
    /// The file was read but carries no slicer settings.
    #[error("{0}")]
    NoSettings(String),
}

/// Read every preset out of `bytes`, choosing the format from the file name and,
/// failing that, the content.
pub(crate) fn read_file(file_name: &str, bytes: &[u8]) -> Result<RawImport, ImportError> {
    let lower = file_name.to_ascii_lowercase();
    let is_zip = bytes.starts_with(b"PK\x03\x04");
    if is_zip {
        return read_zip(file_name, bytes);
    }
    if lower.ends_with(".bgcode") {
        return Err(ImportError::Unreadable(
            "Binary G-code (.bgcode) can't be read. Export plain G-code, the project \
             (.3mf) or the configuration (.ini) instead."
                .to_string(),
        ));
    }

    let text = String::from_utf8_lossy(bytes);
    let text = text.trim_start_matches('\u{feff}');
    let trimmed = text.trim_start();

    if lower.ends_with(".json") || trimmed.starts_with('{') {
        return read_json_preset(file_name, text);
    }
    if is_gcode_name(&lower) || looks_like_gcode(text) {
        return read_gcode(file_name, text);
    }
    read_ini(file_name, text)
}

fn is_gcode_name(lower: &str) -> bool {
    [".gcode", ".gco", ".g", ".gc"]
        .iter()
        .any(|ext| lower.ends_with(ext))
}

/// A G-code file starts with comments and moves, never with `key = value`.
fn looks_like_gcode(text: &str) -> bool {
    text.lines()
        .take(200)
        .any(|line| line.starts_with("G1 ") || line.starts_with("G28") || line.starts_with("M104"))
}

// ── INI ──────────────────────────────────────────────────────────────────────

/// One `[kind:name]` section, or the unnamed top of the file.
struct IniSection {
    kind: Option<String>,
    name: Option<String>,
    values: Map<String, Value>,
}

/// Split INI text into sections. The first comment line is returned separately
/// because that is where the writing slicer names itself.
fn parse_ini(text: &str) -> (Option<String>, Vec<IniSection>) {
    let mut header = None;
    let mut sections = vec![IniSection {
        kind: None,
        name: None,
        values: Map::new(),
    }];
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with('#') || trimmed.starts_with(';') {
            if header.is_none() {
                header = Some(trimmed.trim_start_matches(['#', ';']).trim().to_string());
            }
            continue;
        }
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            let inner = &trimmed[1..trimmed.len() - 1];
            let (kind, name) = match inner.split_once(':') {
                Some((kind, name)) => (kind.trim().to_string(), Some(name.trim().to_string())),
                None => (inner.trim().to_string(), None),
            };
            sections.push(IniSection {
                kind: Some(kind),
                name,
                values: Map::new(),
            });
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim();
            if key.is_empty() || key.contains(char::is_whitespace) {
                continue;
            }
            if let Some(section) = sections.last_mut() {
                section
                    .values
                    .insert(key.to_string(), Value::String(value.trim().to_string()));
            }
        }
    }
    (header, sections)
}

fn read_ini(file_name: &str, text: &str) -> Result<RawImport, ImportError> {
    let (header, sections) = parse_ini(text);
    let (slicer, version) = header
        .as_deref()
        .map(detect_slicer)
        .unwrap_or((SourceSlicer::Unknown, None));
    let bundled = sections.iter().any(|s| s.kind.is_some());

    let mut notes = Vec::new();
    let mut presets = Vec::new();
    if bundled {
        for section in sections {
            let Some(kind) = section.kind.as_deref() else {
                continue;
            };
            let category = match kind {
                "print" => Category::Process,
                "filament" => Category::Filament,
                "printer" => Category::Printer,
                "physical_printer" => {
                    notes.push(ImportNote::info(format!(
                        "The network connection \"{}\" isn't imported. Set it up under the \
                         printer's connection settings.",
                        section.name.as_deref().unwrap_or("unnamed")
                    )));
                    continue;
                }
                _ => continue,
            };
            let name = section.name.unwrap_or_default();
            let inherits = section
                .values
                .get("inherits")
                .and_then(Value::as_str)
                .map(split_inherits)
                .unwrap_or_default();
            presets.push(RawPreset {
                category: Some(category),
                is_template: name.starts_with('*') && name.ends_with('*'),
                name: Some(name),
                inherits,
                encoding: Encoding::Ini,
                values: section.values,
            });
        }
    } else if let Some(top) = sections.into_iter().next() {
        if !top.values.is_empty() {
            presets.push(RawPreset {
                category: None,
                name: None,
                inherits: Vec::new(),
                is_template: false,
                encoding: Encoding::Ini,
                values: top.values,
            });
        }
    }

    if presets.iter().all(|p| p.values.is_empty()) {
        return Err(ImportError::NoSettings(format!(
            "{file_name} has no slicer settings in it."
        )));
    }
    Ok(RawImport {
        source: ImportSource {
            file_name: file_name.to_string(),
            slicer,
            version,
            container: if bundled {
                SourceContainer::IniBundle
            } else {
                SourceContainer::Ini
            },
        },
        presets,
        notes,
    })
}

/// `inherits = A; B` — parents applied left to right.
fn split_inherits(value: &str) -> Vec<String> {
    value
        .split(';')
        .map(|name| name.trim().trim_matches('"').to_string())
        .filter(|name| !name.is_empty())
        .collect()
}

// ── G-code ───────────────────────────────────────────────────────────────────

/// Lines that open and close the configuration block a slicer appends.
const BLOCK_STARTS: &[&str] = &["_config = begin", "CONFIG_BLOCK_START"];
const BLOCK_ENDS: &[&str] = &["_config = end", "CONFIG_BLOCK_END"];

fn read_gcode(file_name: &str, text: &str) -> Result<RawImport, ImportError> {
    let header = text
        .lines()
        .take(80)
        .find(|line| {
            let lower = line.to_ascii_lowercase();
            lower.contains("generated by")
                || lower.contains("generated with")
                || lower.starts_with("; bambustudio")
        })
        .map(|line| line.trim_start_matches(';').trim().to_string());
    let (slicer, version) = header
        .as_deref()
        .map(detect_slicer)
        .unwrap_or((SourceSlicer::Unknown, None));

    // The block sits at the end of a file that can run to hundreds of
    // megabytes, so find its last opening line rather than walking every move.
    let block = BLOCK_STARTS
        .iter()
        .filter_map(|marker| text.rfind(marker))
        .max()
        .map(|start| {
            let after = &text[start..];
            let body_start = after.find('\n').map(|i| i + 1).unwrap_or(after.len());
            let body = &after[body_start..];
            let end = BLOCK_ENDS
                .iter()
                .filter_map(|marker| body.find(marker))
                .min()
                .unwrap_or(body.len());
            &body[..end]
        });
    // Older files have no markers: the `; key = value` lines simply trail the
    // moves.
    let body = block.unwrap_or_else(|| {
        let tail_start = text.len().saturating_sub(256 * 1024);
        let tail_start = (tail_start..text.len())
            .find(|i| text.is_char_boundary(*i))
            .unwrap_or(text.len());
        &text[tail_start..]
    });

    let mut values = Map::new();
    for line in body.lines() {
        let Some(rest) = line.trim_end_matches('\r').strip_prefix(';') else {
            continue;
        };
        let Some((key, value)) = rest.split_once(" = ") else {
            continue;
        };
        let key = key.trim();
        if key.is_empty()
            || !key
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        {
            continue;
        }
        values.insert(key.to_string(), Value::String(value.trim().to_string()));
    }

    if values.is_empty() {
        return Err(ImportError::NoSettings(format!(
            "{file_name} has no slicer settings in it — the slicer that wrote it \
             didn't append its configuration."
        )));
    }
    let mut notes = Vec::new();
    if slicer == SourceSlicer::ColdCrabby {
        notes.push(ImportNote::info(
            "Cold Crabby G-code carries only a summary of its settings. To move \
             profiles between installs, export them from Settings → General instead.",
        ));
    }
    Ok(RawImport {
        source: ImportSource {
            file_name: file_name.to_string(),
            slicer,
            version,
            container: SourceContainer::Gcode,
        },
        presets: vec![RawPreset {
            category: None,
            name: None,
            inherits: Vec::new(),
            is_template: false,
            encoding: Encoding::Ini,
            values,
        }],
        notes,
    })
}

// ── JSON ─────────────────────────────────────────────────────────────────────

fn read_json_preset(file_name: &str, text: &str) -> Result<RawImport, ImportError> {
    let value: Value = serde_json::from_str(text)
        .map_err(|e| ImportError::Unreadable(format!("{file_name} isn't valid JSON ({e}).")))?;
    let Some(preset) = json_preset(value) else {
        return Err(ImportError::NoSettings(format!(
            "{file_name} isn't a slicer preset."
        )));
    };
    Ok(RawImport {
        source: ImportSource {
            file_name: file_name.to_string(),
            slicer: SourceSlicer::OrcaSlicer,
            version: preset.1,
            container: SourceContainer::Json,
        },
        presets: vec![preset.0],
        notes: Vec::new(),
    })
}

/// A JSON preset object, with the version it was written by.
fn json_preset(value: Value) -> Option<(RawPreset, Option<String>)> {
    let Value::Object(values) = value else {
        return None;
    };
    let category = match values.get("type").and_then(Value::as_str) {
        Some("machine") | Some("printer") => Some(Category::Printer),
        Some("filament") => Some(Category::Filament),
        Some("process") | Some("print") => Some(Category::Process),
        _ => None,
    };
    // An object with neither a kind nor any known setting is some other JSON.
    if category.is_none() && !values.contains_key("layer_height") {
        return None;
    }
    let name = values
        .get("name")
        .and_then(Value::as_str)
        .map(str::to_string);
    let inherits = values
        .get("inherits")
        .and_then(Value::as_str)
        .filter(|parent| !parent.is_empty())
        .map(|parent| vec![parent.to_string()])
        .unwrap_or_default();
    let version = values
        .get("version")
        .and_then(Value::as_str)
        .map(str::to_string);
    Some((
        RawPreset {
            category,
            name,
            inherits,
            is_template: values
                .get("instantiation")
                .and_then(Value::as_str)
                .is_some_and(|v| v == "false"),
            encoding: Encoding::Json,
            values,
        },
        version,
    ))
}

// ── Zip containers ───────────────────────────────────────────────────────────

fn read_zip(file_name: &str, bytes: &[u8]) -> Result<RawImport, ImportError> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| ImportError::Unreadable(format!("{file_name} is a damaged archive ({e}).")))?;
    let names: Vec<String> = archive.file_names().map(str::to_string).collect();
    let read_entry = |archive: &mut zip::ZipArchive<Cursor<&[u8]>>, name: &str| {
        let mut entry = archive.by_name(name).ok()?;
        let mut text = String::new();
        entry.read_to_string(&mut text).ok()?;
        Some(text)
    };

    // A project saved by an INI-family slicer.
    if let Some(name) = names.iter().find(|n| {
        n.eq_ignore_ascii_case("Metadata/Slic3r_PE.config")
            || n.eq_ignore_ascii_case("Metadata/SuperSlicer.config")
    }) {
        let text = read_entry(&mut archive, name).unwrap_or_default();
        let mut import = read_ini(file_name, &text)?;
        import.source.container = SourceContainer::Project;
        return Ok(import);
    }

    // A project saved by a JSON-family slicer: one flat object holding every
    // printer, print and filament setting, filaments as per-slot arrays.
    if let Some(name) = names
        .iter()
        .find(|n| n.eq_ignore_ascii_case("Metadata/project_settings.config"))
    {
        let text = read_entry(&mut archive, name).unwrap_or_default();
        let values: Value = serde_json::from_str(&text).map_err(|e| {
            ImportError::Unreadable(format!(
                "{file_name} has a damaged settings file inside ({e})."
            ))
        })?;
        let Value::Object(values) = values else {
            return Err(ImportError::NoSettings(format!(
                "{file_name} has no slicer settings in it."
            )));
        };
        // The model file holds the mesh too and can run to hundreds of
        // megabytes; the `Application` metadata sits in its first few lines.
        let model = names
            .iter()
            .find(|n| n.eq_ignore_ascii_case("3D/3dmodel.model"))
            .and_then(|n| {
                let entry = archive.by_name(n).ok()?;
                let mut head = Vec::new();
                entry.take(64 * 1024).read_to_end(&mut head).ok()?;
                Some(String::from_utf8_lossy(&head).into_owned())
            })
            .unwrap_or_default();
        let (slicer, version) = application_metadata(&model)
            .map(|app| detect_slicer(&app))
            .unwrap_or((SourceSlicer::OrcaSlicer, None));
        return Ok(RawImport {
            source: ImportSource {
                file_name: file_name.to_string(),
                slicer,
                version,
                container: SourceContainer::Project,
            },
            presets: vec![RawPreset {
                category: None,
                name: None,
                inherits: Vec::new(),
                is_template: false,
                encoding: Encoding::Json,
                values,
            }],
            notes: Vec::new(),
        });
    }

    // A bundle of presets: every JSON (or INI) file inside is one.
    let mut presets = Vec::new();
    let mut version = None;
    for name in names.iter().filter(|n| !n.ends_with('/')) {
        let lower = name.to_ascii_lowercase();
        if lower.ends_with("bundle_structure.json") {
            continue;
        }
        let Some(text) = read_entry(&mut archive, name) else {
            continue;
        };
        if lower.ends_with(".json") {
            if let Some((preset, v)) = serde_json::from_str(&text).ok().and_then(json_preset) {
                version = version.or(v);
                presets.push(preset);
            }
        } else if lower.ends_with(".ini") {
            if let Ok(import) = read_ini(name, &text) {
                presets.extend(import.presets);
            }
        }
    }
    if presets.is_empty() {
        return Err(ImportError::NoSettings(format!(
            "{file_name} has no slicer settings in it."
        )));
    }
    Ok(RawImport {
        source: ImportSource {
            file_name: file_name.to_string(),
            slicer: SourceSlicer::OrcaSlicer,
            version,
            container: SourceContainer::JsonBundle,
        },
        presets,
        notes: Vec::new(),
    })
}

/// The `Application` metadata a 3MF model file names its writer with.
fn application_metadata(model_xml: &str) -> Option<String> {
    let start = model_xml.find("name=\"Application\"")?;
    let after = &model_xml[start..];
    let open = after.find('>')? + 1;
    let close = after[open..].find('<')? + open;
    Some(after[open..close].trim().to_string())
}

/// Which slicer, and which version of it, a header line names.
///
/// `generated by PrusaSlicer 2.8.1+win64 on 2024-05-01` → PrusaSlicer, `2.8.1`.
pub(crate) fn detect_slicer(header: &str) -> (SourceSlicer, Option<String>) {
    const KNOWN: &[(&str, SourceSlicer)] = &[
        ("PrusaSlicer", SourceSlicer::PrusaSlicer),
        ("SuperSlicer", SourceSlicer::SuperSlicer),
        ("OrcaSlicer", SourceSlicer::OrcaSlicer),
        ("BambuStudio", SourceSlicer::BambuStudio),
        ("Bambu Studio", SourceSlicer::BambuStudio),
        ("Cold Crabby", SourceSlicer::ColdCrabby),
        ("Slic3r", SourceSlicer::Slic3r),
    ];
    for (needle, slicer) in KNOWN {
        let Some(at) = header.find(needle) else {
            continue;
        };
        let rest = header[at + needle.len()..].trim_start_matches(['-', ' ', 'v']);
        let version = rest
            .split(|c: char| c.is_whitespace() || c == '+')
            .next()
            .filter(|v| v.starts_with(|c: char| c.is_ascii_digit()))
            .map(str::to_string);
        return (*slicer, version);
    }
    (SourceSlicer::Unknown, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_flat_configuration_is_one_preset_with_its_slicer_named() {
        let ini = "# generated by PrusaSlicer 2.8.1+win64 on 2024-05-01 at 10:00:00 UTC\n\
                   layer_height = 0.2\nstart_gcode = M115 ; tell\\nG28\n";
        let import = read_file("config.ini", ini.as_bytes()).unwrap();
        assert_eq!(import.source.slicer, SourceSlicer::PrusaSlicer);
        assert_eq!(import.source.version.as_deref(), Some("2.8.1"));
        assert_eq!(import.source.container, SourceContainer::Ini);
        assert_eq!(import.presets.len(), 1);
        let preset = &import.presets[0];
        assert_eq!(preset.category, None);
        // Values stay as written; escapes are decoded where they are read.
        assert_eq!(preset.values["start_gcode"], "M115 ; tell\\nG28");
    }

    #[test]
    fn a_bundle_keeps_each_section_and_its_parents() {
        let ini = "[print:*common*]\nperimeters = 2\n\n\
                   [print:Fine]\ninherits = *common*\nlayer_height = 0.1\n\n\
                   [physical_printer:Garage]\nprint_host = 10.0.0.2\n\n\
                   [presets]\nprint = Fine\n";
        let import = read_file("bundle.ini", ini.as_bytes()).unwrap();
        assert_eq!(import.source.container, SourceContainer::IniBundle);
        assert_eq!(import.presets.len(), 2);
        assert!(import.presets[0].is_template);
        assert_eq!(import.presets[1].inherits, vec!["*common*"]);
        assert_eq!(import.presets[1].category, Some(Category::Process));
        assert!(import.notes[0].text.contains("Garage"));
    }

    #[test]
    fn a_gcode_file_yields_its_trailing_configuration_block() {
        let gcode = "; generated by OrcaSlicer 2.1.1 on 2024-06-01 at 10:00:00\n\
                     G28\nG1 X10 Y10 E1\n\
                     ; CONFIG_BLOCK_START\n; layer_height = 0.2\n\
                     ; filament used [mm] = 1.2\n; wall_loops = 3\n; CONFIG_BLOCK_END\n";
        let import = read_file("part.gcode", gcode.as_bytes()).unwrap();
        assert_eq!(import.source.slicer, SourceSlicer::OrcaSlicer);
        assert_eq!(import.source.version.as_deref(), Some("2.1.1"));
        let values = &import.presets[0].values;
        assert_eq!(values["wall_loops"], "3");
        assert!(!values.contains_key("filament used [mm]"));
    }

    #[test]
    fn a_json_preset_says_which_kind_it_is() {
        let json = r#"{"type": "filament", "name": "My PLA", "inherits": "Generic PLA",
                       "nozzle_temperature": ["215"], "version": "2.1.0.0"}"#;
        let import = read_file("pla.json", json.as_bytes()).unwrap();
        let preset = &import.presets[0];
        assert_eq!(preset.category, Some(Category::Filament));
        assert_eq!(preset.inherits, vec!["Generic PLA"]);
        assert_eq!(preset.encoding, Encoding::Json);
    }

    #[test]
    fn a_project_reads_the_settings_saved_inside_it() {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("3D/3dmodel.model", options).unwrap();
        std::io::Write::write_all(
            &mut zip,
            br#"<model><metadata name="Application">BambuStudio-01.09.00.70</metadata></model>"#,
        )
        .unwrap();
        zip.start_file("Metadata/project_settings.config", options)
            .unwrap();
        std::io::Write::write_all(&mut zip, br#"{"layer_height": "0.2"}"#).unwrap();
        let bytes = zip.finish().unwrap().into_inner();

        let import = read_file("plate.3mf", &bytes).unwrap();
        assert_eq!(import.source.slicer, SourceSlicer::BambuStudio);
        assert_eq!(import.source.version.as_deref(), Some("01.09.00.70"));
        assert_eq!(import.source.container, SourceContainer::Project);
    }

    #[test]
    fn files_without_settings_say_so() {
        assert!(matches!(
            read_file("empty.ini", b"# nothing here\n"),
            Err(ImportError::NoSettings(_))
        ));
        assert!(matches!(
            read_file("part.bgcode", b"GCDE"),
            Err(ImportError::Unreadable(_))
        ));
        assert!(matches!(
            read_file("bad.json", b"{ not json"),
            Err(ImportError::Unreadable(_))
        ));
    }
}
