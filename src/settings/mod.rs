//! Slicing parameters and settings validation.
//!
//! # Modules
//! - [`params`]: [`SlicingParams`], [`ObjectSettings`], [`LifecycleMarkerConfig`]
//! - [`validator`]: [`SettingValidator`] trait + [`ValidationRules`] stubs
//! - [`diff`]: [`SettingsDiff`] struct + [`compare_settings`] function

pub mod diff;
pub mod params;
pub mod validator;

pub use diff::{compare_settings, SettingsDiff};
pub use params::{AdhesionType, SupportType};
pub use params::{
    AuxFanOverrides, FanConfig, LifecycleMarkerConfig, MeshQuality, ObjectSettings, SlicingParams,
};
pub use validator::{SettingValidator, ValidationRules};
