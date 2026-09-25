//! File-backed library storage, for every runtime with a filesystem.
//!
//! Two directories, because they have two audiences:
//!
//! - **`library_dir()`** — `<config_dir>/library/`: `index.json` and
//!   `thumbs/<id>.png`. The engine's bookkeeping; nobody browses it.
//! - **`models_dir()`** — the copies. On iOS that is `Documents/Models`, which
//!   the Files app shows as *On My iPad › Cold Crabby › Models*: a model saved
//!   there from Safari or AirDrop is in the library at the next scan, without a
//!   file picker in sight. Elsewhere it sits under the library directory.
//!
//! Every mutation is read–modify–write of the one index under a process-wide
//! lock, so two uploads landing at once on the server cannot drop each other's
//! entry.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use super::{ImportOutcome, Library, LibraryEntry, LibraryLocation, LibrarySettings, LocationKind};
use crate::config::config_dir;
use crate::scene::loader::MeshFormat;

/// Serialises every read–modify–write of an index in this process.
static LOCK: Mutex<()> = Mutex::new(());

/// A thumbnail larger than this is not a thumbnail.
const MAX_THUMBNAIL_BYTES: usize = 2 * 1024 * 1024;

/// How deep a folder scan descends. Deep enough for `Downloads/<site>/<model>/`,
/// shallow enough that pointing it at a home directory cannot walk the disk.
const MAX_SCAN_DEPTH: usize = 4;

/// Directory holding the index and thumbnails.
pub fn library_dir() -> PathBuf {
    config_dir().join("library")
}

/// Directory holding the library's own copies.
pub fn models_dir() -> PathBuf {
    #[cfg(target_os = "ios")]
    if let Some(home) = dirs::home_dir() {
        return home.join("Documents").join("Models");
    }
    library_dir().join("models")
}

/// What a scan found.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ScanReport {
    /// Model files looked at.
    pub scanned: usize,
    /// Files that were new or changed since the last scan.
    pub imported: usize,
    /// Entries dropped because their only file was deleted.
    pub removed: usize,
}

/// JSON-backed store for the [`Library`].
#[derive(Debug, Clone)]
pub struct LibraryStore {
    root: PathBuf,
    models: PathBuf,
}

impl Default for LibraryStore {
    fn default() -> Self {
        Self::new()
    }
}

impl LibraryStore {
    /// Store at the default locations ([`library_dir`], [`models_dir`]).
    pub fn new() -> Self {
        Self {
            root: library_dir(),
            models: models_dir(),
        }
    }

    /// Store rooted at an explicit directory, copies in `<root>/models`.
    pub fn at(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        Self {
            models: root.join("models"),
            root,
        }
    }

    /// Where copies are written — and where a user may drop files themselves.
    pub fn models_path(&self) -> &Path {
        &self.models
    }

    fn index_path(&self) -> PathBuf {
        self.root.join("index.json")
    }

    fn thumbnail_path(&self, id: &str) -> Option<PathBuf> {
        is_entry_id(id).then(|| self.root.join("thumbs").join(format!("{id}.png")))
    }

    fn read(&self) -> Result<Library> {
        let path = self.index_path();
        if !path.exists() {
            return Ok(Library::default());
        }
        let content = fs::read_to_string(&path)
            .with_context(|| format!("read library index '{}'", path.display()))?;
        // An index this build cannot parse reads as empty rather than failing
        // every screen that lists it; the next scan rebuilds what it can.
        Ok(serde_json::from_str(&content).unwrap_or_default())
    }

