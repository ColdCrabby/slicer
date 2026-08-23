//! Info command - displays build and library information

use clap::Parser;
use serde_json::{json, Value};

use crate::cli::emit::Emitter;
use crate::cli::output::{EmitPayload, OutputFormat};

/// Display build and library information
#[derive(Parser, Debug)]
pub struct InfoCommand {
    /// Output format (json, human)
    #[arg(long, default_value = "human")]
    pub output_format: String,

    /// Enable verbose output
    #[arg(short, long)]
    pub verbose: bool,
}

/// Result payload emitted by the `info` command.
struct InfoResult {
    name: &'static str,
    version: &'static str,
    git_describe: &'static str,
    build_date: &'static str,
    is_release: bool,
    edition: &'static str,
    features: Option<&'static str>,
}

impl EmitPayload for InfoResult {
    fn schema(&self) -> &'static str {
        "slicer-engine/info-result-v1"
    }

    fn display_human(&self) -> String {
        let channel = if self.is_release {
            "release"
        } else {
            "development"
        };
        let mut s = format!(
            "Slicer Engine\n  Version: {}\n  Channel: {}\n  Build:   {} ({})\n  Edition: {}",
            self.version, channel, self.git_describe, self.build_date, self.edition
        );
        if let Some(f) = self.features {
            s.push_str(&format!("\n  Features: {}", f));
        }
        s
    }

    fn to_json(&self) -> Value {
        json!({
            "name": self.name,
            "version": self.version,
            "git_describe": self.git_describe,
            "build_date": self.build_date,
            "is_release": self.is_release,
            "edition": self.edition,
            "features": self.features,
        })
    }
}

impl InfoCommand {
    /// Execute the info command
    pub fn execute(&self) -> Result<(), Box<dyn std::error::Error>> {
        let format = self
            .output_format
            .parse::<OutputFormat>()
            .map_err(|e| format!("Invalid output format: {}", e))?;

        let emitter = Emitter::new(format);

        let result = InfoResult {
            name: "slicer-engine",
            version: crate::version::VERSION,
            git_describe: crate::version::GIT_DESCRIBE,
            build_date: crate::version::BUILD_DATE,
            is_release: crate::version::is_release(),
            edition: "2021",
            features: if self.verbose {
                Some("clipper2-based polygon clipping")
            } else {
                None
            },
        };

        emitter.emit(&result);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_info_command_creation() {
        let cmd = InfoCommand {
            output_format: "human".to_string(),
            verbose: false,
        };
        assert!(!cmd.verbose);
    }

    #[test]
    fn test_info_result_schema() {
        let r = InfoResult {
            name: "slicer-engine",
            version: "0.1.0",
            git_describe: "v0.1.0",
            build_date: "2026-01-01",
            is_release: true,
            edition: "2021",
            features: None,
        };
        assert_eq!(r.schema(), "slicer-engine/info-result-v1");
    }

    #[test]
    fn test_info_result_human_no_features() {
        let r = InfoResult {
            name: "slicer-engine",
            version: "0.1.0",
            git_describe: "v0.1.0",
            build_date: "2026-01-01",
            is_release: true,
            edition: "2021",
            features: None,
        };
        let s = r.display_human();
        assert!(s.contains("Slicer Engine"));
        assert!(s.contains("0.1.0"));
        assert!(!s.contains("Features"));
    }

    #[test]
    fn test_info_result_human_with_features() {
        let r = InfoResult {
            name: "slicer-engine",
            version: "0.1.0",
            git_describe: "v0.1.0",
            build_date: "2026-01-01",
            is_release: true,
            edition: "2021",
            features: Some("clipper2-based polygon clipping"),
        };
        assert!(r.display_human().contains("Features:"));
    }

    #[test]
    fn test_info_result_json_fields() {
        let r = InfoResult {
            name: "slicer-engine",
            version: "0.1.0",
            git_describe: "v0.1.0",
            build_date: "2026-01-01",
            is_release: true,
            edition: "2021",
            features: None,
        };
        let v = r.to_json();
        assert_eq!(v["name"], "slicer-engine");
        assert_eq!(v["version"], "0.1.0");
        assert_eq!(v["edition"], "2021");
    }
}
