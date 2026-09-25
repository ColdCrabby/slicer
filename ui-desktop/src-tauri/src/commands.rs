use serde_json::{json, Value};
use tauri::State;

use crate::bridge::runtime_bridge::AppState;
use slicer_engine::profiles::PrinterConnection;

#[tauri::command]
pub fn runtime_init(state: State<'_, AppState>) -> Result<Value, String> {
    crate::bridge::runtime_bridge::runtime_init(&state)
}

#[tauri::command]
pub async fn slice_start(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    payload: Value,
) -> Result<Value, String> {
    crate::bridge::runtime_bridge::slice_start(app, &state, payload).await
}

#[tauri::command]
pub fn slice_cancel(state: State<'_, AppState>) -> Result<Value, String> {
    crate::bridge::runtime_bridge::slice_cancel(&state)
}

#[tauri::command]
pub fn preview_get_source(
    state: State<'_, AppState>,
    payload: Option<Value>,
) -> Result<Value, String> {
    crate::bridge::runtime_bridge::preview_get_source(&state, payload)
}

#[tauri::command]
pub fn history_list(state: State<'_, AppState>) -> Result<Value, String> {
    crate::bridge::runtime_bridge::history_list(&state)
}

#[tauri::command]
pub fn history_clear(state: State<'_, AppState>) -> Result<Value, String> {
    crate::bridge::runtime_bridge::history_clear(&state)
}

#[tauri::command]
pub fn get_system_accent() -> Option<String> {
    crate::system_accent::detect()
}

// ── Profile library ───────────────────────────────────────────────────────────
//
// The engine owns the on-disk profile library (`profiles.toml` in the config
// dir); these commands are the native runtime's equivalent of the server's
// `GET`/`PUT /api/profiles`, so printers/filaments/processes/labels live next
// to the slicer instead of only in the webview's localStorage.

/// Load the whole user-owned profile library as JSON.
#[tauri::command]
pub fn profiles_load() -> Result<Value, String> {
    let library = slicer_engine::profiles::ProfileStore::new()
        .load()
        .map_err(|e| e.to_string())?;
    serde_json::to_value(library).map_err(|e| e.to_string())
}

/// Replace one category (`printers`/`filaments`/`processes`/`labels`) from a
/// JSON array and persist it, returning the updated library.
#[tauri::command]
pub fn profiles_save_category(kind: String, items: Value) -> Result<Value, String> {
    let parsed = slicer_engine::profiles::ProfileKind::parse(&kind)
        .ok_or_else(|| format!("unknown profile category '{kind}'"))?;
    let library = slicer_engine::profiles::ProfileStore::new()
        .replace_category(parsed, items)
        .map_err(|e| e.to_string())?;
    serde_json::to_value(library).map_err(|e| e.to_string())
}

// ── Workplates ───────────────────────────────────────────────────────────────
//
// The desktop has no database — its history is in memory — so a plate's saved
// setup lives beside `profiles.toml` in the config dir. Same reasoning as the
// profile library: what the user built has to survive the webview's storage,
// and "next to the engine" is where the engine can still find it.

/// Load one workplate's saved setup, or `null` when it was never configured.
#[tauri::command]
pub fn workplate_load(request_uuid: String) -> Result<Value, String> {
    let setup = slicer_engine::workplate::WorkplateStore::new()
        .load(&request_uuid)
        .map_err(|e| e.to_string())?;
    serde_json::to_value(setup).map_err(|e| e.to_string())
}

/// Replace one workplate's saved setup. Whole-document, last writer wins.
#[tauri::command]
pub fn workplate_save(request_uuid: String, setup: Value) -> Result<Value, String> {
    let mut parsed: slicer_engine::workplate::WorkplateSetup =
        serde_json::from_value(setup).map_err(|e| format!("invalid workplate setup: {e}"))?;
    parsed.updated_at = Some(chrono::Utc::now().to_rfc3339());
    slicer_engine::workplate::WorkplateStore::new()
        .save(&request_uuid, &parsed)
        .map_err(|e| e.to_string())?;
    serde_json::to_value(parsed).map_err(|e| e.to_string())
}

/// A rendered profile export, ready for the webview to save or share.
#[derive(serde::Serialize)]
pub struct ProfileExport {
    /// Suggested save-as filename.
    pub filename: String,
    /// MIME type, for the share sheet and the blob the UI builds.
    pub mime: String,
    /// File contents.
    pub bytes: Vec<u8>,
}

