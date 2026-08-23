//! Changelog command - prints the changelog embedded at build time.

use clap::Parser;

use crate::version;

/// Show the changelog that was embedded into this build.
#[derive(Parser, Debug)]
pub struct ChangelogCommand {
    /// Only show the section for this version label (e.g. `1.2.0` or `unreleased`).
    #[arg(long)]
    pub version: Option<String>,

    /// Emit machine-readable JSON instead of markdown.
    #[arg(long)]
    pub json: bool,
}

impl ChangelogCommand {
    /// Execute the changelog command.
    pub fn execute(&self) -> Result<(), Box<dyn std::error::Error>> {
        match &self.version {
            Some(v) => {
                let entry = version::changelog_entry(v)
                    .ok_or_else(|| format!("no changelog entry for version '{}'", v))?;
                if self.json {
                    println!("{}", serde_json::to_string_pretty(&entry)?);
                } else {
                    match &entry.date {
                        Some(date) => println!("## [{}] - {}\n", entry.version, date),
                        None => println!("## [{}]\n", entry.version),
                    }
                    println!("{}", entry.body);
                }
            }
            None => {
                if self.json {
                    let entries = version::changelog_entries();
                    println!("{}", serde_json::to_string_pretty(&entries)?);
                } else {
                    print!("{}", version::CHANGELOG);
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_changelog_runs() {
        let cmd = ChangelogCommand {
            version: None,
            json: false,
        };
        assert!(cmd.execute().is_ok());
    }

    #[test]
    fn unknown_version_errors() {
        let cmd = ChangelogCommand {
            version: Some("999.999.999".to_string()),
            json: true,
        };
        assert!(cmd.execute().is_err());
    }
}
