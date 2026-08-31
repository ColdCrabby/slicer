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
//! **JSON**. The conversion between the two lives in
//! [`toml_bridge`](super::toml_bridge), which this module and the
//! [exporter](super::export) share so an exported file is the same document
//! this store would write.
//!
//! The library *shape* itself lives in [`library`](super::library) — it is
//! needed by targets that have no filesystem (wasm), which this module does
//! not compile for.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::library::{ProfileKind, ProfileLibrary};
use super::toml_bridge::{parse_library_toml, render_library_toml};
use crate::config::config_dir;

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
        parse_library_toml(&content)
            .with_context(|| format!("load profiles '{}'", self.path.display()))
    }

    /// Persist the whole library, creating parent directories as needed.
    pub fn save(&self, library: &ProfileLibrary) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create profiles dir '{}'", parent.display()))?;
        }
        let content = render_library_toml(library)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profiles::{defaults, Label, LabelTone, ProfileSource};

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
}
