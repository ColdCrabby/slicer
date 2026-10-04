//! Translating the placeholders in imported custom G-code.
//!
//! Start, end and layer G-code is the part of a printer profile most likely to
//! break silently. Other slicers fill `[first_layer_temperature]` or
//! `{bed_temperature_initial_layer_single}` in before the file is written; left
//! as they are, those reach the printer as literal text — a firmware error at
//! best, a heater set to zero at worst. So every placeholder is either
//! translated into the one this slicer fills (`{nozzle_temp_first_layer}`),
//! fixed to the value the file states when it is a property of the printer, or
//! reported as one nothing here can fill.
//!
//! Two placeholder syntaxes are read: `[name]` and `{expression}`, each with an
//! optional extruder index — `[temperature_0]`, `{temperature[0]}`,
//! `{temperature[initial_extruder]}`. Conditionals (`{if …}`) and arithmetic
//! can't be evaluated here and are always reported.

/// A placeholder this slicer fills, and the names other slicers use for it.
const DYNAMIC: &[(&str, &[&str])] = &[
    (
        "nozzle_temp_first_layer",
        &[
            "first_layer_temperature",
            "nozzle_temperature_initial_layer",
        ],
    ),
    ("nozzle_temp", &["temperature", "nozzle_temperature"]),
    (
        "bed_temp_first_layer",
        &[
            "first_layer_bed_temperature",
            "bed_temperature_initial_layer",
            "bed_temperature_initial_layer_single",
            "hot_plate_temp_initial_layer",
            "cool_plate_temp_initial_layer",
            "eng_plate_temp_initial_layer",
            "textured_plate_temp_initial_layer",
            "supertack_plate_temp_initial_layer",
            "textured_cool_plate_temp_initial_layer",
        ],
    ),
    (
        "bed_temp",
        &[
            "bed_temperature",
            "hot_plate_temp",
            "cool_plate_temp",
            "eng_plate_temp",
            "textured_plate_temp",
            "supertack_plate_temp",
            "textured_cool_plate_temp",
        ],
    ),
    (
        "chamber_temp_first_layer",
        &["chamber_temperature_initial_layer"],
    ),
    (
        "chamber_temp",
        &["chamber_temperature", "overall_chamber_temperature"],
    ),
    ("filament_type", &["filament_type"]),
    (
        "first_layer_height",
        &["first_layer_height", "initial_layer_print_height"],
    ),
    ("layer_height", &["layer_height"]),
    ("z", &["layer_z"]),
    ("layer_num", &["layer_num"]),
];

/// Placeholders this slicer fills itself, left exactly as they are.
const OURS: &[&str] = &[
    "nozzle_temp",
    "bed_temp",
    "nozzle_temp_first_layer",
    "bed_temp_first_layer",
    "chamber_temp",
    "chamber_temp_first_layer",
    "filament_type",
    "layer_height",
    "first_layer_height",
    "z",
    "height",
    "layer_num",
];

/// Extruder-number placeholders. A single-extruder profile always prints with
/// extruder 0.
const EXTRUDER_NUMBERS: &[&str] = &[
    "initial_extruder",
    "initial_tool",
    "current_extruder",
    "current_tool",
    "next_extruder",
    "previous_extruder",
];

/// The outcome of translating one block.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Translated {
    /// The block with every placeholder it could resolve resolved.
    pub text: String,
    /// `foreign → ours` pairs, each listed once.
    pub replaced: Vec<(String, String)>,
    /// Placeholders left as written because nothing here fills them.
    pub unresolved: Vec<String>,
    /// Translations that changed meaning slightly, in a sentence each.
    pub caveats: Vec<String>,
}

/// One placeholder, parsed.
struct Placeholder<'a> {
    /// The text as written, delimiters included.
    raw: &'a str,
    /// Its setting name, when it is a plain reference rather than an
    /// expression.
    name: Option<&'a str>,
    /// The extruder index it asks for, if any: `0`, `initial_extruder`, …
    index: Option<&'a str>,
    /// `{layer_num + 1}`: the one piece of arithmetic worth recognising, since
    /// it is exactly this slicer's one-based layer number.
    is_layer_num_plus_one: bool,
}

