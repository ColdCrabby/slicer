//! Tier 2 — externally loaded plugins, sandboxed, desktop only.
//!
//! See [PLUGINS.md](../../PLUGINS.md) for the trust model. The short version:
//! a Tier 1 plugin is compiled in and trusted like the engine; a Tier 2 module
//! is **contained by construction**, able to do only what the host hands it.
//!
//! The load-bearing property of the whole design is visible here: **the loader
//! is itself just a Tier 1 plugin.** It implements the same [`Plugin`] trait
//! everything else does, so adding it changed no hook signature.
//!
//! [`Plugin`]: crate::plugin::Plugin

pub mod abi;

#[cfg(all(
    feature = "external-plugins",
    not(target_arch = "wasm32"),
    not(target_os = "ios")
))]
mod host;

#[cfg(all(
    feature = "external-plugins",
    not(target_arch = "wasm32"),
    not(target_os = "ios")
))]
pub use host::{load_from, ExternalPlugins, LoadError};
