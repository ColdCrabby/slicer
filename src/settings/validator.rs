//! Settings validation infrastructure.
//!
//! Defines the [`SettingValidator`] trait and a [`ValidationRules`] helper with
//! stub methods that always return `Ok(())`.  Real validation rules (e.g.
//! `LayerHeight ≤ 0.8 × NozzleDiameter`) will be added in a follow-up PR.

use crate::settings::params::SlicingParams;

/// Trait for types that can validate their own settings.
///
/// `validate` returns `Ok(())` when all constraints are satisfied, or a
/// `Vec<String>` containing one error message per failing rule.
pub trait SettingValidator {
    /// Validate settings and collect any rule violations.
    fn validate(&self) -> Result<(), Vec<String>>;
}

/// Static helper methods for common validation rules.
///
/// All methods are currently **stubs** that return `Ok(())`.
/// Real logic will be added in a follow-up PR (see scope boundaries in the plan).
pub struct ValidationRules;

impl ValidationRules {
    /// Validate that `layer_height` is within the allowed range.
    ///
    /// TODO: enforce `layer_height ≤ 0.8 × nozzle_diameter` once printer
    /// context is available.
    pub fn validate_layer_height(_layer_height: f64) -> Result<(), String> {
        Ok(())
    }

    /// Validate that `value` is strictly positive.
    ///
    /// TODO: add actual check `value > 0.0`.
    pub fn validate_positive(_value: f64) -> Result<(), String> {
        Ok(())
    }

    /// Validate that `value` lies within `[min, max]`.
    ///
    /// TODO: add actual range check.
    pub fn validate_range(_value: f64, _min: f64, _max: f64) -> Result<(), String> {
        Ok(())
    }

    /// Validate that `path_tolerance` is non-negative.
    ///
    /// A tolerance of `0.0` disables simplification; negative values are not
    /// meaningful and indicate a misconfigured settings file.
    pub fn validate_path_tolerance(path_tolerance: f64) -> Result<(), String> {
        if path_tolerance < 0.0 {
            Err(format!(
                "path_tolerance must be >= 0.0, got {path_tolerance}"
            ))
        } else {
            Ok(())
        }
    }
}

impl SettingValidator for SlicingParams {
    /// Validate all slicing parameters.
    ///
    /// Currently calls stub validation rules that always pass, plus a real
    /// non-negativity check for `path_tolerance`.
    fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();

        if let Err(e) = ValidationRules::validate_layer_height(self.layer_height) {
            errors.push(e);
        }
        if let Err(e) = ValidationRules::validate_positive(self.print_speed) {
            errors.push(e);
        }
        if let Err(e) = ValidationRules::validate_range(self.infill_density, 0.0, 1.0) {
            errors.push(e);
        }
        if let Err(e) = ValidationRules::validate_path_tolerance(self.path_tolerance) {
            errors.push(e);
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::params::SlicingParams;

    #[test]
    fn test_validate_layer_height_stub_returns_ok() {
        assert!(ValidationRules::validate_layer_height(0.2).is_ok());
    }

    #[test]
    fn test_validate_positive_stub_returns_ok() {
        assert!(ValidationRules::validate_positive(60.0).is_ok());
    }

    #[test]
    fn test_validate_range_stub_returns_ok() {
        assert!(ValidationRules::validate_range(0.5, 0.0, 1.0).is_ok());
    }

    #[test]
    fn test_slicing_params_validation_returns_ok() {
        let params = SlicingParams::default();
        assert!(params.validate().is_ok());
    }

    #[test]
    fn test_validate_path_tolerance_zero_is_ok() {
        assert!(ValidationRules::validate_path_tolerance(0.0).is_ok());
    }

    #[test]
    fn test_validate_path_tolerance_positive_is_ok() {
        assert!(ValidationRules::validate_path_tolerance(0.05).is_ok());
        assert!(ValidationRules::validate_path_tolerance(0.1).is_ok());
    }

    #[test]
    fn test_validate_path_tolerance_negative_is_err() {
        let result = ValidationRules::validate_path_tolerance(-0.01);
        assert!(result.is_err());
        let msg = result.unwrap_err();
        assert!(
            msg.contains("path_tolerance"),
            "error message should mention path_tolerance"
        );
    }

    #[test]
    fn test_slicing_params_negative_path_tolerance_fails_validation() {
        let params = SlicingParams {
            path_tolerance: -1.0,
            ..SlicingParams::default()
        };
        let result = params.validate();
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(
            errors.iter().any(|e| e.contains("path_tolerance")),
            "validation errors should mention path_tolerance: {errors:?}"
        );
    }
}
