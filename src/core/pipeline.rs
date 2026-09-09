//! The two entry points every runtime funnels through.
//!
//! The sequence itself lives in [`super::stages`] as a list of named stages;
//! this module only builds the run's context, folds in whatever plugins apply,
//! and hands the result back. Keeping it that thin is the point — a step added
//! here rather than there is a step no plugin can reach.

use crate::logging::ProcessLogger;
use crate::mesh::types::Mesh;
use crate::plugin::{self, Extensions, Plugin, SliceContext};
use crate::settings::params::SlicingParams;

use super::stages::core_stages;
use super::types::SliceLayer;

/// True when a raft will be printed under the object.
///
/// A raft takes over bed contact entirely, which changes two answers elsewhere
/// in the pipeline: the object's first layer is no longer the one that has to
/// cope with an imperfect bed (so elephant-foot compensation is skipped), and
/// it is no longer the layer a first-layer height was chosen for.
fn raft_is_active(params: &SlicingParams) -> bool {
    params.adhesion_type == crate::settings::params::AdhesionType::Raft && params.raft_layers > 0
}

/// Thickness of the object's bottom layer, in mm.
///
/// `first_layer_height` is a bed-contact remedy, so it is deliberately ignored
/// when a raft is present: the object then starts on plastic, and the raft's own
/// layers are documented to print at `layer_height` (see the adhesion module).
/// Falls back to `layer_height` when unset.
///
/// Shared with the G-code generator, which needs the same answer to decide which
/// layer gets first-layer speeds and temperatures — the two must never disagree
/// about which layer is the first.
pub(crate) fn resolved_first_layer_height(params: &SlicingParams) -> f64 {
    if params.first_layer_height > 0.0 && !raft_is_active(params) {
        params.first_layer_height
    } else {
        params.layer_height
    }
}

/// Run the pipeline over `mesh` with `plugins` folded into the core stage
/// list, returning the finished layers and whatever state those plugins left
/// behind.
///
/// Shared by both entry points below, so a debug run and a production run
/// cannot drift apart the way they used to: the sequence, the artifacts and
/// the cancellation behaviour are the same code, and a debug run differs only
/// by the plugin it installs.
fn run_pipeline(
    mesh: &Mesh,
    params: &SlicingParams,
    logger: &dyn ProcessLogger,
    plugins: &[Box<dyn Plugin>],
) -> (Vec<SliceLayer>, Extensions) {
    // Spiral (vase) mode forces a consistent single-wall configuration (no
    // infill / top surfaces / retraction) for the whole pipeline. Normalised
    // here so every entry point — and every stage, and every plugin — observes
    // the same rules rather than each re-deriving them.
    let normalized = params.spiral_vase_normalized();

    logger.log_info(&format!("processing mesh: {} triangles", mesh.faces.len()));

    let mut registry = core_stages();
    plugin::install(&mut registry, plugins, logger);

    let mut cx = SliceContext::new(mesh, normalized.as_ref(), logger);
    registry.run(&mut cx);
    (cx.layers, cx.state)
}

/// Central entry point for the complete slicing pipeline.
///
/// Runs `mesh` through every stage in [`super::stages::core_stages`] plus the
/// plugins this build ships, and returns the finished layers. Progress is
/// reported through `logger` so CLI and WebSocket callers see the same
/// verbosity; `logger.is_cancelled()` is consulted at each stage boundary, and
/// a cancelled run returns the layers built so far.
///
/// # Arguments
/// * `mesh` - The triangle mesh to process
/// * `params` - Slicing parameters controlling all aspects of the slicing process
/// * `logger` - Pipeline logger; use [`NullLogger`] when logging is not needed
///
/// # Returns
/// A `Vec<SliceLayer>` with all processing applied (walls, surfaces, etc.).
///
/// # Example
/// ```
/// use slicer_engine::logging::NullLogger;
/// use slicer_engine::mesh::types::Mesh;
/// use slicer_engine::settings::params::SlicingParams;
/// use slicer_engine::core::process_mesh;
///
/// let mesh = Mesh::new(); // Load your mesh
/// let params = SlicingParams::default();
/// let layers = process_mesh(&mesh, &params, &NullLogger);
/// ```
///
/// [`NullLogger`]: crate::logging::NullLogger
pub fn process_mesh(
    mesh: &Mesh,
    params: &SlicingParams,
    logger: &dyn ProcessLogger,
) -> Vec<SliceLayer> {
    let plugins = plugin::builtin_plugins();
    run_pipeline(mesh, params, logger, &plugins).0
}

/// [`process_mesh`], with an explicit plugin set instead of the built-in one.
///
/// The seam the whole design rests on: the external loader, when it arrives,
/// is a plugin passed in here rather than a second pipeline. Passing an empty
/// slice is the bare core pipeline, which is what the byte-identity tests
/// compare against.
pub fn process_mesh_with_plugins(
    mesh: &Mesh,
    params: &SlicingParams,
    logger: &dyn ProcessLogger,
    plugins: &[Box<dyn Plugin>],
) -> Vec<SliceLayer> {
    run_pipeline(mesh, params, logger, plugins).0
}

/// Debug variant of [`process_mesh`].
///
/// Runs **the same pipeline** and additionally collects geometry snapshots at
/// every major stage into `debug`. It is the production sequence plus one
/// plugin, not a second copy of it — which is why the layers it returns are
/// genuinely identical to `process_mesh`'s. (They were not, before: the copy
/// this replaced had drifted, and silently skipped path ordering and bed
/// adhesion, so `--debug-geometry` wrote unordered G-code with no skirt.)
///
/// Because walls are generated sequentially in debug mode — the snapshot has
/// to be captured in order — this is **significantly slower** than
/// `process_mesh` on large models. Use it only for debugging.
///
/// # Snapshots captured
///
/// | Stage | When |
/// |---|---|
/// | `RawContours` | After slicing and both compensation passes, before wall generation |
/// | `WallNormalisedInput` | Per layer: EvenOdd-union result fed to the generator |
/// | `WallOffsetStep { bead_k }` | Per layer / per bead: each `shrink` intermediate |
/// | `WallBeads` | Per layer: final bead centerlines |
/// | `InteriorRegion` | Per layer: inside-wall region for infill/surfaces |
/// | `SolidSurface` | Per layer: `layer.solid_regions` after surface generation |
/// | `Infill` | Per layer: infill + surface fill paths |
#[cfg(not(target_arch = "wasm32"))]
pub fn process_mesh_debug(
    mesh: &Mesh,
    params: &SlicingParams,
    logger: &dyn ProcessLogger,
    debug: &mut crate::debug::DebugGeometry,
) -> Vec<SliceLayer> {
    let plugins: Vec<Box<dyn Plugin>> = vec![Box::new(plugin::builtin::DebugCapture)];
    let (layers, mut state) = run_pipeline(mesh, params, logger, &plugins);

    if let Some(captured) = state.remove::<crate::debug::DebugGeometry>() {
        *debug = captured;
    }
    logger.log_debug(&format!(
        "debug geometry: {} records captured across {} layers",
        debug.len(),
        layers.len()
    ));
    layers
}
