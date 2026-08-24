use serde_json::Value;
use tauri::State;

use crate::bridge::runtime_bridge::AppState;

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