    fn write(&self, library: &Library) -> Result<()> {
        fs::create_dir_all(&self.root)
            .with_context(|| format!("create library dir '{}'", self.root.display()))?;
        let path = self.index_path();
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_string_pretty(library)?)
            .with_context(|| format!("write library index '{}'", tmp.display()))?;
        fs::rename(&tmp, &path).with_context(|| format!("replace '{}'", path.display()))?;
        Ok(())
    }

    fn update<R>(&self, change: impl FnOnce(&mut Library) -> Result<R>) -> Result<R> {
        let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut library = self.read()?;
        let result = change(&mut library)?;
        self.write(&library)?;
        Ok(result)
    }

    /// The library, with `missing` and `has_thumbnail` filled in from disk.
    pub fn load(&self) -> Result<Library> {
        let mut library = {
            let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
            self.read()?
        };
        for entry in &mut library.entries {
            for location in &mut entry.locations {
                location.missing = !Path::new(&location.path).is_file();
            }
            entry.has_thumbnail = self.thumbnail_path(&entry.id).is_some_and(|p| p.is_file());
        }
        Ok(library)
    }

    /// Replace the settings, keeping every entry.
    pub fn save_settings(&self, settings: LibrarySettings) -> Result<Library> {
        self.update(|library| {
            library.settings = settings;
            Ok(())
        })?;
        self.load()
    }

    /// Record a file the user has on disk.
    ///
    /// A file already inside [`models_path`](Self::models_path) is the
    /// library's own copy. Anything else is referenced when the mode says so
    /// and copied when it says so — see [`StorageMode`](super::StorageMode).
    pub fn import_path(&self, path: &Path) -> Result<ImportOutcome> {
        self.import_path_as(path, false)
    }

    /// `always_reference` keeps a reference whatever the mode: a watched folder
    /// is one by definition, and the recorded size and mtime are what let the
    /// next scan skip an unchanged file without reading it.
    fn import_path_as(&self, path: &Path, always_reference: bool) -> Result<ImportOutcome> {
        let bytes = fs::read(path).with_context(|| format!("read '{}'", path.display()))?;
        let file_name = file_name_of(path);
        let owned = path.starts_with(&self.models);
        let now = now();
        self.update(|library| {
            let mode = library.settings.mode;
            let location = (owned || always_reference || mode.references()).then(|| {
                located(
                    path,
                    if owned {
                        LocationKind::Copy
                    } else {
                        LocationKind::Reference
                    },
                )
            });
            let outcome = library
                .import(&file_name, &bytes, location, &now)
                .map_err(anyhow::Error::msg)?;
            if outcome.needs_copy && !owned {
                let copy = self.write_copy(&file_name, &bytes)?;
                library.add_location(&outcome.entry_id, copy);
            }
            Ok(outcome)
        })
    }

    /// Record a file that arrived as bytes — an upload, a drop in the browser.
    ///
    /// There is no path to reference, so a new object is always copied: an
    /// entry nothing can be read from would be a thumbnail of a model the user
    /// can no longer print.
    pub fn import_bytes(&self, file_name: &str, bytes: &[u8]) -> Result<ImportOutcome> {
        let now = now();
        self.update(|library| {
            let mut outcome = library
                .import(file_name, bytes, None, &now)
                .map_err(anyhow::Error::msg)?;
            let has_any = library
                .get(&outcome.entry_id)
                .is_some_and(|e| e.locations.iter().any(|l| Path::new(&l.path).is_file()));
            if outcome.needs_copy || !has_any {
                let copy = self.write_copy(file_name, bytes)?;
                library.add_location(&outcome.entry_id, copy);
                outcome.needs_copy = false;
            }
            Ok(outcome)
        })
    }

    fn write_copy(&self, file_name: &str, bytes: &[u8]) -> Result<LibraryLocation> {
        fs::create_dir_all(&self.models)
            .with_context(|| format!("create models dir '{}'", self.models.display()))?;
        let path = unique_path(&self.models, &sanitize(file_name));
        fs::write(&path, bytes).with_context(|| format!("write copy '{}'", path.display()))?;
        Ok(located(&path, LocationKind::Copy))
    }

    /// Walk the library's own folder and every watched folder.
    ///
    /// Unchanged files (same path, size and mtime) are skipped unread. An entry
    /// whose only files were the library's own copies, all now gone, is
    /// dropped: on an iPad that is the user deleting a model in Files. A
    /// missing *reference* is kept and shown as missing — a drive that is not
    /// plugged in has not deleted anything.
    pub fn scan(&self) -> Result<ScanReport> {
        let settings = self.load()?.settings;
        let mut roots = vec![(self.models.clone(), false)];
        roots.extend(settings.folders.iter().map(|f| (PathBuf::from(f), true)));

        let mut report = ScanReport::default();
        for (root, watched) in roots {
            let mut files = Vec::new();
            collect_models(&root, 0, &mut files);
            for path in files {
                report.scanned += 1;
                if self.is_unchanged(&path)? {
                    continue;
                }
                if self.import_path_as(&path, watched).is_ok() {
                    report.imported += 1;
                }
            }
        }

        report.removed = self.update(|library| {
            let before = library.entries.len();
            library.entries.retain(|entry| {
                entry.locations.is_empty()
                    || entry
                        .locations
                        .iter()
                        .any(|l| l.kind == LocationKind::Reference || Path::new(&l.path).is_file())
            });
            // Copies that are gone are dropped from the entries that remain.
            for entry in &mut library.entries {
                entry
                    .locations
                    .retain(|l| l.kind == LocationKind::Reference || Path::new(&l.path).is_file());
            }
            Ok(before - library.entries.len())
        })?;
        Ok(report)
    }

    fn is_unchanged(&self, path: &Path) -> Result<bool> {
        let current = located(path, LocationKind::Reference);
        let library = {
            let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
            self.read()?
        };
        let path = path.to_string_lossy();
        Ok(library.entries.iter().flat_map(|e| &e.locations).any(|l| {
            l.path == path
                && l.size.is_some()
                && l.size == current.size
                && l.modified == current.modified
        }))
    }

    /// Note that an entry was put on a plate.
    pub fn touch(&self, id: &str) -> Result<bool> {
        let now = now();
        self.update(|library| Ok(library.touch(id, &now)))
    }

    /// Rename an entry.
    pub fn rename(&self, id: &str, name: &str) -> Result<bool> {
        self.update(|library| Ok(library.rename(id, name)))
    }

    /// Forget an entry, deleting the library's own copies and its thumbnail.
    /// A referenced file is never touched.
    pub fn remove(&self, id: &str) -> Result<bool> {
        let removed = self.update(|library| Ok(library.remove(id)))?;
        let Some(entry) = removed else {
            return Ok(false);
        };
        for location in &entry.locations {
            let path = Path::new(&location.path);
            if location.kind == LocationKind::Copy && path.starts_with(&self.models) {
                let _ = fs::remove_file(path);
            }
        }
        if let Some(thumb) = self.thumbnail_path(id) {
            let _ = fs::remove_file(thumb);
        }
        Ok(true)
    }

    /// The first readable file for an entry, copies first.
    pub fn resolve(&self, id: &str) -> Result<Option<(LibraryEntry, PathBuf)>> {
        let library = self.load()?;
        Ok(library.get(id).and_then(|entry| {
            entry
                .locations
                .iter()
                .find(|l| !l.missing)
                .map(|l| (entry.clone(), PathBuf::from(&l.path)))
        }))
    }

    /// A stored thumbnail's bytes.
    pub fn thumbnail(&self, id: &str) -> Option<Vec<u8>> {
        fs::read(self.thumbnail_path(id)?).ok()
    }

    /// Store an entry's thumbnail — PNG bytes rendered by the webview.
    pub fn set_thumbnail(&self, id: &str, png: &[u8]) -> Result<()> {
        let Some(path) = self.thumbnail_path(id) else {
            bail!("invalid library id '{id}'");
        };
        if png.len() > MAX_THUMBNAIL_BYTES || !png.starts_with(b"\x89PNG\r\n\x1a\n") {
            bail!("a thumbnail must be a PNG under 2 MB");
        }
        if self.load()?.get(id).is_none() {
            bail!("no library entry '{id}'");
        }
        fs::create_dir_all(path.parent().expect("thumbs dir"))?;
        fs::write(&path, png).with_context(|| format!("write thumbnail '{}'", path.display()))?;
        Ok(())
    }
}

