//! The object library — every model the user has ever put on a plate, once.
//!
//! A workplate names its models by `file_id`, and that id is only as durable as
//! the plate: close the plate and the model is something the user has to go and
//! find again. The library is the index that outlives plates. Every model that
//! reaches any plate, in any runtime, is recorded here with a thumbnail, so the
//! next plate can be built from what the user already has instead of from a
//! file picker.
//!
//! > **One object, one entry.** The same model dragged in twice, re-downloaded
//! > under another name, or re-exported as ASCII instead of binary STL is still
//! > one entry with several locations.
//!
//! ## Matching
//!
//! Two keys, tried in order:
//!
//! 1. **Content hash** — SHA-256 of the file's bytes. Exact, cheap, and what a
//!    re-download or a copy in another folder produces.
//! 2. **Shape digest** — a digest of what the file *describes*: part count,
//!    triangle count, bounding-box extents, volume and surface area, each
//!    rounded well below printing precision. It catches the same object saved
//!    by a different exporter, whose bytes share nothing. Extents rather than
//!    corners, so the same model exported at another origin still matches.
//!
//! The matching lives here, in [`Library::record`], and compiles to every
//! target — the browser build runs this exact code through [`wasm`] rather than
//! a TypeScript copy of it.
//!
//! ## Copies and references
//!
//! A location is either a **copy** the library owns or a **reference** to a
//! file the user owns. [`StorageMode`] decides which an import produces; which
//! modes a runtime may offer depends on whether it can reopen a path later —
//! see `src/library/README.md`.
//!
//! ## Non-goals
//!
//! - **No renderer.** The thumbnail is drawn in the webview, like the G-code
//!   thumbnail, and handed to the store as image bytes.
//! - **No deletion of the user's files.** Removing an entry deletes the
//!   library's own copy and thumbnail; a referenced original is never touched.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::scene::loader::{load_bytes_multi, MeshFormat};

#[cfg(not(target_arch = "wasm32"))]
pub mod store;
#[cfg(target_arch = "wasm32")]
pub mod wasm;

#[cfg(not(target_arch = "wasm32"))]
pub use store::{library_dir, models_dir, LibraryStore, ScanReport};

/// How an imported model is kept.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StorageMode {
    /// Copy the file into the library. Survives the original being moved or
    /// deleted; costs the disk.
    #[default]
    Copy,
    /// Remember where the file is. Costs nothing; the entry goes missing when
    /// the original does.
    Reference,
    /// Remember where it is *and* keep a copy to fall back on.
    Both,
}

impl StorageMode {
    /// Whether this mode keeps a copy of an imported file.
    pub fn copies(self) -> bool {
        matches!(self, Self::Copy | Self::Both)
    }

    /// Whether this mode remembers an imported file's own path.
    pub fn references(self) -> bool {
        matches!(self, Self::Reference | Self::Both)
    }
}

/// Whether a location is the library's own file or the user's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LocationKind {
    /// A copy inside the library folder. The library may delete it.
    Copy,
    /// The user's own file. The library never modifies or deletes it.
    Reference,
}

/// One place an entry's bytes can be read from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LibraryLocation {
    pub kind: LocationKind,
    /// Absolute path on the engine's filesystem. In the browser build, where
    /// there is no filesystem, the key the bytes are stored under instead.
    pub path: String,
    /// File size when last seen, so a rescan can skip an unchanged file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    /// Modification time (seconds since the epoch) when last seen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified: Option<i64>,
    /// Set when the file was not there at the last look. Not stored — computed
    /// whenever the library is read.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub missing: bool,
}

/// What a model file describes, independent of how it was encoded.
///
/// Rounded to four significant figures: an ASCII STL keeps about six, so a
/// re-export of the same geometry agrees, while two genuinely different models
/// that agree on all five numbers at once are not something a user's library
/// produces by accident.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ShapeKey {
    /// Objects in the file — 1 for STL/OBJ, one per build item for 3MF.
    pub parts: u32,
    /// Triangles after import-time repair, summed over every part.
    pub triangles: u64,
    /// Bounding-box size in mm, X/Y/Z, as the file places it.
    pub extents_mm: [f64; 3],
    /// Enclosed volume in mm³, summed over every part.
    pub volume_mm3: f64,
    /// Surface area in mm², summed over every part.
    pub area_mm2: f64,
}

