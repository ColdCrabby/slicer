//! Best-effort OS accent-colour detection, returned as a `#rrggbb` string.
//!
//! Deliberately dependency-free: it shells out to platform tools so the crate
//! manifest and cross-compilation stay untouched. Returns `None` when the
//! accent cannot be determined, letting the UI keep its brand default.

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
    use std::process::Command;

    let output = Command::new("reg")
        .args([
            "query",
            r"HKCU\Software\Microsoft\Windows\DWM",
            "/v",
            "AccentColor",
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8_lossy(&output.stdout);
    // The value prints as a `0xAABBGGRR` token (little-endian ABGR).
    let token = text.split_whitespace().find(|t| t.starts_with("0x"))?;
    let raw = u32::from_str_radix(token.trim_start_matches("0x"), 16).ok()?;

    let r = raw & 0xff;
    let g = (raw >> 8) & 0xff;
    let b = (raw >> 16) & 0xff;
    Some(format!("#{r:02x}{g:02x}{b:02x}"))
}