/// Translate every placeholder in `gcode`.
///
/// `static_value(name, index)` returns the file's own value for a printer
/// setting (nozzle diameter, bed size), which is fixed for the profile and so
/// can be written in directly. `firmware_braces` keeps `{…}` expressions that use dotted
/// names — RepRapFirmware evaluates `{move.axes[0].max}` itself — out of the
/// unresolved list.
pub(crate) fn translate(
    gcode: &str,
    static_value: &dyn Fn(&str, Option<&str>) -> Option<String>,
    firmware_braces: bool,
) -> Translated {
    let mut out = Translated {
        text: String::with_capacity(gcode.len()),
        ..Default::default()
    };
    let mut rest = gcode;
    while let Some(at) = rest.find(['[', '{']) {
        out.text.push_str(&rest[..at]);
        let tail = &rest[at..];
        let Some(placeholder) = parse(tail) else {
            out.text.push_str(&tail[..1]);
            rest = &tail[1..];
            continue;
        };
        let consumed = placeholder.raw.len();
        // A placeholder inside a comment is never executed, so one nothing can
        // fill there is no risk to the print.
        let line_start = out.text.rfind('\n').map(|i| i + 1).unwrap_or(0);
        let in_comment = out.text[line_start..].contains(';');
        let unresolved_before = out.unresolved.len();
        let resolved = resolve(&placeholder, static_value, firmware_braces, &mut out);
        if in_comment {
            out.unresolved.truncate(unresolved_before);
        }
        out.text.push_str(&resolved);
        rest = &tail[consumed..];
    }
    out.text.push_str(rest);
    out
}

fn resolve(
    p: &Placeholder<'_>,
    static_value: &dyn Fn(&str, Option<&str>) -> Option<String>,
    firmware_braces: bool,
    out: &mut Translated,
) -> String {
    let raw = p.raw.to_string();
    if p.is_layer_num_plus_one {
        note_replaced(out, &raw, "{layer_num}");
        return "{layer_num}".to_string();
    }
    let Some(name) = p.name else {
        let dotted = raw.starts_with('{') && raw.contains('.') && !raw.contains("if ");
        if !(firmware_braces && dotted) && !out.unresolved.contains(&raw) {
            out.unresolved.push(raw.clone());
        }
        return raw;
    };
    // Already one of ours — written in this slicer's own syntax.
    if raw.starts_with('{') && OURS.contains(&name) && raw == format!("{{{name}}}") {
        if name == "layer_num" {
            note_layer_num(out);
        }
        return raw;
    }
    if let Some((ours, _)) = DYNAMIC.iter().find(|(_, names)| names.contains(&name)) {
        let token = format!("{{{ours}}}");
        if *ours == "layer_num" {
            note_layer_num(out);
        }
        note_replaced(out, &raw, &token);
        return token;
    }
    if EXTRUDER_NUMBERS.contains(&name) {
        note_replaced(out, &raw, "0");
        return "0".to_string();
    }
    if let Some(value) = static_value(name, p.index) {
        note_replaced(out, &raw, &value);
        return value;
    }
    if !out.unresolved.contains(&raw) {
        out.unresolved.push(raw.clone());
    }
    raw
}

fn note_replaced(out: &mut Translated, from: &str, to: &str) {
    if !out.replaced.iter().any(|(f, _)| f == from) {
        out.replaced.push((from.to_string(), to.to_string()));
    }
}

fn note_layer_num(out: &mut Translated) {
    const CAVEAT: &str = "{layer_num} counts layers from 1 here; the source counted from 0, \
                          so anything that compares it with a number may need adjusting.";
    if !out.caveats.iter().any(|c| c == CAVEAT) {
        out.caveats.push(CAVEAT.to_string());
    }
}

