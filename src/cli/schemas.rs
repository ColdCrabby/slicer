//! Schema definitions for all emit payload types.
//!
//! These types are defined with `#[derive(JsonSchema)]` from the `schemars` crate,
//! which automatically generates JSON schemas that match the actual Rust types.

use crate::settings::SlicingParams;
use crate::ws_protocol::{ClientMessage, ServerMessage};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Generic success result payload
#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone)]
pub struct ResultSchema {
    pub status: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
}

/// Error response payload
#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone)]
pub struct ErrorSchema {
    pub status: String,
    pub error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
}

/// Log entry emitted to stderr
#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone)]
pub struct LogSchema {
    pub level: String,
    pub message: String,
}

/// Build and library information
#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone)]
pub struct InfoResultSchema {
    pub name: String,
    pub version: String,
    pub edition: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub features: Option<String>,
}

/// Result of a slicing operation
#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone)]
pub struct SliceResultSchema {
    pub status: String,
    /// Comma-joined summary of every model on the plate.
    pub input: String,
    /// File name of every model on the plate, in placement order.
    pub inputs: Vec<String>,
    /// Number of models merged into this slice.
    pub input_count: u32,
    pub layer_height: f64,
    pub layer_count: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
}

/// Result of settings validation
#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone)]
pub struct ValidateResultSchema {
    pub status: String,
    pub message: String,
}

/// Settings difference between global and object
#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone)]
pub struct SettingsDiffSchema {
    pub field_name: String,
    pub global_value: String,
    pub object_value: String,
    pub is_override: bool,
}

/// Settings show result (array of diffs)
#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone)]
pub struct DiffResultSchema(pub Vec<SettingsDiffSchema>);

/// Slicing parameters schema snapshot.
///
/// Returned by `settings show` (CLI) and `GetSettings` (WebSocket).
#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone)]
pub struct SlicingParamsSchema {
    pub params: SlicingParams,
}

/// Get a single setting value
#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone)]
pub struct GetResultSchema {
    pub key: String,
    pub value: serde_json::Value,
}

/// Set a setting value result
#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone)]
pub struct SetResultSchema {
    pub key: String,
    pub value: serde_json::Value,
    pub message: String,
}

/// Container for a schema definition with its identifier
#[derive(Debug, Clone)]
pub struct SchemaDefinition {
    pub schema_id: &'static str,
    pub schema: serde_json::Value,
}

/// Collect all schema definitions
pub fn all_schemas() -> Vec<SchemaDefinition> {
    vec![
        SchemaDefinition {
            schema_id: "slicer-engine/result-v1",
            schema: serde_json::to_value(schemars::schema_for!(ResultSchema))
                .expect("failed to serialize ResultSchema"),
        },
        SchemaDefinition {
            schema_id: "slicer-engine/error-v1",
            schema: serde_json::to_value(schemars::schema_for!(ErrorSchema))
                .expect("failed to serialize ErrorSchema"),
        },
        SchemaDefinition {
            schema_id: "slicer-engine/log-v1",
            schema: serde_json::to_value(schemars::schema_for!(LogSchema))
                .expect("failed to serialize LogSchema"),
        },
        SchemaDefinition {
            schema_id: "slicer-engine/info-result-v1",
            schema: serde_json::to_value(schemars::schema_for!(InfoResultSchema))
                .expect("failed to serialize InfoResultSchema"),
        },
        SchemaDefinition {
            schema_id: "slicer-engine/slice-result-v1",
            schema: serde_json::to_value(schemars::schema_for!(SliceResultSchema))
                .expect("failed to serialize SliceResultSchema"),
        },
        SchemaDefinition {
            schema_id: "slicer-engine/settings-validate-result-v1",
            schema: serde_json::to_value(schemars::schema_for!(ValidateResultSchema))
                .expect("failed to serialize ValidateResultSchema"),
        },
        SchemaDefinition {
            schema_id: "slicer-engine/settings-diff-result-v1",
            schema: serde_json::to_value(schemars::schema_for!(DiffResultSchema))
                .expect("failed to serialize DiffResultSchema"),
        },
        SchemaDefinition {
            schema_id: "slicer-engine/global-settings-v1",
            schema: serde_json::to_value(schemars::schema_for!(SlicingParamsSchema))
                .expect("failed to serialize SlicingParamsSchema"),
        },
        SchemaDefinition {
            schema_id: "slicer-engine/settings-get-result-v1",
            schema: serde_json::to_value(schemars::schema_for!(GetResultSchema))
                .expect("failed to serialize GetResultSchema"),
        },
        SchemaDefinition {
            schema_id: "slicer-engine/settings-set-result-v1",
            schema: serde_json::to_value(schemars::schema_for!(SetResultSchema))
                .expect("failed to serialize SetResultSchema"),
        },
        SchemaDefinition {
            schema_id: "slicer-engine/ws/client-message-v1",
            schema: serde_json::to_value(schemars::schema_for!(ClientMessage))
                .expect("failed to serialize ClientMessage"),
        },
        SchemaDefinition {
            schema_id: "slicer-engine/ws/server-message-v1",
            schema: serde_json::to_value(schemars::schema_for!(ServerMessage))
                .expect("failed to serialize ServerMessage"),
        },
        SchemaDefinition {
            schema_id: "slicer-engine/filament-profile-v1",
            schema: serde_json::to_value(schemars::schema_for!(crate::profiles::FilamentProfile))
                .expect("failed to serialize FilamentProfile"),
        },
        SchemaDefinition {
            schema_id: "slicer-engine/process-profile-v1",
            schema: serde_json::to_value(schemars::schema_for!(crate::profiles::ProcessProfile))
                .expect("failed to serialize ProcessProfile"),
        },
        SchemaDefinition {
            schema_id: "slicer-engine/printer-profile-v1",
            schema: serde_json::to_value(schemars::schema_for!(crate::profiles::PrinterProfile))
                .expect("failed to serialize PrinterProfile"),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_schemas_generates_definitions() {
        let schemas = all_schemas();
        assert_eq!(schemas.len(), 15);
    }

    #[test]
    fn test_result_schema_has_correct_id() {
        let schemas = all_schemas();
        let result_schema = schemas
            .iter()
            .find(|s| s.schema_id == "slicer-engine/result-v1")
            .expect("result schema not found");
        assert!(!result_schema.schema.is_null());
    }

    #[test]
    fn test_schema_can_serialize() {
        let result = ResultSchema {
            status: "success".to_string(),
            message: "test".to_string(),
            details: None,
        };
        let json = serde_json::to_value(&result).expect("serialize failed");
        assert_eq!(json["status"], "success");
    }
}
