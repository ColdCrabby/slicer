//! The plugin system — one hook interface, used by the engine's own optional
//! features first.
//!
//! The rule this module exists to defend:
//!
//! > **A plugin extends the engine through the same interface we use
//! > ourselves.** If a capability is only reachable by editing the pipeline,
//! > the API has failed.
//!
//! See [PLUGINS.md](../PLUGINS.md) for why the design has the shape it does,
//! and [README.md](README.md) for how the pieces fit together.
//!
//! ## What a plugin is, today
//!
//! A compile-time Rust value implementing [`Plugin`]. It ships in every build
//! and on every target — CLI, WebSocket server, browser WASM, Tauri desktop
//! and iOS — because it is linked in rather than loaded. That is deliberate:
//! anything requiring `dlopen` or a JIT is unavailable on the last two.
//!
//! A plugin declares:
//!
//! | Hook | Method | Serves |
//! | --- | --- | --- |
//! | Stage | [`Plugin::stages`] | work inserted into, or wrapped around, a pipeline step |
//! | Settings | [`Plugin::settings_schema`] | the plugin's own settings, and their whole UI |
//!
//! The registry and move-filter families named in the design arrive with the
//! milestones that give them something to attach to (a strategy registry; a
//! G-code move IR). Both are new **defaulted** trait methods when they land,
//! so no plugin written against this version has to change.
//!
//! ## Trust
//!
//! A compile-time plugin has **the same trust level as the engine** — it is
//! reviewed like core code and is not sandboxed. Isolation is what the later
//! external (WASM) tier is for. See the security model in
//! [PLUGINS.md](../PLUGINS.md), particularly what a promotion from that tier
//! into this one has to clear.

pub mod builtin;
pub mod context;
pub mod manifest;
pub mod schema;
pub mod settings;
pub mod stage;

pub use context::{Artifacts, Extensions, SliceContext};
pub use manifest::{PluginManifest, Stability, PLUGIN_API_VERSION};
pub use settings::ENABLED_KEY;
pub use stage::{
    FnStage, Placement, Stage, StageError, StageId, StageRegistration, StageRegistry, StageWrapper,
};

/// Something that extends the engine.
///
/// Every hook but [`Plugin::manifest`] is defaulted, so a plugin implements
/// only what it actually uses — and so a new hook family can be added without
/// breaking any plugin already written.
///
/// Implementations must be `Send + Sync`: the stages they register may run on
/// a rayon worker thread.
pub trait Plugin: Send + Sync {
    /// Identity, stability and API version.
    fn manifest(&self) -> PluginManifest;

    /// Stages to insert into, or wrap around, the pipeline.
    ///
    /// A registration names an existing stage and says which side of it the
    /// new work goes — see [`StageRegistration`]. Naming a stage that does not
    /// exist is an error rather than an append, so a plugin targeting a
    /// renamed stage says so instead of quietly running at the wrong point.
    fn stages(&self) -> Vec<StageRegistration> {
        Vec::new()
    }

    /// A JSON Schema fragment describing this plugin's settings.
    ///
    /// Only `properties` is read. The engine supplies the reserved `enabled`
    /// toggle and gates every declared field on it, so a fragment carries just
    /// the plugin's own knobs. `None` means the plugin has nothing to
    /// configure beyond being switched on.
    fn settings_schema(&self) -> Option<serde_json::Value> {
        None
    }
}

/// The plugins compiled into this build.
///
/// Empty for now: the two internal plugins that exist
/// ([`builtin::DebugCapture`], [`builtin::FuzzySkin`]) are installed by the
/// pipeline entry point that needs them rather than being always-on, and no
/// user-facing experiment has shipped yet. The settings UI's **Experiments**
/// group therefore appears exactly when the first one does.
pub fn builtin_plugins() -> Vec<Box<dyn Plugin>> {
    Vec::new()
}

/// Fold every plugin's stage registrations into `registry`.
///
/// A rejected registration is logged and skipped rather than aborting the
/// slice: one plugin naming a stage that no longer exists should cost that
/// plugin's feature, not the user's print.
pub fn install(
    registry: &mut StageRegistry,
    plugins: &[Box<dyn Plugin>],
    logger: &dyn crate::logging::ProcessLogger,
) {
    for plugin in plugins {
        let manifest = plugin.manifest();
        if manifest.api_version != PLUGIN_API_VERSION {
            logger.log_warn(&format!(
                "plugin '{}' targets hook API v{} but this engine speaks v{} — skipped",
                manifest.id, manifest.api_version, PLUGIN_API_VERSION
            ));
            continue;
        }
        for registration in plugin.stages() {
            if let Err(err) = registry.apply(registration) {
                logger.log_warn(&format!("plugin '{}': {}", manifest.id, err));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logging::NullLogger;

    struct Stray;

    impl Plugin for Stray {
        fn manifest(&self) -> PluginManifest {
            PluginManifest::experiment("stray", "Stray", "Targets a stage that is not there.")
        }
        fn stages(&self) -> Vec<StageRegistration> {
            vec![StageRegistration::after(
                "no-such-stage",
                FnStage::boxed("stray:work", |_| {}),
            )]
        }
    }

    struct FromTheFuture;

    impl Plugin for FromTheFuture {
        fn manifest(&self) -> PluginManifest {
            PluginManifest {
                api_version: PLUGIN_API_VERSION + 1,
                ..PluginManifest::experiment("future", "Future", "Speaks a later hook API.")
            }
        }
        fn stages(&self) -> Vec<StageRegistration> {
            vec![StageRegistration::after(
                "a",
                FnStage::boxed("future:work", |_| {}),
            )]
        }
    }

    fn registry_with_one_stage() -> StageRegistry {
        let mut registry = StageRegistry::new();
        registry.push(FnStage::boxed("a", |_| {}));
        registry
    }

    #[test]
    fn a_misaimed_registration_costs_only_that_plugin() {
        let mut registry = registry_with_one_stage();
        let plugins: Vec<Box<dyn Plugin>> = vec![Box::new(Stray)];
        install(&mut registry, &plugins, &NullLogger);
        assert_eq!(registry.len(), 1, "the core pipeline must be left intact");
    }

    #[test]
    fn a_plugin_from_a_later_api_version_is_not_installed() {
        let mut registry = registry_with_one_stage();
        let plugins: Vec<Box<dyn Plugin>> = vec![Box::new(FromTheFuture)];
        install(&mut registry, &plugins, &NullLogger);
        assert_eq!(registry.len(), 1);
    }

    #[test]
    fn the_shipped_plugin_set_is_self_consistent() {
        // Guards the two things a plugin can get wrong before it ever runs:
        // an id that does not match its namespace conventions, and an API
        // version this engine does not speak.
        for plugin in builtin_plugins() {
            let m = plugin.manifest();
            assert_eq!(m.api_version, PLUGIN_API_VERSION, "plugin '{}'", m.id);
            assert!(
                !m.id.is_empty()
                    && m.id
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
                "plugin id '{}' must be kebab-case ascii — it is a settings namespace",
                m.id
            );
        }
    }
}
