//! The macOS menu bar.
//!
//! Tauri installs a stock menu on macOS whose File → Close Window owns `⌘W`, so
//! the key every tabbed Mac app uses to close a tab closed the whole window
//! instead. This menu is that stock one with the window item swapped for the
//! workplate commands — New, Close, Close All, Reopen — so the keys are where
//! a Mac user looks for them, and `⌘W` closes the workplate on screen.
//!
//! The menu only reports the choice: it emits [`WORKPLATE_MENU_EVENT`] and the
//! UI, which owns the tab list, acts on it. The UI does not also bind these
//! keys on the Mac, or each would run twice. Windows and Linux have no menu bar
//! (the window is frameless) and take the same keys straight in the webview.

use tauri::menu::{Menu, MenuEvent, MenuItemBuilder, SubmenuBuilder};
use tauri::{AppHandle, Emitter, Runtime};

/// Event the UI listens for; the payload is one of the command names below.
pub const WORKPLATE_MENU_EVENT: &str = "workplate-menu";

/// Menu id prefix that marks a workplate command; the rest is the payload.
const WORKPLATE_ID_PREFIX: &str = "workplate:";

/// Build the menu bar. Everything but the File menu is the stock set, kept so
/// Edit → Copy/Paste and the window commands still work in text fields.
pub fn build<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let command = |name: &str, label: &str, accelerator: &str| {
        MenuItemBuilder::with_id(format!("{WORKPLATE_ID_PREFIX}{name}"), label)
            .accelerator(accelerator)
            .build(app)
    };

    let app_menu = SubmenuBuilder::new(app, app.package_info().name.clone())
        .about(None)
        .separator()
        .services()
        .separator()
        .hide()
        .hide_others()
        .show_all()
        .separator()
        .quit()
        .build()?;

    let file_menu = SubmenuBuilder::new(app, "File")
        .item(&command("new", "New Workplate", "CmdOrCtrl+T")?)
        .separator()
        .item(&command("close", "Close Workplate", "CmdOrCtrl+W")?)
        .item(&command(
            "close-all",
            "Close All Workplates",
            "CmdOrCtrl+Shift+W",
        )?)
        .separator()
        .item(&command(
            "reopen",
            "Reopen Closed Workplate",
            "CmdOrCtrl+Shift+T",
        )?)
        .build()?;

    let edit_menu = SubmenuBuilder::new(app, "Edit")
        .undo()
        .redo()
        .separator()
        .cut()
        .copy()
        .paste()
        .select_all()
        .build()?;

    let view_menu = SubmenuBuilder::new(app, "View").fullscreen().build()?;

    // No Close Window here: its fixed `⌘W` is the key the File menu now uses.
    // The red traffic light still closes the window.
    let window_menu = SubmenuBuilder::new(app, "Window")
        .minimize()
        .maximize()
        .separator()
        .bring_all_to_front()
        .build()?;

    Menu::with_items(
        app,
        &[&app_menu, &file_menu, &edit_menu, &view_menu, &window_menu],
    )
}

/// Forward a workplate command to the UI; every other item is a predefined one
/// the OS handles itself.
pub fn on_event<R: Runtime>(app: &AppHandle<R>, event: MenuEvent) {
    if let Some(name) = event.id().as_ref().strip_prefix(WORKPLATE_ID_PREFIX) {
        let _ = app.emit(WORKPLATE_MENU_EVENT, name);
    }
}