/// An entry id is a SHA-256 hex digest — never a path.
fn is_entry_id(id: &str) -> bool {
    id.len() == 64 && id.bytes().all(|b| b.is_ascii_hexdigit())
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn file_name_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "model.stl".into())
}

fn located(path: &Path, kind: LocationKind) -> LibraryLocation {
    let meta = fs::metadata(path).ok();
    LibraryLocation {
        kind,
        path: path.to_string_lossy().into_owned(),
        size: meta.as_ref().map(|m| m.len()),
        modified: meta
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64),
        missing: false,
    }
}

/// Keep a file name readable in Files and Finder, minus anything that could
/// name a directory.
fn sanitize(file_name: &str) -> String {
    let base = file_name.rsplit(['/', '\\']).next().unwrap_or(file_name);
    let clean: String = base
        .chars()
        .map(|c| match c {
            c if c.is_alphanumeric() => c,
            ' ' | '-' | '_' | '.' | '(' | ')' => c,
            _ => '_',
        })
        .collect();
    let clean = clean.trim_start_matches('.');
    if clean.is_empty() {
        "model.stl".into()
    } else {
        clean.to_string()
    }
}

/// `dir/name`, or `dir/name (2)` and upward when that is taken.
fn unique_path(dir: &Path, file_name: &str) -> PathBuf {
    let candidate = dir.join(file_name);
    if !candidate.exists() {
        return candidate;
    }
    let (stem, ext) = file_name.rsplit_once('.').unwrap_or((file_name, ""));
    (2..)
        .map(|n| dir.join(format!("{stem} ({n}).{ext}")))
        .find(|p| !p.exists())
        .expect("an unused name exists")
}

