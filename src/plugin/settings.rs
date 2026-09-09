//! Reading a plugin's own settings out of [`SlicingParams`].
//!
//! Plugin settings are namespaced (`params.plugins["<id>"]`) rather than
//! flattened into the core key space. That buys three things: no collision
//! with the hundred-odd core settings, obvious ownership when reading a saved
//! profile, and a cache fingerprint that picks plugin state up for free —
//! `cache_fingerprint` serializes the whole struct, so a toggled plugin
//! changes the key and cannot hand back a stale G-code file.

use serde_json::{Map, Value};

use crate::settings::params::SlicingParams;

/// The key every plugin reserves inside its own namespace.
///
/// The engine synthesizes it, so a plugin never declares it and every
/// experiment's gate is spelled the same way — which is what lets the settings
/// UI hide a plugin's fields behind its own toggle using the `x-relevant-when`
/// machinery that already existed.
pub const ENABLED_KEY: &str = "enabled";

impl SlicingParams {
    /// The settings object a plugin owns, or `None` when the user has never
    /// configured it.
    ///
    /// Absence is normal: a plugin that has never been touched has no entry at
    /// all, which is exactly why an untouched build fingerprints as it always
    /// did. A plugin must therefore read every value with a default in hand.
    pub fn plugin_settings(&self, plugin_id: &str) -> Option<&Map<String, Value>> {
        self.plugins.get(plugin_id)?.as_object()
    }

    /// One raw value from a plugin's namespace.
    pub fn plugin_value(&self, plugin_id: &str, key: &str) -> Option<&Value> {
        self.plugin_settings(plugin_id)?.get(key)
    }

    /// Whether the plugin's reserved [`ENABLED_KEY`] is set.
    ///
    /// Defaults to `false`: an experiment the user has not switched on must
    /// not run, and a missing namespace and an explicit `false` mean the same
    /// thing.
    pub fn plugin_enabled(&self, plugin_id: &str) -> bool {
        self.plugin_value(plugin_id, ENABLED_KEY)
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }

    /// A boolean from a plugin's namespace, or `default` when unset or the
    /// wrong shape.
    pub fn plugin_bool(&self, plugin_id: &str, key: &str, default: bool) -> bool {
        self.plugin_value(plugin_id, key)
            .and_then(Value::as_bool)
            .unwrap_or(default)
    }

    /// A number from a plugin's namespace, or `default` when unset or the
    /// wrong shape.
    pub fn plugin_f64(&self, plugin_id: &str, key: &str, default: f64) -> f64 {
        self.plugin_value(plugin_id, key)
            .and_then(Value::as_f64)
            .unwrap_or(default)
    }

    /// An unsigned integer from a plugin's namespace, or `default` when unset,
    /// negative, or the wrong shape.
    pub fn plugin_u32(&self, plugin_id: &str, key: &str, default: u32) -> u32 {
        self.plugin_value(plugin_id, key)
            .and_then(Value::as_u64)
            .and_then(|v| u32::try_from(v).ok())
            .unwrap_or(default)
    }

    /// A string from a plugin's namespace, or `None` when unset or the wrong
    /// shape.
    pub fn plugin_str(&self, plugin_id: &str, key: &str) -> Option<&str> {
        self.plugin_value(plugin_id, key).and_then(Value::as_str)
    }

    /// Overwrite a single value in a plugin's namespace, creating the
    /// namespace if it does not exist yet.
    ///
    /// Mostly for tests and for the CLI; the UI writes the whole nested object
    /// through the ordinary settings path.
    pub fn set_plugin_value(&mut self, plugin_id: &str, key: &str, value: Value) {
        let entry = self
            .plugins
            .entry(plugin_id.to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        if !entry.is_object() {
            *entry = Value::Object(Map::new());
        }
        if let Some(object) = entry.as_object_mut() {
            object.insert(key.to_string(), value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn params_with(plugin: &str, settings: Value) -> SlicingParams {
        let mut params = SlicingParams::default();
        params.plugins.insert(plugin.to_string(), settings);
        params
    }

    #[test]
    fn an_unconfigured_plugin_reads_as_disabled_with_defaults() {
        let params = SlicingParams::default();
        assert!(!params.plugin_enabled("nobody"));
        assert_eq!(params.plugin_f64("nobody", "amount", 1.5), 1.5);
        assert_eq!(params.plugin_u32("nobody", "count", 3), 3);
        assert!(params.plugin_str("nobody", "mode").is_none());
    }

    #[test]
    fn values_of_the_wrong_shape_fall_back_rather_than_panic() {
        // A profile written by a different build, or hand-edited TOML, can put
        // anything here. Reading must degrade to the default, never abort a
        // slice.
        let params = params_with(
            "demo",
            json!({ "enabled": "yes", "amount": "lots", "count": -1 }),
        );
        assert!(!params.plugin_enabled("demo"));
        assert_eq!(params.plugin_f64("demo", "amount", 0.25), 0.25);
        assert_eq!(params.plugin_u32("demo", "count", 7), 7);
    }

    #[test]
    fn set_plugin_value_creates_the_namespace() {
        let mut params = SlicingParams::default();
        params.set_plugin_value("demo", ENABLED_KEY, json!(true));
        params.set_plugin_value("demo", "amount", json!(0.4));
        assert!(params.plugin_enabled("demo"));
        assert_eq!(params.plugin_f64("demo", "amount", 0.0), 0.4);
    }

    #[test]
    fn an_empty_plugin_map_leaves_the_cache_fingerprint_as_it_was() {
        // The whole point of skip_serializing_if: a build that ships no
        // configured plugin must fingerprint exactly as it did before plugins
        // existed, or every cached G-code file is invalidated on upgrade.
        let plain = SlicingParams::default();
        assert!(
            !plain.cache_fingerprint().contains("plugins"),
            "an empty plugin map must not appear in the fingerprint"
        );
    }

    #[test]
    fn toggling_a_plugin_changes_the_cache_fingerprint() {
        // The correctness half: without this a user turns an experiment on and
        // gets handed back the G-code from before they did.
        let off = SlicingParams::default();
        let mut on = SlicingParams::default();
        on.set_plugin_value("demo", ENABLED_KEY, json!(true));
        assert_ne!(off.cache_fingerprint(), on.cache_fingerprint());

        let mut tuned = on.clone();
        tuned.set_plugin_value("demo", "amount", json!(0.4));
        assert_ne!(on.cache_fingerprint(), tuned.cache_fingerprint());
    }

    #[test]
    fn plugin_settings_survive_a_json_round_trip() {
        // SlicingParams has no deny_unknown_fields anywhere, so an unknown key
        // is silently dropped. A namespaced bag is what stops a plugin's
        // settings vanishing that way.
        let mut params = SlicingParams::default();
        params.set_plugin_value("demo", ENABLED_KEY, json!(true));
        params.set_plugin_value("demo", "amount", json!(0.4));

        let text = serde_json::to_string(&params).unwrap();
        let back: SlicingParams = serde_json::from_str(&text).unwrap();
        assert!(back.plugin_enabled("demo"));
        assert_eq!(back.plugin_f64("demo", "amount", 0.0), 0.4);
    }
}