/// Parse the placeholder `text` starts with, or `None` when the bracket or
/// brace is just a character.
fn parse(text: &str) -> Option<Placeholder<'_>> {
    if text.starts_with('[') {
        // `[name]`, `[name_0]` or `[name[index]]`.
        let body = &text[1..];
        let name_len = ident_len(body);
        if name_len == 0 {
            return None;
        }
        let after = &body[name_len..];
        let (extra, bracket_index) = if let Some(index) = after.strip_prefix('[') {
            let close = index.find(']')?;
            (close + 2, Some(&index[..close]))
        } else {
            (0, None)
        };
        let end = 1 + name_len + extra;
        if !text[end..].starts_with(']') {
            return None;
        }
        let (name, suffix_index) = split_index_suffix(&body[..name_len]);
        return Some(Placeholder {
            raw: &text[..end + 1],
            name: Some(name),
            index: bracket_index.or(suffix_index),
            is_layer_num_plus_one: false,
        });
    }
    // `{…}` — up to the matching close brace on the same line.
    let close = text.find('}')?;
    let raw = &text[..close + 1];
    if raw.contains('\n') || raw.len() < 3 {
        return None;
    }
    let inner = raw[1..raw.len() - 1].trim();
    let compact: String = inner.chars().filter(|c| !c.is_whitespace()).collect();
    if compact == "layer_num+1" {
        return Some(Placeholder {
            raw,
            name: None,
            index: None,
            is_layer_num_plus_one: true,
        });
    }
    let name_len = ident_len(inner);
    let mut index = None;
    let name = if name_len > 0 {
        let after = inner[name_len..].trim();
        let bracketed = after.starts_with('[')
            && after.ends_with(']')
            && ident_or_number(&after[1..after.len() - 1]);
        if bracketed {
            index = Some(after[1..after.len() - 1].trim());
        }
        (after.is_empty() || bracketed).then(|| {
            let (name, suffix) = split_index_suffix(&inner[..name_len]);
            index = index.or(suffix);
            name
        })
    } else {
        None
    };
    Some(Placeholder {
        raw,
        name,
        index,
        is_layer_num_plus_one: false,
    })
}

fn ident_len(text: &str) -> usize {
    let mut len = 0;
    for (i, c) in text.char_indices() {
        let ok = if i == 0 {
            c.is_ascii_lowercase() || c == '_'
        } else {
            c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'
        };
        if !ok {
            break;
        }
        len = i + c.len_utf8();
    }
    len
}

fn ident_or_number(text: &str) -> bool {
    let text = text.trim();
    !text.is_empty() && (text.chars().all(|c| c.is_ascii_digit()) || ident_len(text) == text.len())
}

/// `temperature_0` → (`temperature`, `0`): the legacy per-extruder suffix.
fn split_index_suffix(name: &str) -> (&str, Option<&str>) {
    match name.rsplit_once('_') {
        Some((base, index)) if !index.is_empty() && index.chars().all(|c| c.is_ascii_digit()) => {
            (base, Some(index))
        }
        _ => (name, None),
    }
}