fn collect_models(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let hidden = path
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with('.'));
        if hidden {
            continue;
        }
        match entry.file_type() {
            Ok(t) if t.is_dir() && depth < MAX_SCAN_DEPTH => collect_models(&path, depth + 1, out),
            Ok(t) if t.is_file() && MeshFormat::from_path(&path).is_some() => out.push(path),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::{ImportMatch, StorageMode};

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("library-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    fn triangle_stl(size: f32) -> Vec<u8> {
        let mut out = vec![0u8; 80];
        out.extend_from_slice(&1u32.to_le_bytes());
        out.extend_from_slice(&[0u8; 12]);
        for v in [[0.0, 0.0, 0.0], [size, 0.0, 0.0], [0.0, size, 5.0]] {
            for c in v {
                out.extend_from_slice(&c.to_le_bytes());
            }
        }
        out.extend_from_slice(&[0u8; 2]);
        out
    }

    #[test]
    fn bytes_are_copied_once() {
        let root = temp_dir("bytes");
        let store = LibraryStore::at(&root);
        let first = store
            .import_bytes("tri.stl", &triangle_stl(10.0))
            .expect("import");
        let second = store
            .import_bytes("tri.stl", &triangle_stl(10.0))
            .expect("import");
        assert_eq!(first.entry_id, second.entry_id);
        assert_eq!(second.matched, ImportMatch::SameFile);
        assert_eq!(fs::read_dir(root.join("models")).unwrap().count(), 1);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn reference_mode_leaves_the_file_where_it_is() {
        let root = temp_dir("reference");
        let store = LibraryStore::at(root.join("lib"));
        store
            .save_settings(LibrarySettings {
                mode: StorageMode::Reference,
                folders: vec![],
            })
            .expect("settings");
        let original = root.join("part.stl");
        fs::write(&original, triangle_stl(10.0)).unwrap();

        let outcome = store.import_path(&original).expect("import");
        let (_, path) = store.resolve(&outcome.entry_id).unwrap().expect("resolves");
        assert_eq!(path, original);
        assert!(!root.join("lib/models").exists(), "nothing was copied");

        store.remove(&outcome.entry_id).expect("remove");
        assert!(original.exists(), "the user's file is never deleted");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_scan_picks_up_files_dropped_into_the_models_folder() {
        let root = temp_dir("scan");
        let store = LibraryStore::at(&root);
        fs::create_dir_all(root.join("models/sub")).unwrap();
        fs::write(root.join("models/sub/a.stl"), triangle_stl(10.0)).unwrap();
        fs::write(root.join("models/notes.txt"), b"not a model").unwrap();

        let report = store.scan().expect("scan");
        assert_eq!((report.scanned, report.imported), (1, 1));
        assert_eq!(
            store.scan().expect("rescan").imported,
            0,
            "unchanged files are skipped"
        );

        fs::remove_file(root.join("models/sub/a.stl")).unwrap();
        assert_eq!(store.scan().expect("scan").removed, 1);
        assert!(store.load().unwrap().entries.is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_missing_reference_is_kept_and_flagged() {
        let root = temp_dir("missing");
        let store = LibraryStore::at(root.join("lib"));
        let watched = root.join("downloads");
        fs::create_dir_all(&watched).unwrap();
        fs::write(watched.join("a.stl"), triangle_stl(10.0)).unwrap();
        store
            .save_settings(LibrarySettings {
                mode: StorageMode::Reference,
                folders: vec![watched.to_string_lossy().into_owned()],
            })
            .unwrap();
        store.scan().unwrap();
        fs::remove_file(watched.join("a.stl")).unwrap();
        assert_eq!(store.scan().unwrap().removed, 0);
        let library = store.load().unwrap();
        assert!(library.entries[0].locations[0].missing);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn thumbnails_must_be_pngs_for_real_entries() {
        let root = temp_dir("thumbs");
        let store = LibraryStore::at(&root);
        let id = store
            .import_bytes("tri.stl", &triangle_stl(10.0))
            .unwrap()
            .entry_id;
        assert!(store.set_thumbnail(&id, b"GIF89a").is_err());
        assert!(store
            .set_thumbnail("../../etc", b"\x89PNG\r\n\x1a\n")
            .is_err());
        store.set_thumbnail(&id, b"\x89PNG\r\n\x1a\nrest").unwrap();
        assert!(store.load().unwrap().entries[0].has_thumbnail);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn copies_keep_a_readable_unique_name() {
        let root = temp_dir("names");
        let store = LibraryStore::at(&root);
        store
            .import_bytes("../Benchy.stl", &triangle_stl(10.0))
            .unwrap();
        store
            .import_bytes("Benchy.stl", &triangle_stl(12.0))
            .unwrap();
        let mut names: Vec<_> = fs::read_dir(root.join("models"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, ["Benchy (2).stl", "Benchy.stl"]);
        let _ = fs::remove_dir_all(&root);
    }
}
