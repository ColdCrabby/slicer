//! Exporting the profile library as TOML the CLI already understands.
//!
//! # Why this module exists
//!
//! A user's printers, filaments, process profiles and labels are theirs — they
//! must be able to take them somewhere else: another machine, a backup, a
//! git repo, or a headless `slicer-engine` invocation. The engine already
//! *speaks* TOML for exactly this data ([`profiles.toml`][store]), so an export
//! is not a new format — it is the same document, optionally split up.
//!
//! # The one rule: never name a field
//!
//! Everything here works on the **serialized value** of a
//! [`ProfileLibrary`], never on a hand-written list of categories or settings.
//! A new profile setting, or a whole new category added to `ProfileLibrary`,
//! flows into the export with no change to this module. That is the entire
//! design constraint: the export is written once and never touched again as
//! features land. (The one exception is deliberate: [`SECRET_FIELDS`] names the
//! credentials that must be stripped — and that too is matched by name at the
//! value level, so a future credential is covered.)
//!
//! # What the export is faithful to
//!
//! The export mirrors the library **as this engine understands it** — exactly
//! what [`ProfileStore::load`][load] produced and what the next save would write
//! back. It is not a verbatim copy of the bytes on disk: a *typed* field written
//! by a different build is dropped by serde during load, before the exporter
//! sees it, just as it is for `GET /api/profiles` and for the next save. Values
//! in a profile's free-form `params` bag — where slicing settings live, and
//! therefore where new settings actually land — are preserved whatever they are.
//!
//! [load]: super::store::ProfileStore::load
//!
//! # The two shapes
//!
//! | Format | Artifact | Shape |
//! | --- | --- | --- |
//! | [`Bundle`] | `slicer-profiles.zip` | `printers/<slug>.toml`, `filaments/…`, `processes/…`, `labels.toml`, plus `manifest.toml` and `README.md` |
//! | [`Single`] | `profiles.toml` | Byte-identical to what [`ProfileStore::save`][save] writes |
//!
//! Every file in a bundle is an **array of tables** (`[[printers]]`), so
//!
//! ```text
//! cat printers/*.toml filaments/*.toml processes/*.toml labels.toml > profiles.toml
//! ```
//!
//! reconstructs a valid library file. That property is what makes the split
//! bundle CLI-compatible without a second serializer — and what a future
//! importer can rely on.
//!
//! [`Bundle`]: ProfileExportFormat::Bundle
//! [`Single`]: ProfileExportFormat::Single
//! [store]: super::store
//! [save]: super::store::ProfileStore::save

use std::io::{Cursor, Write};

use anyhow::{Context, Result};
use serde_json::Value;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use super::library::ProfileLibrary;
use super::toml_bridge::{json_to_toml, library_to_value, render_value_toml};

/// Which artifact an export produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileExportFormat {
    /// A ZIP archive with one TOML file per profile.
    Bundle,
    /// A single `profiles.toml` — the file the engine and CLI read directly.
    Single,
}

impl ProfileExportFormat {
    /// Parse the wire token used by the transports (`bundle` / `toml`).
    ///
    /// Unknown tokens are rejected rather than silently defaulting, so a typo
    /// in a URL never hands the user the wrong artifact.
    pub fn parse(token: &str) -> Option<Self> {
        match token.to_ascii_lowercase().as_str() {
            "bundle" | "zip" => Some(Self::Bundle),
            "single" | "toml" | "cli" => Some(Self::Single),
            _ => None,
        }
    }
}

/// A rendered export: the bytes plus how to present them to the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileExportArtifact {
    /// Suggested download / save-as filename.
    pub filename: String,
    /// MIME type for the HTTP response and the browser download.
    pub mime: &'static str,
    /// The file contents.
    pub bytes: Vec<u8>,
}

/// Render a library in the requested shape.
///
/// Credentials are stripped from both shapes — see [`redact_secrets`].
pub fn export_library(
    library: &ProfileLibrary,
    format: ProfileExportFormat,
) -> Result<ProfileExportArtifact> {
    let mut value = library_to_value(library)?;
    redact_secrets(&mut value);

    match format {
        ProfileExportFormat::Single => Ok(ProfileExportArtifact {
            filename: "profiles.toml".to_string(),
            mime: "application/toml",
            bytes: render_value_toml(value)?.into_bytes(),
        }),
        ProfileExportFormat::Bundle => Ok(ProfileExportArtifact {
            filename: "slicer-profiles.zip".to_string(),
            mime: "application/zip",
            bytes: render_bundle(value)?,
        }),
    }
}

