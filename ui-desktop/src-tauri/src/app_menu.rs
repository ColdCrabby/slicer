//! The macOS menu bar.
//!
//! A Mac app is expected to answer from the menu bar: File › Open, Export, a
//! Help menu, and the key equivalents listed beside them. Tauri installs a
//! bare default menu on macOS; this replaces it with one that knows what the
//! app does.
//!
//! macOS only, on purpose. Windows and Linux create the window frameless and
//! draw their own title bar, so a native menu bar has nowhere to sit there —
//! those platforms keep the web shortcuts and the in-app Help menu.
//!
//! The menu owns no behaviour. Each custom item carries an id, and choosing it
//! emits that id to the webview as [`MENU_EVENT`], where `AppMenu` maps it onto
//! the same code the in-app buttons run. Edit, View and Window use the
//! predefined items, which is what makes copy and paste work in text fields.

use tauri::menu::{AboutMetadata, MenuBuilder, MenuItemBuilder, SubmenuBuilder};
use tauri::{AppHandle, Emitter, Runtime};

/// Event the chosen item's id is emitted on.
pub const MENU_EVENT: &str = "app-menu";

/// Build the menu bar, install it, and forward choices to the webview.
pub fn install<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let item = |id: &str, label: &str, accelerator: Option<&str>| {
        let builder = MenuItemBuilder::with_id(id, label);
        match accelerator {
            Some(keys) => builder.accelerator(keys).build(app),
            None => builder.build(app),
        }
    };

    let app_menu = SubmenuBuilder::new(app, "Cold Crabby")
        .about(Some(AboutMetadata::default()))
        .separator()
        .item(&item("settings", "Settings…", Some("CmdOrCtrl+,"))?)
        .separator()
        .services()
        .separator()
        .hide()
        .hide_others()
        .show_all()
        .separator()
        .quit()
        .build()?;

    // Slice has no accelerator here: ⌘↵ is already a web shortcut, and a menu
    // key equivalent for the same keys would fire alongside it.
    let file = SubmenuBuilder::new(app, "File")
        .item(&item("new-plate", "New Plate", Some("CmdOrCtrl+N"))?)
        .item(&item("add-model", "Add Model…", Some("CmdOrCtrl+O"))?)
        .separator()
        .item(&item("slice", "Slice", None)?)
        .item(&item(
            "export-gcode",
            "Export G-code…",
            Some("CmdOrCtrl+E"),
        )?)
        .separator()
        .close_window()
        .build()?;

    let edit = SubmenuBuilder::new(app, "Edit")
        .undo()
        .redo()
        .separator()
        .cut()
        .copy()
        .paste()
        .select_all()
        .build()?;

    let view = SubmenuBuilder::new(app, "View").fullscreen().build()?;

    let window = SubmenuBuilder::new(app, "Window")
        .minimize()
        .maximize()
        .build()?;

    let help = SubmenuBuilder::new(app, "Help")
        .item(&item("help-docs", "Cold Crabby Documentation", None)?)
        .item(&item("help-shortcuts", "Keyboard Shortcuts", None)?)
        .build()?;

    let menu = MenuBuilder::new(app)
        .items(&[&app_menu, &file, &edit, &view, &window, &help])
        .build()?;
    app.set_menu(menu)?;

    app.on_menu_event(|app, event| {
        let _ = app.emit(MENU_EVENT, event.id().as_ref());
    });
    Ok(())
}
