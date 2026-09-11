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
    /// The three active profiles plus the user's sparse override diff.
    ///
    /// The desktop resolves it here, with the engine's own
    /// [`slicer_engine::profiles::resolve`] — the same call the server makes in
    /// `ws_session` and the browser makes through the wasm binding. The webview
    /// sends only what the user changed; every inherited value is composed on
    /// this side, so a profile edit can never be undone by a stale flattened
    /// copy the front-end happened to be holding.
    #[serde(default)]
    profiles: Option<Box<slicer_engine::profiles::ProfileSelection>>,
    /// Pre-flattened parameters. Superseded by `profiles`; kept so an older
    /// webview bundle paired with a newer shell still slices.
    #[serde(default)]
    settings: Option<Value>,
    /// PNG preview of the plate, base64-encoded, rendered by the webview's own
    /// 3D view and sent on every slice. The engine has no renderer, so this is
    /// the only place a thumbnail can come from; it rides its own field because
    /// it is a per-slice artifact, not a setting the user changed.
    #[serde(default)]
    thumbnail_png_base64: Option<String>,
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
    /// Support paint for this object, encoded by
    /// `slicer_engine::mesh::paint::FacetPaint::encode`, or `None` when
    /// unpainted. Mirrors the cloud server's `SceneObjectSliceDto.support_paint`
    /// so the two runtimes accept the same wire format.
    #[serde(default)]
    support_paint: Option<String>,
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

/// A previously-generated slice keyed by content hash. Written after every
/// slice but never read back to skip one — every request runs the full
/// pipeline, matching the cloud server's `gcode_cache` table.
// Both fields are written on every slice and never read back — see the doc
// comment above — so plain dead-code analysis flags them; keep them anyway,
// they are the point of the struct.
#[derive(Clone)]
#[allow(dead_code)]
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
    /// Content hash → most recent slice result for that hash. Every slice
    /// writes here; nothing reads it to skip a slice — see `CachedSlice`.
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
        let mut payload: SliceStartPayload =
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

        let params = resolve_slice_params(
            payload.profiles.take(),
            payload.settings.take(),
            payload.thumbnail_png_base64.take(),
        )?;

        // No cache lookup here on purpose: every slice request runs the full
        // pipeline, even when `cache_key` matches a previous run byte-for-byte.
        // The map below still gets written after slicing — it stays warm for
        // whatever else might read it — it is just never consulted to *skip*
        // a slice.
        let cache_key = compute_slice_cache_key(&params, file_path.as_deref(), &payload.scene);

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