/// Field names whose values are credentials and must never leave the machine.
///
/// Matched by **name, anywhere in the tree**, so a credential added to any
/// profile in future is redacted without touching this module — the same
/// value-level approach the rest of the exporter uses.
const SECRET_FIELDS: &[&str] = &["api_key", "password", "token", "secret", "access_token"];

/// Remove every credential from a serialized library, in place.
///
/// An export is built to be *handed over* — dropped in a git repo, AirDropped,
/// mailed. A printer's `api_key` grants control of that printer, so it is
/// stripped rather than shared; the README says so, and the user re-enters it
/// on the machine that restores the library.
fn redact_secrets(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.retain(|key, _| !SECRET_FIELDS.contains(&key.as_str()));
            for entry in map.values_mut() {
                redact_secrets(entry);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(redact_secrets),
        _ => {}
    }
}

/// One file in a bundle, before it is compressed.
struct BundleEntry {
    path: String,
    contents: String,
}

/// Whether a category is split into one file per item.
///
/// Profiles are — a printer is a thing you hand to someone. Labels are a flat
/// vocabulary where a file per tag would be absurd, so they stay in one file.
/// Anything added later is a profile category until proven otherwise, so the
/// default is to split.
fn splits_per_item(category: &str) -> bool {
    category != "labels"
}

/// Build the ZIP payload from the serialized library.
fn render_bundle(library: Value) -> Result<Vec<u8>> {
    let mut entries = collect_entries(library)?;
    entries.sort_by(|a, b| a.path.cmp(&b.path));

    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    // Deflate with a fixed timestamp: exporting the same library twice yields
    // byte-identical archives, so users can diff or version-control them.
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .last_modified_time(zip::DateTime::default());

    for entry in entries {
        writer
            .start_file(&entry.path, options)
            .with_context(|| format!("add '{}' to the export archive", entry.path))?;
        writer
            .write_all(entry.contents.as_bytes())
            .with_context(|| format!("write '{}' into the export archive", entry.path))?;
    }

    Ok(writer
        .finish()
        .context("finish the export archive")?
        .into_inner())
}

/// Walk the serialized library and turn every category into files.
///
/// This is the generic core: the only thing it knows about a category is its
/// key and that it is (or is not) an array.
fn collect_entries(library: Value) -> Result<Vec<BundleEntry>> {
    let Value::Object(root) = library else {
        anyhow::bail!("profile library did not serialize to a table");
    };

    let mut entries = Vec::new();
    let mut counts: Vec<(String, usize)> = Vec::new();
    // Non-array fields (a future scalar or table on the library) still have to
    // survive a round-trip, so they are collected into one `library.toml`.
    let mut leftovers = serde_json::Map::new();

    for (category, value) in &root {
        let Value::Array(items) = value else {
            leftovers.insert(category.clone(), value.clone());
            continue;
        };
        counts.push((category.clone(), items.len()));
        if items.is_empty() {
            continue;
        }

        if splits_per_item(category) {
            let width = index_width(items.len());
            for (index, item) in items.iter().enumerate() {
                let stem = file_stem(item, index, width);
                entries.push(BundleEntry {
                    path: format!("{category}/{stem}.toml"),
                    contents: render_category_file(category, std::slice::from_ref(item))?,
                });
            }
        } else {
            entries.push(BundleEntry {
                path: format!("{category}.toml"),
                contents: render_category_file(category, items)?,
            });
        }
    }

    if !leftovers.is_empty() {
        entries.push(BundleEntry {
            path: "library.toml".to_string(),
            contents: render_table(&Value::Object(leftovers))?,
        });
    }

    entries.push(BundleEntry {
        path: "manifest.toml".to_string(),
        contents: render_manifest(&counts)?,
    });
    entries.push(BundleEntry {
        path: "README.md".to_string(),
        contents: readme(),
    });

    Ok(entries)
}

/// Render items as `[[category]]` array-of-tables, with a provenance header.
fn render_category_file(category: &str, items: &[Value]) -> Result<String> {
    let mut table = serde_json::Map::new();
    table.insert(category.to_string(), Value::Array(items.to_vec()));
    let body = render_table(&Value::Object(table))?;
    Ok(format!(
        "# slicer-engine {version} — {category}\n\
         # Append this file to profiles.toml, or concatenate the whole bundle.\n\n\
         {body}",
        version = crate::version::VERSION,
    ))
}

