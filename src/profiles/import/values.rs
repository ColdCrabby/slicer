//! Reading one raw value back the way its file encoded it.
//!
//! INI files escape newlines as `\n`, write per-extruder numbers as `0.4,0.4`
//! and per-extruder strings as `"a b";c`; JSON files use real strings and
//! arrays. The mapping asks for "this setting, for this extruder slot" and gets
//! plain text either way.

use serde_json::Value;

use super::read::{Encoding, RawPreset};

/// Undo INI C-style escapes. Unknown sequences are kept as written, so a
/// Windows path or a stray backslash in G-code survives.
pub(crate) fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('\\') => out.push('\\'),
            Some('"') => out.push('"'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// Split an INI string list — `"Prusament PLA";PLA;""` — into its elements.
///
/// Elements are separated by `;` outside quotes; a quoted element is unescaped,
/// a bare one taken as written.
pub(crate) fn split_string_list(text: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut in_quotes = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if !in_quotes && current.trim().is_empty() => {
                in_quotes = true;
                quoted = true;
                current.clear();
            }
            '"' if in_quotes => in_quotes = false,
            '\\' if in_quotes => {
                current.push('\\');
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            ';' if !in_quotes => {
                items.push(finish(&current, quoted));
                current.clear();
                quoted = false;
            }
            _ => current.push(c),
        }
    }
    items.push(finish(&current, quoted));
    items
}

fn finish(item: &str, quoted: bool) -> String {
    if quoted {
        unescape(item)
    } else {
        item.trim().to_string()
    }
}

/// Whether `text` is a comma-separated list of numbers, booleans or
/// percentages — the shape per-extruder numeric settings take in INI files.
fn is_number_list(text: &str) -> bool {
    text.contains(',')
        && text.split(',').all(|part| {
            let part = part.trim().trim_end_matches('%');
            !part.is_empty() && part.parse::<f64>().is_ok()
        })
}

fn json_text(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(if *b { "1" } else { "0" }.to_string()),
        Value::Array(items) => items.first().and_then(json_text),
        _ => None,
    }
}

impl RawPreset {
    /// Whether the file states `key` at all.
    pub(crate) fn has(&self, key: &str) -> bool {
        self.values.contains_key(key)
    }

    /// A single string setting — G-code, a name, notes — fully decoded.
    pub(crate) fn text(&self, key: &str) -> Option<String> {
        let value = self.values.get(key)?;
        match self.encoding {
            Encoding::Json => json_text(value),
            Encoding::Ini => {
                let raw = value.as_str()?;
                if raw.starts_with('"') && raw.ends_with('"') && raw.len() >= 2 {
                    split_string_list(raw).into_iter().next()
                } else {
                    Some(unescape(raw))
                }
            }
        }
    }

    /// Element `slot` of a per-extruder number, switch or choice, or the value
    /// itself when it isn't a list. A list shorter than `slot` yields its first
    /// element, which is what the source applies to extra extruders.
    pub(crate) fn item(&self, key: &str, slot: usize) -> Option<String> {
        let value = self.values.get(key)?;
        match (self.encoding, value) {
            (_, Value::Array(items)) => items.get(slot).or(items.first()).and_then(json_text),
            (Encoding::Ini, Value::String(raw)) if is_number_list(raw) => {
                let parts: Vec<&str> = raw.split(',').collect();
                Some(parts.get(slot).unwrap_or(&parts[0]).trim().to_string())
            }
            (Encoding::Ini, Value::String(raw)) => Some(unescape(raw.trim())),
            (Encoding::Json, other) => json_text(other),
            _ => None,
        }
    }

    /// Element `slot` of a per-extruder string list (filament type, colour,
    /// filament G-code), fully decoded.
    pub(crate) fn string_item(&self, key: &str, slot: usize) -> Option<String> {
        let value = self.values.get(key)?;
        match (self.encoding, value) {
            (_, Value::Array(items)) => items.get(slot).or(items.first()).and_then(json_text),
            (Encoding::Ini, Value::String(raw)) => {
                let items = split_string_list(raw);
                items.get(slot).or(items.first()).cloned()
            }
            (Encoding::Json, other) => json_text(other),
            _ => None,
        }
    }

    /// How many elements a per-extruder setting has (one for a plain value).
    pub(crate) fn list_len(&self, key: &str) -> usize {
        match self.values.get(key) {
            Some(Value::Array(items)) => items.len(),
            Some(Value::String(raw)) if self.encoding == Encoding::Ini => {
                if is_number_list(raw) {
                    raw.split(',').count()
                } else if raw.starts_with('"') {
                    split_string_list(raw).len()
                } else {
                    1
                }
            }
            Some(_) => 1,
            None => 0,
        }
    }

