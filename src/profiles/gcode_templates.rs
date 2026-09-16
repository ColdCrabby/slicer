//! The G-code preset catalog — one definition, shared by every runtime.
//!
//! A **template** is the ready-made start / end / layer-change G-code for a
//! common firmware setup: plain Marlin, mainline Klipper, Klippain. It exists so
//! a user can populate a printer's G-code blocks in one click instead of
//! hand-writing macros.
//!
//! # Why this lives in the engine
//!
//! It used to live in both places: the engine kept the blocks a blank profile
//! starts with, and the Angular UI kept the selectable presets. The two drifted
//! — the Klipper blocks disagreed on argument order, and Klippain existed only
//! on the UI side with the wrong argument names, so a Klippain user's bed
//! temperature was silently dropped by the macro.
//!
//! So the catalog is engine-owned, like every other piece of profile knowledge
//! ([`super`]): the UI's copy is generated from this one (`gen-gcode-templates`)
//! rather than maintained beside it. The UI still owns everything *about*
//! choosing a template — the dropdown, the "modified from …" tracking — because
//! that is presentation, not copy.
//!
//! # A template's arguments are the macro's, not ours
//!
//! The `{placeholder}` tokens are substituted by
//! [`render_script_placeholders`](crate::gcode::generator::render_script_placeholders),
//! which only ever rewrites the `{...}` token and passes the rest of the line
//! through verbatim. **The text to the left of each `=` is the receiving macro's
//! parameter name and must match it exactly.** Klipper silently ignores a
//! parameter its macro does not declare, so a near-miss like `BED=` where the
//! macro reads `BED_TEMP=` does not error — it just prints at the macro's
//! default temperature.

use serde::Serialize;

/// The firmware dialect a template targets, as the printer profile spells it.
///
/// Applying a template also switches the printer to this flavor, so a Klipper
/// preset cannot be left sitting on a Marlin printer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TemplateFlavor {
    Marlin,
    Klipper,
}

/// One selectable preset: its identity, and the three blocks it writes.
///
/// Serialised in camelCase because the UI consumes the generated JSON directly
/// as its own `GcodeTemplate`, rather than through a mapping layer.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GcodeTemplate {
    /// Stable identifier, used as the dropdown value and stored on the profile.
    pub id: &'static str,
    /// Human-readable name shown in the dropdown.
    pub label: &'static str,
    /// Short one-liner describing the preset.
    pub description: &'static str,
    /// Flavor the printer is switched to when this template is applied.
    pub flavor: TemplateFlavor,
    pub start_gcode: &'static str,
    pub end_gcode: &'static str,
    /// Emitted at every layer change. Empty for templates that need none.
    pub layer_gcode: &'static str,
}

/// Plain Marlin: home, heat and wait using raw M-commands.
pub const STANDARD_MARLIN: GcodeTemplate = GcodeTemplate {
    id: "marlin-standard",
    label: "Standard Marlin",
    description: "Home, heat and wait using raw M-commands.",
    flavor: TemplateFlavor::Marlin,
    start_gcode: "; Cold Crabby standard Marlin start\nG21 ; millimetres\nG90 ; absolute positioning\nM82 ; extruder absolute mode\nM140 S{bed_temp_first_layer} ; set bed temperature\nM104 S{nozzle_temp_first_layer} ; set nozzle temperature\nG28 ; home all axes\nM190 S{bed_temp_first_layer} ; wait for bed temperature\nM109 S{nozzle_temp_first_layer} ; wait for nozzle temperature\nG92 E0 ; reset extruder\nG1 Z2.0 F3000 ; lift nozzle",
    end_gcode: "; Cold Crabby standard Marlin end\nG91 ; relative positioning\nG1 E-2 F2700 ; retract\nG1 Z10 F3000 ; lift\nG90 ; absolute positioning\nM104 S0 ; nozzle off\nM140 S0 ; bed off\nM84 ; disable steppers",
    layer_gcode: "",
};

/// Mainline Klipper, which conventionally names its macros `PRINT_START` /
/// `PRINT_END` and reads bare `EXTRUDER` / `BED` arguments.
pub const STANDARD_KLIPPER: GcodeTemplate = GcodeTemplate {
    id: "klipper-standard",
    label: "Standard Klipper",
    description: "PRINT_START / PRINT_END macros (mainline convention).",
    flavor: TemplateFlavor::Klipper,
    start_gcode: "PRINT_START EXTRUDER={nozzle_temp_first_layer} BED={bed_temp_first_layer}",
    end_gcode: "PRINT_END",
    layer_gcode: "",
};