/// Export the on-disk profile library as TOML (`bundle` ZIP or single
/// `profiles.toml`).
///
/// Reads `profiles.toml` rather than taking the UI's copy, so what the user
/// downloads is exactly what this machine's CLI would read.
#[tauri::command]
pub fn profiles_export(format: String) -> Result<ProfileExport, String> {
    let parsed = slicer_engine::profiles::ProfileExportFormat::parse(&format)
        .ok_or_else(|| format!("unknown export format '{format}'"))?;
    let library = slicer_engine::profiles::ProfileStore::new()
        .load()
        .map_err(|e| e.to_string())?;
    let artifact =
        slicer_engine::profiles::export_library(&library, parsed).map_err(|e| e.to_string())?;
    Ok(ProfileExport {
        filename: artifact.filename,
        mime: artifact.mime.to_string(),
        bytes: artifact.bytes,
    })
}

// ── Printer transport ─────────────────────────────────────────────────────────
//
// The desktop runtime talks to printers **from this native process** using the
// OS network stack (`slicer_engine::printer`, backed by `reqwest`), exactly
// like the cloud `serve` WebSocket. This is what keeps printer probes/uploads
// **off the browser `fetch` path**, which Moonraker (Klipper) blocks via CORS —
// so the desktop app reports honest online/offline status instead of a
// misleading "blocked (CORS)".

/// Probe a printer connection and return its live status (same JSON shape as the
/// cloud WS `PrinterStatus` payload, minus the `printer_id` envelope).
#[tauri::command]
pub async fn printer_check(connection: PrinterConnection) -> Result<Value, String> {
    let report = slicer_engine::printer::check_status(&connection).await;
    serde_json::to_value(report).map_err(|e| e.to_string())
}

/// Probe a single host to identify a printer and prefill the setup wizard (same
/// JSON shape as the cloud WS `PrinterDetected` payload, minus the `host`).
#[tauri::command]
pub async fn printer_detect(host: String) -> Result<Value, String> {
    let detection = slicer_engine::printer::detect_printer(&host).await;
    serde_json::to_value(detection).map_err(|e| e.to_string())
}

/// Upload the most recently sliced G-code to a printer, optionally starting the
/// print. The desktop runtime is single-active-slice, so the authoritative
/// source is `AppState::last_gcode_path`.
#[tauri::command]
pub async fn printer_send(
    state: State<'_, AppState>,
    connection: PrinterConnection,
    filename: Option<String>,
    start: bool,
) -> Result<Value, String> {
    let gcode_path = state
        .last_gcode_path
        .lock()
        .map_err(|e| e.to_string())?
        .clone()
        .ok_or_else(|| "No sliced G-code found for this scene — slice it first".to_string())?;

    let path = std::path::PathBuf::from(&gcode_path);
    let name = filename.unwrap_or_else(|| {
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("print.gcode")
            .to_string()
    });

    match slicer_engine::printer::send_gcode(&connection, &path, &name, start).await {
        Ok(outcome) => Ok(json!({
            "ok": true,
            "message": outcome.message,
            "started": outcome.started,
        })),
        Err(e) => Ok(json!({ "ok": false, "message": e, "started": false })),
    }
}

// ── Object library ────────────────────────────────────────────────────────────
//
// Every model that reaches a plate is recorded in the engine's library, next
// to the profiles and workplates. The heavy commands — hashing, measuring and
// copying models — run on a blocking thread so the webview never waits on a
// 200 MB STL being read. Model and thumbnail bytes cross the IPC boundary raw,
// not as a JSON array of numbers.

fn library() -> slicer_engine::library::LibraryStore {
    slicer_engine::library::LibraryStore::new()
}

async fn off_thread<T: Send + 'static>(
    work: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// The whole library, with missing files and thumbnails marked.
#[tauri::command]
pub async fn library_load() -> Result<Value, String> {
    let library = off_thread(|| library().load()).await?;
    serde_json::to_value(library).map_err(|e| e.to_string())
}

