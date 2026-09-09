//! Plugin identity and stability.

/// Version of the plugin hook API a plugin was written against.
///
/// Bumped whenever an existing hook's *signature* or *contract* changes in a
/// way a plugin could observe. Adding a new defaulted hook does not bump it —
/// existing plugins keep compiling and keep behaving identically, which is the
/// property that lets the trait grow one milestone at a time.
pub const PLUGIN_API_VERSION: u32 = 1;

/// How settled a plugin is, and therefore how it is presented to the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stability {
    /// Off by default, grouped under **Experiments** in the settings UI, and
    /// free to change its settings between releases.
    Experimental,
    /// Held to the same compatibility bar as a core setting.
    Stable,
}

impl Stability {
    /// Short lowercase token used in schema fragments and log lines.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Experimental => "experimental",
            Self::Stable => "stable",
        }
    }
}

/// Identity of a plugin.
///
/// `id` is the namespace everything else keys off: the plugin's settings live
/// at `SlicingParams::plugins[id]`, its schema fragment is published under the
/// same key, and its stages are named `<id>:<stage>`. It must be a stable,
/// kebab-case token — renaming one silently orphans every saved profile that
/// configured it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PluginManifest {
    /// Stable namespace token, e.g. `"debug-capture"`.
    pub id: &'static str,
    /// Human-readable name shown as the settings group heading.
    pub name: &'static str,
    /// One-line summary shown under the group heading.
    pub description: &'static str,
    /// Whether this ships as an experiment or as settled behaviour.
    pub stability: Stability,
    /// The [`PLUGIN_API_VERSION`] this plugin was written against.
    pub api_version: u32,
}

impl PluginManifest {
    /// A manifest for an experiment, filled in with the current API version.
    pub const fn experiment(
        id: &'static str,
        name: &'static str,
        description: &'static str,
    ) -> Self {
        Self {
            id,
            name,
            description,
            stability: Stability::Experimental,
            api_version: PLUGIN_API_VERSION,
        }
    }

    /// A manifest for settled behaviour, filled in with the current API version.
    pub const fn stable(id: &'static str, name: &'static str, description: &'static str) -> Self {
        Self {
            id,
            name,
            description,
            stability: Stability::Stable,
            api_version: PLUGIN_API_VERSION,
        }
    }
}
