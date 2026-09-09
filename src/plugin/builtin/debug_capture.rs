//! Per-stage geometry capture, as a plugin.
//!
//! This is what `process_mesh_debug` used to be: a second, hand-maintained
//! copy of the whole pipeline whose only purpose was to snapshot geometry
//! between steps. It had already drifted — the copy skipped path ordering and
//! bed adhesion, so `--debug-geometry` quietly emitted unordered G-code with
//! no skirt.
//!
//! Expressed as stages, the duplication disappears: snapshots are stages
//! inserted after the ones they observe, and the only step that genuinely
//! differs in a debug run — wall generation, which must run sequentially to
//! capture its intermediates in order — is a wrapper over that one stage
//! rather than a reason to re-implement the other sixteen.

use crate::core::stages::ids;
use crate::core::ExtrusionRole;
use crate::debug::{DebugGeometry, DebugStage};
use crate::plugin::{
    FnStage, Plugin, PluginManifest, SliceContext, Stage, StageId, StageRegistration, StageWrapper,
};

/// Captures the geometry snapshots that back `--debug-geometry` and the QA
/// report's layer gallery.
///
/// Installed only by [`crate::core::process_mesh_debug`]; it is not part of
/// [`crate::plugin::builtin_plugins`], so an ordinary slice never pays for it.
pub struct DebugCapture;

impl Plugin for DebugCapture {
    fn manifest(&self) -> PluginManifest {
        PluginManifest::stable(
            "debug-capture",
            "Debug geometry capture",
            "Snapshots per-layer geometry at each pipeline stage.",
        )
    }

    fn stages(&self) -> Vec<StageRegistration> {
        vec![
            // The store the other stages write into. Seeded before anything
            // runs so no capture stage has to cope with it being absent.
            StageRegistration::before(
                ids::SLICING,
                FnStage::boxed("debug-capture:open", |cx: &mut SliceContext<'_>| {
                    cx.state.insert(DebugGeometry::new());
                }),
            ),
            // After both compensation passes, so `RawContours` shows exactly
            // the shapes the wall generator is about to receive.
            StageRegistration::after(
                ids::ELEPHANT_FOOT,
                FnStage::boxed("debug-capture:raw-contours", |cx: &mut SliceContext<'_>| {
                    capture(cx, |layers, debug| {
                        for (i, layer) in layers.iter().enumerate() {
                            debug.push(DebugStage::RawContours, i, layer.z, layer.paths.clone());
                        }
                    });
                }),
            ),
            StageRegistration::wrap(ids::WALL_GENERATION, Box::new(SequentialWalls)),
            StageRegistration::after(
                ids::INTERIOR_REGIONS,
                FnStage::boxed("debug-capture:interior", |cx: &mut SliceContext<'_>| {
                    let regions = std::mem::take(&mut cx.artifacts.interior_regions);
                    capture(cx, |layers, debug| {
                        for (i, (layer, region)) in layers.iter().zip(regions.iter()).enumerate() {
                            debug.push(DebugStage::InteriorRegion, i, layer.z, region.clone());
                        }
                    });
                    cx.artifacts.interior_regions = regions;
                }),
            ),
            StageRegistration::after(
                ids::SURFACES,
                FnStage::boxed("debug-capture:surfaces", |cx: &mut SliceContext<'_>| {
                    if cx.params.top_layers == 0 && cx.params.bottom_layers == 0 {
                        return;
                    }
                    capture(cx, |layers, debug| {
                        for (i, layer) in layers.iter().enumerate() {
                            debug.push(
                                DebugStage::SolidSurface,
                                i,
                                layer.z,
                                layer.solid_regions.clone(),
                            );
                        }
                    });
                }),
            ),
            StageRegistration::after(
                ids::INFILL,
                FnStage::boxed("debug-capture:infill", |cx: &mut SliceContext<'_>| {
                    if cx.params.infill_density <= 0.0 {
                        return;
                    }
                    capture(cx, |layers, debug| {
                        for (layer_index, layer) in layers.iter().enumerate() {
                            let infill_paths: Vec<clipper2::Path> = layer
                                .paths
                                .iter()
                                .enumerate()
                                .filter(|(i, _)| {
                                    matches!(
                                        layer.role_for_path(*i),
                                        ExtrusionRole::Infill
                                            | ExtrusionRole::TopSurface
                                            | ExtrusionRole::BottomSurface
                                            | ExtrusionRole::Bridge
                                    )
                                })
                                .map(|(_, p)| p.clone())
                                .collect();
                            if !infill_paths.is_empty() {
                                debug.push(
                                    DebugStage::Infill,
                                    layer_index,
                                    layer.z,
                                    clipper2::Paths::new(infill_paths),
                                );
                            }
                        }
                    });
                }),
            ),
        ]
    }
}

/// Run `capture` with the layers and the snapshot store borrowed at once.
///
/// The store is taken out of the context for the duration because both live
/// behind the same `&mut SliceContext`; a plugin reading its own state while
/// reading the layers is the ordinary case, so it is worth doing tidily here.
fn capture(
    cx: &mut SliceContext<'_>,
    f: impl FnOnce(&[crate::core::SliceLayer], &mut DebugGeometry),
) {
    let Some(mut debug) = cx.state.remove::<DebugGeometry>() else {
        return;
    };
    f(&cx.layers, &mut debug);
    cx.state.insert(debug);
}

/// Generates walls sequentially, capturing each bead offset as it goes.
///
/// Replaces the wall stage outright rather than bracketing it — the debug
/// generator does the same work, in order, with snapshots. This is the one
/// step of a debug run that is not simply the production step.
struct SequentialWalls;

impl StageWrapper for SequentialWalls {
    fn id(&self) -> StageId {
        StageId::new("debug-capture:walls")
    }

    fn run(&self, cx: &mut SliceContext<'_>, _inner: &dyn Stage) {
        cx.logger
            .log_debug("generating walls (debug mode, sequential)");
        let Some(mut debug) = cx.state.remove::<DebugGeometry>() else {
            return;
        };
        crate::walls::generate_walls_debug(&mut cx.layers, cx.params, &mut debug);
        cx.state.insert(debug);
        cx.logger.log_debug("wall generation complete");
    }
}