/// Whether a start script heats the bed and the nozzle itself — by command, or
/// by passing a temperature to a macro that does.
pub(crate) fn heats(script: &str) -> (bool, bool) {
    let mut bed = false;
    let mut nozzle = false;
    for line in script.lines() {
        let code = line
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_uppercase();
        if code.is_empty() {
            continue;
        }
        let command = code.split_whitespace().next().unwrap_or("");
        match command {
            "M140" | "M190" => bed = true,
            "M104" | "M109" | "M568" => nozzle = true,
            "G10" if code.contains(" P") && code.contains(" S") => nozzle = true,
            _ => {}
        }
        // A macro handed the temperatures, or Klipper's own heater command,
        // does the heating itself.
        if code.contains("{BED_TEMP")
            || code.contains("BED=")
            || code.contains("BED_TEMP=")
            || (code.contains("HEATER_BED") && code.contains("TARGET="))
        {
            bed = true;
        }
        if code.contains("HEATER=EXTRUDER") && code.contains("TARGET=") {
            nozzle = true;
        }
        if code.contains("{NOZZLE_TEMP")
            || code.contains("EXTRUDER=")
            || code.contains("EXTRUDER_TEMP=")
            || code.contains("HOTEND=")
        {
            nozzle = true;
        }
    }
    (bed, nozzle)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_statics(_: &str, _: Option<&str>) -> Option<String> {
        None
    }

    #[test]
    fn temperatures_in_either_syntax_become_ours() {
        let t = translate(
            "M190 S[first_layer_bed_temperature]\nM109 S{first_layer_temperature[0]}\n\
             M104 S[temperature_0] T[initial_extruder]",
            &no_statics,
            false,
        );
        assert_eq!(
            t.text,
            "M190 S{bed_temp_first_layer}\nM109 S{nozzle_temp_first_layer}\n\
             M104 S{nozzle_temp} T0"
        );
        assert!(t.unresolved.is_empty());
        assert_eq!(t.replaced.len(), 4);
    }

    #[test]
    fn json_family_names_translate_too() {
        let t = translate(
            "PRINT_START EXTRUDER=[nozzle_temperature_initial_layer] \
             BED={bed_temperature_initial_layer_single}",
            &no_statics,
            false,
        );
        assert_eq!(
            t.text,
            "PRINT_START EXTRUDER={nozzle_temp_first_layer} BED={bed_temp_first_layer}"
        );
    }

    #[test]
    fn printer_facts_are_written_in_and_expressions_are_reported() {
        let statics =
            |name: &str, _: Option<&str>| (name == "nozzle_diameter").then(|| "0.4".to_string());
        let t = translate(
            "; nozzle [nozzle_diameter]\n{if layer_z > 2}M106 S255{endif}\nM117 [input_filename_base]",
            &statics,
            false,
        );
        assert!(t.text.starts_with("; nozzle 0.4\n"));
        assert_eq!(
            t.unresolved,
            vec!["{if layer_z > 2}", "{endif}", "[input_filename_base]"]
        );
    }

    #[test]
    fn layer_numbers_shift_base_and_say_so() {
        let t = translate(
            "SET_PRINT_STATS_INFO CURRENT_LAYER={layer_num + 1}\nM117 [layer_num] at [layer_z]",
            &no_statics,
            false,
        );
        assert_eq!(
            t.text,
            "SET_PRINT_STATS_INFO CURRENT_LAYER={layer_num}\nM117 {layer_num} at {z}"
        );
        assert_eq!(
            t.caveats.len(),
            1,
            "the zero-based [layer_num] is flagged once"
        );
    }

    #[test]
    fn plain_brackets_and_firmware_expressions_are_left_alone() {
        let t = translate(
            "EXCLUDE_OBJECT_DEFINE POLYGON=[[1,2],[3,4]]\nM557 X{move.axes[0].min}:{move.axes[0].max}",
            &no_statics,
            true,
        );
        assert_eq!(
            t.text,
            "EXCLUDE_OBJECT_DEFINE POLYGON=[[1,2],[3,4]]\nM557 X{move.axes[0].min}:{move.axes[0].max}"
        );
        assert!(t.unresolved.is_empty(), "{:?}", t.unresolved);
    }

    #[test]
    fn heating_is_found_in_commands_and_macro_arguments() {
        assert_eq!(heats("G28\nG1 Z5"), (false, false));
        assert_eq!(heats("M140 S60 ; bed\nM109 S{nozzle_temp}"), (true, true));
        assert_eq!(
            heats("PRINT_START BED={bed_temp_first_layer} EXTRUDER={nozzle_temp_first_layer}"),
            (true, true)
        );
        assert_eq!(heats("; M190 S60 is commented out"), (false, false));
    }
}
