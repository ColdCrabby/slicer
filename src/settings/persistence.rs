//! Settings persistence - load and save global settings from/to disk.

use std::fs;
use std::path::{Path, PathBuf};

use crate::config;
use crate::settings::params::GlobalSettings;
use dirs;
use serde_json::Value;

/// Determines the configuration directory for slicer-engine settings.
///
/// Uses the `dirs` crate to get platform-specific config directories:
/// - Windows: `%APPDATA%\slicer-engine` (e.g., `C:\Users\User\AppData\Roaming\slicer-engine`)
/// - macOS: `~/Library/Application Support/slicer-engine`
/// - Linux: `~/.config/slicer-engine` (respects `XDG_CONFIG_HOME`)
pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .map(|path| path.join("slicer-engine"))
        .unwrap_or_else(|| PathBuf::from(".config/slicer-engine"))
}

/// Returns the path to the global settings file.
pub fn settings_file() -> PathBuf {
    config_dir().join("settings.json")
}

/// Load global settings from disk.
///
/// If the settings file doesn't exist, returns the default settings.
/// If the file exists but is invalid, returns an error.
pub fn load_settings() -> Result<GlobalSettings, Box<dyn std::error::Error>> {
    let path = settings_file();

    if !path.exists() {
        // File doesn't exist; return defaults
        return Ok(GlobalSettings::default());
    }

    let content = fs::read_to_string(&path)?;
    let settings: GlobalSettings = serde_json::from_str(&content)?;
    Ok(settings)
}

/// Save global settings to disk.
///
/// Creates the config directory if it doesn't exist.
pub fn save_settings(settings: &GlobalSettings) -> Result<(), Box<dyn std::error::Error>> {
    let path = settings_file();
    let dir = path
        .parent()
        .expect("settings file should have parent directory");

    // Create config directory if it doesn't exist
    if !dir.exists() {
        fs::create_dir_all(dir)?;
    }

    let json = serde_json::to_string_pretty(&settings)?;
    fs::write(&path, json)?;
    Ok(())
}

/// Search for `slicer.json` in the current working directory.
///
/// Returns the path if found, `None` otherwise.
pub fn find_project_config() -> Option<PathBuf> {
    let path = std::env::current_dir().ok()?.join("slicer.json");
    if path.exists() {
        Some(path)
    } else {
        None
    }
}

/// Load a project-level config file as a raw JSON `Value`.
///
/// The file may be a partial `GlobalSettings` object — only the keys present
/// are used as overrides.  Returns an error for missing or malformed files.
pub fn load_project_config(path: &Path) -> Result<Value, Box<dyn std::error::Error>> {
    let content = fs::read_to_string(path)
        .map_err(|e| format!("Cannot read project config '{}': {}", path.display(), e))?;
    let value: Value = serde_json::from_str(&content)
        .map_err(|e| format!("Invalid JSON in project config '{}': {}", path.display(), e))?;
    if !value.is_object() {
        return Err(format!("Project config '{}' must be a JSON object", path.display()).into());
    }
    Ok(value)
}

/// Recursively merge two JSON `Value`s.
///
/// Object fields from `overlay` are merged on top of `base` fields.
/// All other types (arrays, scalars) are replaced wholesale by `overlay`.
pub fn merge_json_configs(base: Value, overlay: Value) -> Value {
    match (base, overlay) {
        (Value::Object(mut base_map), Value::Object(overlay_map)) => {
            for (key, overlay_val) in overlay_map {
                let merged = match base_map.remove(&key) {
                    Some(base_val) => merge_json_configs(base_val, overlay_val),
                    None => overlay_val,
                };
                base_map.insert(key, merged);
            }
            Value::Object(base_map)
        }
        (_, overlay) => overlay,
    }
}

/// Load and merge settings following the priority hierarchy:
///
/// 1. Global defaults (built-in)
/// 2. User config (`~/.config/slicer-engine/settings.json`)
/// 3. Project config (`slicer.json` in CWD, or explicit `project_config_path`)
///
/// A `project_config_path` of `None` triggers automatic discovery via
/// [`find_project_config`].  Pass `Some(path)` to use an explicit file.
/// Pass `Some(path)` to a non-existent file to skip the project config layer
/// (useful when the user did not supply `--config`).
pub fn load_and_merge_settings(
    project_config_path: Option<&Path>,
) -> Result<GlobalSettings, Box<dyn std::error::Error>> {
    // Layer 1: built-in defaults
    let defaults = GlobalSettings::default();
    let mut merged: Value = serde_json::to_value(&defaults)?;

    // Layer 2: TOML config (global user config → project config)
    let toml_config = config::load_and_merge_config(project_config_path)?;
    let toml_overlay = toml_slicing_to_json(toml_config.slicing.as_ref());
    if !toml_overlay.is_null() {
        merged = merge_json_configs(merged, toml_overlay);
    }

    // Layer 3: legacy JSON user config (settings.json) takes priority over TOML
    let user_path = settings_file();
    if user_path.exists() {
        let content = fs::read_to_string(&user_path)?;
        let user_val: Value = serde_json::from_str(&content)?;
        merged = merge_json_configs(merged, user_val);
    }

    // Layer 4: legacy project JSON config (slicer.json) — kept for back-compat
    let project_json_path: Option<PathBuf> = project_config_path
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("json") && p.exists())
        .map(|p| p.to_path_buf())
        .or_else(find_project_config);

    if let Some(ref p) = project_json_path {
        let project_val = load_project_config(p)?;
        merged = merge_json_configs(merged, project_val);
    }

    // The same percentages a profile may state are valid in `slicer.json` and
    // the user config, so both roads into `SlicingParams` resolve them.
    let mut merged = merged;
    if let Some(params) = merged.get_mut("params") {
        crate::settings::params::resolve_derived_values(params);
    }
    let settings: GlobalSettings = serde_json::from_value(merged)
        .map_err(|e| format!("Merged settings are invalid: {}", e))?;
    Ok(settings)
}

