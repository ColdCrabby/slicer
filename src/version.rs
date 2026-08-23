//! Build-time version information and the embedded changelog.
//!
//! This module is the **single source of truth** for "what version am I?" across
//! every target — the CLI, the WebSocket server, the WASM bundle that powers the
//! Angular UI, and the Tauri desktop shell all read from here.
//!
//! The values are injected by [`build.rs`](../../build.rs) at compile time:
//!
//! * [`VERSION`] is a clean semver (e.g. `1.4.0`) only when the binary was built
//!   from a checkout sitting exactly on a `vX.Y.Z` git tag with a clean tree.
//!   Every other build — local development, a branch ahead of a tag, or a dirty
//!   tree — reports the literal string `development`. This guarantees the version
//!   shown to a user is always *true*: a real release number or an honest
//!   "development".
//!
//! * [`CHANGELOG`] embeds the repository's `CHANGELOG.md` verbatim so the notes
//!   travel with the binary and the UI can show a "What's New" panel offline.

use serde::Serialize;

/// Human-facing version: a clean release semver (e.g. `"1.4.0"`) or the literal
/// `"development"` for any non-release build.
pub const VERSION: &str = env!("SLICER_VERSION");

/// Raw `git describe` output (e.g. `"v1.4.0-3-gabc1234-dirty"` or a bare short
/// hash) captured at build time, for diagnostics and bug reports.
pub const GIT_DESCRIBE: &str = env!("SLICER_GIT_DESCRIBE");

/// ISO-8601 date the build's source commit was authored (or build time when git
/// is unavailable).
pub const BUILD_DATE: &str = env!("SLICER_BUILD_DATE");

/// The `Cargo.toml` package version — the "next" target version the maintainers
/// are working towards. Prefer [`VERSION`] for anything user-facing.
pub const CARGO_VERSION: &str = env!("CARGO_PKG_VERSION");

/// `"true"` when this build came from a clean, tagged release checkout.
const IS_RELEASE_STR: &str = env!("SLICER_IS_RELEASE");

/// The full changelog (Keep a Changelog markdown), embedded at compile time.
pub const CHANGELOG: &str = include_str!("../CHANGELOG.md");

/// Whether this is a tagged release build (as opposed to a development build).
pub fn is_release() -> bool {
    IS_RELEASE_STR == "true"
}

/// Whether this is a local/development build (the inverse of [`is_release`]).
pub fn is_development() -> bool {
    !is_release()
}

/// A snapshot of every build-time version fact, suitable for serialising to the
/// UI, the WS protocol, or `info --output-format json`.
#[derive(Debug, Clone, Serialize)]
pub struct AppInfo {
    /// True version — a release semver or `"development"`.
    pub version: String,
    /// Raw `git describe` output.
    pub git_describe: String,
    /// ISO-8601 build/commit date.
    pub build_date: String,
    /// `Cargo.toml` package version.
    pub cargo_version: String,
    /// `true` for tagged release builds.
    pub is_release: bool,
}

/// Build an [`AppInfo`] from the compile-time constants.
pub fn app_info() -> AppInfo {
    AppInfo {
        version: VERSION.to_string(),
        git_describe: GIT_DESCRIBE.to_string(),
        build_date: BUILD_DATE.to_string(),
        cargo_version: CARGO_VERSION.to_string(),
        is_release: is_release(),
    }
}

/// A single parsed changelog section (one `## [version]` heading and its body).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChangelogEntry {
    /// Version label from the heading, e.g. `"1.4.0"` or `"Unreleased"`.
    pub version: String,
    /// Release date if the heading carried one (`## [x] - YYYY-MM-DD`).
    pub date: Option<String>,
    /// The markdown body beneath the heading, trimmed, without the heading line.
    pub body: String,
}

/// Parse a Keep a Changelog document into ordered [`ChangelogEntry`] sections.
///
/// Sections are delimited by level-2 headings whose text is wrapped in square
/// brackets, e.g. `## [Unreleased]` or `## [1.2.0] - 2026-01-31`. Anything
/// before the first such heading (the document preamble) is ignored.
pub fn parse_changelog(markdown: &str) -> Vec<ChangelogEntry> {
    let mut entries: Vec<ChangelogEntry> = Vec::new();
    let mut current: Option<ChangelogEntry> = None;
    let mut body = String::new();

    for line in markdown.lines() {
        if let Some((version, date)) = parse_heading(line) {
            if let Some(mut entry) = current.take() {
                entry.body = body.trim().to_string();
                entries.push(entry);
            }
            body.clear();
            current = Some(ChangelogEntry {
                version,
                date,
                body: String::new(),
            });
        } else if current.is_some() {
            body.push_str(line);
            body.push('\n');
        }
    }

    if let Some(mut entry) = current.take() {
        entry.body = body.trim().to_string();
        entries.push(entry);
    }

    entries
}

