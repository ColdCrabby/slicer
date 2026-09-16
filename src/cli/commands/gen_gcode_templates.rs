//! Emit the built-in G-code presets as JSON for the UI.
//!
//! The presets themselves live in [`crate::profiles::gcode_templates`]; this
//! only serialises them. Keeping one definition is the point: the Klipper blocks
//! used to be written out twice and had already drifted apart.

use clap::Parser;
use std::path::PathBuf;

use crate::profiles::gcode_templates::presets;

/// Generate the UI's built-in G-code presets from the engine's definitions.
#[derive(Parser, Debug)]
pub struct GenGcodeTemplatesCommand {
    /// Output file for the generated JSON presets.
    #[arg(short, long, default_value = "ui/src/generated/gcode-templates.json")]
    pub output: PathBuf,
}

impl GenGcodeTemplatesCommand {
    /// Execute the gen-gcode-templates command.
    pub fn execute(&self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(parent) = self.output.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let presets = presets();
        std::fs::write(&self.output, serde_json::to_string_pretty(&presets)?)?;

        println!(
            "Generated {} G-code templates -> {}",
            presets.templates.len(),
            self.output.display()
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The UI reads these straight off the JSON, so the field names have to be
    /// the ones its `GcodeTemplate` interface declares.
    #[test]
    fn serialises_with_the_field_names_the_ui_expects() {
        let json = serde_json::to_value(presets()).expect("presets serialise");
        assert!(json["defaultTemplateId"].is_string());

        let first = &json["templates"][0];
        for key in [
            "id",
            "label",
            "description",
            "flavor",
            "startGcode",
            "endGcode",
            "layerGcode",
        ] {
            assert!(!first[key].is_null(), "missing key {key}");
        }
        assert!(first["startGcode"].as_str().unwrap().contains('\n'));
    }
}
