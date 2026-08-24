//! On-disk persistence for user-owned profile *instances*.
//!
//! # Why this module exists
//!
//! The engine owns profile *definitions*; users own *instances* — their
//! printers, filaments, and process profiles, plus a flat vocabulary of
//! [`Label`]s. Historically those instances lived only in the browser's
//! `localStorage`, which meant a cloud user who cleared their browser — or
//! reinstalled it — silently lost every printer and filament they had set up,
//! even though the slicer itself was running safely on a server.
//!
//! Slicing-relevant data must live **next to the engine**: on the server in
//! cloud mode, on the local machine in native mode. Only the in-browser wasm
//! build, where the browser *is* the engine, legitimately keeps them in
//! `localStorage` (and is inherently losable).
//!
//! # Format: TOML at rest, JSON on the wire
//!
//! The library is persisted as **TOML** (`profiles.toml`, beside `slicer.toml`
//! in the [config dir][crate::config::config_dir]) so it is human-readable and
//! sits with the other engine config. The transport between UI and engine is
//! **JSON**.
//!
//! The profile structs are JSON-native ([`#[serde(flatten)]`][flatten] metadata
//! plus a dynamic `serde_json::Value` `params` bag), a shape the `toml`
//! serializer cannot encode directly. So save/load goes through a JSON↔TOML
//! bridge: the typed structs are taken to a `serde_json::Value` first (which
//! handles flatten and the params bag perfectly), then converted to a
//! `toml::Value` with nulls dropped. This keeps the wire schema identical to
//! the JSON the UI already speaks and never forces a change on the canonical
//! profile definitions.
//!
//! [flatten]: https://serde.rs/field-attrs.html#flatten

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{FilamentProfile, PrinterProfile, ProcessProfile};
use crate::config::config_dir;

/// Shade of a [`Label`] tint (mirrors `ui/src/app/models/label.model.ts`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum LabelTone {
    /// Deeper, slightly stronger tint.
    #[default]
    Dark,
    /// The same hue rendered more transparent, so it reads softer.
    Light,
}

/// A user-defined, cross-area label — think Finder tags with GitHub-style
/// colours. Attached to profiles via [`ProfileMeta::label_ids`], kept as a
/// single flat vocabulary rather than three per-area lists.
///
/// [`ProfileMeta::label_ids`]: super::ProfileMeta::label_ids
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Label {
    /// Stable identifier.
    pub id: String,
    /// Human-readable name shown in the UI.
    pub name: String,
    /// Base hue as a hex string (e.g. `#b8942f`).
    pub color: String,
    /// Shade applied to the base hue.
    #[serde(default)]
    pub tone: LabelTone,
}

/// The complete set of user-owned profile instances the engine persists.
///
/// Each field is a syncable category; the UI mirrors these as its four profile
/// stores. Whole-category replacement (last-writer-wins) is the sync unit — see
/// [`ProfileStore::replace_category`].
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ProfileLibrary {
    /// User-owned printer (machine) profiles.
    #[serde(default)]
    pub printers: Vec<PrinterProfile>,
    /// User-owned filament (material) profiles.
    #[serde(default)]
    pub filaments: Vec<FilamentProfile>,
    /// User-owned process (print/quality) profiles.
    #[serde(default)]
    pub processes: Vec<ProcessProfile>,
    /// Flat, cross-area label vocabulary.
    #[serde(default)]
    pub labels: Vec<Label>,
}

/// One syncable profile category — the granularity of a write-through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ProfileKind {
    /// [`ProfileLibrary::printers`].
    Printers,
    /// [`ProfileLibrary::filaments`].
    Filaments,
    /// [`ProfileLibrary::processes`].
    Processes,
    /// [`ProfileLibrary::labels`].
    Labels,
}

