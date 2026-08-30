//! Unified scene engine — single source of truth for object placement,
//! orientation, and transforms across CLI, WS server, and (via WASM) the UI.
//!
//! See [issue #51](https://github.com/max-scopp/slicer-engine/issues/51)
//! for the architecture plan.

pub mod bed;
pub mod loader;
pub mod ops;
pub mod state;
pub mod transform;

#[cfg(target_arch = "wasm32")]
pub mod wasm;

pub use bed::{BedConfig, BedShape};
pub use loader::{
    load_bytes, load_bytes_multi, load_bytes_multi_reporting, load_bytes_reporting, load_path,
    load_path_multi, load_path_multi_reporting, load_path_reporting, LoadedPart, MeshFormat,
};
pub use ops::{OpReceipt, SceneError, SceneOp, SceneOptions};
pub use state::{ObjectId, ObjectPlacement, SceneObject, SceneState};
pub use transform::{apply_transform, transformed_aabb, Transform};
