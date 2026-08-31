use crate::bridge::tauri_logger::TauriAppLogger;
use serde_json::{json, Value};
use slicer_engine::core::ObjectInput;
use slicer_engine::logging::ProcessLogger;
use slicer_engine::scene::loader::MeshFormat;
use slicer_engine::scene::transform::Transform;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::Manager;

#[derive(Debug, Clone, serde::Serialize)]
pub struct HistorySession {
    pub request_uuid: String,
    pub created_at: String,
    pub original_filename: Option<String>,
    pub layer_count: Option<i32>,
    pub download_url: String,
}

#[derive(Debug, serde::Deserialize)]
struct SliceStartPayload {
    slice_id: Option<String>,
    settings: Value,
    /// Filesystem path to the model. Rust reads the file directly,
    /// avoiding any byte arrays crossing the IPC boundary.
    file_path: Option<String>,
    scene: Option<SceneSnapshotPayload>,
}

#[derive(Debug, Default, serde::Deserialize)]
struct SceneSnapshotPayload {
    #[serde(default)]
    objects: Vec<SceneObjectPayload>,
}

#[derive(Debug, serde::Deserialize)]
struct SceneObjectPayload {
    #[serde(default)]
    translation: Option<[f32; 3]>,
    #[serde(default)]
    euler_xyz_deg: Option<[f32; 3]>,
    #[serde(default)]
    scale: Option<[f32; 3]>,
    /// Which object inside the source file this one is (0 for single-part
    /// files).
    ///
    /// A 3MF is a scene: its build items become separate plate objects that
    /// all share one file, so the path alone does not say which geometry to
    /// slice.
    #[serde(default)]
    source_part: Option<usize>,
    /// The file this object was loaded from.
    ///
    /// A plate can hold several *different* models, so the file is a property
    /// of the object, not of the request. Absent for a client that has not
    /// resolved one, in which case the request-level `file_path` is used.
    #[serde(default)]
    file_path: Option<String>,
}

impl SceneObjectPayload {
    /// The object's placement on the plate.
    fn transform(&self) -> Transform {
        Transform::from_euler_xyz_deg(
            self.translation.unwrap_or([0.0, 0.0, 0.0]),
            self.euler_xyz_deg.unwrap_or([0.0, 0.0, 0.0]),
            self.scale.unwrap_or([1.0, 1.0, 1.0]),
        )
    }

    /// Index of this object's geometry within its source file.
    fn part_index(&self) -> usize {
        self.source_part.unwrap_or(0)
    }
}

// Managed application state

/// A previously-generated slice keyed by content hash, so an identical scene +
/// settings can skip the pipeline entirely (mirrors the cloud server's
/// `gcode_cache`).
#[derive(Clone)]
struct CachedSlice {
    gcode_path: String,
    layer_count: usize,
}

