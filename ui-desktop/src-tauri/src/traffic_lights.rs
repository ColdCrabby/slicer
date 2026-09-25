//! Centre the macOS traffic lights in the web title bar.
//!
//! The window uses the Overlay title-bar style, so the native close /
//! minimise / zoom buttons float over our own 40px title bar and have to be
//! moved into it. Tauri's `trafficLightPosition` does the moving, but its `y`
//! is not "distance from the top": it resizes the buttons' container to
//! `button height + y` and leaves each button at AppKit's own offset from the
//! container's *bottom*. The button's top therefore lands at `y - offset`, and
//! that offset changes between macOS releases — so a `y` tuned on one release
//! puts the lights too high on the next.
//!
//! This module measures the offset on the running system and builds the main
//! window with the `y` that actually centres the buttons. The window is marked
//! `create: false` in `tauri.macos.conf.json` for that reason: Tauri reads the
//! position only when a window is built, and has no setter afterwards.

use objc2::MainThreadMarker;
use objc2_app_kit::{
    NSBackingStoreType, NSWindow, NSWindowButton, NSWindowStyleMask, NSWindowTitleVisibility,
};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use tauri::{LogicalPosition, WebviewWindowBuilder};

/// Height of the web title bar the buttons centre in. Mirrors
/// `--titlebar-height` in the UI theme; the touch-sized 46px variant never
/// applies on macOS, which has no coarse pointer.
const TITLEBAR_HEIGHT: f64 = 40.0;

/// Left edge of the close button. The UI's `.is-mac.is-desktop` title-bar
/// padding clears the cluster from here, so move both together.
const LIGHTS_X: f64 = 19.0;

/// Build the `main` window from its config with the traffic lights centred.
pub fn create_main_window(app: &tauri::App) -> tauri::Result<()> {
    let config = app
        .config()
        .app
        .windows
        .iter()
        .find(|w| w.label == "main")
        .expect("tauri.macos.conf.json declares the main window");

    let mut builder = WebviewWindowBuilder::from_config(app.handle(), config)?;
    if let Some(y) = centred_inset_y() {
        builder = builder.traffic_light_position(LogicalPosition::new(LIGHTS_X, y));
    }
    builder.build()?;
    Ok(())
}

/// The `trafficLightPosition.y` that centres the buttons in the title bar, or
/// `None` if AppKit would not hand over a close button to measure (the window
/// then keeps the system's default placement rather than a guessed one).
///
/// Measured on a throwaway, never-shown window with the same style as the real
/// one: the buttons are laid out as soon as the window exists, and the real
/// window cannot be measured first because the position is fixed at build.
fn centred_inset_y() -> Option<f64> {
    let mtm = MainThreadMarker::new()?;
    let style = NSWindowStyleMask::Titled
        | NSWindowStyleMask::Closable
        | NSWindowStyleMask::Miniaturizable
        | NSWindowStyleMask::Resizable
        | NSWindowStyleMask::FullSizeContentView;

    // SAFETY: called on the main thread (checked above); the window is never
    // shown and is released when it drops, so it is not closed by AppKit too.
    let probe = unsafe {
        let window = NSWindow::initWithContentRect_styleMask_backing_defer(
            mtm.alloc::<NSWindow>(),
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(400.0, 300.0)),
            style,
            NSBackingStoreType::Buffered,
            true,
        );
        window.setReleasedWhenClosed(false);
        window
    };
    probe.setTitlebarAppearsTransparent(true);
    probe.setTitleVisibility(NSWindowTitleVisibility::Hidden);

    let close = probe.standardWindowButton(NSWindowButton::CloseButton)?;
    let frame = close.frame();
    let top = (TITLEBAR_HEIGHT - frame.size.height) / 2.0;
    Some(top + frame.origin.y)
}