/// Render any JSON value that is a table as a TOML document.
fn render_table(value: &Value) -> Result<String> {
    let toml_value =
        json_to_toml(value.clone()).context("export section serialized to a null document")?;
    toml::to_string_pretty(&toml_value).context("render export section to TOML")
}

/// The archive's self-description: what produced it and what is inside.
///
/// Counts are derived from the walk, so a new category is described here
/// automatically.
fn render_manifest(counts: &[(String, usize)]) -> Result<String> {
    let mut export = serde_json::Map::new();
    export.insert("generator".into(), Value::from("slicer-engine"));
    export.insert("version".into(), Value::from(crate::version::VERSION));
    export.insert("format".into(), Value::from("bundle"));

    let mut counted = serde_json::Map::new();
    for (category, count) in counts {
        counted.insert(category.clone(), Value::from(*count as i64));
    }

    let mut root = serde_json::Map::new();
    root.insert("export".into(), Value::Object(export));
    root.insert("counts".into(), Value::Object(counted));
    render_table(&Value::Object(root))
}

/// Human-facing instructions. Deliberately generic — it describes the *rule*
/// (every file is an array of tables) rather than listing categories, so it
/// stays true as the library grows.
fn readme() -> String {
    format!(
        "# Profile export\n\n\
         Exported by slicer-engine {version}.\n\n\
         Each `.toml` file here holds one profile as a TOML *array of tables*\n\
         (`[[printers]]`, `[[filaments]]`, …), which is exactly how the engine\n\
         stores them in `profiles.toml`.\n\n\
         ## Restoring the whole library\n\n\
         Concatenate the files back into a single `profiles.toml` and drop it in\n\
         the slicer's config directory (next to `slicer.toml`). Every `.toml`\n\
         except `manifest.toml` is part of the library:\n\n\
         ```sh\n\
         cat */*.toml $(ls *.toml | grep -v '^manifest.toml$') > profiles.toml\n\
         ```\n\n\
         Files are numbered so that name order is library order — keep the\n\
         prefixes and the profiles come back in the order you had them.\n\n\
         ## Taking a single profile\n\n\
         Append any one file to an existing `profiles.toml`. Because every entry\n\
         is an array-of-tables element, appending adds a profile instead of\n\
         replacing one.\n\n\
         ## Manifest\n\n\
         `manifest.toml` records the engine version that wrote the export and how\n\
         many entries each category had.\n\n\
         ## Printer credentials are not included\n\n\
         API keys and other secrets are stripped, so this archive is safe to\n\
         share or commit. Re-enter them in the printer settings after restoring.\n",
        version = crate::version::VERSION,
    )
}

/// Zero-padding width for the ordinal prefix, so 100 profiles still sort right.
fn index_width(count: usize) -> usize {
    count.to_string().len().max(2)
}

/// A filesystem-safe, human-recognisable name for one profile, prefixed with
/// its position in the category.
///
/// The ordinal is not decoration: profile order is user-visible (it is the
/// order the settings list shows), and a bundle is reassembled by concatenating
/// files in name order. The prefix is what preserves that order — and it makes
/// every filename unique, so two profiles both called "Draft" cannot collide.
///
/// The readable part prefers the profile's `name`, falls back to its `id`, then
/// to nothing at all (the ordinal alone still identifies the file).
fn file_stem(item: &Value, index: usize, width: usize) -> String {
    let slug = item
        .get("name")
        .and_then(Value::as_str)
        .map(slugify)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            item.get("id")
                .and_then(Value::as_str)
                .map(slugify)
                .filter(|s| !s.is_empty())
        });

    let ordinal = format!("{:0width$}", index + 1, width = width);
    match slug {
        Some(slug) => format!("{ordinal}-{slug}"),
        None => ordinal,
    }
}