/// Resolve a slice request's parameters, preferring the structured profile
/// selection over the legacy pre-flattened blob.
///
/// Mirrors `ws_session::resolve_slice_params`: the composition rules live in
/// the engine, and both hosts call the same one. The desktop has a real
/// `profiles.toml` on disk, so a selection that names its profiles by id
/// resolves against that — the webview sends three ids and the user's diff,
/// nothing more. Falling through to engine defaults when neither form is
/// present keeps a minimal payload sliceable.
fn resolve_slice_params(
    profiles: Option<Box<slicer_engine::profiles::ProfileSelection>>,
    settings: Option<Value>,
    thumbnail_png_base64: Option<String>,
) -> Result<slicer_engine::settings::params::SlicingParams, String> {
    let mut params = match profiles {
        Some(selection) => {
            let library = slicer_engine::profiles::ProfileStore::new()
                .load()
                .map_err(|e| format!("could not read this slicer's profile library: {e}"))?;
            selection
                .resolve(Some(&library))
                .map_err(|e| e.to_string())?
        }
        None => match settings {
            Some(value) => {
                serde_json::from_value(value).map_err(|e| format!("invalid settings: {e}"))?
            }
            None => Default::default(),
        },
    };
    if let Some(png) = thumbnail_png_base64 {
        params.thumbnail_png_base64 = Some(png);
    }
    Ok(params)
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
                "[{}#{}|{:?}|{:?}|{:?}|paint={:?}]",
                obj.file_path.as_deref().or(file_path).unwrap_or(""),
                obj.part_index(),
                obj.translation,
                obj.euler_xyz_deg,
                obj.scale,
                obj.support_paint.as_deref().unwrap_or("")
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
    // (path, part index, transform, support paint) — one entry per object.
    let placements: Vec<(String, usize, Transform, Option<String>)> = match scene {
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
                placements.push((
                    path,
                    object.part_index(),
                    object.transform(),
                    object.support_paint.clone(),
                ));
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
                .map(|index| (path.clone(), index, Transform::IDENTITY, None))
                .collect()
        }
    };

    let mut cache: HashMap<String, Vec<slicer_engine::scene::LoadedPart>> = HashMap::new();
    let mut objects = Vec::with_capacity(placements.len());
    for (path, part_index, transform, support_paint) in placements {
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
        let baked = slicer_engine::scene::apply_transform(&part.mesh, &transform);
        let mut object_input = ObjectInput::new(name, baked);
        if let Some(encoded) = support_paint.as_deref() {
            let paint = slicer_engine::mesh::paint::FacetPaint::decode(
                encoded,
                object_input.mesh.faces.len(),
            )
            .map_err(|e| format!("Invalid support paint for '{}': {}", object_input.name, e))?;
            object_input = object_input.with_paint(paint);
        }
        objects.push(object_input);
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
            support_paint: None,
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

    #[test]
    fn the_cache_key_changes_when_support_paint_is_added() {
        let params = slicer_engine::settings::params::SlicingParams::default();
        let path = cube_path();
        let unpainted = Some(SceneSnapshotPayload {
            objects: vec![object_from(&path, 0, [0.0; 3])],
        });
        let painted = Some(SceneSnapshotPayload {
            objects: vec![SceneObjectPayload {
                support_paint: Some("abc".to_string()),
                ..object_from(&path, 0, [0.0; 3])
            }],
        });

        assert_ne!(
            compute_slice_cache_key(&params, Some(&path), &unpainted),
            compute_slice_cache_key(&params, Some(&path), &painted),
            "a repaint must bust the cache, or the desktop app would keep \
             serving G-code sliced before the stroke"
        );
    }

    #[test]
    fn a_valid_support_paint_payload_is_applied_to_the_object() {
        let path = cube_path();
        let plain = load_plate_objects(Some(&path), None, &NullLogger).unwrap();
        let face_count = plain[0].mesh.faces.len();

        let mut paint = slicer_engine::mesh::paint::FacetPaint::new();
        paint.set(
            0,
            slicer_engine::mesh::paint::PaintState::Enforcer,
            face_count,
        );
        let encoded = paint.encode().unwrap();

        let scene = Some(SceneSnapshotPayload {
            objects: vec![SceneObjectPayload {
                support_paint: Some(encoded),
                ..object_from(&path, 0, [0.0; 3])
            }],
        });
        let painted = load_plate_objects(Some(&path), scene.as_ref(), &NullLogger).unwrap();
        assert_eq!(painted[0].paint.painted_count(), 1);
    }

    #[test]
    fn a_support_paint_payload_with_the_wrong_face_count_is_a_clear_error() {
        let path = cube_path();
        // Encoded against a face count the cube does not actually have, so
        // decode must reject it rather than silently painting the wrong
        // triangles.
        let mut paint = slicer_engine::mesh::paint::FacetPaint::new();
        paint.set(0, slicer_engine::mesh::paint::PaintState::Enforcer, 999);
        let encoded = paint.encode().unwrap();

        let scene = Some(SceneSnapshotPayload {
            objects: vec![SceneObjectPayload {
                support_paint: Some(encoded),
                ..object_from(&path, 0, [0.0; 3])
            }],
        });
        let error = load_plate_objects(Some(&path), scene.as_ref(), &NullLogger).unwrap_err();
        assert!(error.contains("Invalid support paint"), "{error}");
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

#[cfg(test)]
mod slice_param_tests {
    use super::*;

    use slicer_engine::profiles::ProfileRef;

    /// A selection that carries its profiles inline. Inline is the fallback
    /// form; `by_id` below is what the webview actually sends.
    fn selection(overrides: Value) -> Box<slicer_engine::profiles::ProfileSelection> {
        Box::new(slicer_engine::profiles::ProfileSelection {
            printer: ProfileRef::Inline(Box::new(
                slicer_engine::profiles::defaults::default_printer(),
            )),
            filament: ProfileRef::Inline(Box::new(
                slicer_engine::profiles::defaults::default_filament(),
            )),
            process: ProfileRef::Inline(Box::new(
                slicer_engine::profiles::defaults::default_process(),
            )),
            overrides,
        })
    }

    /// The webview sends deviations only. Everything the user did not touch has
    /// to come back from the profiles the engine composes here — if it did not,
    /// a plate would silently print with engine defaults instead of its process
    /// profile.
    #[test]
    fn a_sparse_diff_still_resolves_the_whole_stack() {
        let params =
            resolve_slice_params(Some(selection(json!({ "layer_height": 0.12 }))), None, None)
                .expect("resolve");

        assert!(
            (params.layer_height - 0.12).abs() < 1e-9,
            "the override wins"
        );
        assert_eq!(
            params.infill_pattern,
            slicer_engine::infill::InfillPattern::Gyroid,
            "from the process profile"
        );
        assert!(
            (params.nozzle_diameter_mm - 0.4).abs() < 1e-9,
            "from the printer"
        );
    }

    /// An override of `null` is a value, not an omission: it must not be
    /// mistaken for "inherit" and drop through to the profile's value.
    #[test]
    fn an_absent_override_bag_is_accepted() {
        let params =
            resolve_slice_params(Some(selection(Value::Null)), None, None).expect("resolve");
        assert!((params.layer_height - 0.2).abs() < 1e-9);
    }

    /// The pre-flattened blob is only the fallback for an older webview.
    #[test]
    fn profiles_win_over_legacy_settings() {
        let params = resolve_slice_params(
            Some(selection(json!({ "layer_height": 0.12 }))),
            Some(json!({ "layer_height": 0.3 })),
            None,
        )
        .expect("resolve");
        assert!((params.layer_height - 0.12).abs() < 1e-9);
    }

    #[test]
    fn legacy_settings_still_slice_when_no_profiles_are_sent() {
        let params = resolve_slice_params(None, Some(json!({ "layer_height": 0.3 })), None)
            .expect("resolve");
        assert!((params.layer_height - 0.3).abs() < 1e-9);
    }

    /// The webview names its profiles by id and the desktop looks them up in
    /// the same `profiles.toml` the CLI on this machine reads.
    #[test]
    fn ids_resolve_against_the_engines_own_library() {
        let library = slicer_engine::profiles::ProfileLibrary::default().seeded();
        let selection = slicer_engine::profiles::ProfileSelection {
            printer: ProfileRef::Id("builtin-generic-printer".into()),
            filament: ProfileRef::Id("builtin-generic-petg".into()),
            process: ProfileRef::Id("builtin-standard-02".into()),
            overrides: json!({ "layer_height": 0.12 }),
        };
        let params = selection.resolve(Some(&library)).expect("resolve");

        assert!(
            (params.layer_height - 0.12).abs() < 1e-9,
            "the override wins"
        );
        assert_eq!(params.filament_type, "PETG", "from the referenced filament");
        assert_eq!(
            params.infill_pattern,
            slicer_engine::infill::InfillPattern::Gyroid,
            "from the referenced process"
        );
    }

    /// The browser renders the thumbnail; the engine only carries it.
    #[test]
    fn the_thumbnail_reaches_the_generator_from_its_own_field() {
        let params = resolve_slice_params(
            Some(selection(Value::Null)),
            None,
            Some("iVBORw0KGgo=".to_string()),
        )
        .expect("resolve");
        assert_eq!(params.thumbnail_png_base64.as_deref(), Some("iVBORw0KGgo="));
    }

    #[test]
    fn an_empty_payload_falls_through_to_engine_defaults() {
        let params = resolve_slice_params(None, None, None).expect("resolve");
        assert_eq!(
            params.layer_height,
            slicer_engine::settings::params::SlicingParams::default().layer_height
        );
    }
}