impl ShapeKey {
    /// Measure every part of a model file.
    pub fn of_bytes(bytes: &[u8], format: MeshFormat) -> Result<Self, String> {
        let parts = load_bytes_multi(bytes, format)?;
        let mut triangles = 0u64;
        let mut volume = 0.0;
        let mut area = 0.0;
        let mut min = [f64::INFINITY; 3];
        let mut max = [f64::NEG_INFINITY; 3];
        for part in &parts {
            triangles += part.mesh.faces.len() as u64;
            volume += crate::mesh::analysis::calculate_volume(&part.mesh).unwrap_or(0.0);
            area += crate::mesh::analysis::calculate_surface_area(&part.mesh);
            for face in &part.mesh.faces {
                for v in &face.vertices {
                    for (axis, value) in [v.x, v.y, v.z].into_iter().enumerate() {
                        min[axis] = min[axis].min(value);
                        max[axis] = max[axis].max(value);
                    }
                }
            }
        }
        if triangles == 0 {
            return Err("model has no triangles".into());
        }
        Ok(Self {
            parts: parts.len() as u32,
            triangles,
            extents_mm: std::array::from_fn(|axis| significant(max[axis] - min[axis])),
            volume_mm3: significant(volume),
            area_mm2: significant(area),
        })
    }

    /// Short, stable digest of the key — what entries are compared on.
    pub fn digest(&self) -> String {
        let canonical = format!(
            "{}|{}|{}|{}|{}|{}|{}",
            self.parts,
            self.triangles,
            self.extents_mm[0],
            self.extents_mm[1],
            self.extents_mm[2],
            self.volume_mm3,
            self.area_mm2
        );
        hex(&Sha256::digest(canonical.as_bytes()))[..16].to_string()
    }
}

/// Round to four significant figures.
fn significant(value: f64) -> f64 {
    if value == 0.0 || !value.is_finite() {
        return 0.0;
    }
    let magnitude = value.abs().log10().floor() as i32;
    let scale = 10f64.powi(3 - magnitude);
    (value * scale).round() / scale
}

/// SHA-256 of a file's bytes, lowercase hex. The library's exact-match key.
pub fn content_hash(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// One object in the library.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LibraryEntry {
    /// Stable id: the content hash of the first file seen for this object.
    pub id: String,
    /// Display name — the first file's name without its extension.
    pub name: String,
    /// `stl`, `obj` or `3mf`.
    pub format: String,
    /// Size of the first file seen, in bytes.
    pub size: u64,
    /// Every content hash known to be this object.
    #[serde(default)]
    pub hashes: Vec<String>,
    /// What the file describes, when it could be measured.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shape: Option<ShapeKey>,
    /// [`ShapeKey::digest`], stored so matching needs no re-measure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shape_digest: Option<String>,
    /// Everywhere this object's bytes can be read from, copies first.
    #[serde(default)]
    pub locations: Vec<LibraryLocation>,
    /// RFC 3339.
    pub added_at: String,
    /// RFC 3339 — the last time it was put on a plate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<String>,
    /// How many times it has been put on a plate.
    #[serde(default)]
    pub use_count: u32,
    /// Whether a thumbnail is stored. Computed on read, like `missing`.
    #[serde(default)]
    pub has_thumbnail: bool,
}

impl LibraryEntry {
    /// Whether any location is still readable.
    pub fn is_available(&self) -> bool {
        self.locations.iter().any(|l| !l.missing)
    }

    /// Whether the library holds its own copy of this object.
    pub fn has_copy(&self) -> bool {
        self.locations
            .iter()
            .any(|l| l.kind == LocationKind::Copy && !l.missing)
    }
}

/// The user's choices about how the library keeps things.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LibrarySettings {
    /// What an import keeps. Clamped to what the runtime can honour.
    #[serde(default)]
    pub mode: StorageMode,
    /// Folders scanned for models, in addition to the library's own.
    #[serde(default)]
    pub folders: Vec<String>,
}

