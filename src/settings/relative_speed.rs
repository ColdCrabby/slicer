//! A speed that is either a literal mm/s value or a percentage of another
//! field's value.
//!
//! Some speeds only make sense relative to another speed the profile already
//! states — an overhang band as a fraction of `perimeter_speed`, say — and
//! pinning that relationship to one absolute number means it silently goes
//! stale the moment the source field changes. [`RelativeSpeed`] lets a field
//! say "40% of X" once and have it hold under any `X`.

use std::borrow::Cow;
use std::fmt;

use schemars::{json_schema, JsonSchema, Schema, SchemaGenerator};
use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A speed expressed either as a literal `mm/s` value or as a fraction of a
/// named "source" field (declared on the owning schema field via the
/// `x-relative-to` extension).
///
/// On the wire, a bare JSON number is [`Absolute`](Self::Absolute) and a
/// string ending in `%` (e.g. `"40%"`) is [`Percent`](Self::Percent) — the two
/// variants map to genuinely different JSON *shapes*, which is what lets a
/// plain profile diff hold either without a tag field.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RelativeSpeed {
    /// A literal speed, in mm/s.
    Absolute(f64),
    /// A fraction of the source field's value (`0.4` = "40%").
    Percent(f64),
}

impl RelativeSpeed {
    /// Resolve against the source field's current speed, in mm/s.
    ///
    /// Returns `None` for the legacy `Absolute(0.0)` sentinel this field used
    /// before it could hold a percentage — profiles saved with the literal
    /// `0` that meant "no override, use the base speed" keep meaning exactly
    /// that. Every other value resolves to a real number, including
    /// `Percent(0.0)` ("0%"), which is a deliberate (if unusual) choice to
    /// resolve to `0.0`, not a no-op.
    pub fn resolve(&self, source_mm_s: f64) -> Option<f64> {
        match self {
            RelativeSpeed::Absolute(v) if *v == 0.0 => None,
            RelativeSpeed::Absolute(v) => Some(*v),
            RelativeSpeed::Percent(p) => Some(p * source_mm_s),
        }
    }
}

impl fmt::Display for RelativeSpeed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RelativeSpeed::Absolute(v) => write!(f, "{v} mm/s"),
            RelativeSpeed::Percent(p) => write!(f, "{}%", p * 100.0),
        }
    }
}

impl Serialize for RelativeSpeed {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            RelativeSpeed::Absolute(v) => serializer.serialize_f64(*v),
            RelativeSpeed::Percent(p) => serializer.serialize_str(&format!("{}%", p * 100.0)),
        }
    }
}

impl<'de> Deserialize<'de> for RelativeSpeed {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct RelativeSpeedVisitor;

        impl<'de> Visitor<'de> for RelativeSpeedVisitor {
            type Value = RelativeSpeed;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a number in mm/s, or a percent string like \"40%\"")
            }

            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Self::Value, E> {
                Ok(RelativeSpeed::Absolute(v))
            }

            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
                Ok(RelativeSpeed::Absolute(v as f64))
            }

            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
                Ok(RelativeSpeed::Absolute(v as f64))
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                let percent = v
                    .trim()
                    .strip_suffix('%')
                    .ok_or_else(|| de::Error::invalid_value(de::Unexpected::Str(v), &self))?;
                let fraction: f64 = percent
                    .trim()
                    .parse()
                    .map_err(|_| de::Error::invalid_value(de::Unexpected::Str(v), &self))?;
                Ok(RelativeSpeed::Percent(fraction / 100.0))
            }
        }

        deserializer.deserialize_any(RelativeSpeedVisitor)
    }
}

impl JsonSchema for RelativeSpeed {
    // A scalar value type, inlined at every field that uses it rather than
    // hung off `$defs` — the same treatment `f64`/`String` get.
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("RelativeSpeed")
    }

    fn schema_id() -> Cow<'static, str> {
        Cow::Borrowed(concat!(module_path!(), "::RelativeSpeed"))
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "oneOf": [
                { "type": "number", "description": "Absolute speed, in mm/s." },
                {
                    "type": "string",
                    "pattern": "^-?\\d+(\\.\\d+)?%$",
                    "description": "Percentage of the source field named by `x-relative-to`."
                }
            ]
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_absolute() {
        assert_eq!(RelativeSpeed::Absolute(32.0).resolve(120.0), Some(32.0));
    }

    #[test]
    fn resolves_percent_against_the_source() {
        assert_eq!(RelativeSpeed::Percent(0.4).resolve(80.0), Some(32.0));
        assert_eq!(RelativeSpeed::Percent(0.4).resolve(200.0), Some(80.0));
    }

    #[test]
    fn legacy_zero_sentinel_means_no_override() {
        assert_eq!(RelativeSpeed::Absolute(0.0).resolve(80.0), None);
    }

    #[test]
    fn explicit_zero_percent_resolves_to_zero() {
        assert_eq!(RelativeSpeed::Percent(0.0).resolve(80.0), Some(0.0));
    }

    #[test]
    fn serializes_absolute_as_a_bare_number() {
        let json = serde_json::to_value(RelativeSpeed::Absolute(32.0)).unwrap();
        assert_eq!(json, serde_json::json!(32.0));
    }

    #[test]
    fn serializes_percent_as_a_suffixed_string() {
        let json = serde_json::to_value(RelativeSpeed::Percent(0.4)).unwrap();
        assert_eq!(json, serde_json::json!("40%"));
    }

    #[test]
    fn round_trips_through_json() {
        for value in [RelativeSpeed::Absolute(32.0), RelativeSpeed::Percent(0.4)] {
            let json = serde_json::to_value(value).unwrap();
            let back: RelativeSpeed = serde_json::from_value(json).unwrap();
            assert_eq!(back, value);
        }
    }

    #[test]
    fn deserializes_a_bare_number_as_absolute() {
        let v: RelativeSpeed = serde_json::from_value(serde_json::json!(32.0)).unwrap();
        assert_eq!(v, RelativeSpeed::Absolute(32.0));
    }

    #[test]
    fn deserializes_a_percent_string() {
        let v: RelativeSpeed = serde_json::from_value(serde_json::json!("40%")).unwrap();
        assert_eq!(v, RelativeSpeed::Percent(0.4));
    }

    #[test]
    fn displays_each_variant_with_its_own_unit() {
        assert_eq!(RelativeSpeed::Absolute(32.0).to_string(), "32 mm/s");
        assert_eq!(RelativeSpeed::Percent(0.4).to_string(), "40%");
    }

    #[test]
    fn rejects_a_string_without_a_percent_suffix() {
        let err = serde_json::from_value::<RelativeSpeed>(serde_json::json!("40"));
        assert!(err.is_err());
    }
}
