//! The state a pipeline stage may read and write.
//!
//! Before this module, everything below lived in local variables inside
//! `process_mesh`: the layers under construction, the interior-region
//! snapshots taken at one point and consumed at another, the resolved first
//! layer height. Local variables cannot be reached from an inserted stage, so
//! they are gathered here instead — that, and nothing more, is what
//! [`SliceContext`] is for.

use std::any::{Any, TypeId};
use std::collections::HashMap;

use clipper2::Paths;

use crate::core::SliceLayer;
use crate::logging::ProcessLogger;
use crate::mesh::types::Mesh;
use crate::settings::params::SlicingParams;

/// Inter-stage geometry that one stage computes and a later one consumes.
///
/// Each field is `None`/empty until the stage that owns it runs, and several
/// are deliberately left unset when their feature is off — a stage reading one
/// must cope with absence rather than assume its producer ran.
#[derive(Default)]
pub struct Artifacts {
    /// Per-layer interior regions snapshotted **before** inner walls are
    /// stripped, so the infill boundary cannot expand into the space the
    /// stripped walls occupied. `None` when single-wall restrictions are off.
    pub pre_strip_infill_regions: Option<Vec<Paths>>,
    /// Per-layer interior regions used to place surfaces. Empty when the model
    /// has no top or bottom layers.
    pub interior_regions: Vec<Paths>,
    /// Per-layer pristine `OuterWall` outlines, captured before surface
    /// generation splits any wall, used to grade overhang degree. `None` when
    /// dynamic overhang speed is off.
    pub overhang_support: Option<Vec<Paths>>,
    /// The thickness the object's bottom layer is sliced and charged at.
    pub first_layer_height: f64,
}

/// A heterogeneous, type-keyed store for state a plugin owns.
///
/// One value per concrete type. It exists so a plugin can carry state across
/// its own stages without that state having to be named in [`Artifacts`] —
/// the engine should not have to grow a field every time a plugin needs to
/// remember something.
#[derive(Default)]
pub struct Extensions {
    slots: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
}

impl Extensions {
    /// Store `value`, returning whatever was previously stored for its type.
    pub fn insert<T: Any + Send + Sync>(&mut self, value: T) -> Option<T> {
        self.slots
            .insert(TypeId::of::<T>(), Box::new(value))
            .and_then(|prev| prev.downcast::<T>().ok().map(|b| *b))
    }

    /// Borrow the stored value of type `T`, if any.
    pub fn get<T: Any + Send + Sync>(&self) -> Option<&T> {
        self.slots
            .get(&TypeId::of::<T>())
            .and_then(|b| b.downcast_ref::<T>())
    }

    /// Mutably borrow the stored value of type `T`, if any.
    pub fn get_mut<T: Any + Send + Sync>(&mut self) -> Option<&mut T> {
        self.slots
            .get_mut(&TypeId::of::<T>())
            .and_then(|b| b.downcast_mut::<T>())
    }

    /// Take the stored value of type `T` out of the store.
    pub fn remove<T: Any + Send + Sync>(&mut self) -> Option<T> {
        self.slots
            .remove(&TypeId::of::<T>())
            .and_then(|b| b.downcast::<T>().ok().map(|v| *v))
    }

    /// Whether a value of type `T` is present.
    pub fn contains<T: Any + Send + Sync>(&self) -> bool {
        self.slots.contains_key(&TypeId::of::<T>())
    }
}

/// Everything a stage is handed.
///
/// The lifetime is the slicing run: `mesh`, `params` and `logger` are borrowed
/// from the caller of `process_mesh` and are read-only, while `layers`,
/// `artifacts` and `state` are the run's working set.
pub struct SliceContext<'a> {
    /// The mesh being sliced, in its final placed orientation.
    pub mesh: &'a Mesh,
    /// The resolved slicing parameters. Already spiral-vase normalised, so a
    /// stage never has to re-apply that itself.
    pub params: &'a SlicingParams,
    /// The run's logger; also the cancellation signal.
    pub logger: &'a dyn ProcessLogger,
    /// The layers under construction — the pipeline's actual output.
    pub layers: Vec<SliceLayer>,
    /// Inter-stage geometry.
    pub artifacts: Artifacts,
    /// Plugin-owned state.
    pub state: Extensions,
}

impl<'a> SliceContext<'a> {
    /// Start a run with no layers yet — the slicing stage produces them.
    pub fn new(mesh: &'a Mesh, params: &'a SlicingParams, logger: &'a dyn ProcessLogger) -> Self {
        Self {
            mesh,
            params,
            logger,
            layers: Vec::new(),
            artifacts: Artifacts::default(),
            state: Extensions::default(),
        }
    }

    /// The settings object a plugin owns, or `None` when the user has never
    /// configured it. See [`SlicingParams::plugin_settings`].
    pub fn plugin_settings(
        &self,
        plugin_id: &str,
    ) -> Option<&serde_json::Map<String, serde_json::Value>> {
        self.params.plugin_settings(plugin_id)
    }

    /// Whether a plugin's reserved `enabled` flag is set.
    pub fn plugin_enabled(&self, plugin_id: &str) -> bool {
        self.params.plugin_enabled(plugin_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq)]
    struct Marker(u32);

    #[test]
    fn extensions_round_trip_by_type() {
        let mut ext = Extensions::default();
        assert!(!ext.contains::<Marker>());
        assert!(ext.insert(Marker(1)).is_none());
        assert_eq!(ext.get::<Marker>(), Some(&Marker(1)));
        ext.get_mut::<Marker>().unwrap().0 = 2;
        assert_eq!(ext.insert(Marker(3)), Some(Marker(2)));
        assert_eq!(ext.remove::<Marker>(), Some(Marker(3)));
        assert!(!ext.contains::<Marker>());
    }
}
