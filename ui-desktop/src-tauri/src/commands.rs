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