impl ProfileKind {
    /// The lowercase wire/route token for this category (`"printers"`, …).
    pub fn as_str(self) -> &'static str {
        match self {
            ProfileKind::Printers => "printers",
            ProfileKind::Filaments => "filaments",
            ProfileKind::Processes => "processes",
            ProfileKind::Labels => "labels",
        }
    }

    /// Parse a category from its wire/route token (case-insensitive).
    pub fn parse(token: &str) -> Option<Self> {
        match token.to_ascii_lowercase().as_str() {
            "printers" => Some(ProfileKind::Printers),
            "filaments" => Some(ProfileKind::Filaments),
            "processes" => Some(ProfileKind::Processes),
            "labels" => Some(ProfileKind::Labels),
            _ => None,
        }
    }
}

/// Default library file: `<config_dir>/profiles.toml`, beside `slicer.toml`.
pub fn profiles_file() -> PathBuf {
    config_dir().join("profiles.toml")
}

/// TOML-backed store for a [`ProfileLibrary`].
///
/// Cheap to construct and stateless beyond its path — every call reads or
/// writes the file, so concurrent callers always see the latest on-disk state
/// (last-writer-wins on save).
#[derive(Debug, Clone)]
pub struct ProfileStore {
    path: PathBuf,
}

impl Default for ProfileStore {
    fn default() -> Self {
        Self::new()
    }
}

impl ProfileStore {
    /// Store at the default location ([`profiles_file`]).
    pub fn new() -> Self {
        Self {
            path: profiles_file(),
        }
    }

    /// Store at an explicit path (tests, custom deployments).
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// The file this store reads and writes.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Load the library, returning an empty one when the file is absent.
    pub fn load(&self) -> Result<ProfileLibrary> {
        if !self.path.exists() {
            return Ok(ProfileLibrary::default());
        }
        let content = fs::read_to_string(&self.path)
            .with_context(|| format!("read profiles '{}'", self.path.display()))?;
        let toml_value: toml::Value = toml::from_str(&content)
            .with_context(|| format!("parse profiles TOML '{}'", self.path.display()))?;
        let json = toml_to_json(toml_value);
        let library = serde_json::from_value(json)
            .with_context(|| format!("decode profiles '{}'", self.path.display()))?;
        Ok(library)
    }

    /// Persist the whole library, creating parent directories as needed.
    pub fn save(&self, library: &ProfileLibrary) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create profiles dir '{}'", parent.display()))?;
        }
        let json = serde_json::to_value(library).context("serialize profile library")?;
        // The root is always a struct → a TOML table; only an all-null value
        // could yield `None`, which cannot happen for `ProfileLibrary`.
        let toml_value =
            json_to_toml(json).context("profile library serialized to a null TOML document")?;
        let content =
            toml::to_string_pretty(&toml_value).context("render profile library to TOML")?;
        fs::write(&self.path, content)
            .with_context(|| format!("write profiles '{}'", self.path.display()))?;
        Ok(())
    }

    /// Replace a single category from a JSON array and persist the library.
    ///
    /// This is the whole-category write-through the UI performs on any add /
    /// edit / delete. Returns the updated library.
    pub fn replace_category(
        &self,
        kind: ProfileKind,
        items: serde_json::Value,
    ) -> Result<ProfileLibrary> {
        let mut library = self.load()?;
        match kind {
            ProfileKind::Printers => {
                library.printers = serde_json::from_value(items).context("decode printers")?;
            }
            ProfileKind::Filaments => {
                library.filaments = serde_json::from_value(items).context("decode filaments")?;
            }
            ProfileKind::Processes => {
                library.processes = serde_json::from_value(items).context("decode processes")?;
            }
            ProfileKind::Labels => {
                library.labels = serde_json::from_value(items).context("decode labels")?;
            }
        }
        self.save(&library)?;
        Ok(library)
    }
}