/// Klippain, which names its macros the other way round (`START_PRINT` /
/// `END_PRINT`) and reads `_TEMP`-suffixed arguments.
///
/// The suffixes are not cosmetic: Klippain's `START_PRINT` declares
/// `EXTRUDER_TEMP`, `BED_TEMP` and `CHAMBER_TEMP`, and Klipper discards any
/// argument a macro does not declare without raising an error. Shortening one to
/// `BED=` prints the whole plate at the macro's default bed temperature.
pub const KLIPPAIN: GcodeTemplate = GcodeTemplate {
    id: "klippain",
    label: "Klippain",
    description: "START_PRINT / END_PRINT with temperature, chamber and material parameters.",
    flavor: TemplateFlavor::Klipper,
    start_gcode: "START_PRINT EXTRUDER_TEMP={nozzle_temp_first_layer} BED_TEMP={bed_temp_first_layer} CHAMBER_TEMP={chamber_temp} MATERIAL={filament_type}",
    end_gcode: "END_PRINT",
    layer_gcode: "_ON_LAYER_CHANGE LAYER={layer_num} Z={z}",
};

/// Every selectable preset, in dropdown order.
pub const GCODE_TEMPLATES: &[GcodeTemplate] = &[STANDARD_MARLIN, STANDARD_KLIPPER, KLIPPAIN];

/// The template a from-scratch printer starts attached to.
pub const DEFAULT_TEMPLATE_ID: &str = STANDARD_MARLIN.id;

/// The preset with this id, or `None` when nothing matches.
pub fn template_by_id(id: &str) -> Option<&'static GcodeTemplate> {
    GCODE_TEMPLATES.iter().find(|t| t.id == id)
}

/// The whole catalog in the shape the UI reads it: the presets plus which one a
/// from-scratch printer starts on, so one file answers both questions.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GcodeTemplateCatalog {
    pub default_template_id: &'static str,
    pub templates: &'static [GcodeTemplate],
}

/// The catalog, ready to serialise.
pub fn catalog() -> GcodeTemplateCatalog {
    GcodeTemplateCatalog {
        default_template_id: DEFAULT_TEMPLATE_ID,
        templates: GCODE_TEMPLATES,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique() {
        for (i, template) in GCODE_TEMPLATES.iter().enumerate() {
            assert!(
                !GCODE_TEMPLATES[..i].iter().any(|t| t.id == template.id),
                "duplicate template id {}",
                template.id
            );
        }
    }

    #[test]
    fn every_template_is_reachable_by_id() {
        for template in GCODE_TEMPLATES {
            assert_eq!(template_by_id(template.id).map(|t| t.id), Some(template.id));
        }
        assert!(template_by_id("no-such-template").is_none());
    }

    /// Klippain reads `_TEMP`-suffixed arguments. Klipper drops an argument the
    /// macro never declared without erroring, so getting this wrong prints at
    /// the macro's default temperature instead of failing loudly.
    #[test]
    fn klippain_passes_the_argument_names_its_macro_declares() {
        assert!(KLIPPAIN
            .start_gcode
            .contains("BED_TEMP={bed_temp_first_layer}"));
        assert!(KLIPPAIN
            .start_gcode
            .contains("EXTRUDER_TEMP={nozzle_temp_first_layer}"));
        assert!(KLIPPAIN.start_gcode.contains("CHAMBER_TEMP={chamber_temp}"));
        assert!(KLIPPAIN.start_gcode.starts_with("START_PRINT "));
    }

    /// The mainline convention is the other one, and must not drift into the
    /// Klippain spelling.
    #[test]
    fn mainline_klipper_passes_bare_argument_names() {
        assert!(STANDARD_KLIPPER.start_gcode.starts_with("PRINT_START "));
        assert!(STANDARD_KLIPPER
            .start_gcode
            .contains("BED={bed_temp_first_layer}"));
        assert!(!STANDARD_KLIPPER.start_gcode.contains("BED_TEMP="));
    }

    /// Every placeholder a template uses must be one the renderer substitutes,
    /// or it reaches the printer as literal `{...}` text.
    #[test]
    fn templates_only_use_placeholders_the_renderer_resolves() {
        const KNOWN: &[&str] = &[
            "nozzle_temp",
            "bed_temp",
            "nozzle_temp_first_layer",
            "bed_temp_first_layer",
            "chamber_temp",
            "chamber_temp_first_layer",
            "filament_type",
            "layer_height",
            "first_layer_height",
            "z",
            "height",
            "layer_num",
        ];
        for template in GCODE_TEMPLATES {
            for block in [
                template.start_gcode,
                template.end_gcode,
                template.layer_gcode,
            ] {
                for (_, rest) in block.match_indices('{').map(|(i, _)| (i, &block[i + 1..])) {
                    let name = rest.split('}').next().unwrap_or_default();
                    assert!(
                        KNOWN.contains(&name),
                        "template {} uses unknown placeholder {{{}}}",
                        template.id,
                        name
                    );
                }
            }
        }
    }
}
