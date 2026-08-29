//! Native iOS dialogs and the share sheet.
//!
//! Companion to [`crate::context_menu`]: the same "use the platform's own UI
//! rather than an HTML lookalike" idea, applied to the two other places the app
//! talks to the user.
//!
//! - **Alerts / confirmations.** A `UIAlertController` in `.alert` style, with
//!   the destructive choice in system red. HTML modals still handle rich
//!   dialogs that embed a component; only plain title/message/confirm/cancel
//!   dialogs route here.
//! - **Sharing a file.** `UIActivityViewController`, the standard "export this"
//!   surface (Save to Files, AirDrop, Mail, …). iOS has no Save-As panel, so
//!   this is what replaces the desktop save dialog — see [`share_file`].

use serde::Deserialize;

/// A plain confirm/alert, mirroring the UI's `DialogConfig`.
///
/// Only the fields a native alert can express: a dialog carrying a `content`
/// component keeps using the HTML implementation.
#[derive(Debug, Clone, Deserialize)]
#[cfg_attr(not(target_os = "ios"), allow(dead_code))]
pub struct NativeDialogRequest {
    pub title: String,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub confirm_label: Option<String>,
    /// Absent for an alert (single OK button).
    #[serde(default)]
    pub cancel_label: Option<String>,
    /// Render the confirm action in the system's destructive red.
    #[serde(default)]
    pub destructive: bool,
}

#[cfg(target_os = "ios")]
mod imp {
    use super::NativeDialogRequest;

    use std::ptr::NonNull;
    use std::sync::mpsc;

    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{MainThreadMarker, MainThreadOnly};
    use objc2_core_foundation::{CGPoint, CGRect, CGSize};
    use objc2_foundation::{NSArray, NSString, NSURL};
    use objc2_ui_kit::{
        UIActivityViewController, UIAlertAction, UIAlertActionStyle, UIAlertController,
        UIAlertControllerStyle, UIApplication, UIPopoverArrowDirection, UIViewController,
    };

    /// The view controller a modal should be presented from: the deepest one
    /// already presenting, so we never present onto a covered controller (UIKit
    /// refuses, and the dialog silently never appears).
    pub fn topmost_view_controller(mtm: MainThreadMarker) -> Option<Retained<UIViewController>> {
        let app = UIApplication::sharedApplication(mtm);

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

    /// Anchor a popover at a point, which iPad **requires** for any controller
    /// presented as one: without a source view UIKit raises
    /// `NSInvalidArgumentException` and the app dies.
    pub fn anchor(controller: &UIViewController, presenter: &UIViewController, x: f64, y: f64) {
        if let (Some(popover), Some(view)) =
            (controller.popoverPresentationController(), presenter.view())
        {
            popover.setSourceView(Some(&view));
            popover.setSourceRect(CGRect {
                origin: CGPoint { x, y },
                size: CGSize {
                    width: 1.0,
                    height: 1.0,
                },
            });
            popover.setPermittedArrowDirections(UIPopoverArrowDirection::Any);
        }
    }

    /// Present a native alert. Sends `true` on confirm, and drops the sender
    /// without sending on cancel — so a dismissal reads as "not confirmed".
    pub fn present_dialog(request: NativeDialogRequest, tx: mpsc::Sender<bool>) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let Some(controller) = topmost_view_controller(mtm) else {
            return;
        };

        let alert = UIAlertController::alertControllerWithTitle_message_preferredStyle(
            Some(&NSString::from_str(&request.title)),
            request
                .message
                .as_deref()
                .map(NSString::from_str)
                .as_deref(),
            UIAlertControllerStyle::Alert,
            mtm,
        );

        // Cancel first so UIKit lays it out on the left, matching every system
        // alert. Its handler sends nothing: dropping the sender is the signal.
        if let Some(cancel_label) = request.cancel_label.as_deref() {
            let cancel = UIAlertAction::actionWithTitle_style_handler(
                Some(&NSString::from_str(cancel_label)),
                UIAlertActionStyle::Cancel,
                None,
                mtm,
            );
            alert.addAction(&cancel);
        }

        let confirm_style = if request.destructive {
            UIAlertActionStyle::Destructive
        } else {
            UIAlertActionStyle::Default
        };
        let confirm_tx = tx.clone();
        let confirm_handler = RcBlock::new(move |_: NonNull<UIAlertAction>| {
            let _ = confirm_tx.send(true);
        });
        let confirm = UIAlertAction::actionWithTitle_style_handler(
            Some(&NSString::from_str(
                request.confirm_label.as_deref().unwrap_or("OK"),
            )),
            confirm_style,
            Some(&confirm_handler),
            mtm,
        );
        alert.addAction(&confirm);
        alert.setPreferredAction(Some(&confirm));

        // `.alert` is centred and never a popover, so no anchoring is needed.
        controller.presentViewController_animated_completion(&alert, true, None);
    }