/// The whole library document.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Library {
    #[serde(default)]
    pub settings: LibrarySettings,
    /// Most recently added first.
    #[serde(default)]
    pub entries: Vec<LibraryEntry>,
}

/// A file about to be recorded.
#[derive(Debug, Clone)]
pub struct ImportCandidate {
    /// File name, with extension.
    pub file_name: String,
    pub format: MeshFormat,
    pub size: u64,
    /// [`content_hash`] of the bytes.
    pub hash: String,
    /// `None` when the file could not be measured — it can still be matched by
    /// hash, and still be recorded.
    pub shape: Option<ShapeKey>,
    /// Where the bytes live, when the caller already has somewhere.
    pub location: Option<LibraryLocation>,
}

/// How [`Library::record`] matched a candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ImportMatch {
    /// Nothing like it was in the library.
    New,
    /// The same bytes were already there.
    SameFile,
    /// Different bytes describing the same object.
    SameShape,
}

/// The result of recording a file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ImportOutcome {
    /// The entry the file now belongs to.
    pub entry_id: String,
    #[serde(rename = "match")]
    pub matched: ImportMatch,
    /// Whether the caller should store a copy — the mode asks for one and the
    /// entry does not yet have one.
    pub needs_copy: bool,
}

impl Library {
    /// Look an entry up by id.
    pub fn get(&self, id: &str) -> Option<&LibraryEntry> {
        self.entries.iter().find(|e| e.id == id)
    }

    fn get_mut(&mut self, id: &str) -> Option<&mut LibraryEntry> {
        self.entries.iter_mut().find(|e| e.id == id)
    }

    /// Which entry, if any, a file with this hash and shape already is.
    pub fn find_match(&self, hash: &str, shape: Option<&ShapeKey>) -> Option<(&str, ImportMatch)> {
        if let Some(entry) = self
            .entries
            .iter()
            .find(|e| e.hashes.iter().any(|h| h == hash))
        {
            return Some((&entry.id, ImportMatch::SameFile));
        }
        let digest = shape.map(ShapeKey::digest)?;
        self.entries
            .iter()
            .find(|e| e.shape_digest.as_deref() == Some(digest.as_str()))
            .map(|e| (e.id.as_str(), ImportMatch::SameShape))
    }

    /// Record a file: fold it into the entry it matches, or add a new one.
    ///
    /// A matching file adds its hash and location to the existing entry and
    /// changes nothing else — the entry keeps its first name and its first
    /// copy, so one object is never stored twice.
    pub fn record(&mut self, candidate: ImportCandidate, now: &str) -> ImportOutcome {
        let mode = self.settings.mode;
        let found = self
            .find_match(&candidate.hash, candidate.shape.as_ref())
            .map(|(id, m)| (id.to_string(), m));

        let (entry_id, matched) = match found {
            Some((id, matched)) => {
                let entry = self.get_mut(&id).expect("matched entry exists");
                if !entry.hashes.contains(&candidate.hash) {
                    entry.hashes.push(candidate.hash);
                }
                if entry.shape.is_none() {
                    entry.shape_digest = candidate.shape.as_ref().map(ShapeKey::digest);
                    entry.shape = candidate.shape;
                }
                if let Some(location) = candidate.location {
                    add_location(entry, location);
                }
                (id, matched)
            }
            None => {
                let mut entry = LibraryEntry {
                    id: candidate.hash.clone(),
                    name: display_name(&candidate.file_name),
                    format: candidate.format.as_str().to_string(),
                    size: candidate.size,
                    hashes: vec![candidate.hash],
                    shape_digest: candidate.shape.as_ref().map(ShapeKey::digest),
                    shape: candidate.shape,
                    locations: Vec::new(),
                    added_at: now.to_string(),
                    last_used_at: None,
                    use_count: 0,
                    has_thumbnail: false,
                };
                if let Some(location) = candidate.location {
                    add_location(&mut entry, location);
                }
                let id = entry.id.clone();
                self.entries.insert(0, entry);
                (id, ImportMatch::New)
            }
        };

        let needs_copy = mode.copies() && !self.get(&entry_id).is_some_and(LibraryEntry::has_copy);
        ImportOutcome {
            entry_id,
            matched,
            needs_copy,
        }
    }

