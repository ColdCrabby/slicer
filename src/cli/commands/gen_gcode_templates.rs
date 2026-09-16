//! Emit the G-code preset catalog as TypeScript for the UI.
//!
//! The catalog itself lives in [`crate::profiles::gcode_templates`]; this only
//! renders it. Keeping one definition is the point: the Klipper blocks used to
//! be written out twice and had already drifted apart.

use clap::Parser;
use std::path::PathBuf;

use crate::profiles::gcode_templates::{GCODE_TEMPLATES, STANDARD_MARLIN};

/// Generate the UI's G-code template catalog from the engine's definitions.
#[derive(Parser, Debug)]
pub struct GenGcodeTemplatesCommand {
    /// Output file for the generated TypeScript module.
    #[arg(
        short,
        long,
        default_value = "ui/src/generated/gcode-templates.data.ts"
    )]
    pub output: PathBuf,
}

/// Render a Rust string as a TypeScript string literal.
///
/// JSON string syntax is a subset of TypeScript's, so `serde_json` already
/// escapes the quotes and the newlines these blocks are full of.
fn ts_string(value: &str) -> String {
    serde_json::to_string(value).expect("a string always serializes")
}

impl GenGcodeTemplatesCommand {
    /// Execute the gen-gcode-templates command.
    pub fn execute(&self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(parent) = self.output.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let mut out = String::new();
        out.push_str("// GENERATED FILE - DO NOT EDIT.\n");
        out.push_str("// Source: src/profiles/gcode_templates.rs\n");
        out.push_str("// Regenerate: pnpm run gen-gcode-templates\n\n");
        out.push_str("/** A ready-made G-code preset, as the engine defines it. */\n");
        out.push_str("export interface GeneratedGcodeTemplate {\n");
        out.push_str("  readonly id: string;\n");
        out.push_str("  readonly label: string;\n");
        out.push_str("  readonly description: string;\n");
        out.push_str("  readonly flavor: 'marlin' | 'klipper';\n");
        out.push_str("  readonly startGcode: string;\n");
        out.push_str("  readonly endGcode: string;\n");
        out.push_str("  readonly layerGcode: string;\n");
        out.push_str("}\n\n");

        out.push_str("/** Every selectable preset, in dropdown order. */\n");
        out.push_str(
            "export const GENERATED_GCODE_TEMPLATES: readonly GeneratedGcodeTemplate[] = [\n",
        );
        for template in GCODE_TEMPLATES {
            let flavor = serde_json::to_value(template.flavor)?;
            let flavor = flavor.as_str().unwrap_or_default();
            out.push_str("  {\n");
            out.push_str(&format!("    id: {},\n", ts_string(template.id)));
            out.push_str(&format!("    label: {},\n", ts_string(template.label)));
            out.push_str(&format!(
                "    description: {},\n",
                ts_string(template.description)
            ));
            out.push_str(&format!("    flavor: {},\n", ts_string(flavor)));
            out.push_str(&format!(
                "    startGcode: {},\n",
                ts_string(template.start_gcode)
            ));
            out.push_str(&format!(
                "    endGcode: {},\n",
                ts_string(template.end_gcode)
            ));
            out.push_str(&format!(
                "    layerGcode: {},\n",
                ts_string(template.layer_gcode)
            ));
            out.push_str("  },\n");
        }
        out.push_str("];\n\n");

        out.push_str("/** The template a from-scratch printer starts attached to. */\n");
        out.push_str(&format!(
            "export const GENERATED_DEFAULT_TEMPLATE_ID = {};\n",
            ts_string(STANDARD_MARLIN.id)
        ));

        std::fs::write(&self.output, out)?;
        println!(
            "Generated {} G-code templates -> {}",
            GCODE_TEMPLATES.len(),
            self.output.display()
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multiline_blocks_survive_as_escaped_literals() {
        // The Marlin blocks are full of newlines; a raw paste would produce an
        // unterminated TypeScript string.
        let rendered = ts_string(STANDARD_MARLIN.start_gcode);
        assert!(rendered.starts_with('"') && rendered.ends_with('"'));
        assert!(!rendered.contains('\n'), "literal newline left unescaped");
        assert!(rendered.contains("\\n"));
    }
}
