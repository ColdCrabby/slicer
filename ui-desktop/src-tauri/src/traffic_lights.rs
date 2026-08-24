//! Runtime placement of the macOS traffic-light buttons.
//!
//! With `titleBarStyle: Overlay`, Tauri applies `trafficLightPosition` once at
//! window creation, but the webview re-lays-out the standard window buttons
//! afterwards and snaps them back to the native default. Re-running the same
//! inset tao uses — and repeating it on the events that trigger a re-layout —
//! is the only reliable way to keep them centered in our taller title bar.

/// Inset from the window's top-left to the top-left of the button cluster, in
/// logical points. `x` clears the nav-rail; `y` centers the 12px buttons in the
/// 40px title bar (`13 + buttonHeight/2 ≈ 20`).
pub const INSET_X: f64 = 19.0;
pub const INSET_Y: f64 = 13.0;

#[cfg(target_os = "macos")]
fn log(msg: &str) {
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/tmp/nexus_tl.log")
    {
        let _ = writeln!(f, "{msg}");
    }
}

#[cfg(target_os = "macos")]
pub fn apply(window: &tauri::WebviewWindow) {
    use objc2_app_kit::{NSWindow, NSWindowButton};

    log("apply() called");

    let ptr = match window.ns_window() {
        Ok(p) if !p.is_null() => p,
        _ => {
            log("  bail: ns_window null/err");
            return;
        }
    };

    // SAFETY: `ns_window()` yields this window's live NSWindow, and every caller
    // runs on the main thread (setup hook + window-event callbacks).
    let ns_window: &NSWindow = unsafe { &*(ptr as *const NSWindow) };

    let (close, mini, zoom) = match (
        ns_window.standardWindowButton(NSWindowButton::CloseButton),
        ns_window.standardWindowButton(NSWindowButton::MiniaturizeButton),
        ns_window.standardWindowButton(NSWindowButton::ZoomButton),
    ) {
        (Some(c), Some(m), Some(z)) => (c, m, z),
        _ => {
            log("  bail: missing standard window button(s)");
            return;
        }
    };

    // close → button row → titlebar container view.
    let container = match unsafe { close.superview() }.and_then(|v| unsafe { v.superview() }) {
        Some(v) => v,
        None => {
            log("  bail: no titlebar container superview");
            return;
        }
    };

    let close_frame = close.frame();
    let bar_height = close_frame.size.height + INSET_Y;
    let mut container_frame = container.frame();
    container_frame.size.height = bar_height;
    container_frame.origin.y = ns_window.frame().size.height - bar_height;
    container.setFrame(container_frame);

    let spacing = mini.frame().origin.x - close_frame.origin.x;
    for (i, button) in [close, mini, zoom].iter().enumerate() {
        let mut origin = button.frame().origin;
        origin.x = INSET_X + i as f64 * spacing;
        button.setFrameOrigin(origin);
    }

    log(&format!(
        "  done: win_h={} close_h={} bar_h={} spacing={} inset=({},{})",
        ns_window.frame().size.height,
        close_frame.size.height,
        bar_height,
        spacing,
        INSET_X,
        INSET_Y,
    ));
}

#[cfg(not(target_os = "macos"))]
pub fn apply(_window: &tauri::WebviewWindow) {}
