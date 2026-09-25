//! The macOS menu bar.
//!
//! Tauri installs a stock menu on macOS whose File → Close Window owns `⌘W`, so
//! the key every tabbed Mac app uses to close a tab closed the whole window
//! instead. This menu is that stock one with the window item swapped for the
//! workplate commands — New, Close, Close All, Reopen — so the keys are where
//! a Mac user looks for them, and `⌘W` closes the workplate on screen.
//!
//! It also carries what a Mac user expects to find there for the plate itself —
//! Add Model, Slice, Export G-code, Settings and a Help menu.
//!
//! The menu only reports the choice: workplate commands go out on
//! [`WORKPLATE_MENU_EVENT`] to the UI, which owns the tab list; the rest go out
//! on [`APP_MENU_EVENT`] to `AppMenu`, which runs the code the in-app buttons
//! run. The UI does not also bind these keys on the Mac, or each would run
//! twice — which is why Slice carries no key equivalent (`⌘↵` is a web
//! shortcut). Windows and Linux have no menu bar (the window is frameless) and
//! take the same keys straight in the webview.

use tauri::menu::{Menu, MenuEvent, MenuItemBuilder, SubmenuBuilder};
use tauri::{AppHandle, Emitter, Runtime};

/// Event the UI listens for; the payload is one of the command names below.
pub const WORKPLATE_MENU_EVENT: &str = "workplate-menu";

/// Menu id prefix that marks a workplate command; the rest is the payload.
const WORKPLATE_ID_PREFIX: &str = "workplate:";

/// Event for every other app command; the payload is the item's name.
pub const APP_MENU_EVENT: &str = "app-menu";

/// Menu id prefix that marks an app command.
const APP_ID_PREFIX: &str = "app:";

/// Build the menu bar. Everything but the File menu is the stock set, kept so
/// Edit → Copy/Paste and the window commands still work in text fields.
pub fn build<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let command = |name: &str, label: &str, accelerator: &str| {
        MenuItemBuilder::with_id(format!("{WORKPLATE_ID_PREFIX}{name}"), label)
            .accelerator(accelerator)
            .build(app)
    };

    let action = |name: &str, label: &str, accelerator: Option<&str>| {
        let builder = MenuItemBuilder::with_id(format!("{APP_ID_PREFIX}{name}"), label);
        match accelerator {
            Some(keys) => builder.accelerator(keys).build(app),
            None => builder.build(app),
        }
    };

    let app_menu = SubmenuBuilder::new(app, app.package_info().name.clone())
        .about(None)
        .separator()
        .item(&action("settings", "Settings…", Some("CmdOrCtrl+,"))?)
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
        .item(&action("add-model", "Add Model…", Some("CmdOrCtrl+O"))?)
        .separator()
        .item(&action("slice", "Slice", None)?)
        .item(&action(
            "export-gcode",
            "Export G-code…",
            Some("CmdOrCtrl+E"),
        )?)
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

    let help_menu = SubmenuBuilder::new(app, "Help")
        .item(&action("help-docs", "Cold Crabby Documentation", None)?)
        .item(&action("help-shortcuts", "Keyboard Shortcuts", None)?)
        .build()?;

    Menu::with_items(
        app,
        &[
            &app_menu,
            &file_menu,
            &edit_menu,
            &view_menu,
            &window_menu,
            &help_menu,
        ],
    )
}

/// Forward a workplate or app command to the UI; every other item is a
/// predefined one the OS handles itself.
pub fn on_event<R: Runtime>(app: &AppHandle<R>, event: MenuEvent) {
    let id = event.id().as_ref();
    if let Some(name) = id.strip_prefix(WORKPLATE_ID_PREFIX) {
        let _ = app.emit(WORKPLATE_MENU_EVENT, name);
    } else if let Some(name) = id.strip_prefix(APP_ID_PREFIX) {
        let _ = app.emit(APP_MENU_EVENT, name);
    }
}
