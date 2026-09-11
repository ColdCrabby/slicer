//! Models handed to the app by the operating system — "Open with Cold Crabby".
//!
//! Every platform we ship has a way to say *this file belongs to that app*, and
//! all of them arrive at different moments through different APIs:
//!
//! | Platform        | How the file arrives                                  |
//! | --------------- | ----------------------------------------------------- |
//! | Windows / Linux | `argv[1]` — a fresh launch, or the single-instance hop |
//! | macOS           | `RunEvent::Opened`, never `argv`                      |
//! | iOS / iPadOS    | `RunEvent::Opened`, from Files, Shapr3D, Mail, AirDrop |
//!
//! This module is the one place those three become the same thing: a list of
//! readable paths the webview can load, delivered both as an event and as a
//! drainable buffer.
//!
//! **Both delivery routes are required, not belt-and-braces.** A cold launch
//! races the webview: on iOS the URL arrives from
//! `application:openURL:options:` before the frontend has run a line of
//! JavaScript, so an event alone would be shouted into an empty room. The
//! frontend therefore drains [`take_opened_files`] once it is listening, and
//! anything that arrives afterwards comes through the event.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::Serialize;
#[cfg(any(target_os = "macos", target_os = "ios"))]
use tauri::Url;
use tauri::{AppHandle, Emitter, Manager, Runtime, State};

use slicer_engine::mesh::io::SUPPORTED_EXTENSIONS;

/// Event the webview listens on for models opened while it is already running.
const OPENED_EVENT: &str = "open-with://files";

/// One model the OS asked us to open.
#[derive(Clone, Debug, Serialize)]
pub struct OpenedFile {
    /// Absolute path the webview can read — inside the sandbox on iOS.
    pub path: String,
    /// The name to show, which is the *original* file's name even when the
    /// bytes were staged under a collision-proof one.
    pub file_name: String,
}

/// Models that arrived before the webview was listening.
///
/// Drained exactly once, by the first [`take_opened_files`] call — a second
/// drain would re-plate the same model after a reload.
#[derive(Default)]
pub struct OpenedFiles(Mutex<Vec<OpenedFile>>);

/// Hand the webview every model that arrived before it could listen.
#[tauri::command]
pub fn take_opened_files(state: State<'_, OpenedFiles>) -> Result<Vec<OpenedFile>, String> {
    let mut queue = state.0.lock().map_err(|e| e.to_string())?;
    Ok(std::mem::take(&mut *queue))
}

/// Ingest models named on the command line.
///
/// Used for the initial `argv` on Windows and Linux and for the argv a second
/// launch forwards through the single-instance plugin. Flags are skipped:
/// macOS appends `-psn_0_…` to a double-clicked bundle's argv, and a stray
/// `--flag` must never be mistaken for a model.
///
/// `already_running` says which of those two this is. A second launch raises
/// the window it hands the model to; the first must not, or it undoes the
/// hidden launch that keeps WebView2's cold start off screen.
#[cfg(desktop)]
pub fn ingest_args<R: Runtime, I, S>(app: &AppHandle<R>, args: I, already_running: bool)
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    ingest(
        app,
        args.into_iter()
            .skip(1)
            .filter(|arg| !arg.as_ref().starts_with('-'))
            .map(|arg| PathBuf::from(arg.as_ref())),
    );
    if already_running {
        raise(app);
    }
}

/// Ingest models from a `RunEvent::Opened`.
///
/// The only route on macOS and iOS. A URL that is not a `file://` URL is
/// ignored rather than rejected loudly — the same event carries custom-scheme
/// deep links, which are simply not this module's business.
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub fn ingest_urls<R: Runtime>(app: &AppHandle<R>, urls: &[Url]) {
    ingest(
        app,
        urls.iter().filter_map(|url| {
            let path = url.to_file_path().ok()?;
            // The URL, not the path, is what carries iOS's permission to read.
            Some(stage_url(app, url, &path).unwrap_or(path))
        }),
    );
}

