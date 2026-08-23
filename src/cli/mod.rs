//! Command-line interface layer
//!
//! Provides user-friendly commands that bridge the library API.
//! Uses clap v4 for argument parsing with derive macros.

pub mod commands;
pub mod emit;
pub mod error;
pub mod io;
pub mod output;
pub mod schemas;

use clap::Parser;
use commands::{
    ChangelogCommand, ConfigCommand, GenSchemasCommand, InfoCommand, ServeCommand, SettingsCommand,
    SliceCommand,
};

/// Slicer Engine CLI
#[derive(Parser, Debug)]
#[command(name = "slicer-engine")]
#[command(version = crate::version::VERSION)]
#[command(about = "High-performance 3D model slicer powered by Clipper2")]
#[command(long_about = None)]
pub struct CliArgs {
    /// Command to execute
    #[command(subcommand)]
    pub command: Commands,
}

/// Available CLI commands
#[derive(Parser, Debug)]
pub enum Commands {
    /// Slice a 3D model into layers
    Slice(Box<SliceCommand>),

    /// Display build and library information
    Info(InfoCommand),

    /// Show the changelog (embedded at build time)
    Changelog(ChangelogCommand),

    /// Validate or diff slicing settings
    Settings(SettingsCommand),

    /// Manage the centralized TOML configuration
    Config(ConfigCommand),

    /// Serve the Angular UI over a local HTTP server
    Serve(ServeCommand),

    /// Generate JSON schemas for all emit payloads
    #[command(name = "gen-schemas")]
    GenSchemas(GenSchemasCommand),
}

impl CliArgs {
    /// Parse command-line arguments and execute command
    pub fn run() -> Result<(), Box<dyn std::error::Error>> {
        let args = CliArgs::parse();
        args.execute()
    }

    /// Execute the selected command
    pub fn execute(&self) -> Result<(), Box<dyn std::error::Error>> {
        match &self.command {
            Commands::Slice(cmd) => cmd.execute(),
            Commands::Info(cmd) => cmd.execute(),
            Commands::Changelog(cmd) => cmd.execute(),
            Commands::Settings(cmd) => cmd.execute(),
            Commands::Config(cmd) => cmd.execute(),
            Commands::Serve(cmd) => cmd.execute(),
            Commands::GenSchemas(cmd) => cmd.execute(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_args_parse_help() {
        // This test verifies the CLI structure is valid
        // Actual help testing would require integration tests
        let _cli = CliArgs {
            command: Commands::Info(InfoCommand {
                output_format: "human".to_string(),
                verbose: false,
            }),
        };
    }
}
