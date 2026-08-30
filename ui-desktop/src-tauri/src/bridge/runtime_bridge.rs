use crate::bridge::tauri_logger::TauriAppLogger;
use serde_json::{json, Value};
use slicer_engine::logging::ProcessLogger;
use slicer_engine::mesh::types::Mesh;
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
        let file_path = payload
            .file_path
            .ok_or_else(|| "slice requires a file_path".to_string())?;
        let original_filename = std::path::Path::new(&file_path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string());

        let params: slicer_engine::settings::params::SlicingParams =
            serde_json::from_value(payload.settings)
                .map_err(|e| format!("invalid settings: {e}"))?;

        // Cache lookup: an identical scene + settings (+ engine version + source
        // file identity) sliced before can reuse the stored G-code and skip the
        // whole pipeline — including the mesh parse — exactly like the cloud
        // server's `gcode_cache`. This is what keeps repeated desktop slices as
        // fast as cloud.
        let cache_key = compute_slice_cache_key(&params, &file_path, &payload.scene);
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

        let mesh = load_model_from_path(&file_path, &logger)?;
        logger.log_info(&format!("mesh loaded: {} faces", mesh.faces.len()));

        let combined = bake_scene(&mesh, payload.scene, &logger);

        if combined.faces.is_empty() {
            return Err("combined scene has no triangles; nothing to slice".to_string());
        }

        logger.log_info(&format!("slicing {} faces\u{2026}", combined.faces.len()));
        // One object per plate here (see `bake_scene`), but it still goes
        // through `slice_plate` so the desktop honours exclude-object exactly
        // like the CLI and the server.
        let object_name = original_filename
            .clone()
            .unwrap_or_else(|| "object".to_string());
        let plate = slicer_engine::core::slice_plate(
            &[slicer_engine::core::ObjectInput::new(object_name, combined)],
            &params,
            &logger,
        );
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
/// the file's length + mtime stand in for the server's content-addressed upload
/// token, so editing the source model on disk busts the entry.
///
/// The params are fingerprinted via `SlicingParams::cache_fingerprint`, which
/// omits the ephemeral, camera-derived thumbnail PNG payload — so a fresh
/// render's bytes never bust the cache (issue #106).
fn compute_slice_cache_key(
    params: &slicer_engine::settings::params::SlicingParams,
    file_path: &str,
    scene: &Option<SceneSnapshotPayload>,
) -> String {
    let mut canonical = String::new();
    canonical.push_str("v=");
    canonical.push_str(slicer_engine::version::VERSION);
    canonical.push_str(";params=");
    canonical.push_str(&params.cache_fingerprint());

    canonical.push_str(";file=");
    canonical.push_str(file_path);
    if let Ok(meta) = std::fs::metadata(file_path) {
        canonical.push_str(&format!("|len={}", meta.len()));
        if let Ok(mtime) = meta.modified() {
            if let Ok(dur) = mtime.duration_since(std::time::UNIX_EPOCH) {
                canonical.push_str(&format!("|mtime={}", dur.as_nanos()));
            }
        }
    }

    canonical.push_str(";scene=");
    if let Some(scene) = scene {
        for obj in &scene.objects {
            canonical.push_str(&format!(
                "[{:?}|{:?}|{:?}]",
                obj.translation, obj.euler_xyz_deg, obj.scale
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

/// Load a mesh from a filesystem path, reading bytes directly in the Rust
/// process. The bytes never cross the IPC boundary.
fn load_model_from_path(path: &str, logger: &dyn ProcessLogger) -> Result<Mesh, String> {
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .ok_or_else(|| format!("cannot determine format from path: {path}"))?;
    let format = parse_format(ext)?;
    let bytes = std::fs::read(path).map_err(|e| format!("failed to read {path}: {e}"))?;
    logger.log_debug(&format!("read {} bytes from disk", bytes.len()));
    slicer_engine::scene::load_bytes(&bytes, format)
}

/// Apply the scene transform to `mesh`.
///
/// When the scene contains multiple objects a warning is emitted, since only
/// the first object's transform is applied. Multi-model native slicing is not
/// yet supported.
fn bake_scene(
    mesh: &Mesh,
    scene: Option<SceneSnapshotPayload>,
    logger: &dyn ProcessLogger,
) -> Mesh {
    let objects = scene.unwrap_or_default().objects;
    if objects.len() > 1 {
        logger.log_warn(&format!(
            "scene contains {} objects; only the first will be sliced \
             (multi-model native slicing is not yet supported)",
            objects.len()
        ));
    }
    let transform = objects
        .into_iter()
        .next()
        .map(|object| {
            Transform::from_euler_xyz_deg(
                object.translation.unwrap_or([0.0, 0.0, 0.0]),
                object.euler_xyz_deg.unwrap_or([0.0, 0.0, 0.0]),
                object.scale.unwrap_or([1.0, 1.0, 1.0]),
            )
        })
        .unwrap_or(Transform::IDENTITY);

    slicer_engine::scene::apply_transform(mesh, &transform)
}

fn parse_format(format_str: &str) -> Result<MeshFormat, String> {
    MeshFormat::from_extension(format_str)
        .ok_or_else(|| format!("unsupported mesh format: {format_str}"))
}
