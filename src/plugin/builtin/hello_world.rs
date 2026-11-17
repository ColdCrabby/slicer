//! A worked example of a plugin, shipped so the hooks are exercised by
//! something a person can turn on.
//!
//! It exists to be read as much as to be run. Everything a real experiment
//! needs is here and nothing else is: a manifest, settings that generate their
//! own UI, a stage that reads the layers, and a filter that rewrites the
//! G-code program. Copy this file, replace the two `run` bodies, and you have
//! a plugin.
//!
//! It is off by default and does nothing until switched on, so a build that
//! ships it prints exactly what a build without it would.

use serde_json::json;

use crate::core::ExtrusionRole;
use crate::gcode::{Move, MoveFilter, MoveProgram};
use crate::plugin::{FnStage, Plugin, PluginManifest, SliceContext, StageRegistration};
use crate::settings::params::SlicingParams;

/// The plugin's id. Its settings live at `params.plugins["hello-world"]`, its
/// schema fragment is published under the same key, and its stages are named
/// with it — so this string is the one thing that must never change casually.
const ID: &str = "hello-world";

/// Default for the greeting, used when the user has not set one.
const DEFAULT_GREETING: &str = "Hello from a plugin";

/// A demonstration experiment: greets the G-code and counts the layers.
pub struct HelloWorld;

impl Plugin for HelloWorld {
    fn manifest(&self) -> PluginManifest {
        PluginManifest::experiment(
            ID,
            "Hello world",
            "A worked example. Writes a greeting into the G-code and reports \
             what it saw, so you can confirm plugins are running.",
        )
    }

    /// The plugin's own settings.
    ///
    /// Only `properties` is read, and only the plugin's *own* knobs go here —
    /// the engine adds the `enabled` toggle and gates everything below it on
    /// that, so an experiment cannot forget to hide its settings when it is
    /// switched off.
    fn settings_schema(&self) -> Option<serde_json::Value> {
        Some(json!({
            "properties": {
                "greeting": {
                    "type": "string",
                    "title": "Greeting",
                    "description": "Written into the G-code as a comment, once at the top.",
                    "default": DEFAULT_GREETING,
                },
                "count_layers": {
                    "type": "boolean",
                    "title": "Report layer count",
                    "description":
                        "Log how many layers were sliced, to confirm the plugin's stage ran.",
                    "default": true,
                },
            }
        }))
    }

    /// Where the plugin's work runs.
    ///
    /// A registration names an existing stage and says which side of it to go.
    /// Naming one that does not exist is an error, not an append — so a plugin
    /// targeting a renamed stage loses its own feature rather than quietly
    /// running at the wrong point.
    fn stages(&self) -> Vec<StageRegistration> {
        vec![StageRegistration::after(
            crate::core::stages::ids::SLICING,
            FnStage::boxed("hello-world:count", |cx: &mut SliceContext<'_>| {
                // Every hook checks this itself. An experiment the user has not
                // switched on must do nothing at all.
                if !cx.plugin_enabled(ID) || !cx.params.plugin_bool(ID, "count_layers", true) {
                    return;
                }
                let walls = cx
                    .layers
                    .iter()
                    .flat_map(|l| (0..l.paths.len()).map(|i| l.role_for_path(i)))
                    .filter(|r| *r == ExtrusionRole::OuterWall)
                    .count();
                cx.logger.log_info(&format!(
                    "hello-world: {} layers, {} outer-wall paths so far",
                    cx.layers.len(),
                    walls
                ));
            }),
        )]
    }

    fn move_filter(&self) -> Option<Box<dyn MoveFilter>> {
        Some(Box::new(Greeting))
    }
}

/// Writes the greeting into the program as a comment.
struct Greeting;

impl MoveFilter for Greeting {
    fn name(&self) -> &str {
        ID
    }

    fn filter(&self, program: &mut MoveProgram, params: &SlicingParams) {
        if !params.plugin_enabled(ID) {
            return;
        }
        let greeting = params
            .plugin_str(ID, "greeting")
            .unwrap_or(DEFAULT_GREETING)
            .trim();
        if greeting.is_empty() {
            return;
        }

        // Newlines would forge extra G-code lines out of one setting, so the
        // greeting is flattened to a single comment. A plugin writing into the
        // output owns the shape of what it writes.
        let flattened: String = greeting
            .chars()
            .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
            .collect();

        let moves = program.moves_mut();
        moves.insert(0, Move::Raw(format!("; {flattened}\n")));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::ENABLED_KEY;

    fn enabled_params() -> SlicingParams {
        let mut p = SlicingParams::default();
        p.set_plugin_value(ID, ENABLED_KEY, json!(true));
        p
    }

    #[test]
    fn it_does_nothing_until_switched_on() {
        let mut program = MoveProgram::new();
        program.raw("G28\n");
        let before = program.clone();

        Greeting.filter(&mut program, &SlicingParams::default());
        assert_eq!(before.moves(), program.moves());
    }

    #[test]
    fn it_writes_its_greeting_once_switched_on() {
        let mut program = MoveProgram::new();
        program.raw("G28\n");
        Greeting.filter(&mut program, &enabled_params());
        assert!(matches!(
            &program.moves()[0],
            Move::Raw(text) if text == "; Hello from a plugin\n"
        ));
    }

    #[test]
    fn the_greeting_is_configurable() {
        let mut params = enabled_params();
        params.set_plugin_value(ID, "greeting", json!("custom text"));
        let mut program = MoveProgram::new();
        Greeting.filter(&mut program, &params);
        assert!(matches!(&program.moves()[0], Move::Raw(t) if t == "; custom text\n"));
    }

    #[test]
    fn a_multi_line_greeting_cannot_forge_extra_gcode() {
        // A settings string reaching the output is the obvious place to inject
        // commands, so it is flattened rather than trusted.
        let mut params = enabled_params();
        params.set_plugin_value(ID, "greeting", json!("hi\nG1 X0 Y0\nbye"));
        let mut program = MoveProgram::new();
        Greeting.filter(&mut program, &params);
        match &program.moves()[0] {
            Move::Raw(text) => {
                assert_eq!(text, "; hi G1 X0 Y0 bye\n");
                assert_eq!(text.matches('\n').count(), 1, "exactly one line");
            }
            other => panic!("expected a comment, got {other:?}"),
        }
    }

    #[test]
    fn an_empty_greeting_writes_nothing() {
        let mut params = enabled_params();
        params.set_plugin_value(ID, "greeting", json!("   "));
        let mut program = MoveProgram::new();
        program.raw("G28\n");
        Greeting.filter(&mut program, &params);
        assert_eq!(program.len(), 1, "no empty comment line");
    }

    #[test]
    fn its_settings_leave_the_engines_own_gate_alone() {
        // The engine supplies `enabled`; a plugin declaring it would be
        // redefining the one field it does not own.
        let schema = HelloWorld.settings_schema().unwrap();
        let props = schema["properties"].as_object().unwrap();
        assert!(!props.contains_key(ENABLED_KEY));
        assert!(props.contains_key("greeting"));
    }
}