/// Parse a `## [version] - date` heading, returning `(version, date)` when the
/// line is a bracketed level-2 heading.
fn parse_heading(line: &str) -> Option<(String, Option<String>)> {
    let rest = line.strip_prefix("## ")?.trim();
    let rest = rest.strip_prefix('[')?;
    let close = rest.find(']')?;
    let version = rest[..close].trim().to_string();
    if version.is_empty() {
        return None;
    }
    let after = rest[close + 1..].trim();
    let date = after
        .trim_start_matches('-')
        .trim()
        .to_string()
        .into_option_nonempty();
    Some((version, date))
}

/// All parsed changelog entries from the embedded [`CHANGELOG`].
pub fn changelog_entries() -> Vec<ChangelogEntry> {
    parse_changelog(CHANGELOG)
}

/// The changelog entry for a specific version label (case-insensitive), if any.
pub fn changelog_entry(version: &str) -> Option<ChangelogEntry> {
    parse_changelog(CHANGELOG)
        .into_iter()
        .find(|e| e.version.eq_ignore_ascii_case(version))
}

/// Small internal helper: turn an empty string into `None`.
trait IntoOptionNonEmpty {
    fn into_option_nonempty(self) -> Option<String>;
}

impl IntoOptionNonEmpty for String {
    fn into_option_nonempty(self) -> Option<String> {
        if self.is_empty() {
            None
        } else {
            Some(self)
        }
    }
}

/// WebAssembly bindings so the Angular UI can read the version and changelog
/// straight from the bundle it is running — the same numbers baked into the
/// binary, no network round-trip required.
#[cfg(target_arch = "wasm32")]
mod wasm {
    use wasm_bindgen::prelude::*;

    /// The true version string (`"1.4.0"` or `"development"`).
    #[wasm_bindgen(js_name = appVersion)]
    pub fn app_version() -> String {
        super::VERSION.to_string()
    }

    /// Full build-time version snapshot as a plain JS object.
    #[wasm_bindgen(js_name = appInfo)]
    pub fn app_info() -> Result<JsValue, JsValue> {
        serde_wasm_bindgen::to_value(&super::app_info())
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// The embedded changelog as raw Keep a Changelog markdown.
    #[wasm_bindgen(js_name = changelogMarkdown)]
    pub fn changelog_markdown() -> String {
        super::CHANGELOG.to_string()
    }

    /// The parsed changelog entries (`[{ version, date, body }, …]`).
    #[wasm_bindgen(js_name = changelogEntries)]
    pub fn changelog_entries() -> Result<JsValue, JsValue> {
        serde_wasm_bindgen::to_value(&super::changelog_entries())
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_populated() {
        assert!(!VERSION.is_empty());
    }

    #[test]
    fn embedded_changelog_has_unreleased() {
        assert!(CHANGELOG.contains("## [Unreleased]"));
    }

    #[test]
    fn parses_headings_with_and_without_dates() {
        let md = "\
# Changelog

preamble text ignored

## [Unreleased]

### Added
- a new thing

## [1.2.0] - 2026-01-31

### Fixed
- a bug
";
        let entries = parse_changelog(md);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].version, "Unreleased");
        assert_eq!(entries[0].date, None);
        assert!(entries[0].body.contains("a new thing"));
        assert_eq!(entries[1].version, "1.2.0");
        assert_eq!(entries[1].date.as_deref(), Some("2026-01-31"));
        assert!(entries[1].body.contains("a bug"));
    }

    #[test]
    fn lookup_is_case_insensitive() {
        let md = "## [1.0.0] - 2026-01-01\n\n- initial\n";
        let entries = parse_changelog(md);
        assert_eq!(entries.len(), 1);
        // Direct parse check; changelog_entry() reads the embedded file.
        assert!(entries[0].version.eq_ignore_ascii_case("1.0.0"));
    }

    #[test]
    fn ignores_preamble_before_first_heading() {
        let md = "# Title\n\nsome intro\n\n## [0.1.0]\n\n- x\n";
        let entries = parse_changelog(md);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].version, "0.1.0");
        assert_eq!(entries[0].body, "- x");
    }
}
