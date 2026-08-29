//! The JSON↔TOML bridge the profile library is persisted and exported through.
//!
//! # Why a bridge instead of `toml::to_string(&library)`
//!
//! The profile structs are JSON-native: [`#[serde(flatten)]`][flatten] metadata
//! plus a dynamic `serde_json::Value` `params` bag. The `toml` serializer
//! cannot encode that shape directly (flattened maps and TOML's lack of `null`
//! both trip it). So every render goes through a `serde_json::Value` first —
//! which handles flatten and the params bag perfectly — and is then converted
//! to a `toml::Value` with nulls dropped.
//!
//! Everything here works at the **value level**: it never names a profile
//! field. A new setting added to any profile therefore appears in
//! `profiles.toml` and in an export with no change to this module — the
//! property the exporter is built on.
//!
//! [flatten]: https://serde.rs/field-attrs.html#flatten

use anyhow::{Context, Result};

use super::library::ProfileLibrary;

/// Serialize a library to the JSON value every renderer works from.
pub fn library_to_value(library: &ProfileLibrary) -> Result<serde_json::Value> {
    serde_json::to_value(library).context("serialize profile library")
}

/// Render an already-serialized library value as a TOML document.
///
/// Split from [`render_library_toml`] so callers that need to transform the
/// value first — the exporter, which redacts credentials — still go through the
/// one renderer.
pub fn render_value_toml(value: serde_json::Value) -> Result<String> {
    // The root is always a struct → a TOML table; only an all-null value could
    // yield `None`, which cannot happen for `ProfileLibrary`.
    let toml_value =
        json_to_toml(value).context("profile library serialized to a null TOML document")?;
    toml::to_string_pretty(&toml_value).context("render profile library to TOML")
}

/// Render a library as the exact TOML document written to `profiles.toml`.
///
/// This is the single renderer behind both
/// [`ProfileStore::save`](super::store::ProfileStore::save) and the
/// [single-file export](super::export::ProfileExportFormat::Single), so the
/// two can never drift.
pub fn render_library_toml(library: &ProfileLibrary) -> Result<String> {
    render_value_toml(library_to_value(library)?)
}

/// Parse a `profiles.toml` document back into a library.
pub fn parse_library_toml(text: &str) -> Result<ProfileLibrary> {
    let toml_value: toml::Value = toml::from_str(text).context("parse profiles TOML")?;
    serde_json::from_value(toml_to_json(toml_value)).context("decode profiles")
}

/// Convert a `serde_json::Value` into a `toml::Value`, dropping nulls.
///
/// Returns `None` for a bare `null` (and omits null object entries / array
/// elements) because TOML has no null. This is what lets the profiles' sparse
/// `params` bag — which defaults to `Value::Null` — round-trip cleanly.
pub fn json_to_toml(value: serde_json::Value) -> Option<toml::Value> {
    use serde_json::Value as J;
    Some(match value {
        J::Null => return None,
        J::Bool(b) => toml::Value::Boolean(b),
        J::Number(n) => {
            if let Some(i) = n.as_i64() {
                toml::Value::Integer(i)
            } else if let Some(f) = n.as_f64() {
                toml::Value::Float(f)
            } else {
                // u64 above i64::MAX — keep it losslessly as a string.
                toml::Value::String(n.to_string())
            }
        }
        J::String(s) => toml::Value::String(s),
        J::Array(items) => toml::Value::Array(items.into_iter().filter_map(json_to_toml).collect()),
        J::Object(map) => {
            let mut table = toml::map::Map::new();
            for (key, val) in map {
                if let Some(converted) = json_to_toml(val) {
                    table.insert(key, converted);
                }
            }
            toml::Value::Table(table)
        }
    })
}

/// Convert a `toml::Value` back into a `serde_json::Value`.
pub fn toml_to_json(value: toml::Value) -> serde_json::Value {
    use toml::Value as T;
    match value {
        T::String(s) => serde_json::Value::String(s),
        T::Integer(i) => serde_json::Value::Number(i.into()),
        T::Float(f) => serde_json::Number::from_f64(f)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        T::Boolean(b) => serde_json::Value::Bool(b),
        T::Datetime(dt) => serde_json::Value::String(dt.to_string()),
        T::Array(items) => serde_json::Value::Array(items.into_iter().map(toml_to_json).collect()),
        T::Table(table) => serde_json::Value::Object(
            table
                .into_iter()
                .map(|(k, v)| (k, toml_to_json(v)))
                .collect(),
        ),
    }
}