/// Lowercase ASCII-alphanumeric with single dashes; anything else becomes a
/// separator. Non-ASCII names (「試作」, "Prüfung") can slug down to nothing —
/// [`unique_slug`] falls back to the id in that case.
fn slugify(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let trimmed = out.trim_matches('-');
    trimmed.chars().take(60).collect::<String>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profiles::library::{Label, LabelTone};
    use crate::profiles::{defaults, ProfileSource};
    use std::io::Read;

    fn sample_library() -> ProfileLibrary {
        let mut printer = defaults::default_printer();
        printer.meta.id = "user-printer-1".into();
        printer.meta.name = "Voron 2.4 / 0.6mm".into();
        printer.meta.source = ProfileSource::User;
        printer.params = serde_json::json!({
            "nozzle_diameter_mm": 0.6,
            "retract_mm": 1.2,
            "gcode_flavor": "marlin",
        });

        // Same display name as the first: the slug must not collide.
        let mut twin = defaults::default_printer();
        twin.meta.id = "user-printer-2".into();
        twin.meta.name = "Voron 2.4 / 0.6mm".into();
        twin.meta.source = ProfileSource::User;

        ProfileLibrary {
            printers: vec![printer, twin],
            filaments: vec![defaults::default_filament()],
            processes: vec![defaults::default_process()],
            labels: vec![Label {
                id: "label-favorite".into(),
                name: "Favorite".into(),
                color: "#b8942f".into(),
                tone: LabelTone::Dark,
            }],
        }
    }

    fn unzip(bytes: &[u8]) -> Vec<(String, String)> {
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes.to_vec())).expect("open archive");
        let mut files = Vec::new();
        for i in 0..archive.len() {
            let mut file = archive.by_index(i).expect("entry");
            let name = file.name().to_string();
            let mut contents = String::new();
            file.read_to_string(&mut contents).expect("read entry");
            files.push((name, contents));
        }
        files
    }

    #[test]
    fn single_export_matches_the_engine_on_disk_format() {
        let library = sample_library();
        let artifact =
            export_library(&library, ProfileExportFormat::Single).expect("export single");

        assert_eq!(artifact.filename, "profiles.toml");
        assert_eq!(
            String::from_utf8(artifact.bytes.clone()).expect("utf8"),
            crate::profiles::toml_bridge::render_library_toml(&library).expect("render"),
            "the single-file export must be the exact document the store writes",
        );

        let restored = crate::profiles::toml_bridge::parse_library_toml(
            std::str::from_utf8(&artifact.bytes).expect("utf8"),
        )
        .expect("parse");
        assert_eq!(restored, library);
    }

    #[test]
    fn credentials_never_leave_in_either_shape() {
        let mut library = sample_library();
        library.printers[0].connection.api_key = Some("super-secret-moonraker-key".into());
        // A credential nested in the free-form params bag must go too.
        library.printers[1].params = serde_json::json!({
            "nested": { "token": "another-secret" },
            "retract_mm": 1.0,
        });

        for format in [ProfileExportFormat::Single, ProfileExportFormat::Bundle] {
            let artifact = export_library(&library, format).expect("export");
            let haystack = String::from_utf8_lossy(&artifact.bytes).into_owned();
            assert!(
                !haystack.contains("super-secret-moonraker-key"),
                "api_key must not appear in {format:?}",
            );
            assert!(
                !haystack.contains("another-secret"),
                "a nested credential must not appear in {format:?}",
            );
        }

        // Everything else about the connection survives, so the profile is
        // still usable after restoring — only the key has to be re-entered.
        let single = export_library(&library, ProfileExportFormat::Single).expect("export");
        let restored = crate::profiles::toml_bridge::parse_library_toml(
            std::str::from_utf8(&single.bytes).expect("utf8"),
        )
        .expect("parse");
        assert_eq!(restored.printers[0].connection.api_key, None);
        assert_eq!(
            restored.printers[0].connection.host,
            library.printers[0].connection.host,
        );
        assert_eq!(restored.printers[1].params["retract_mm"], 1.0);
    }

    #[test]
    fn bundle_splits_profiles_and_keeps_labels_together() {
        let artifact =
            export_library(&sample_library(), ProfileExportFormat::Bundle).expect("export bundle");
        assert_eq!(artifact.filename, "slicer-profiles.zip");

        let names: Vec<String> = unzip(&artifact.bytes)
            .into_iter()
            .map(|(name, _)| name)
            .collect();

        assert!(
            names.contains(&"printers/01-voron-2-4-0-6mm.toml".to_string()),
            "{names:?}"
        );
        // The duplicate display name must land in its own file, not overwrite.
        assert!(
            names.contains(&"printers/02-voron-2-4-0-6mm.toml".to_string()),
            "{names:?}"
        );
        assert!(
            names.iter().any(|n| n.starts_with("filaments/")),
            "{names:?}"
        );
        assert!(
            names.iter().any(|n| n.starts_with("processes/")),
            "{names:?}"
        );
        assert!(names.contains(&"labels.toml".to_string()), "{names:?}");
        assert!(names.contains(&"manifest.toml".to_string()), "{names:?}");
        assert!(names.contains(&"README.md".to_string()), "{names:?}");
    }

    /// `cat` the bundle back into one document, the way the README tells users
    /// to. Entries come back in name order, which is library order.
    fn reassemble(bytes: &[u8]) -> String {
        let mut document = String::new();
        for (name, contents) in unzip(bytes) {
            if name.ends_with(".toml") && name != "manifest.toml" {
                document.push_str(&contents);
                document.push('\n');
            }
        }
        document
    }

    #[test]
    fn concatenating_a_bundle_reconstructs_the_library() {
        let library = sample_library();
        let artifact = export_library(&library, ProfileExportFormat::Bundle).expect("export");

        let restored =
            crate::profiles::toml_bridge::parse_library_toml(&reassemble(&artifact.bytes))
                .expect("parse bundle");
        assert_eq!(
            restored, library,
            "concatenating the bundle must yield the original library",
        );
    }

    #[test]
    fn an_empty_category_still_round_trips() {
        // An empty category contributes no file, so the reassembled document
        // simply omits that key — which `#[serde(default)]` reads back as an
        // empty list. The contract is equality of the *library*, not of the
        // text.
        let mut library = sample_library();
        library.filaments.clear();
        library.labels.clear();

        let artifact = export_library(&library, ProfileExportFormat::Bundle).expect("export");
        let restored =
            crate::profiles::toml_bridge::parse_library_toml(&reassemble(&artifact.bytes))
                .expect("parse bundle");
        assert_eq!(restored, library);
    }

    #[test]
    fn unknown_settings_in_the_params_bag_survive_the_round_trip() {
        // The future-proofing guarantee, precisely scoped: a setting this build
        // has never heard of rides along *when it lives in the free-form params
        // bag* — which is where slicing settings land — because the exporter
        // walks values, not fields. (A new *typed* field on a profile struct is
        // dropped by serde at load time, before the exporter sees it, exactly as
        // it is for `GET /api/profiles` and the next `ProfileStore::save`.)
        let mut library = sample_library();
        library.printers[0].params = serde_json::json!({
            "a_setting_from_the_future": 42.5,
            "nested": { "deeply": ["a", "b"] },
        });

        let artifact = export_library(&library, ProfileExportFormat::Bundle).expect("export");
        let mut document = String::new();
        for (name, contents) in unzip(&artifact.bytes) {
            if name.ends_with(".toml") && name != "manifest.toml" {
                document.push_str(&contents);
                document.push('\n');
            }
        }

        let restored = crate::profiles::toml_bridge::parse_library_toml(&document).expect("parse");
        assert_eq!(restored.printers[0].params, library.printers[0].params);
    }

    #[test]
    fn empty_library_still_produces_a_readable_archive() {
        let artifact = export_library(&ProfileLibrary::default(), ProfileExportFormat::Bundle)
            .expect("export");
        let files = unzip(&artifact.bytes);
        let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["README.md", "manifest.toml"]);

        let manifest = &files.iter().find(|(n, _)| n == "manifest.toml").unwrap().1;
        assert!(manifest.contains("printers = 0"), "{manifest}");
    }

    #[test]
    fn export_is_reproducible() {
        let library = sample_library();
        let a = export_library(&library, ProfileExportFormat::Bundle).expect("a");
        let b = export_library(&library, ProfileExportFormat::Bundle).expect("b");
        assert_eq!(
            a.bytes, b.bytes,
            "same library must export byte-identically"
        );
    }

    #[test]
    fn file_stem_falls_back_when_a_name_has_no_ascii() {
        let item = serde_json::json!({ "id": "user-42", "name": "試作" });
        assert_eq!(file_stem(&item, 0, 2), "01-user-42");

        let nameless = serde_json::json!({});
        assert_eq!(file_stem(&nameless, 3, 2), "04");
    }

    #[test]
    fn format_tokens_parse() {
        assert_eq!(
            ProfileExportFormat::parse("bundle"),
            Some(ProfileExportFormat::Bundle)
        );
        assert_eq!(
            ProfileExportFormat::parse("TOML"),
            Some(ProfileExportFormat::Single)
        );
        assert_eq!(ProfileExportFormat::parse("pdf"), None);
    }
}
