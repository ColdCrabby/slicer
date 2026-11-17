//! Grafting each plugin's settings schema onto the generated `SlicingParams`
//! schema.
//!
//! The settings form is generated entirely from JSON Schema — a field with an
//! `x-group` becomes a labelled, grouped, validated control with no Angular
//! code at all. So a plugin that emits a schema fragment gets its whole
//! settings UI for free, and this module is the seam where that happens: it
//! walks the plugin set at generation time and writes each fragment under
//! `properties.plugins.properties.<id>`.
//!
//! Two things are added rather than asked of the plugin, so every experiment
//! behaves the same way:
//!
//! - the reserved `enabled` toggle, titled with the plugin's own name; and
//! - an `x-relevant-when` gate on every other field, pointing at that toggle,
//!   so a plugin's settings stay hidden until it is switched on.

use serde_json::{json, Map, Value};

use crate::plugin::manifest::Stability;
use crate::plugin::settings::ENABLED_KEY;
use crate::plugin::Plugin;

/// The `x-group` every plugin's settings render under.
///
/// One group rather than one per plugin, deliberately: `x-group` names are a
/// closed taxonomy claimed by a settings contract in the UI (and pinned by a
/// test), so a plugin inventing its own group name would land unclaimed and
/// iconless at the bottom of Process.
pub const EXPERIMENTS_GROUP: &str = "Experiments";

/// Write every plugin's settings fragment into `schema`.
///
/// `schema` is the generated `SlicingParams` schema — either the root object
/// or a `$defs` entry; both shapes are handled. A plugin with no fragment
/// contributes only its `enabled` toggle, so it is still switchable from the
/// UI. A no-op when the plugin set is empty, which is what keeps a build with
/// no plugins byte-identical to one from before this module existed.
pub fn inject_plugin_settings(schema: &mut Value, plugins: &[Box<dyn Plugin>]) {
    if plugins.is_empty() {
        return;
    }
    let render = || -> Map<String, Value> {
        plugins
            .iter()
            .map(|plugin| {
                let manifest = plugin.manifest();
                (
                    manifest.id.to_string(),
                    namespace_schema(
                        manifest.name,
                        manifest.description,
                        manifest.stability,
                        manifest.id,
                        plugin.settings_schema(),
                    ),
                )
            })
            .collect()
    };
    inject_everywhere(schema, &render);
}

/// Build one plugin's namespace object: the reserved toggle plus its own
/// fields, each gated on that toggle.
fn namespace_schema(
    name: &str,
    description: &str,
    stability: Stability,
    plugin_id: &str,
    fragment: Option<Value>,
) -> Value {
    let mut properties = Map::new();
    properties.insert(
        ENABLED_KEY.to_string(),
        json!({
            "type": "boolean",
            "title": name,
            "description": description,
            "default": false,
            "x-group": EXPERIMENTS_GROUP,
        }),
    );

    // The gate every other field points at. Written as the full dotted path
    // because the UI evaluates relevance against the whole settings object,
    // not against the plugin's namespace in isolation.
    let gate = json!({
        "field": format!("plugins.{}.{}", plugin_id, ENABLED_KEY),
        "equals": true,
    });

    if let Some(fields) = fragment
        .as_ref()
        .and_then(|f| f.get("properties"))
        .and_then(Value::as_object)
    {
        for (key, field) in fields {
            if key == ENABLED_KEY {
                // Reserved. A plugin redeclaring it would be redefining its own
                // gate, which is the one field it does not own.
                continue;
            }
            let mut field = field.clone();
            if let Some(object) = field.as_object_mut() {
                object.insert("x-group".to_string(), json!(EXPERIMENTS_GROUP));
                object
                    .entry("x-relevant-when")
                    .or_insert_with(|| gate.clone());
            }
            properties.insert(key.clone(), field);
        }
    }

    json!({
        "type": "object",
        "title": name,
        "description": description,
        "x-plugin-stability": stability.as_str(),
        "properties": Value::Object(properties),
        "additionalProperties": false,
    })
}