    /// The value as the file has it, made readable for the report: escapes
    /// decoded, arrays joined.
    pub(crate) fn display(&self, key: &str) -> String {
        match self.values.get(key) {
            Some(Value::Array(items)) => items
                .iter()
                .filter_map(json_text)
                .collect::<Vec<_>>()
                .join(", "),
            Some(Value::String(raw)) if self.encoding == Encoding::Ini => unescape(raw),
            Some(other) => json_text(other).unwrap_or_default(),
            None => String::new(),
        }
    }
}

/// Parse a number, tolerating a trailing `%` the caller has already accounted
/// for and surrounding whitespace.
pub(crate) fn number(text: &str) -> Option<f64> {
    text.trim()
        .trim_end_matches('%')
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|n| n.is_finite())
}

/// Whether a value switches something on: a non-zero number, `true`, or any
/// word other than the ones the source writes for "off".
pub(crate) fn truthy(text: &str) -> bool {
    let t = text.trim().to_ascii_lowercase();
    if let Some(n) = number(&t) {
        return n != 0.0;
    }
    !matches!(
        t.as_str(),
        "" | "false"
            | "no"
            | "none"
            | "nil"
            | "off"
            | "disabled"
            | "no_brim"
            | "no ironing"
            | "regular"
    )
}

/// `true` / `false` for a switch, accepting `1`/`0`, `true`/`false`, `yes`/`no`.
pub(crate) fn boolean(text: &str) -> Option<bool> {
    match text.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Map;

    fn ini(pairs: &[(&str, &str)]) -> RawPreset {
        let mut values = Map::new();
        for (k, v) in pairs {
            values.insert(k.to_string(), Value::String(v.to_string()));
        }
        RawPreset {
            category: None,
            name: None,
            inherits: Vec::new(),
            is_template: false,
            encoding: Encoding::Ini,
            values,
        }
    }

    #[test]
    fn escapes_are_decoded_and_unknown_ones_kept() {
        assert_eq!(
            unescape(r"G28\nM104 S[temperature]"),
            "G28\nM104 S[temperature]"
        );
        assert_eq!(unescape(r"C:\path"), r"C:\path");
        assert_eq!(unescape(r#"say \"hi\""#), r#"say "hi""#);
    }

    #[test]
    fn string_lists_split_outside_quotes_only() {
        assert_eq!(
            split_string_list(r#""Prusament PLA";PLA;"a;b""#),
            vec!["Prusament PLA", "PLA", "a;b"]
        );
        assert_eq!(
            split_string_list(r#""; filament\nG92 E0""#),
            vec!["; filament\nG92 E0"]
        );
    }

    #[test]
    fn per_extruder_numbers_pick_their_slot() {
        let preset = ini(&[
            ("nozzle_diameter", "0.4,0.6"),
            ("bed_shape", "0x0,250x0,250x210,0x210"),
            ("start_gcode", r"G1 X1,Y2\nG28"),
        ]);
        assert_eq!(preset.item("nozzle_diameter", 1).as_deref(), Some("0.6"));
        assert_eq!(preset.item("nozzle_diameter", 5).as_deref(), Some("0.4"));
        assert_eq!(preset.list_len("nozzle_diameter"), 2);
        // Points and G-code are not number lists, however many commas they hold.
        assert_eq!(
            preset.item("bed_shape", 0).as_deref(),
            Some("0x0,250x0,250x210,0x210")
        );
        assert_eq!(preset.text("start_gcode").as_deref(), Some("G1 X1,Y2\nG28"));
    }

    #[test]
    fn json_arrays_and_strings_read_the_same_way() {
        let mut values = Map::new();
        values.insert(
            "nozzle_temperature".into(),
            serde_json::json!(["215", "240"]),
        );
        values.insert("layer_height".into(), serde_json::json!("0.2"));
        let preset = RawPreset {
            encoding: Encoding::Json,
            values,
            ..ini(&[])
        };
        assert_eq!(preset.item("nozzle_temperature", 1).as_deref(), Some("240"));
        assert_eq!(preset.text("layer_height").as_deref(), Some("0.2"));
        assert_eq!(preset.display("nozzle_temperature"), "215, 240");
    }

    #[test]
    fn switches_read_the_words_slicers_write_for_off() {
        assert!(truthy("1") && truthy("organic") && truthy("15%"));
        assert!(!truthy("0") && !truthy("none") && !truthy("no_brim") && !truthy(""));
        assert_eq!(boolean("true"), Some(true));
        assert_eq!(boolean("maybe"), None);
    }
}
