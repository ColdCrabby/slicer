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
/// Native context menus. iOS has no Tauri menu API, so this is where the
/// platform's own action sheet is built.
mod context_menu;
/// Native iOS alerts, confirmations and the share sheet.
mod native_dialog;
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
                // macOS keeps native decorations (`titleBarStyle: Overlay`, so
                // the traffic lights overlay our custom title bar) and shows the
                // window from the start — WKWebView paints fast enough that there
                // is nothing to hide. Windows and Linux instead create the window
                // frameless *and* hidden (see tauri.windows.conf.json /
                // tauri.linux.conf.json):
                //
                //  - Frameless at creation avoids a runtime `set_decorations(false)`,
                //    whose window-style change forced a WebView2 relayout on
                //    Windows that visibly froze the app for a moment on launch.
                //  - Staying hidden until the web UI paints its first frame hides
                //    WebView2's slow cold start, which otherwise left a blank,
                //    unresponsive window on screen.
                //
                // Together those were the "app hangs for a bit before it works"
                // report. The frontend calls `getCurrentWindow().show()` once it
                // has rendered; this timer is a safety net so a frontend failure
                // can never leave the window permanently invisible.
                #[cfg(not(target_os = "macos"))]
                {
                    use tauri::Manager;
                    let handle = _app.handle().clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(std::time::Duration::from_secs(5));
                        if let Some(window) = handle.get_webview_window("main") {
                            if !window.is_visible().unwrap_or(true) {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                    });
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
            commands::profiles_export,
            commands::printer_check,
            commands::printer_detect,
            commands::printer_send,
            context_menu::show_context_menu,
            native_dialog::show_native_dialog,
            native_dialog::share_file,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run desktop runtime");
}
