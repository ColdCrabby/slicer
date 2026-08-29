//! Shared Tauri application entry point for every platform we ship.
//!
//! Desktop launches this from [`main`](../src/main.rs); on mobile there is no
//! Rust `main` at all — iOS builds this crate as a **static library** that the
//! generated Xcode project links into the app binary and calls through the
//! `start_app` symbol that [`tauri::mobile_entry_point`] expands to. Keeping
//! the builder here (rather than in `main.rs`) is what makes both possible from
//! one code path, so a desktop-only change can never silently skip mobile.

mod bridge;
mod commands;
mod system_accent;

/// Build and run the Tauri application.
///
/// `mobile_entry_point` is a no-op on desktop; on iOS/Android it exports the
/// `extern "C"` symbol the platform launcher invokes.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .manage(bridge::runtime_bridge::AppState::new())
        .setup(|_app| {
            // Window chrome only exists on desktop — mobile has no resizable,
            // decorated window to correct.
            #[cfg(desktop)]
            {
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

                // Track live OS accent changes and push them to the UI.
                system_accent::spawn_watcher(_app.handle().clone());
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::runtime_init,
            commands::slice_start,
            commands::slice_cancel,
            commands::preview_get_source,
            commands::history_list,
            commands::history_clear,
            commands::get_system_accent,
            commands::profiles_load,
            commands::profiles_save_category,
            commands::printer_check,
            commands::printer_detect,
            commands::printer_send,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run desktop runtime");
}