/// Inject `render` into every `plugins` bag anywhere in `schema`.
///
/// The generated documents nest `SlicingParams` in several shapes — sometimes
/// as the root object, sometimes behind a `$ref` into `$defs`, sometimes as
/// one field of a wrapper that has `properties` of its own, and sometimes as
/// the `params` bag of a profile. Looking only at the first `properties` found
/// is what made this silently do nothing for the very schema the settings UI
/// reads: the wrapper's own `properties` was found first, had no `plugins` key,
/// and the fragment went nowhere.
///
/// So the whole document is walked and every `plugins` object is filled in.
/// There is exactly one meaning of that key, so there is no ambiguity to
/// resolve — and a schema that embeds `SlicingParams` twice gets both.
fn inject_everywhere(schema: &mut Value, render: &dyn Fn() -> Map<String, Value>) {
    match schema {
        Value::Object(map) => {
            if let Some(Value::Object(properties)) = map.get_mut("properties") {
                if let Some(Value::Object(bag)) = properties.get_mut("plugins") {
                    let entry = bag
                        .entry("properties")
                        .or_insert_with(|| Value::Object(Map::new()));
                    if let Value::Object(target) = entry {
                        for (id, fragment) in render() {
                            target.insert(id, fragment);
                        }
                    }
                }
            }
            for (_, child) in map.iter_mut() {
                inject_everywhere(child, render);
            }
        }
        Value::Array(items) => {
            for item in items {
                inject_everywhere(item, render);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::manifest::PluginManifest;

    struct Demo;

    impl Plugin for Demo {
        fn manifest(&self) -> PluginManifest {
            PluginManifest::experiment("demo", "Demo", "A demonstration plugin.")
        }
        fn settings_schema(&self) -> Option<Value> {
            Some(json!({
                "properties": {
                    "amount": { "type": "number", "title": "Amount", "default": 0.3 },
                    "enabled": { "type": "string" },
                }
            }))
        }
    }

    fn injected() -> Value {
        let mut schema = json!({
            "type": "object",
            "properties": { "plugins": { "type": "object", "properties": {} } }
        });
        let plugins: Vec<Box<dyn Plugin>> = vec![Box::new(Demo)];
        inject_plugin_settings(&mut schema, &plugins);
        schema
    }

    fn demo_props(schema: &Value) -> &Map<String, Value> {
        schema["properties"]["plugins"]["properties"]["demo"]["properties"]
            .as_object()
            .unwrap()
    }

    #[test]
    fn the_engine_supplies_the_enabled_toggle() {
        let schema = injected();
        let enabled = &demo_props(&schema)[ENABLED_KEY];
        assert_eq!(enabled["type"], json!("boolean"));
        assert_eq!(enabled["title"], json!("Demo"));
        assert_eq!(enabled["default"], json!(false));
    }

    #[test]
    fn a_plugin_cannot_redeclare_its_own_gate() {
        // Demo declares `enabled` as a string. Letting that through would give
        // the plugin a text box where its on/off switch should be.
        let schema = injected();
        assert_eq!(demo_props(&schema)[ENABLED_KEY]["type"], json!("boolean"));
    }

    #[test]
    fn every_other_field_is_gated_on_that_toggle() {
        let schema = injected();
        let amount = &demo_props(&schema)["amount"];
        assert_eq!(amount["x-group"], json!(EXPERIMENTS_GROUP));
        assert_eq!(
            amount["x-relevant-when"],
            json!({ "field": "plugins.demo.enabled", "equals": true })
        );
    }

    #[test]
    fn an_empty_plugin_set_leaves_the_schema_untouched() {
        let before = json!({
            "type": "object",
            "properties": { "plugins": { "type": "object", "properties": {} } }
        });
        let mut after = before.clone();
        inject_plugin_settings(&mut after, &[]);
        assert_eq!(before, after);
    }

    #[test]
    fn a_ref_wrapped_schema_is_resolved_through_defs() {
        let mut schema = json!({
            "$ref": "#/$defs/SlicingParams",
            "$defs": {
                "SlicingParams": {
                    "type": "object",
                    "properties": { "plugins": { "type": "object" } }
                }
            }
        });
        let plugins: Vec<Box<dyn Plugin>> = vec![Box::new(Demo)];
        inject_plugin_settings(&mut schema, &plugins);
        assert!(
            schema["$defs"]["SlicingParams"]["properties"]["plugins"]["properties"]["demo"]
                .is_object(),
            "the fragment must land inside the $defs entry, not beside the $ref"
        );
    }

    #[test]
    fn a_wrapper_with_properties_of_its_own_does_not_hide_the_bag() {
        // The shape the settings UI actually reads: a root object whose own
        // `properties` holds a single `params` field, with SlicingParams over
        // in `$defs`. Stopping at the first `properties` found means the
        // fragment goes nowhere — silently, because there is nothing to error
        // about.
        let mut schema = json!({
            "type": "object",
            "properties": { "params": { "$ref": "#/$defs/SlicingParams" } },
            "$defs": {
                "SlicingParams": {
                    "type": "object",
                    "properties": {
                        "layer_height": { "type": "number" },
                        "plugins": { "type": "object", "properties": {} }
                    }
                }
            }
        });
        let plugins: Vec<Box<dyn Plugin>> = vec![Box::new(Demo)];
        inject_plugin_settings(&mut schema, &plugins);
        assert!(
            schema["$defs"]["SlicingParams"]["properties"]["plugins"]["properties"]["demo"]
                .is_object(),
            "the fragment must reach the bag however deeply it is nested"
        );
    }

    #[test]
    fn every_embedded_copy_gets_the_fragment() {
        // Several generated documents embed SlicingParams more than once — the
        // profile bags alongside the settings schema. All of them should offer
        // the same plugin settings.
        let mut schema = json!({
            "$defs": {
                "A": { "properties": { "plugins": { "type": "object" } } },
                "B": { "properties": { "plugins": { "type": "object" } } }
            }
        });
        let plugins: Vec<Box<dyn Plugin>> = vec![Box::new(Demo)];
        inject_plugin_settings(&mut schema, &plugins);
        for def in ["A", "B"] {
            assert!(
                schema["$defs"][def]["properties"]["plugins"]["properties"]["demo"].is_object(),
                "{def} missed the fragment"
            );
        }
    }
}
