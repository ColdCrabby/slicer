//! Slicing parameters and settings validation.
//!
//! # Modules
//! - [`params`]: [`SlicingParams`], [`ObjectSettings`], [`LifecycleMarkerConfig`]
//! - [`validator`]: [`SettingValidator`] trait + [`ValidationRules`] stubs
//! - [`diff`]: [`SettingsDiff`] struct + [`compare_settings`] function
//! - [`relative_speed`]: [`RelativeSpeed`], a speed given as either mm/s or a
//!   percentage of another field

pub mod diff;
pub mod params;
pub mod relative_speed;
pub mod validator;

pub use diff::{compare_settings, SettingsDiff};
pub use params::{AdhesionType, BrimType, SupportType};
pub use params::{
    AuxFanOverrides, FanConfig, LifecycleMarkerConfig, MeshQuality, ObjectSettings, SlicingParams,
};
pub use relative_speed::RelativeSpeed;
pub use validator::{SettingValidator, ValidationRules};
