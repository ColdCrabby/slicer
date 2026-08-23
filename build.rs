use std::process::Command;

fn main() {
    // Print cargo instructions
    println!(
        "cargo:rustc-env=PROFILE={}",
        std::env::var("PROFILE").unwrap()
    );

    emit_version();

    // Platform-specific settings
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();

    match target_os.as_str() {
        "windows" => {
            println!("cargo:rustc-env=PLATFORM_NAME=windows");
            println!("cargo:rustc-env=PLATFORM_ARCH={}", target_arch);
        }
        "macos" => {
            println!("cargo:rustc-env=PLATFORM_NAME=macos");
            println!("cargo:rustc-env=PLATFORM_ARCH={}", target_arch);
        }
        "unknown" if target_arch == "wasm32" => {
            println!("cargo:rustc-env=PLATFORM_NAME=wasm");
            println!("cargo:rustc-env=PLATFORM_ARCH=wasm32");
        }
        _ => {
            println!("cargo:rustc-env=PLATFORM_NAME=unknown");
            println!("cargo:rustc-env=PLATFORM_ARCH={}", target_arch);
        }
    }
}

/// Derive the "true" version from git tags and expose it (plus build metadata)
/// as compile-time environment variables consumed by `src/version.rs`.
///
/// Rules (single source of truth = git tags):
/// * A clean checkout sitting exactly on a `vX.Y.Z` tag  → that version.
/// * Anything else (untagged, ahead of a tag, or dirty)  → `development`.
///
/// `SLICER_VERSION` may be set explicitly in the environment to override the
/// git probe — used by release CI when a shallow checkout lacks tag history.
fn emit_version() {
    // Rebuild when the checked-out commit, tag set, or changelog changes so the
    // baked-in version never goes stale between builds.
    println!("cargo:rerun-if-changed=CHANGELOG.md");
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs/tags");
    println!("cargo:rerun-if-changed=.git/packed-refs");
    println!("cargo:rerun-if-env-changed=SLICER_VERSION");
    println!("cargo:rerun-if-env-changed=SLICER_GIT_SHA");

    let describe = git(&[
        "describe", "--tags", "--always", "--dirty", "--match", "v[0-9]*",
    ])
    .unwrap_or_else(|| "unknown".to_string());

    // Always-present short commit hash so even a clean, tagged release build can
    // report exactly which commit it was cut from — `git describe` omits the
    // hash when sitting on an exact tag, so we capture it separately.
    let git_sha = std::env::var("SLICER_GIT_SHA")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| git(&["rev-parse", "--short", "HEAD"]))
        .unwrap_or_else(|| "unknown".to_string());

    let version = match std::env::var("SLICER_VERSION") {
        Ok(v) if !v.trim().is_empty() => v.trim().to_string(),
        _ => {
            let exact_tag = git(&["describe", "--tags", "--exact-match", "--match", "v[0-9]*"]);
            let dirty = git(&["status", "--porcelain"])
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false);
            match (exact_tag, dirty) {
                (Some(tag), false) => tag.trim_start_matches('v').to_string(),
                _ => "development".to_string(),
            }
        }
    };

    let is_release = version != "development";

    // Prefer the committed date for reproducibility; fall back to build time.
    let build_date = git(&["log", "-1", "--format=%cI"])
        .or_else(|| std::env::var("SOURCE_DATE_EPOCH").ok())
        .unwrap_or_else(|| "unknown".to_string());

    println!("cargo:rustc-env=SLICER_VERSION={version}");
    println!("cargo:rustc-env=SLICER_GIT_DESCRIBE={describe}");
    println!("cargo:rustc-env=SLICER_GIT_SHA={git_sha}");
    println!("cargo:rustc-env=SLICER_BUILD_DATE={build_date}");
    println!("cargo:rustc-env=SLICER_IS_RELEASE={is_release}");
}

/// Run a `git` subcommand, returning trimmed stdout on success.
fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let text = text.trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}
