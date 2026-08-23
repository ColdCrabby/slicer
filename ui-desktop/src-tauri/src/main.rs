#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod bridge;
mod commands;
mod system_accent;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .manage(bridge::runtime_bridge::AppState::new())
        .setup(|_app| {
            // The window is configured with `decorations: true` +
            // `titleBarStyle: Overlay` so macOS keeps its native traffic
            // lights overlaid on our custom title bar. On Windows/Linux there
            // is no overlay style, so native decorations would draw a second
            // title bar on top of ours — strip them here so those platforms
            // stay frameless and use the custom min/max/close controls.
            #[cfg(not(target_os = "macos"))]
            {
                use tauri::Manager;
                if let Some(window) = _app.get_webview_window("main") {
                    let _ = window.set_decorations(false);
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::runtime_init,
            commands::slice_start,
            commands::slice_cancel,
            commands::preview_get_source,
            commands::history_list,
            commands::get_system_accent,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run desktop runtime");
}
