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

/// Shared state managed by Tauri across all commands.
pub struct AppState {
    /// Path of the most recently generated GCode file on disk.
    pub last_gcode_path: Arc<Mutex<Option<String>>>,
    /// Map from slice_id → GCode file path on disk (never inline strings).
    pub gcode_path_by_slice: Arc<Mutex<HashMap<String, String>>>,
    pub history_sessions: Arc<Mutex<Vec<HistorySession>>>,
    pub cancel_flag: Arc<AtomicBool>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            last_gcode_path: Arc::new(Mutex::new(None)),
            gcode_path_by_slice: Arc::new(Mutex::new(HashMap::new())),
            history_sessions: Arc::new(Mutex::new(Vec::new())),
            cancel_flag: Arc::new(AtomicBool::new(false)),
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

    tauri::async_runtime::spawn_blocking(move || {
        let payload: SliceStartPayload =
            serde_json::from_value(payload).map_err(|e| format!("invalid slice payload: {e}"))?;

        let slice_id = payload.slice_id.unwrap_or_else(|| "unknown".to_string());
        let logger = TauriAppLogger::new(app.clone(), cancel_flag.clone());
        logger.log_info(&format!("slice_id={slice_id}"));

        // Resolve mesh from file path. Rust reads the file directly so that
        // no bytes cross the IPC boundary.
        let file_path = payload
            .file_path
            .ok_or_else(|| "slice requires a file_path".to_string())?;
        let mesh = load_model_from_path(&file_path, &logger)?;
        let original_filename = std::path::Path::new(&file_path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string());

        logger.log_info(&format!("mesh loaded: {} faces", mesh.faces.len()));

        let params: slicer_engine::settings::params::SlicingParams =
            serde_json::from_value(payload.settings)
                .map_err(|e| format!("invalid settings: {e}"))?;

        let combined = bake_scene(&mesh, payload.scene, &logger);

        if combined.faces.is_empty() {
            return Err("combined scene has no triangles; nothing to slice".to_string());
        }

        logger.log_info(&format!("slicing {} faces\u{2026}", combined.faces.len()));
        let layers = slicer_engine::core::process_mesh(&combined, &params, &logger);
        logger.log_info(&format!("{} layers produced", layers.len()));

        if cancel_flag.load(Ordering::SeqCst) {
            return Err("Slice cancelled by user".to_string());
        }

        let gcode = slicer_engine::gcode::generate_gcode_from_params(&layers, &params);
        let layer_count = layers.len();
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

        *last_gcode_path.lock().unwrap() = Some(gcode_path.clone());
        gcode_path_by_slice
            .lock()
            .unwrap()
            .insert(slice_id.clone(), gcode_path.clone());

        let now = chrono::Utc::now().to_rfc3339();
        history_sessions.lock().unwrap().insert(
            0,
            HistorySession {
                request_uuid: slice_id.clone(),
                created_at: now,
                original_filename: original_filename.clone(),
                layer_count: Some(layer_count as i32),
                download_url: String::new(),
            },
        );

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