/// Convert a `SlicingParams` from TOML config into a JSON overlay for `params`.
///
/// Returns `Null` when no `[slicing]` section was present in TOML.
fn toml_slicing_to_json(slicing: Option<&crate::settings::params::SlicingParams>) -> Value {
    match slicing {
        Some(params) => {
            let params_val = serde_json::to_value(params).unwrap_or(Value::Null);
            serde_json::json!({ "params": params_val })
        }
        None => Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::params::SlicingParams;

    #[test]
    fn test_config_dir_returns_path() {
        let dir = config_dir();
        assert!(!dir.as_os_str().is_empty());
    }

    #[test]
    fn test_settings_file_has_json_extension() {
        let path = settings_file();
        assert!(path.to_string_lossy().ends_with("settings.json"));
    }

    #[test]
    fn test_settings_file_includes_config_dir() {
        let path = settings_file();
        let config = config_dir();
        assert!(path.starts_with(&config));
    }

    #[test]
    fn test_find_project_config_missing() {
        // In the test environment CWD doesn't have a slicer.json by default;
        // we just verify the function returns None without panicking.
        // (If a slicer.json happens to exist in CWD the function returns Some.)
        let _ = find_project_config();
    }

    #[test]
    fn test_load_project_config_valid() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("slicer.json");
        fs::write(
            &path,
            r#"{"params":{"layer_height":0.15},"gcode_flavor":"klipper"}"#,
        )
        .unwrap();
        let val = load_project_config(&path).unwrap();
        assert_eq!(val["params"]["layer_height"], 0.15);
        assert_eq!(val["gcode_flavor"], "klipper");
    }

    #[test]
    fn test_load_project_config_missing_file() {
        let path = Path::new("/nonexistent/slicer.json");
        assert!(load_project_config(path).is_err());
    }

    #[test]
    fn test_load_project_config_bad_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("slicer.json");
        fs::write(&path, "not json {{{").unwrap();
        assert!(load_project_config(&path).is_err());
    }

    #[test]
    fn test_load_project_config_non_object() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("slicer.json");
        fs::write(&path, "[1,2,3]").unwrap();
        assert!(load_project_config(&path).is_err());
    }

    #[test]
    fn test_merge_json_configs_scalars() {
        let base = serde_json::json!({"a": 1, "b": 2});
        let overlay = serde_json::json!({"b": 99, "c": 3});
        let merged = merge_json_configs(base, overlay);
        assert_eq!(merged["a"], 1);
        assert_eq!(merged["b"], 99);
        assert_eq!(merged["c"], 3);
    }

    #[test]
    fn test_merge_json_configs_nested() {
        let base = serde_json::json!({"params": {"layer_height": 0.2, "nozzle_temp": 210.0}});
        let overlay = serde_json::json!({"params": {"layer_height": 0.15}});
        let merged = merge_json_configs(base, overlay);
        assert_eq!(merged["params"]["layer_height"], 0.15);
        assert_eq!(merged["params"]["nozzle_temp"], 210.0);
    }

    #[test]
    fn test_merge_json_configs_deep_nested() {
        let base = serde_json::json!({"a": {"b": {"c": 1, "d": 2}}});
        let overlay = serde_json::json!({"a": {"b": {"c": 99}}});
        let merged = merge_json_configs(base, overlay);
        assert_eq!(merged["a"]["b"]["c"], 99);
        assert_eq!(merged["a"]["b"]["d"], 2);
    }

    #[test]
    fn test_load_and_merge_settings_no_project_config() {
        // With a non-existent explicit path, we just get defaults (or user config).
        let result = load_and_merge_settings(Some(Path::new("/nonexistent/slicer.json")));
        assert!(result.is_ok());
        // Verify the defaults came through
        let settings = result.unwrap();
        assert_eq!(
            settings.params.layer_height,
            SlicingParams::default().layer_height
        );
    }

    #[test]
    fn test_load_and_merge_settings_project_overrides_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("slicer.json");
        // Project config sets layer_height to 0.15, leaving other params at default
        fs::write(&path, r#"{"params":{"layer_height":0.15}}"#).unwrap();
        let settings = load_and_merge_settings(Some(&path)).unwrap();
        assert_eq!(settings.params.layer_height, 0.15);
        // Other params still at default
        assert_eq!(
            settings.params.nozzle_temp,
            SlicingParams::default().nozzle_temp
        );
    }

    #[test]
    fn test_load_and_merge_settings_priority() {
        let dir = tempfile::tempdir().unwrap();
        // Project config sets layer_height = 0.15
        let project_path = dir.path().join("slicer.json");
        fs::write(&project_path, r#"{"params":{"layer_height":0.15}}"#).unwrap();
        let settings = load_and_merge_settings(Some(&project_path)).unwrap();
        // Project value wins over default (0.2)
        assert_eq!(settings.params.layer_height, 0.15);
    }
}