/// Shared state managed by Tauri across all commands.
pub struct AppState {
    /// Path of the most recently generated GCode file on disk.
    pub last_gcode_path: Arc<Mutex<Option<String>>>,
    /// Map from slice_id → GCode file path on disk (never inline strings).
    pub gcode_path_by_slice: Arc<Mutex<HashMap<String, String>>>,
    pub history_sessions: Arc<Mutex<Vec<HistorySession>>>,
    pub cancel_flag: Arc<AtomicBool>,
    /// Content hash → cached slice result. Lets a re-slice of an identical
    /// scene + settings reuse the stored G-code instead of re-running the
    /// pipeline, matching the cloud server's skip-on-cache-hit behaviour.
    gcode_cache: Arc<Mutex<HashMap<String, CachedSlice>>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            last_gcode_path: Arc::new(Mutex::new(None)),
            gcode_path_by_slice: Arc::new(Mutex::new(HashMap::new())),
            history_sessions: Arc::new(Mutex::new(Vec::new())),
            cancel_flag: Arc::new(AtomicBool::new(false)),
            gcode_cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

// Command implementations

pub fn runtime_init(state: &AppState) -> Result<Value, String> {
    *state.last_gcode_path.lock().map_err(|e| e.to_string())? = None;
    state.cancel_flag.store(false, Ordering::SeqCst);
    Ok(json!({ "ok": true }))
}

pub async fn slice_start(
    app: tauri::AppHandle,
    state: &AppState,
    payload: Value,
) -> Result<Value, String> {
    let cancel_flag = state.cancel_flag.clone();
    cancel_flag.store(false, Ordering::SeqCst);
    let last_gcode_path = Arc::clone(&state.last_gcode_path);
    let gcode_path_by_slice = Arc::clone(&state.gcode_path_by_slice);
    let history_sessions = Arc::clone(&state.history_sessions);
    let gcode_cache = Arc::clone(&state.gcode_cache);

    tauri::async_runtime::spawn_blocking(move || {
        let payload: SliceStartPayload =
            serde_json::from_value(payload).map_err(|e| format!("invalid slice payload: {e}"))?;

        let slice_id = payload.slice_id.unwrap_or_else(|| "unknown".to_string());
        let logger = TauriAppLogger::new(app.clone(), cancel_flag.clone());
        logger.log_info(&format!("slice_id={slice_id}"));

        // Resolve the model path up front. Rust reads the file directly so that
        // no bytes cross the IPC boundary.
        // The request-level path is a fallback now that each scene object names
        // its own file — a plate can hold several different models.
        let file_path = payload.file_path.clone();
        // Name the download after whichever file the plate's first object came
        // from, falling back to the request-level one.
        let original_filename = payload
            .scene
            .as_ref()
            .and_then(|scene| scene.objects.iter().find_map(|o| o.file_path.as_deref()))
            .or(file_path.as_deref())
            .map(file_name_of);

        let params: slicer_engine::settings::params::SlicingParams =
            serde_json::from_value(payload.settings)
                .map_err(|e| format!("invalid settings: {e}"))?;

        // Cache lookup: an identical scene + settings (+ engine version + source
        // file identity) sliced before can reuse the stored G-code and skip the
        // whole pipeline — including the mesh parse — exactly like the cloud
        // server's `gcode_cache`. This is what keeps repeated desktop slices as
        // fast as cloud.
        let cache_key = compute_slice_cache_key(&params, file_path.as_deref(), &payload.scene);
        if let Some(hit) = gcode_cache
            .lock()
            .ok()
            .and_then(|cache| cache.get(&cache_key).cloned())
        {
            if std::path::Path::new(&hit.gcode_path).exists() {
                logger.log_info("cache hit: reusing previously-sliced G-code");
                register_slice_result(
                    &slice_id,
                    &hit.gcode_path,
                    hit.layer_count,
                    original_filename.clone(),
                    &last_gcode_path,
                    &gcode_path_by_slice,
                    &history_sessions,
                );
                return Ok(json!({
                    "ok": true,
                    "sliceId": slice_id,
                    "layer_count": hit.layer_count,
                    "gcode_path": hit.gcode_path,
                    "cached": true,
                }));
            }
            // Stale entry (file cleaned up) — drop it and re-slice.
            if let Ok(mut cache) = gcode_cache.lock() {
                cache.remove(&cache_key);
            }
        }

        let plate_objects =
            load_plate_objects(file_path.as_deref(), payload.scene.as_ref(), &logger)?;

        if plate_objects.iter().all(|o| o.mesh.faces.is_empty()) {
            return Err("combined scene has no triangles; nothing to slice".to_string());
        }

        let total_faces: usize = plate_objects.iter().map(|o| o.mesh.faces.len()).sum();
        logger.log_info(&format!(
            "slicing {} object(s), {total_faces} faces\u{2026}",
            plate_objects.len()
        ));
        // The objects stay apart so the desktop honours exclude-object and
        // sequential printing exactly like the CLI and the server; `slice_plate`
        // merges them itself when the settings do not need per-object identity.
        let plate = slicer_engine::core::slice_plate(&plate_objects, &params, &logger);
        logger.log_info(&format!("{} layers produced", plate.layers.len()));

        if cancel_flag.load(Ordering::SeqCst) {
            return Err("Slice cancelled by user".to_string());
        }

        let gcode = slicer_engine::gcode::generate_gcode_for_plate(&plate, &params);
        let layer_count = plate.layers.len();
        logger.log_info(&format!("GCode generated ({} chars)", gcode.len()));

        // Write GCode to the app cache directory. This avoids returning a
        // potentially 50 MB string through the IPC channel. The TS side
        // receives only the file path and converts it to an asset:// URL via
        // convertFileSrc(), which is served directly by the OS URI scheme
        // handler without touching the IPC channel at all.
        let cache_dir = app.path().app_cache_dir().map_err(|e| e.to_string())?;
        std::fs::create_dir_all(&cache_dir).map_err(|e| e.to_string())?;
        let gcode_file = cache_dir.join(format!("{slice_id}.gcode"));
        std::fs::write(&gcode_file, &gcode).map_err(|e| e.to_string())?;
        let gcode_path = gcode_file.to_string_lossy().to_string();
        logger.log_debug(&format!("GCode written to: {gcode_path}"));

        register_slice_result(
            &slice_id,
            &gcode_path,
            layer_count,
            original_filename.clone(),
            &last_gcode_path,
            &gcode_path_by_slice,
            &history_sessions,
        );

        // Remember this result so an identical re-slice skips the pipeline.
        if let Ok(mut cache) = gcode_cache.lock() {
            cache.insert(
                cache_key,
                CachedSlice {
                    gcode_path: gcode_path.clone(),
                    layer_count,
                },
            );
        }

        Ok(json!({
            "ok": true,
            "sliceId": slice_id,
            "layer_count": layer_count,
            // File path on disk; TS converts to asset:// URL via convertFileSrc.
            "gcode_path": gcode_path,
        }))
    })
    .await
    .map_err(|e| e.to_string())?
}

pub fn slice_cancel(state: &AppState) -> Result<Value, String> {
    state.cancel_flag.store(true, Ordering::SeqCst);
    Ok(json!({ "ok": true }))
}

/// Register a finished (or cache-reused) slice in the shared state: mark it the
/// most-recent result, map its `slice_id` to the G-code path, and prepend a
/// history entry. Shared by the fresh-slice and cache-hit paths so both record
/// identical bookkeeping.
#[allow(clippy::too_many_arguments)]
fn register_slice_result(
    slice_id: &str,
    gcode_path: &str,
    layer_count: usize,
    original_filename: Option<String>,
    last_gcode_path: &Arc<Mutex<Option<String>>>,
    gcode_path_by_slice: &Arc<Mutex<HashMap<String, String>>>,
    history_sessions: &Arc<Mutex<Vec<HistorySession>>>,
) {
    if let Ok(mut guard) = last_gcode_path.lock() {
        *guard = Some(gcode_path.to_string());
    }
    if let Ok(mut guard) = gcode_path_by_slice.lock() {
        guard.insert(slice_id.to_string(), gcode_path.to_string());
    }
    if let Ok(mut guard) = history_sessions.lock() {
        guard.insert(
            0,
            HistorySession {
                request_uuid: slice_id.to_string(),
                created_at: chrono::Utc::now().to_rfc3339(),
                original_filename,
                layer_count: Some(layer_count as i32),
                download_url: String::new(),
            },
        );
    }
}

/// Hash `settings + scene + engine version + source-file identity` into a stable
/// cache key. Mirrors the cloud server's `compute_slice_cache_key`
/// ([src/server/ws_session.rs]) so the two runtimes cache on the same inputs;
/// a file's length + mtime stand in for the server's content-addressed upload
/// token, so editing a source model on disk busts the entry.
///
/// **Every file the plate references is fingerprinted**, not just one: a plate
/// can hold several different models, and hashing only the first would let two
/// plates that differ in their *second* model collide on one cached G-code.
///
/// The params are fingerprinted via `SlicingParams::cache_fingerprint`, which
/// omits the ephemeral, camera-derived thumbnail PNG payload — so a fresh
/// render's bytes never bust the cache.
fn compute_slice_cache_key(
    params: &slicer_engine::settings::params::SlicingParams,
    file_path: Option<&str>,
    scene: &Option<SceneSnapshotPayload>,
) -> String {
    let mut canonical = String::new();
    canonical.push_str("v=");
    canonical.push_str(slicer_engine::version::VERSION);
    canonical.push_str(";params=");
    canonical.push_str(&params.cache_fingerprint());

    canonical.push_str(";files=");
    let mut seen: Vec<&str> = Vec::new();
    let paths = scene
        .iter()
        .flat_map(|s| s.objects.iter())
        .filter_map(|o| o.file_path.as_deref())
        .chain(file_path);
    for path in paths {
        if seen.contains(&path) {
            continue;
        }
        seen.push(path);
        canonical.push_str(path);
        if let Ok(meta) = std::fs::metadata(path) {
            canonical.push_str(&format!("|len={}", meta.len()));
            if let Ok(mtime) = meta.modified() {
                if let Ok(dur) = mtime.duration_since(std::time::UNIX_EPOCH) {
                    canonical.push_str(&format!("|mtime={}", dur.as_nanos()));
                }
            }
        }
        canonical.push(';');
    }

    canonical.push_str(";scene=");
    if let Some(scene) = scene {
        for obj in &scene.objects {
            // The **effective** file is part of the identity, not the raw
            // optional one: an object with no file of its own resolves to the
            // request-level path, so two plates whose fallbacks differ would
            // otherwise hash identically while loading different geometry.
            // `source_part` matters for the same reason — two objects can share
            // a file yet be different parts of it.
            canonical.push_str(&format!(
                "[{}#{}|{:?}|{:?}|{:?}]",
                obj.file_path.as_deref().or(file_path).unwrap_or(""),
                obj.part_index(),
                obj.translation,
                obj.euler_xyz_deg,
                obj.scale
            ));
        }
    }

    format!("{:016x}", fnv1a_64(canonical.as_bytes()))
}

/// FNV-1a 64-bit hash — deterministic across runs and platforms (unlike
/// `std::hash::DefaultHasher`, whose output is not stability-guaranteed).
fn fnv1a_64(bytes: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = OFFSET;
    for &b in bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

pub fn preview_get_source(state: &AppState, payload: Option<Value>) -> Result<Value, String> {
    let slice_id = payload.as_ref().and_then(|value| {
        value["sliceId"]
            .as_str()
            .or_else(|| value["slice_id"].as_str())
    });

    if let Some(slice_id) = slice_id {
        let path = state
            .gcode_path_by_slice
            .lock()
            .map_err(|e| e.to_string())?
            .get(slice_id)
            .cloned();
        if let Some(path) = path {
            return Ok(json!({ "ok": true, "kind": "gcode-path", "path": path }));
        }
    }

    let guard = state.last_gcode_path.lock().map_err(|e| e.to_string())?;
    match guard.as_ref() {
        Some(path) => Ok(json!({ "ok": true, "kind": "gcode-path", "path": path })),
        None => Ok(json!({ "ok": true, "kind": "none" })),
    }
}

pub fn history_list(state: &AppState) -> Result<Value, String> {
    let sessions = state
        .history_sessions
        .lock()
        .map_err(|e| e.to_string())?
        .clone();
    Ok(json!({ "ok": true, "sessions": sessions }))
}

/// Drop the desktop app's in-memory slice history. Backs the settings Danger
/// Zone "Clear slice history" action; the native runtime keeps its history in
/// `AppState`, so there is nothing on disk to remove.
pub fn history_clear(state: &AppState) -> Result<Value, String> {
    state
        .history_sessions
        .lock()
        .map_err(|e| e.to_string())?
        .clear();
    Ok(json!({ "ok": true }))
}

/// Load every model the plate references and place each object on it.
///
/// Reading happens in the Rust process, so the bytes never cross the IPC
/// boundary.
///
/// Two properties make this reproduce the plate the user arranged, and both
/// were once missing:
///
/// - **Each object names its own file.** A workplate is a build plate, not a
///   file: it can hold several different models. Slicing them all out of one
///   path prints the first model as many times as there are objects.
/// - **Multi-part files stay apart.** A 3MF is a scene, not a model, and each
///   build item's transform is already baked into its vertices — so a *merged*
///   load hands back the file exactly as its author assembled it: parts
///   stacked, geometry floating above the bed.
///
/// Every distinct file is read and repaired **once**, however many plate
/// objects it backs, and the objects are returned separately so `slice_plate`
/// can honour exclude-object and sequential printing.
fn load_plate_objects(
    fallback_path: Option<&str>,
    scene: Option<&SceneSnapshotPayload>,
    logger: &dyn ProcessLogger,
) -> Result<Vec<ObjectInput>, String> {
    // (path, part index, transform) — one entry per object on the plate.
    let placements: Vec<(String, usize, Transform)> = match scene {
        Some(scene) if !scene.objects.is_empty() => {
            let mut placements = Vec::with_capacity(scene.objects.len());
            for object in &scene.objects {
                let path = object
                    .file_path
                    .as_deref()
                    .or(fallback_path)
                    .ok_or_else(|| {
                        "slice requires a file_path, either per object or for the request"
                            .to_string()
                    })?
                    .to_string();
                placements.push((path, object.part_index(), object.transform()));
            }
            placements
        }
        // Without a scene there is nothing placing the parts, so print the file
        // as authored: every part, untransformed.
        _ => {
            let path = fallback_path
                .ok_or_else(|| "slice requires a file_path".to_string())?
                .to_string();
            let count = load_parts(&path, logger, &mut HashMap::new())?;
            (0..count)
                .map(|index| (path.clone(), index, Transform::IDENTITY))
                .collect()
        }
    };

    let mut cache: HashMap<String, Vec<slicer_engine::scene::LoadedPart>> = HashMap::new();
    let mut objects = Vec::with_capacity(placements.len());
    for (path, part_index, transform) in placements {
        load_parts(&path, logger, &mut cache)?;
        let parts = &cache[&path];
        let file_name = file_name_of(&path);
        let part = parts.get(part_index).ok_or_else(|| {
            format!(
                "{file_name} has no object at index {part_index} (it contains {})",
                parts.len()
            )
        })?;
        // Name the object after the part inside its file, falling back to the
        // file stem — this is what the firmware's cancel UI shows.
        let name = part
            .name
            .as_deref()
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .map(str::to_string)
            .or_else(|| {
                std::path::Path::new(&path)
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
            })
            .unwrap_or_else(|| format!("object_{}", objects.len()));
        // Bake the per-object transform exactly once, at the slicer boundary —
        // see the SSOT contract in src/scene/README.md.
        objects.push(ObjectInput::new(
            name,
            slicer_engine::scene::apply_transform(&part.mesh, &transform),
        ));
    }

    Ok(objects)
}

/// Read and repair a model's parts, memoised by path.
///
/// A file backing several plate objects — a multi-part 3MF, or a model
/// duplicated across the plate — is parsed once, not once per object. Returns
/// how many parts it holds.
fn load_parts(
    path: &str,
    logger: &dyn ProcessLogger,
    cache: &mut HashMap<String, Vec<slicer_engine::scene::LoadedPart>>,
) -> Result<usize, String> {
    if let Some(parts) = cache.get(path) {
        return Ok(parts.len());
    }

    let file = std::path::Path::new(path);
    if MeshFormat::from_path(file).is_none() {
        return Err(format!("cannot determine format from path: {path}"));
    }

    let parts = slicer_engine::scene::load_path_multi_reporting(
        file,
        &slicer_engine::mesh::repair::RepairOptions::default(),
    )?;

    // Report each part's health once, on the single read, however many plate
    // objects it ends up backing.
    let file_name = file_name_of(path);
    let multi = parts.len() > 1;
    for (index, part) in parts.iter().enumerate() {
        let label = match (&part.name, multi) {
            (Some(name), _) => format!("{file_name} ({name})"),
            (None, true) => format!("{file_name} #{}", index + 1),
            (None, false) => file_name.clone(),
        };
        slicer_engine::mesh::repair::log_report(logger, &label, &part.report);
    }
    logger.log_debug(&format!(
        "loaded {} object(s) from {file_name}",
        parts.len()
    ));

    let count = parts.len();
    cache.insert(path.to_string(), parts);
    Ok(count)
}

/// The display name of a path — its file name, or the whole path if it has none.
fn file_name_of(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}

#[cfg(test)]
mod plate_loading_tests {
    use super::*;
    use slicer_engine::logging::NullLogger;
    use slicer_engine::mesh::types::AABB;

    /// A 3MF whose two build items ("top" and "bottom") are stacked as the
    /// authoring tool assembled them: bottom spans z 0..42, top z 42..67.
    fn top_ac_path() -> String {
        fixture("TopAC.3mf")
    }

    /// A single-part STL, for plates that mix different models.
    fn cube_path() -> String {
        fixture("simple-cube.stl")
    }

    fn fixture(name: &str) -> String {
        format!("{}/../../tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))
    }

    /// An object backed by the request-level file (no per-object path), as an
    /// older client would send it.
    fn object(part: usize, translation: [f32; 3]) -> SceneObjectPayload {
        SceneObjectPayload {
            translation: Some(translation),
            euler_xyz_deg: None,
            scale: None,
            source_part: Some(part),
            file_path: None,
        }
    }

    /// An object that names its own file — what the app sends now.
    fn object_from(path: &str, part: usize, translation: [f32; 3]) -> SceneObjectPayload {
        SceneObjectPayload {
            file_path: Some(path.to_string()),
            ..object(part, translation)
        }
    }

    fn aabb_of(object: &ObjectInput) -> AABB {
        let mut mesh = object.mesh.clone();
        mesh.calculate_aabb().expect("object has geometry").clone()
    }

    #[test]
    fn a_multi_part_3mf_becomes_one_plate_object_per_part() {
        let scene = SceneSnapshotPayload {
            objects: vec![object(0, [0.0; 3]), object(1, [0.0; 3])],
        };
        let objects = load_plate_objects(Some(&top_ac_path()), Some(&scene), &NullLogger).unwrap();

        assert_eq!(objects.len(), 2);
        assert_eq!(objects[0].name, "top");
        assert_eq!(objects[1].name, "bottom");
        // Each object carries only its own part — not the whole merged file.
        assert!(objects
            .iter()
            .all(|o| o.mesh.faces.len() < 42_108 && !o.mesh.faces.is_empty()));
    }

    /// The regression this fix exists for: the plate used to be sliced as the
    /// file defines it (parts stacked, one transform for the lot) rather than
    /// as the user arranged it.
    #[test]
    fn every_object_is_placed_where_the_scene_puts_it() {
        // What the UI does with this file: drop each part to the bed and stand
        // them side by side.
        let scene = SceneSnapshotPayload {
            objects: vec![object(0, [0.0, 0.0, -42.0]), object(1, [150.0, 0.0, 0.0])],
        };
        let objects = load_plate_objects(Some(&top_ac_path()), Some(&scene), &NullLogger).unwrap();

        let top = aabb_of(&objects[0]);
        let bottom = aabb_of(&objects[1]);

        // The top part was lowered onto the bed instead of floating at z 42.
        assert!((top.min.z - 0.0).abs() < 1e-3, "top.min.z = {}", top.min.z);
        // The bottom part moved 150 mm along X and stayed on the bed.
        assert!(
            (bottom.min.x - 92.5).abs() < 1e-3,
            "bottom.min.x = {}",
            bottom.min.x
        );
        assert!((bottom.min.z - 0.0).abs() < 1e-3);
    }

    /// The second regression: a plate holding two *different* models used to
    /// resolve every object into the first file, slicing one model twice.
    #[test]
    fn a_plate_of_two_different_files_slices_each_from_its_own() {
        let scene = SceneSnapshotPayload {
            objects: vec![
                object_from(&cube_path(), 0, [0.0; 3]),
                object_from(&top_ac_path(), 1, [150.0, 0.0, 0.0]),
            ],
        };
        let objects = load_plate_objects(None, Some(&scene), &NullLogger).unwrap();

        assert_eq!(objects.len(), 2);
        assert_eq!(objects[0].name, "simple-cube");
        assert_eq!(objects[1].name, "bottom");
        // Genuinely different geometry, not one model sliced twice.
        assert_ne!(objects[0].mesh.faces.len(), objects[1].mesh.faces.len());
    }

    #[test]
    fn an_object_without_its_own_file_falls_back_to_the_request_path() {
        let scene = SceneSnapshotPayload {
            objects: vec![object(1, [0.0; 3])],
        };
        let objects = load_plate_objects(Some(&top_ac_path()), Some(&scene), &NullLogger).unwrap();

        assert_eq!(objects.len(), 1);
        assert_eq!(objects[0].name, "bottom");
    }

    #[test]
    fn a_scene_with_no_file_anywhere_is_an_error() {
        let scene = SceneSnapshotPayload {
            objects: vec![object(0, [0.0; 3])],
        };
        let error = load_plate_objects(None, Some(&scene), &NullLogger).unwrap_err();
        assert!(error.contains("file_path"), "{error}");
    }

    #[test]
    fn two_objects_sharing_one_part_are_both_placed() {
        // Duplicating a model produces two scene objects backed by one part —
        // the plate must slice both, not just the first.
        let scene = SceneSnapshotPayload {
            objects: vec![object(1, [0.0; 3]), object(1, [150.0, 0.0, 0.0])],
        };
        let objects = load_plate_objects(Some(&top_ac_path()), Some(&scene), &NullLogger).unwrap();

        assert_eq!(objects.len(), 2);
        let first = aabb_of(&objects[0]);
        let second = aabb_of(&objects[1]);
        assert!((second.min.x - first.min.x - 150.0).abs() < 1e-3);
    }

    #[test]
    fn a_scene_referencing_a_missing_part_is_an_error_not_wrong_geometry() {
        let scene = SceneSnapshotPayload {
            objects: vec![object(7, [0.0; 3])],
        };
        let error =
            load_plate_objects(Some(&top_ac_path()), Some(&scene), &NullLogger).unwrap_err();
        assert!(error.contains("no object at index 7"), "{error}");
    }

    #[test]
    fn without_a_scene_the_file_is_sliced_as_authored() {
        let objects = load_plate_objects(Some(&top_ac_path()), None, &NullLogger).unwrap();

        assert_eq!(objects.len(), 2);
        // Untransformed: the parts keep the stack the 3MF describes.
        assert!((aabb_of(&objects[0]).min.z - 42.0).abs() < 1e-3);
        assert!((aabb_of(&objects[1]).min.z - 0.0).abs() < 1e-3);
    }

    #[test]
    fn the_cache_key_distinguishes_two_parts_of_one_file() {
        let params = slicer_engine::settings::params::SlicingParams::default();
        let path = top_ac_path();
        let first = Some(SceneSnapshotPayload {
            objects: vec![object(0, [0.0; 3])],
        });
        let second = Some(SceneSnapshotPayload {
            objects: vec![object(1, [0.0; 3])],
        });

        assert_ne!(
            compute_slice_cache_key(&params, Some(&path), &first),
            compute_slice_cache_key(&params, Some(&path), &second)
        );
    }

    /// Two plates that differ only in their *second* model must not collide —
    /// hashing just the first file would serve one plate's G-code for the other.
    #[test]
    fn the_cache_key_covers_every_file_on_the_plate() {
        let params = slicer_engine::settings::params::SlicingParams::default();
        let one = Some(SceneSnapshotPayload {
            objects: vec![
                object_from(&cube_path(), 0, [0.0; 3]),
                object_from(&top_ac_path(), 0, [0.0; 3]),
            ],
        });
        let two = Some(SceneSnapshotPayload {
            objects: vec![
                object_from(&cube_path(), 0, [0.0; 3]),
                object_from(&cube_path(), 0, [0.0; 3]),
            ],
        });

        assert_ne!(
            compute_slice_cache_key(&params, None, &one),
            compute_slice_cache_key(&params, None, &two)
        );
    }

    /// A pathless object resolves to the request-level file, so two requests
    /// whose fallback differs load different geometry and must key differently
    /// — even though both mention the same set of paths overall.
    #[test]
    fn the_cache_key_follows_the_fallback_a_pathless_object_resolves_to() {
        let params = slicer_engine::settings::params::SlicingParams::default();
        let cube = cube_path();
        let top_ac = top_ac_path();
        let scene = Some(SceneSnapshotPayload {
            objects: vec![
                object_from(&cube, 0, [0.0; 3]),
                object_from(&top_ac, 0, [0.0; 3]),
                // No file of its own — takes whatever the request supplies.
                object(0, [0.0; 3]),
            ],
        });

        assert_ne!(
            compute_slice_cache_key(&params, Some(&cube), &scene),
            compute_slice_cache_key(&params, Some(&top_ac), &scene)
        );
    }
}
