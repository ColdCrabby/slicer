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