    /// Attach a location to an entry (a copy just written, a path learned).
    pub fn add_location(&mut self, id: &str, location: LibraryLocation) {
        if let Some(entry) = self.get_mut(id) {
            add_location(entry, location);
        }
    }

    /// Note that an entry was put on a plate.
    pub fn touch(&mut self, id: &str, now: &str) -> bool {
        match self.get_mut(id) {
            Some(entry) => {
                entry.use_count = entry.use_count.saturating_add(1);
                entry.last_used_at = Some(now.to_string());
                true
            }
            None => false,
        }
    }

    /// Rename an entry. The files keep their names.
    pub fn rename(&mut self, id: &str, name: &str) -> bool {
        let name = name.trim();
        match self.get_mut(id) {
            Some(entry) if !name.is_empty() => {
                entry.name = name.to_string();
                true
            }
            _ => false,
        }
    }

    /// Drop an entry, returning it so the caller can remove its own files.
    pub fn remove(&mut self, id: &str) -> Option<LibraryEntry> {
        let index = self.entries.iter().position(|e| e.id == id)?;
        Some(self.entries.remove(index))
    }
}

/// Add a location unless the path is already known; copies sort first, so the
/// file the library owns is the one read when both are there.
fn add_location(entry: &mut LibraryEntry, location: LibraryLocation) {
    if let Some(existing) = entry.locations.iter_mut().find(|l| l.path == location.path) {
        *existing = location;
    } else {
        entry.locations.push(location);
    }
    entry
        .locations
        .sort_by_key(|l| matches!(l.kind, LocationKind::Reference));
}

/// A file name without its directory or extension.
pub fn display_name(file_name: &str) -> String {
    let base = file_name.rsplit(['/', '\\']).next().unwrap_or(file_name);
    let stem = match base.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem,
        _ => base,
    };
    if stem.is_empty() {
        "model".into()
    } else {
        stem.to_string()
    }
}

