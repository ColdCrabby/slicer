//! Native context menus.
//!
//! Desktop gets a real OS menu from `@tauri-apps/api/menu` directly in the
//! webview — Tauri already implements that. iOS has no such API (Tauri gates its
//! whole `menu` module behind `#[cfg(desktop)]`), so this module supplies the
//! iOS equivalent from UIKit rather than letting the UI fall back to an
//! HTML-drawn imitation.
//!
//! The iOS-native idiom for "show these actions, anchored here" is a
//! `UIAlertController` in `.actionSheet` style, which UIKit renders as a real
//! popover on iPad (and a bottom sheet on iPhone). That gives us the system
//! blur, typography, Dynamic Type, dark mode, VoiceOver and dismissal gestures
//! for free — none of which an HTML menu reproduces faithfully.

use serde::Deserialize;

/// One entry in a context menu, mirroring the UI's `ContextMenuItem`.
///
/// The fields are only *read* by the iOS implementation, but the struct must
/// exist on every target so the command signature compiles everywhere.
#[derive(Debug, Clone, Deserialize)]
#[cfg_attr(not(target_os = "ios"), allow(dead_code))]
pub struct ContextMenuItem {
    pub label: String,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default)]
    pub separator: bool,
    /// Destructive actions (delete/remove) get the system's red treatment.
    #[serde(default)]
    pub danger: bool,
}

#[cfg(target_os = "ios")]
mod imp {
    use super::ContextMenuItem;

    use std::ptr::NonNull;
    use std::sync::mpsc;

    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2::MainThreadMarker;
    use objc2_core_foundation::{CGPoint, CGRect, CGSize};
    use objc2_foundation::NSString;
    use objc2_ui_kit::{
        UIAlertAction, UIAlertActionStyle, UIAlertController, UIAlertControllerStyle,
        UIApplication, UIPopoverArrowDirection, UIViewController,
    };

    /// Size of the anchor rect handed to the popover. A zero-size rect at the
    /// touch point makes UIKit aim the arrow exactly where the finger was.
    const ANCHOR_SIZE: CGSize = CGSize {
        width: 1.0,
        height: 1.0,
    };

    /// The view controller a modal should be presented from: the deepest
    /// controller already presenting, so we never try to present on top of a
    /// controller that is itself covered (UIKit refuses, and the menu silently
    /// never appears).
    fn topmost_view_controller(mtm: MainThreadMarker) -> Option<Retained<UIViewController>> {
        let app = UIApplication::sharedApplication(mtm);

        // `keyWindow` is deprecated but still populated for the single-window
        // app Tauri builds; fall back to scanning the window list.
        #[allow(deprecated)]
        let window = app.keyWindow().or_else(|| {
            app.windows()
                .iter()
                .find(|window| window.isKeyWindow())
                .or_else(|| app.windows().iter().next())
        })?;

        let mut controller = window.rootViewController()?;
        while let Some(presented) = controller.presentedViewController() {
            controller = presented;
        }
        Some(controller)
    }

    /// Present `items` as a native action sheet anchored at `(x, y)` in webview
    /// coordinates, sending the index of the chosen item to `tx`.
    ///
    /// Returns as soon as the sheet is on screen. **Must not block**: this runs
    /// on the main thread, which is also the UI event loop, so waiting here
    /// would freeze the very sheet we just presented. The result arrives later
    /// through `tx`, and dismissal is signalled by the channel disconnecting
    /// when UIKit releases the handler blocks.
    pub fn present(items: Vec<ContextMenuItem>, x: f64, y: f64, tx: mpsc::Sender<usize>) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let Some(controller) = topmost_view_controller(mtm) else {
            return;
        };

        let alert = UIAlertController::alertControllerWithTitle_message_preferredStyle(
            None,
            None,
            UIAlertControllerStyle::ActionSheet,
            mtm,
        );

        for (index, item) in items.iter().enumerate() {
            // Action sheets have no separator concept; the grouping a divider
            // implies simply does not exist in this idiom.
            if item.separator {
                continue;
            }

            let style = if item.danger {
                UIAlertActionStyle::Destructive
            } else {
                UIAlertActionStyle::Default
            };

            // Each handler owns a clone of the sender, so the channel stays
            // open exactly as long as the sheet does.
            let tx = tx.clone();
            let handler = RcBlock::new(move |_: NonNull<UIAlertAction>| {
                let _ = tx.send(index);
            });

            let action = UIAlertAction::actionWithTitle_style_handler(
                Some(&NSString::from_str(&item.label)),
                style,
                Some(&handler),
                mtm,
            );
            action.setEnabled(!item.disabled);
            alert.addAction(&action);
        }

        // Without a cancel action an action sheet is a trap on iPhone: sheets
        // there are modal from the bottom and ignore taps outside, so the only
        // way out would be to pick something. UIKit hides this button when the
        // sheet is presented as an iPad popover (dismissal is the outside tap),
        // so it costs nothing on the primary target.
        //
        // No handler: dropping the block is itself the "dismissed" signal, since
        // that releases the last sender and disconnects the channel.
        let cancel = UIAlertAction::actionWithTitle_style_handler(
            Some(&NSString::from_str("Cancel")),
            UIAlertActionStyle::Cancel,
            None,
            mtm,
        );
        alert.addAction(&cancel);

        // An action sheet on iPad *must* have a popover anchor or UIKit raises.
        // The webview fills the window, so webview coordinates are the
        // presenting view's coordinates.
        if let (Some(popover), Some(view)) =
            (alert.popoverPresentationController(), controller.view())
        {
            popover.setSourceView(Some(&view));
            popover.setSourceRect(CGRect {
                origin: CGPoint { x, y },
                size: ANCHOR_SIZE,
            });
            // A context menu points at a spot, not at a control with sides, so
            // let UIKit place the arrow wherever it fits best.
            popover.setPermittedArrowDirections(UIPopoverArrowDirection::Any);
        }

        controller.presentViewController_animated_completion(&alert, true, None);
    }
}

/// Show a native context menu at `(x, y)` in webview coordinates and resolve to
/// the index of the chosen item, or `None` when dismissed.
///
/// Only iOS implements this: desktop builds a real menu in the webview through
/// `@tauri-apps/api/menu`, and the browser has no native menu to borrow.
#[tauri::command]
pub async fn show_context_menu(
    app: tauri::AppHandle,
    items: Vec<ContextMenuItem>,
    x: f64,
    y: f64,
) -> Result<Option<usize>, String> {
    #[cfg(target_os = "ios")]
    {
        // UIKit is main-thread-only, so presentation hops to the main thread —
        // but it returns immediately rather than blocking the event loop. The
        // user's choice arrives on this channel; if they dismiss instead, UIKit
        // releases the handlers, every sender drops, and `recv` reports the
        // disconnect, which is exactly "nothing was chosen".
        let (tx, rx) = std::sync::mpsc::channel::<usize>();

        app.run_on_main_thread(move || imp::present(items, x, y, tx))
            .map_err(|e| e.to_string())?;

        tauri::async_runtime::spawn_blocking(move || rx.recv().ok())
            .await
            .map_err(|e| e.to_string())
    }

    #[cfg(not(target_os = "ios"))]
    {
        let _ = (app, items, x, y);
        Err("native context menus are only implemented for iOS here; desktop uses @tauri-apps/api/menu".into())
    }
}
