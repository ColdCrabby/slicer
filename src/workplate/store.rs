//! File-backed workplate storage, for hosts without a database.
//!
//! The server keeps a plate's setup on the plate's own row, where `DELETE
//! /api/history` drops it along with everything else about that plate. The
//! desktop app has no database at all — its history is in memory and its
//! profiles are a TOML file — so its plates live beside them, as
//! `<config_dir>/workplates/<request_uuid>.json`.
//!
//! One file per plate rather than one map: plates accumulate, and rewriting
//! every plate to record that one of them moved an object is the kind of thing
//! that is fine until someone has four hundred of them.

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};

use super::WorkplateSetup;
use crate::config::config_dir;

/// Directory holding one JSON document per workplate.
pub fn workplates_dir() -> PathBuf {
    config_dir().join("workplates")
}

/// JSON-backed store for [`WorkplateSetup`] documents.
///
/// Stateless beyond its directory: every call touches the filesystem, so
/// concurrent callers always see the latest state (last writer wins).
#[derive(Debug, Clone)]
pub struct WorkplateStore {
    dir: PathBuf,
}

impl Default for WorkplateStore {
    fn default() -> Self {
        Self::new()
    }
}

impl WorkplateStore {
    /// Store at the default location ([`workplates_dir`]).
    pub fn new() -> Self {
        Self {
            dir: workplates_dir(),
        }
    }

    /// Store at an explicit directory (tests, custom deployments).
    pub fn at(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// Path for one plate's document. `None` when the id is not a plain uuid —
    /// the id reaches this from the webview, and a path separator in it would
    /// otherwise write outside the store.
    fn path_for(&self, request_uuid: &str) -> Option<PathBuf> {
        let valid = !request_uuid.is_empty()
            && request_uuid
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-');
        valid.then(|| self.dir.join(format!("{request_uuid}.json")))
    }

    /// Load one plate's setup. A plate that was never configured, and a
    /// document this build cannot parse, both read as `None`: the plate still
    /// opens with the defaults rather than refusing to open at all.
    pub fn load(&self, request_uuid: &str) -> Result<Option<WorkplateSetup>> {
        let Some(path) = self.path_for(request_uuid) else {
            anyhow::bail!("invalid workplate id '{request_uuid}'");
        };
        if !path.exists() {
            return Ok(None);
        }
        let content = fs::read_to_string(&path)
            .with_context(|| format!("read workplate '{}'", path.display()))?;
        Ok(serde_json::from_str(&content).ok())
    }

    /// Persist one plate's setup, creating the directory as needed.
    pub fn save(&self, request_uuid: &str, setup: &WorkplateSetup) -> Result<()> {
        let Some(path) = self.path_for(request_uuid) else {
            anyhow::bail!("invalid workplate id '{request_uuid}'");
        };
        fs::create_dir_all(&self.dir)
            .with_context(|| format!("create workplates dir '{}'", self.dir.display()))?;
        let content = serde_json::to_string_pretty(setup)?;
        fs::write(&path, content)
            .with_context(|| format!("write workplate '{}'", path.display()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workplate::WorkplatePresets;

    fn temp_store(tag: &str) -> (WorkplateStore, PathBuf) {
        let dir = std::env::temp_dir().join(format!("workplates-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        (WorkplateStore::at(&dir), dir)
    }

    #[test]
    fn a_plate_round_trips() {
        let (store, dir) = temp_store("rt");
        let uuid = "11111111-1111-1111-1111-111111111111";
        let setup = WorkplateSetup {
            name: Some("Benchy".into()),
            presets: WorkplatePresets {
                filament: Some("builtin-generic-petg".into()),
                ..Default::default()
            },
            overrides: serde_json::json!({ "layer_height": 0.12 }),
            ..Default::default()
        };

        store.save(uuid, &setup).expect("save");
        assert_eq!(store.load(uuid).expect("load"), Some(setup));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unconfigured_plate_reads_as_nothing() {
        let (store, _) = temp_store("missing");
        assert_eq!(
            store
                .load("22222222-2222-2222-2222-222222222222")
                .expect("load"),
            None
        );
    }

    /// The id arrives from the webview, so it must not be able to name a path.
    #[test]
    fn an_id_that_is_not_a_uuid_is_refused() {
        let (store, _) = temp_store("traversal");
        assert!(store.load("../../profiles").is_err());
        assert!(store.save("../escape", &WorkplateSetup::default()).is_err());
    }

    /// Each plate is its own file, so saving one never rewrites another.
    #[test]
    fn plates_do_not_share_a_file() {
        let (store, dir) = temp_store("separate");
        let a = "33333333-3333-3333-3333-333333333333";
        let b = "44444444-4444-4444-4444-444444444444";

        store
            .save(
                a,
                &WorkplateSetup {
                    name: Some("A".into()),
                    ..Default::default()
                },
            )
            .expect("save a");
        store
            .save(
                b,
                &WorkplateSetup {
                    name: Some("B".into()),
                    ..Default::default()
                },
            )
            .expect("save b");

        assert_eq!(
            store.load(a).expect("a").and_then(|s| s.name).as_deref(),
            Some("A")
        );
        assert_eq!(
            store.load(b).expect("b").and_then(|s| s.name).as_deref(),
            Some("B")
        );

        let _ = fs::remove_dir_all(&dir);
    }
}
