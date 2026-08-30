//! Best-effort OS accent-colour detection, returned as a `#rrggbb` string, plus
//! an **event-driven** watcher that pushes changes to the UI.
//!
//! The watcher does not poll. Each OS tells us when its accent changes:
//!
//! - **Windows** blocks a thread on `RegNotifyChangeKeyValue` for the DWM key
//!   and only wakes when it actually changes. The colour itself is read straight
//!   from the registry via `RegGetValueW` — no `reg.exe` child process, so
//!   nothing can flash a console window (the bug this replaced: a 2-second poll
//!   that shelled out to `reg query` popped a `cmd` window on every tick).
//! - **macOS** observes the `AppleColorPreferencesChangedNotification`
//!   distributed notification on the app's main run loop.
//!
//! Returns `None` when the accent cannot be determined, letting the UI keep its
//! brand default.

/// Event name emitted to the UI when the OS accent colour changes.
///
/// Mobile has no user-selectable accent, so the watcher — and this name with
/// it — is desktop-only.
#[cfg(desktop)]
pub const ACCENT_CHANGED_EVENT: &str = "system-accent-changed";

/// Watch the OS accent in the background and emit [`ACCENT_CHANGED_EVENT`]
/// (payload: the new `#rrggbb` string, or `null`) whenever it changes, so the
/// UI can recolour live without a restart. No-op on platforms without accent
/// detection.
///
/// Desktop only: mobile has no user-selectable accent colour, and the whole
/// watcher would be dead code there.
#[cfg(all(desktop, any(target_os = "macos", target_os = "windows")))]
pub fn spawn_watcher(app: tauri::AppHandle) {
    spawn_platform_watcher(app);
}

#[cfg(all(desktop, not(any(target_os = "macos", target_os = "windows"))))]
pub fn spawn_watcher(_app: tauri::AppHandle) {}

/// Windows: block a dedicated thread on registry change notifications for the
/// DWM key and re-read the accent when it fires.
#[cfg(target_os = "windows")]
fn spawn_platform_watcher(app: tauri::AppHandle) {
    use tauri::Emitter;

    std::thread::spawn(move || {
        let mut last = detect();
        loop {
            if !wait_for_accent_change() {
                // The key could not be watched (unexpected). Back off rather
                // than spin so a broken watch never pegs a core.
                std::thread::sleep(std::time::Duration::from_secs(5));
            }
            let current = detect();
            if current != last {
                last = current.clone();
                let _ = app.emit(ACCENT_CHANGED_EVENT, current);
            }
        }
    });
}

/// Block until the DWM accent registry key next changes. Synchronous
/// (`fAsynchronous = false`), so this call parks the thread with no polling.
/// Returns `false` if the key could not be watched.
#[cfg(target_os = "windows")]
fn wait_for_accent_change() -> bool {
    use windows::core::w;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegNotifyChangeKeyValue, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, KEY_NOTIFY,
        REG_NOTIFY_CHANGE_LAST_SET,
    };

    unsafe {
        let mut hkey = HKEY(std::ptr::null_mut());
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\DWM"),
            None,
            KEY_NOTIFY,
            &mut hkey,
        )
        .is_err()
        {
            return false;
        }

        let status = RegNotifyChangeKeyValue(hkey, false, REG_NOTIFY_CHANGE_LAST_SET, None, false);
        let _ = RegCloseKey(hkey);

        status.is_ok()
    }
}

/// macOS: observe the OS accent/appearance distributed notifications on the
/// app's main run loop. Registering here (from Tauri's `setup`, on the main
/// thread) means the app's own run loop delivers the callbacks — no extra
/// thread and no poll.
#[cfg(target_os = "macos")]
fn spawn_platform_watcher(app: tauri::AppHandle) {
    use std::cell::RefCell;
    use std::ptr::NonNull;

    use block2::RcBlock;
    use objc2_foundation::{NSDistributedNotificationCenter, NSNotification, NSString};
    use tauri::Emitter;

    // The block runs serially on the main run loop, so a `RefCell` is enough to
    // remember the last value and suppress no-op re-emits.
    let last = RefCell::new(detect());
    let block = RcBlock::new(move |_notification: NonNull<NSNotification>| {
        let current = detect();
        let mut last = last.borrow_mut();
        if *last != current {
            *last = current.clone();
            let _ = app.emit(ACCENT_CHANGED_EVENT, current);
        }
    });

    // Accent selection posts `AppleColorPreferencesChangedNotification`; the
    // older graphite/colour toggle posts `AppleAquaColorVariantChanged`. Watch
    // both so any control-tint change recolours the UI live.
    let names = [
        NSString::from_str("AppleColorPreferencesChangedNotification"),
        NSString::from_str("AppleAquaColorVariantChanged"),
    ];

    // SAFETY: `defaultCenter` is always valid; the block matches the observer's
    // `(NSNotification*)` signature and outlives the observers because both it
    // and the returned tokens are leaked for the app's lifetime.
    unsafe {
        let center = NSDistributedNotificationCenter::defaultCenter();
        for name in &names {
            let token =
                center.addObserverForName_object_queue_usingBlock(Some(name), None, None, &block);
            // The observer must live as long as the app; there is no teardown.
            std::mem::forget(token);
        }
    }
    std::mem::forget(block);
}

/// Detect the current OS accent colour as `#rrggbb`, if available.
pub fn detect() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        detect_macos()
    }
    #[cfg(target_os = "windows")]
    {
        detect_windows()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        None
    }
}

#[cfg(target_os = "macos")]
fn detect_macos() -> Option<String> {
    use std::process::Command;

    // `AppleAccentColor` is an integer index. When the key is absent the user
    // is on the default ("Multicolor"), whose effective control accent is blue.
    let output = Command::new("defaults")
        .args(["read", "-g", "AppleAccentColor"])
        .output()
        .ok()?;

    let hex = if output.status.success() {
        let raw = String::from_utf8_lossy(&output.stdout);
        match raw.trim().parse::<i32>() {
            Ok(-1) => "#98989d", // graphite
            Ok(0) => "#ff5257",  // red
            Ok(1) => "#f7821b",  // orange
            Ok(2) => "#ffc600",  // yellow
            Ok(3) => "#62ba46",  // green
            Ok(4) => "#007aff",  // blue
            Ok(5) => "#953d96",  // purple
            Ok(6) => "#f74f9e",  // pink
            _ => "#007aff",
        }
    } else {
        "#007aff"
    };

    Some(hex.to_string())
}

#[cfg(target_os = "windows")]
fn detect_windows() -> Option<String> {
    use windows::core::w;
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};

    // Read the DWORD straight from the registry — no child process, so nothing
    // can flash a console window. The value is `0xAABBGGRR` (little-endian ABGR).
    let mut data: u32 = 0;
    let mut size = std::mem::size_of::<u32>() as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\DWM"),
            w!("AccentColor"),
            RRF_RT_REG_DWORD,
            None,
            Some(std::ptr::addr_of_mut!(data).cast()),
            Some(&mut size),
        )
    };

    if status.is_err() {
        return None;
    }

    let r = data & 0xff;
    let g = (data >> 8) & 0xff;
    let b = (data >> 16) & 0xff;
    Some(format!("#{r:02x}{g:02x}{b:02x}"))
}
