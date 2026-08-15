//! Settings diff tool: detect and report overrides between Global and Object settings.

use serde::Serialize;

use crate::settings::params::{ObjectSettings, SlicingParams};

/// Represents the difference for a single setting field between the global
/// defaults and an object-level override.
#[derive(Debug, Clone, Serialize)]
pub struct SettingsDiff {
    /// The name of the setting field (e.g. `"layer_height"`).
    pub field_name: String,
    /// The global value formatted as a string.
    pub global_value: String,
    /// The object value formatted as a string (or the global value if not overridden).
    pub object_value: String,
    /// `true` if the object value differs from the global value.
    pub is_override: bool,
}

/// Compare global and object settings and return a diff for every field.
///
/// If `object.overrides` is `None` the object inherits all global values;
/// every field will be listed with `is_override = false`.
pub fn compare_settings(global: &SlicingParams, object: &ObjectSettings) -> Vec<SettingsDiff> {
    let g = global;
    let o: &SlicingParams = object.overrides.as_ref().unwrap_or(g);

    macro_rules! diff_f64 {
        ($field:ident) => {
            SettingsDiff {
                field_name: stringify!($field).to_string(),
                global_value: g.$field.to_string(),
                object_value: o.$field.to_string(),
                is_override: (g.$field - o.$field).abs() > 1e-9,
            }
        };
    }

    macro_rules! diff_usize {
        ($field:ident) => {
            SettingsDiff {
                field_name: stringify!($field).to_string(),
                global_value: g.$field.to_string(),
                object_value: o.$field.to_string(),
                is_override: g.$field != o.$field,
            }
        };
    }

    vec![
        diff_f64!(layer_height),
        diff_usize!(wall_count),
        diff_f64!(wall_line_width_min),
        diff_f64!(wall_line_width_max),
        diff_f64!(wall_transition_threshold),
        diff_f64!(wall_transition_length),
        diff_usize!(wall_distribution_count),
        diff_f64!(wall_transition_angle),
        diff_f64!(wall_transition_filter_distance),
        diff_f64!(infill_density),
        diff_f64!(print_speed),
        diff_f64!(perimeter_speed),
        diff_f64!(infill_speed),
        diff_f64!(bridge_speed),
        diff_f64!(bridge_flow_ratio),
        diff_f64!(bridge_min_area_mm2),
        diff_f64!(bridge_noise_filter_mm),
        diff_f64!(bridge_anchor_mm),
        diff_f64!(top_surface_speed),
        diff_f64!(gap_fill_speed),
        diff_f64!(gap_fill_min_length_mm),
        diff_f64!(wall_overlap_compensation),
        diff_f64!(first_layer_speed),
        diff_f64!(fan_speed),
        diff_f64!(bridge_fan_speed),
        diff_f64!(first_layer_fan_speed),
        diff_f64!(coasting_distance_mm),
        diff_f64!(nozzle_temp),
        diff_f64!(bed_temp),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::params::{ObjectSettings, SlicingParams};

    #[test]
    fn test_no_overrides_all_false() {
        let global = SlicingParams::default();
        let object = ObjectSettings {
            object_name: "obj".to_string(),
            overrides: None,
        };
        let diff = compare_settings(&global, &object);
        assert_eq!(diff.len(), 29, "Should have 29 fields");
        for d in &diff {
            assert!(
                !d.is_override,
                "Field '{}' should not be overridden",
                d.field_name
            );
            assert_eq!(d.global_value, d.object_value);
        }
    }

    #[test]
    fn test_partial_override_detected() {
        let global = SlicingParams::default();
        let object = ObjectSettings {
            object_name: "obj".to_string(),
            overrides: Some(SlicingParams {
                layer_height: 0.1, // overridden
                ..SlicingParams::default()
            }),
        };
        let diff = compare_settings(&global, &object);

        let lh = diff
            .iter()
            .find(|d| d.field_name == "layer_height")
            .unwrap();
        assert!(lh.is_override, "layer_height should be flagged as override");
        assert_eq!(lh.global_value, "0.2");
        assert_eq!(lh.object_value, "0.1");

        // All other fields should not be overridden
        for d in diff.iter().filter(|d| d.field_name != "layer_height") {
            assert!(
                !d.is_override,
                "Field '{}' should not be overridden",
                d.field_name
            );
        }
    }

    #[test]
    fn test_diff_contains_all_fields() {
        let global = SlicingParams::default();
        let object = ObjectSettings {
            object_name: "obj".to_string(),
            overrides: None,
        };
        let diff = compare_settings(&global, &object);
        let field_names: Vec<&str> = diff.iter().map(|d| d.field_name.as_str()).collect();
        assert!(field_names.contains(&"layer_height"));
        assert!(field_names.contains(&"wall_count"));
        assert!(field_names.contains(&"wall_line_width_min"));
        assert!(field_names.contains(&"wall_line_width_max"));
        assert!(field_names.contains(&"wall_transition_threshold"));
        assert!(field_names.contains(&"wall_transition_length"));
        assert!(field_names.contains(&"wall_distribution_count"));
        assert!(field_names.contains(&"infill_density"));
        assert!(field_names.contains(&"print_speed"));
        assert!(field_names.contains(&"perimeter_speed"));
        assert!(field_names.contains(&"infill_speed"));
        assert!(field_names.contains(&"bridge_speed"));
        assert!(field_names.contains(&"bridge_flow_ratio"));
        assert!(field_names.contains(&"bridge_min_area_mm2"));
        assert!(field_names.contains(&"bridge_noise_filter_mm"));
        assert!(field_names.contains(&"bridge_anchor_mm"));
        assert!(field_names.contains(&"top_surface_speed"));
        assert!(field_names.contains(&"gap_fill_speed"));
        assert!(field_names.contains(&"gap_fill_min_length_mm"));
        assert!(field_names.contains(&"first_layer_speed"));
        assert!(field_names.contains(&"fan_speed"));
        assert!(field_names.contains(&"bridge_fan_speed"));
        assert!(field_names.contains(&"first_layer_fan_speed"));
        assert!(field_names.contains(&"coasting_distance_mm"));
        assert!(field_names.contains(&"nozzle_temp"));
        assert!(field_names.contains(&"bed_temp"));
    }

    #[test]
    fn test_wall_count_override_detected() {
        let global = SlicingParams::default();
        let object = ObjectSettings {
            object_name: "obj".to_string(),
            overrides: Some(SlicingParams {
                wall_count: 5,
                ..SlicingParams::default()
            }),
        };
        let diff = compare_settings(&global, &object);
        let wc = diff.iter().find(|d| d.field_name == "wall_count").unwrap();
        assert!(wc.is_override, "wall_count should be flagged as override");
        assert_eq!(wc.global_value, "3");
        assert_eq!(wc.object_value, "5");
    }
}