/// Convert a `serde_json::Value` into a `toml::Value`, dropping nulls.
///
/// Returns `None` for a bare `null` (and omits null object entries / array
/// elements) because TOML has no null. This is what lets the profiles' sparse
/// `params` bag — which defaults to `Value::Null` — round-trip cleanly.
fn json_to_toml(value: serde_json::Value) -> Option<toml::Value> {
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
fn toml_to_json(value: toml::Value) -> serde_json::Value {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profiles::{defaults, ProfileSource};

    fn sample_library() -> ProfileLibrary {
        let mut printer = defaults::default_printer();
        // Exercise a real (non-null) params override on one entry.
        printer.params = serde_json::json!({
            "nozzle_diameter_mm": 0.6,
            "retract_mm": 1.2,
            "gcode_flavor": "marlin",
        });
        printer.meta.id = "user-printer-1".into();
        printer.meta.source = ProfileSource::User;
        printer.meta.label_ids = vec!["label-favorite".into()];

        // A second printer whose params stays the default null — the bridge
        // must drop it rather than choke on an unrepresentable TOML null.
        let mut bare = defaults::default_printer();
        bare.meta.id = "user-printer-2".into();
        bare.meta.source = ProfileSource::User;
        bare.params = serde_json::Value::Null;

        ProfileLibrary {
            printers: vec![printer, bare],
            filaments: vec![defaults::default_filament()],
            processes: vec![defaults::default_process()],
            labels: vec![
                Label {
                    id: "label-favorite".into(),
                    name: "Favorite".into(),
                    color: "#b8942f".into(),
                    tone: LabelTone::Dark,
                },
                Label {
                    id: "label-experimental".into(),
                    name: "Experimental".into(),
                    color: "#8a6fbf".into(),
                    tone: LabelTone::Light,
                },
            ],
        }
    }

    #[test]
    fn round_trips_through_toml_on_disk() {
        let dir = std::env::temp_dir().join(format!("profiles-test-{}", std::process::id()));
        let path = dir.join("profiles.toml");
        let store = ProfileStore::at(&path);

        let library = sample_library();
        store.save(&library).expect("save");
        assert!(path.exists(), "profiles.toml should be written");

        let loaded = store.load().expect("load");
        assert_eq!(loaded, library, "library must survive a TOML round-trip");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn on_disk_form_is_readable_toml() {
        let dir = std::env::temp_dir().join(format!("profiles-fmt-{}", std::process::id()));
        let path = dir.join("profiles.toml");
        let store = ProfileStore::at(&path);
        store.save(&sample_library()).expect("save");

        let text = fs::read_to_string(&path).expect("read");
        assert!(
            text.contains("[[printers]]"),
            "printers should be an array of tables:\n{text}"
        );
        assert!(
            text.contains("[[labels]]"),
            "labels should be an array of tables:\n{text}"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_file_loads_empty_library() {
        let store = ProfileStore::at("/nonexistent/does/not/exist/profiles.toml");
        let library = store.load().expect("missing file is not an error");
        assert_eq!(library, ProfileLibrary::default());
    }

    #[test]
    fn replace_category_swaps_one_list_only() {
        let dir = std::env::temp_dir().join(format!("profiles-repl-{}", std::process::id()));
        let path = dir.join("profiles.toml");
        let store = ProfileStore::at(&path);
        store.save(&sample_library()).expect("seed");

        let new_labels = serde_json::json!([
            { "id": "label-only", "name": "Only", "color": "#5a9a5e", "tone": "dark" }
        ]);
        let updated = store
            .replace_category(ProfileKind::Labels, new_labels)
            .expect("replace labels");

        assert_eq!(updated.labels.len(), 1);
        assert_eq!(updated.labels[0].id, "label-only");
        // Other categories are untouched.
        assert_eq!(updated.printers.len(), 2);
        assert_eq!(updated.filaments.len(), 1);
        assert_eq!(updated.processes.len(), 1);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn profile_kind_parses_and_stringifies() {
        for kind in [
            ProfileKind::Printers,
            ProfileKind::Filaments,
            ProfileKind::Processes,
            ProfileKind::Labels,
        ] {
            assert_eq!(ProfileKind::parse(kind.as_str()), Some(kind));
            assert_eq!(
                ProfileKind::parse(&kind.as_str().to_uppercase()),
                Some(kind)
            );
        }
        assert_eq!(ProfileKind::parse("bogus"), None);
    }
}