/// Replace the library settings.
///
/// iOS cannot reopen a path it was handed once — a picked file arrives as a
/// throwaway copy — so the mode is clamped to `copy` there, whatever was sent.
#[tauri::command]
pub async fn library_save_settings(settings: Value) -> Result<Value, String> {
    #[allow(unused_mut)]
    let mut parsed: slicer_engine::library::LibrarySettings =
        serde_json::from_value(settings).map_err(|e| format!("invalid library settings: {e}"))?;
    #[cfg(target_os = "ios")]
    {
        parsed.mode = slicer_engine::library::StorageMode::Copy;
        parsed.folders.clear();
    }
    let library = off_thread(move || library().save_settings(parsed)).await?;
    serde_json::to_value(library).map_err(|e| e.to_string())
}

/// Rescan the library's own folder and every watched folder.
#[tauri::command]
pub async fn library_scan() -> Result<Value, String> {
    let report = off_thread(|| library().scan()).await?;
    serde_json::to_value(report).map_err(|e| e.to_string())
}

/// Record files the user has on disk. One outcome per path, in order; a file
/// that could not be recorded answers `{ "error": … }` rather than failing the
/// rest.
#[tauri::command]
pub async fn library_import_paths(paths: Vec<String>) -> Result<Value, String> {
    off_thread(move || {
        let store = library();
        Ok(paths
            .iter()
            .map(|path| match store.import_path(std::path::Path::new(path)) {
                Ok(outcome) => serde_json::to_value(outcome).unwrap_or(Value::Null),
                Err(e) => json!({ "error": e.to_string() }),
            })
            .collect::<Vec<_>>())
    })
    .await
    .map(Value::from)
}

/// Record a file that arrived as bytes. The body is the raw file; the
/// `x-file-name` header carries its name.
#[tauri::command]
pub async fn library_import_bytes(request: tauri::ipc::Request<'_>) -> Result<Value, String> {
    let name = header(&request, "x-file-name")?;
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body().clone() else {
        return Err("library_import_bytes expects a raw body".into());
    };
    let outcome = off_thread(move || library().import_bytes(&name, &bytes)).await?;
    serde_json::to_value(outcome).map_err(|e| e.to_string())
}

/// Where to read an entry from: `{ path, name, format }`, or `null` when none of
/// its files is still there.
#[tauri::command]
pub async fn library_resolve(id: String) -> Result<Value, String> {
    let resolved = off_thread(move || library().resolve(&id)).await?;
    Ok(match resolved {
        Some((entry, path)) => json!({
            "path": path.to_string_lossy(),
            "name": entry.name,
            "format": entry.format,
        }),
        None => Value::Null,
    })
}

/// Note that an entry was put on a plate.
#[tauri::command]
pub async fn library_touch(id: String) -> Result<bool, String> {
    off_thread(move || library().touch(&id)).await
}

/// Rename an entry.
#[tauri::command]
pub async fn library_rename(id: String, name: String) -> Result<bool, String> {
    off_thread(move || library().rename(&id, &name)).await
}

/// Forget an entry. The library's own copy goes with it; a referenced file
/// never does.
#[tauri::command]
pub async fn library_remove(id: String) -> Result<bool, String> {
    off_thread(move || library().remove(&id)).await
}

/// A stored thumbnail, raw. Empty when there is none.
#[tauri::command]
pub fn library_thumbnail(id: String) -> tauri::ipc::Response {
    tauri::ipc::Response::new(library().thumbnail(&id).unwrap_or_default())
}

/// Store a thumbnail the webview rendered. The body is the PNG; the
/// `x-entry-id` header names the entry.
#[tauri::command]
pub fn library_set_thumbnail(request: tauri::ipc::Request<'_>) -> Result<(), String> {
    let id = header(&request, "x-entry-id")?;
    let tauri::ipc::InvokeBody::Raw(png) = request.body() else {
        return Err("library_set_thumbnail expects a raw body".into());
    };
    library().set_thumbnail(&id, png).map_err(|e| e.to_string())
}

/// Where the library keeps its copies — shown in settings, and on iPad the
/// folder the Files app exposes.
#[tauri::command]
pub fn library_models_dir() -> String {
    library().models_path().to_string_lossy().into_owned()
}

fn header(request: &tauri::ipc::Request<'_>, name: &str) -> Result<String, String> {
    request
        .headers()
        .get(name)
        .and_then(|v| v.to_str().ok())
        // Header values are ASCII; the webview percent-encodes the name.
        .map(percent_decode)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| format!("missing '{name}' header"))
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(&value[i + 1..i + 3], 16) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}