    /// Present the system share sheet for a file already on disk.
    pub fn present_share(path: &str, x: f64, y: f64) -> Result<(), String> {
        let mtm = MainThreadMarker::new().ok_or("share sheet needs the main thread")?;
        let controller =
            topmost_view_controller(mtm).ok_or("no view controller to present from")?;

        let url = NSURL::fileURLWithPath(&NSString::from_str(path));
        // `UIActivityViewController` takes a heterogeneous item list, so the
        // array is typed as `AnyObject` rather than `NSURL`.
        let item: &AnyObject = &url;
        let items = NSArray::from_slice(&[item]);

        let activity = unsafe {
            UIActivityViewController::initWithActivityItems_applicationActivities(
                UIActivityViewController::alloc(mtm),
                &items,
                None,
            )
        };

        anchor(&activity, &controller, x, y);
        controller.presentViewController_animated_completion(&activity, true, None);
        Ok(())
    }
}

/// Show a native confirm/alert and resolve to whether the user confirmed.
#[tauri::command]
pub async fn show_native_dialog(
    app: tauri::AppHandle,
    request: NativeDialogRequest,
) -> Result<bool, String> {
    #[cfg(target_os = "ios")]
    {
        // As with the context menu: presentation hops to the main thread and
        // returns immediately, and the answer arrives on the channel. A
        // dismissal drops every sender, so `recv` fails — which is "cancelled".
        let (tx, rx) = std::sync::mpsc::channel::<bool>();

        app.run_on_main_thread(move || imp::present_dialog(request, tx))
            .map_err(|e| e.to_string())?;

        tauri::async_runtime::spawn_blocking(move || rx.recv().unwrap_or(false))
            .await
            .map_err(|e| e.to_string())
    }

    #[cfg(not(target_os = "ios"))]
    {
        let _ = (app, request);
        Err("native dialogs are only implemented on iOS".into())
    }
}

/// Share a file that already exists on disk through the system share sheet.
///
/// This is iOS's answer to a "Save As" dialog. The platform has no save panel:
/// the user picks a destination *inside* the share sheet (Save to Files,
/// AirDrop, Mail, …) and iOS performs the copy itself, with the real bytes.
///
/// `x`/`y` anchor the iPad popover — mandatory, or UIKit raises and the app
/// terminates.
#[tauri::command]
pub async fn share_file(app: tauri::AppHandle, path: String, x: f64, y: f64) -> Result<(), String> {
    #[cfg(target_os = "ios")]
    {
        let (tx, rx) = std::sync::mpsc::channel::<Result<(), String>>();

        app.run_on_main_thread(move || {
            let _ = tx.send(imp::present_share(&path, x, y));
        })
        .map_err(|e| e.to_string())?;

        tauri::async_runtime::spawn_blocking(move || {
            rx.recv()
                .unwrap_or_else(|_| Err("share sheet never reported back".into()))
        })
        .await
        .map_err(|e| e.to_string())?
    }

    #[cfg(not(target_os = "ios"))]
    {
        let _ = (app, path, x, y);
        Err("the share sheet is only implemented on iOS".into())
    }
}