impl Library {
    /// Record a file's bytes: hash them, measure them only if the hash is new,
    /// and fold the result in with [`Library::record`].
    ///
    /// Measuring means parsing the whole model, so a file the library has
    /// already seen byte-for-byte skips it — which is every file on a rescan.
    /// A file the loader rejects is still recorded, matched by hash alone.
    pub fn import(
        &mut self,
        file_name: &str,
        bytes: &[u8],
        location: Option<LibraryLocation>,
        now: &str,
    ) -> Result<ImportOutcome, String> {
        let format = file_name
            .rsplit_once('.')
            .and_then(|(_, ext)| MeshFormat::from_extension(ext))
            .ok_or_else(|| format!("'{file_name}' is not an STL, OBJ or 3MF file"))?;
        let hash = content_hash(bytes);
        let shape = match self.find_match(&hash, None) {
            Some(_) => None,
            None => ShapeKey::of_bytes(bytes, format).ok(),
        };
        Ok(self.record(
            ImportCandidate {
                file_name: file_name.to_string(),
                format,
                size: bytes.len() as u64,
                hash,
                shape,
                location,
            },
            now,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cube_stl(size: f64, ascii: bool) -> Vec<u8> {
        let s = size;
        let v = [
            [0.0, 0.0, 0.0],
            [s, 0.0, 0.0],
            [s, s, 0.0],
            [0.0, s, 0.0],
            [0.0, 0.0, s],
            [s, 0.0, s],
            [s, s, s],
            [0.0, s, s],
        ];
        let tris = [
            [0, 2, 1],
            [0, 3, 2],
            [4, 5, 6],
            [4, 6, 7],
            [0, 1, 5],
            [0, 5, 4],
            [1, 2, 6],
            [1, 6, 5],
            [2, 3, 7],
            [2, 7, 6],
            [3, 0, 4],
            [3, 4, 7],
        ];
        if ascii {
            let mut out = String::from("solid cube\n");
            for t in tris {
                out.push_str(" facet normal 0 0 0\n  outer loop\n");
                for i in t {
                    let p = v[i];
                    out.push_str(&format!("   vertex {} {} {}\n", p[0], p[1], p[2]));
                }
                out.push_str("  endloop\n endfacet\n");
            }
            out.push_str("endsolid cube\n");
            out.into_bytes()
        } else {
            let mut out = vec![0u8; 80];
            out.extend_from_slice(&(tris.len() as u32).to_le_bytes());
            for t in tris {
                out.extend_from_slice(&[0u8; 12]);
                for i in t {
                    for c in v[i] {
                        out.extend_from_slice(&(c as f32).to_le_bytes());
                    }
                }
                out.extend_from_slice(&[0u8; 2]);
            }
            out
        }
    }

    fn record(library: &mut Library, name: &str, bytes: &[u8]) -> ImportOutcome {
        library
            .import(name, bytes, None, "2026-09-25T00:00:00Z")
            .expect("supported")
    }

    #[test]
    fn the_same_bytes_are_one_entry() {
        let mut library = Library::default();
        let cube = cube_stl(10.0, false);
        assert_eq!(
            record(&mut library, "cube.stl", &cube).matched,
            ImportMatch::New
        );
        let again = record(&mut library, "cube (1).stl", &cube);
        assert_eq!(again.matched, ImportMatch::SameFile);
        assert_eq!(library.entries.len(), 1);
        assert_eq!(library.entries[0].name, "cube", "the first name is kept");
    }

    /// An ASCII and a binary STL of one cube share no bytes, and are one object.
    #[test]
    fn a_re_export_matches_by_shape() {
        let mut library = Library::default();
        record(&mut library, "cube.stl", &cube_stl(10.0, false));
        let ascii = record(&mut library, "cube-ascii.stl", &cube_stl(10.0, true));
        assert_eq!(ascii.matched, ImportMatch::SameShape);
        assert_eq!(library.entries.len(), 1);
        assert_eq!(library.entries[0].hashes.len(), 2);
    }

    #[test]
    fn a_different_object_is_a_new_entry() {
        let mut library = Library::default();
        record(&mut library, "cube.stl", &cube_stl(10.0, false));
        let bigger = record(&mut library, "cube.stl", &cube_stl(12.0, false));
        assert_eq!(bigger.matched, ImportMatch::New);
        assert_eq!(library.entries.len(), 2);
    }

    #[test]
    fn a_copy_is_asked_for_once() {
        let mut library = Library::default();
        let cube = cube_stl(10.0, false);
        assert!(record(&mut library, "cube.stl", &cube).needs_copy);
        let id = library.entries[0].id.clone();
        library.add_location(
            &id,
            LibraryLocation {
                kind: LocationKind::Copy,
                path: "/lib/cube.stl".into(),
                size: None,
                modified: None,
                missing: false,
            },
        );
        assert!(!record(&mut library, "cube.stl", &cube).needs_copy);
    }

    #[test]
    fn reference_mode_never_asks_for_a_copy() {
        let mut library = Library::default();
        library.settings.mode = StorageMode::Reference;
        assert!(!record(&mut library, "cube.stl", &cube_stl(10.0, false)).needs_copy);
    }

    #[test]
    fn copies_are_read_before_references() {
        let mut library = Library::default();
        record(&mut library, "cube.stl", &cube_stl(10.0, false));
        let id = library.entries[0].id.clone();
        for (kind, path) in [
            (LocationKind::Reference, "/home/me/cube.stl"),
            (LocationKind::Copy, "/lib/cube.stl"),
        ] {
            library.add_location(
                &id,
                LibraryLocation {
                    kind,
                    path: path.into(),
                    size: None,
                    modified: None,
                    missing: false,
                },
            );
        }
        assert_eq!(library.entries[0].locations[0].kind, LocationKind::Copy);
    }

    #[test]
    fn an_unsupported_file_is_refused() {
        assert!(Library::default()
            .import("notes.txt", b"hi", None, "now")
            .is_err());
    }

    #[test]
    fn display_names_drop_directories_and_extensions() {
        assert_eq!(display_name("/a/b/Benchy.3mf"), "Benchy");
        assert_eq!(display_name("C:\\x\\part.v2.stl"), "part.v2");
        assert_eq!(display_name(".stl"), ".stl");
    }

    #[test]
    fn significant_figures_absorb_ascii_rounding() {
        assert_eq!(significant(10.000_001), significant(9.999_999));
        assert_ne!(significant(10.0), significant(10.1));
    }
}
