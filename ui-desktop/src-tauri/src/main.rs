#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod bridge;
mod commands;
mod system_accent;
mod traffic_lights;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .manage(bridge::runtime_bridge::AppState::new())
        .setup(|app| {
            // The window is configured with `decorations: true` +
            // `titleBarStyle: Overlay` so macOS keeps its native traffic
            // lights overlaid on our custom title bar. On Windows/Linux there
            // is no overlay style, so native decorations would draw a second
            // title bar on top of ours — strip them here so those platforms
            // stay frameless and use the custom min/max/close controls.
            #[cfg(not(target_os = "macos"))]
            {
                use tauri::Manager;
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.set_decorations(false);
                }
            }

            // macOS: `titleBarStyle: Overlay` lets the webview reset the native
            // traffic lights to their default spot, so re-apply our centered
            // inset now and on every event that triggers a button re-layout.
            #[cfg(target_os = "macos")]
            {
                use tauri::{Manager, WindowEvent};
                if let Some(window) = app.get_webview_window("main") {
                    traffic_lights::apply(&window);
                    let w = window.clone();
                    window.on_window_event(move |event| {
                        if matches!(
                            event,
                            WindowEvent::Resized(_)
                                | WindowEvent::Focused(true)
                                | WindowEvent::ThemeChanged(_)
                        ) {
                            traffic_lights::apply(&w);
                        }
                    });

                    // A window that launches already-focused never fires a
                    // Focused event, and the webview resets the buttons a beat
                    // after setup — re-assert once the first layout settles.
                    let w2 = window.clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(std::time::Duration::from_millis(500));
                        let w3 = w2.clone();
                        let _ = w2.run_on_main_thread(move || traffic_lights::apply(&w3));
                    });
                }
            }

            // Track live OS accent changes and push them to the UI.
            system_accent::spawn_watcher(app.handle().clone());
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