fn ingest<R: Runtime, I: IntoIterator<Item = PathBuf>>(app: &AppHandle<R>, paths: I) {
    let opened: Vec<OpenedFile> = paths
        .into_iter()
        .filter(|path| is_model(path))
        .filter_map(|path| {
            let file_name = path.file_name()?.to_str()?.to_string();
            Some(OpenedFile {
                path: path.to_str()?.to_string(),
                file_name,
            })
        })
        .collect();

    if opened.is_empty() {
        return;
    }

    // Buffer first, then announce. A frontend that is already listening drains
    // nothing and acts on the event; one still booting finds the buffer full.
    // Doing it the other way round leaves a window where the event has fired
    // and the buffer is still empty.
    if let Ok(mut queue) = app.state::<OpenedFiles>().0.lock() {
        queue.extend(opened.iter().cloned());
    }
    let _ = app.emit(OPENED_EVENT, &opened);
}

/// Bring the window forward, because a model was opened into an app that was
/// already running and is probably behind something.
///
/// **Not called on the launch path.** Windows and Linux create the window
/// hidden on purpose and reveal it once the web UI has painted (see
/// [`lib.rs`](lib.rs)); showing it here would put WebView2's blank cold-start
/// frame back on screen. macOS needs no equivalent — it activates the app
/// itself when a document is opened into it.
#[cfg(desktop)]
fn raise<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Is this a model the engine can load?
///
/// The list is the engine's own, so a loader gained there is a format the OS
/// hands us here — no second catalogue to forget to update.
fn is_model(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .is_some_and(|ext| SUPPORTED_EXTENSIONS.contains(&ext.as_str()))
}

/// Copy an incoming iOS document into the app's own cache.
///
/// `LSSupportsOpeningDocumentsInPlace` means a model opened from Files, iCloud
/// Drive or another app's container stays *where it is* and reaches us as a
/// **security-scoped** URL: readable only between
/// `startAccessingSecurityScopedResource` and its `stop`, and only from native
/// code holding that `NSURL`. The webview has neither, so it would see a bare
/// sandbox denial. Copying the bytes inside the scope, into a directory we own,
/// is what makes the model readable for the rest of its life on the plate.
///
/// Returns `None` when the copy could not be made, leaving the original path in
/// play rather than dropping the model on the floor.
#[cfg(target_os = "ios")]
fn stage_url<R: Runtime>(app: &AppHandle<R>, url: &Url, path: &Path) -> Option<PathBuf> {
    use objc2_foundation::{NSString, NSURL};

    let ns_url = NSURL::URLWithString(&NSString::from_str(url.as_str()))?;
    let scoped = unsafe { ns_url.startAccessingSecurityScopedResource() };
    let bytes = std::fs::read(path);
    if scoped {
        unsafe { ns_url.stopAccessingSecurityScopedResource() };
    }
    let bytes = bytes.ok()?;

    let file_name = path.file_name()?.to_str()?;
    let dir = app.path().app_cache_dir().ok()?.join("opened");
    std::fs::create_dir_all(&dir).ok()?;
    // Two models named `part.stl` from two different apps must not become one.
    let staged = dir.join(format!(
        "{}-{file_name}",
        chrono::Utc::now().timestamp_millis()
    ));
    std::fs::write(&staged, bytes).ok()?;

    // Files handed over by *copy* rather than in place land in our own
    // `Documents/Inbox`, where iOS never cleans up after us. The staged copy is
    // the one the plate uses, so the inbox original is now litter.
    if path
        .parent()
        .is_some_and(|parent| parent.ends_with("Inbox"))
    {
        let _ = std::fs::remove_file(path);
    }

    Some(staged)
}

/// macOS reads the file where it lies — no sandbox to escape.
#[cfg(target_os = "macos")]
fn stage_url<R: Runtime>(_app: &AppHandle<R>, _url: &Url, _path: &Path) -> Option<PathBuf> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_every_format_the_engine_loads() {
        for ext in SUPPORTED_EXTENSIONS {
            assert!(is_model(&PathBuf::from(format!("/models/part.{ext}"))));
        }
    }

    #[test]
    fn extension_match_ignores_case() {
        assert!(is_model(&PathBuf::from("/models/Part.STL")));
        assert!(is_model(&PathBuf::from("/models/Part.3MF")));
    }

    #[test]
    fn rejects_anything_else() {
        assert!(!is_model(&PathBuf::from("/models/part.gcode")));
        assert!(!is_model(&PathBuf::from("/models/part")));
        assert!(!is_model(&PathBuf::from("/models/stl")));
    }
}
