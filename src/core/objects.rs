//! Multi-object plates: slicing that preserves per-object identity.
//!
//! The scene engine tracks discrete objects, but the slicer historically saw
//! one merged mesh — by the time layers existed, every trace of "which part is
//! this?" was gone.  Two features need that identity back:
//!
//! - **Exclude object** (issue #22) — wrap each object's moves in firmware
//!   markers so one failed part can be cancelled without aborting the plate.
//! - **Sequential printing** (issue #112) — finish one object before starting
//!   the next.
//!
//! Both are served by the same segmentation, which is why they are built here
//! once.  [`slice_plate`] is the single entry point: CLI, WebSocket server and
//! the in-browser wasm slicer all call it instead of merging meshes themselves.
//!
//! ## The merged fast path is not optional
//!
//! When neither feature is enabled ([`SlicingParams::object_aware`] is false)
//! the plate is merged into one mesh and handed to [`process_mesh`] exactly as
//! before, so the default configuration keeps producing byte-identical G-code.
//! Object-aware slicing runs the pipeline once *per object*, which is not
//! output-equivalent: `calculate_interior_region` averages the wall-bead count
//! across the islands of a layer, so an island's interior estimate depends on
//! what else shares its layer.  Slicing a part on its own is the more faithful
//! result, but it is still a *change*, and it must only happen when the user
//! asked for a feature that requires it.
//!
//! ## Where adhesion goes
//!
//! A skirt is drawn around everything on the plate and a brim spans whatever it
//! touches, so in `by_layer` order adhesion is generated **once, on the merged
//! layer stack**, and its paths are tagged as belonging to no object — a
//! cancelled part must not take the plate's adhesion with it.  In `by_object`
//! order each object is a self-contained print, so it gets (and owns) its own
//! adhesion.

use crate::logging::ProcessLogger;
use crate::mesh::types::Mesh;
use crate::settings::params::{AdhesionType, PrintSequence, SlicingParams};

use super::pipeline::process_mesh;
use super::types::{OverhangClass, SliceLayer};

/// Upper bound on the vertex count of an emitted exclusion polygon.
///
/// Klipper accepts an arbitrarily long `POLYGON=`, but a turned cylinder hulls
/// to one point per facet and would push a multi-kilobyte line into the file
/// for no added precision.  The hull is evenly resampled down to this many
/// vertices, which still traces a round part to well under a nozzle width.
const MAX_EXCLUSION_POLYGON_POINTS: usize = 64;

/// One object on the plate, as handed to the slicer.
///
/// The mesh is expected to be **already baked** into plate coordinates — the
/// scene engine applies transforms exactly once, at this boundary (see
/// `src/scene/README.md`).
#[derive(Debug, Clone)]
pub struct ObjectInput {
    /// Display name, used verbatim in the G-code object markers after
    /// sanitisation.
    pub name: String,
    /// Baked triangle mesh in plate coordinates.
    pub mesh: Mesh,
}

impl ObjectInput {
    /// Construct an input from a name and a baked mesh.
    pub fn new(name: impl Into<String>, mesh: Mesh) -> Self {
        Self {
            name: name.into(),
            mesh,
        }
    }
}

/// Identity and footprint of one print object, as the G-code needs it.
///
/// Produced by [`slice_plate`] and consumed by
/// [`crate::gcode::GcodeGenerator`], which renders it into
/// `EXCLUDE_OBJECT_DEFINE` (Klipper) or `M486` (Marlin / RepRapFirmware).
#[derive(Debug, Clone, PartialEq)]
pub struct ObjectIdentity {
    /// Index into [`PlateSlice::objects`]; matches the tag stored in
    /// [`SliceLayer::path_objects`].
    pub index: usize,
    /// Firmware-safe object name, unique across the plate.
    pub name: String,
    /// XY centre of the object's bounding box, in mm.
    pub center: (f64, f64),
    /// Convex hull of the object's XY footprint, in mm, counter-clockwise.
    pub polygon: Vec<(f64, f64)>,
    /// Axis-aligned XY bounds as `(min_x, min_y, max_x, max_y)`, in mm.
    pub bbox: (f64, f64, f64, f64),
    /// Height of the object above the bed, in mm.
    pub height_mm: f64,
}

impl ObjectIdentity {
    /// Shortest XY gap between this object's bounding box and `other`'s.
    ///
    /// `0.0` when the boxes touch or overlap.
    pub fn bbox_gap(&self, other: &ObjectIdentity) -> f64 {
        let (ax0, ay0, ax1, ay1) = self.bbox;
        let (bx0, by0, bx1, by1) = other.bbox;
        let dx = (bx0 - ax1).max(ax0 - bx1).max(0.0);
        let dy = (by0 - ay1).max(ay0 - by1).max(0.0);
        (dx * dx + dy * dy).sqrt()
    }
}

/// A sliced plate: the layer stream to emit, plus who owns what.
///
/// `objects` is empty for a plate sliced through the merged fast path; in that
/// case every path in `layers` carries no object tag and the G-code contains no
/// object markers.
#[derive(Debug, Clone, Default)]
pub struct PlateSlice {
    /// Layers in emission order.  In `by_object` order this is the
    /// concatenation of each object's own stack, so `z` restarts at every
    /// object boundary.
    pub layers: Vec<SliceLayer>,
    /// Object identities, indexed by the tag in [`SliceLayer::path_objects`].
    pub objects: Vec<ObjectIdentity>,
}

impl PlateSlice {
    /// Wrap an already-sliced, untagged layer stack (the single-object case).
    pub fn from_layers(layers: Vec<SliceLayer>) -> Self {
        Self {
            layers,
            objects: Vec::new(),
        }
    }
}

/// Slice a whole plate, preserving per-object identity when the configuration
/// needs it.
///
/// This is the one place any runtime turns a list of placed objects into
/// layers.  See the module docs for why the merged path is kept intact.
pub fn slice_plate(
    objects: &[ObjectInput],
    params: &SlicingParams,
    logger: &dyn ProcessLogger,
) -> PlateSlice {
    if !params.object_aware() || objects.is_empty() {
        return PlateSlice::from_layers(process_mesh(&merge_meshes(objects), params, logger));
    }

    let sequential = params.print_sequence == PrintSequence::ByObject;

    // In `by_layer` order the plate's adhesion is generated once on the merged
    // stack (below), so the per-object passes must not each grow their own.
    let per_object_params = if sequential || params.adhesion_type == AdhesionType::None {
        None
    } else {
        let mut p = params.clone();
        p.adhesion_type = AdhesionType::None;
        // Clearing the adhesion type stops each object growing its own skirt or
        // brim, but it also erases the fact that the plate sits on a raft — and
        // elephant-foot compensation reads exactly that to decide whether the
        // first layer ever meets the bed. Carry the answer across explicitly, or
        // an object-aware raft print gets its base shrunk for a squish that
        // never happens.
        if params.adhesion_type == AdhesionType::Raft {
            p.elephant_foot_compensation_mm = 0.0;
        }
        Some(p)
    };
    let slice_params = per_object_params.as_ref().unwrap_or(params);

    logger.log_info(&format!(
        "slicing {} objects individually ({})",
        objects.len(),
        if sequential {
            "sequential print order"
        } else {
            "object-tagged layers"
        }
    ));

    let mut identities = Vec::with_capacity(objects.len());
    let mut per_object_layers: Vec<Vec<SliceLayer>> = Vec::with_capacity(objects.len());
    let mut used_names: Vec<String> = Vec::with_capacity(objects.len());

    let object_count = objects.len();
    for (index, object) in objects.iter().enumerate() {
        logger.log_debug(&format!(
            "object {}/{}: '{}'",
            index + 1,
            object_count,
            object.name
        ));
        let name = unique_object_name(&object.name, index, &used_names);
        used_names.push(name.clone());
        identities.push(identity_for(index, name, &object.mesh));
        // Scope the phase markers to this object so a progress bar keyed on
        // phase names keeps moving forward as the pipeline restarts per object,
        // and can label each phase "(i of N)".
        logger.set_object_scope(index + 1, object_count);
        per_object_layers.push(process_mesh(&object.mesh, slice_params, logger));
    }
    logger.clear_object_scope();

    let layers = if sequential {
        for warning in sequential_warnings(&identities, params) {
            logger.log_warn(&warning);
        }
        let order = sequential_order(&identities);
        logger.log_info(&format!(
            "sequential print order: {}",
            order
                .iter()
                .map(|&i| identities[i].name.as_str())
                .collect::<Vec<_>>()
                .join(" → ")
        ));
        let mut layers = Vec::new();
        for &object_index in &order {
            for layer in &per_object_layers[object_index] {
                // Drop empty layers: in one merged stack they were harmless
                // (an extra Z move), but concatenated per object they would be
                // the *only* layers carrying no object tag, leaving the
                // generator unable to tell whose Z they are — and an untagged
                // layer at an object boundary is exactly where the nozzle must
                // not descend.
                if layer.paths.is_empty() {
                    continue;
                }
                layers.push(tagged_copy(layer, object_index));
            }
        }
        layers
    } else {
        let mut layers = merge_layers_by_z(&per_object_layers, params.layer_height);
        if params.adhesion_type != AdhesionType::None {
            crate::adhesion::apply_adhesion(&mut layers, params);
        }
        layers
    };

    PlateSlice {
        layers,
        objects: identities,
    }
}

/// Concatenate every object's baked triangles into the single mesh the
/// classic (non-object-aware) pipeline slices.
///
/// `Face` stores its vertices by value, so concatenation needs no index
/// remapping.
pub fn merge_meshes(objects: &[ObjectInput]) -> Mesh {
    let mut combined = Mesh::new();
    for object in objects {
        combined
            .vertices
            .extend(object.mesh.vertices.iter().copied());
        combined.faces.extend(object.mesh.faces.iter().cloned());
    }
    combined
}

/// Print order for sequential printing: front to back, then left to right.
///
/// A cartesian gantry sweeps over the bed from behind, so finishing the parts
/// nearest the front first keeps the carriage away from everything already
/// printed for as long as possible — the same heuristic PrusaSlicer, Cura and
/// OrcaSlicer apply.  Ties fall back to the object index so the order is
/// deterministic.
pub fn sequential_order(objects: &[ObjectIdentity]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..objects.len()).collect();
    order.sort_by(|&a, &b| {
        let (ax, ay) = (objects[a].bbox.0, objects[a].bbox.1);
        let (bx, by) = (objects[b].bbox.0, objects[b].bbox.1);
        ay.partial_cmp(&by)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(ax.partial_cmp(&bx).unwrap_or(std::cmp::Ordering::Equal))
            .then(a.cmp(&b))
    });
    order
}

/// Collision risks that make a plate unsafe to print one object at a time.
///
/// Two independent hazards, both measured against the machine's declared
/// clearances:
///
/// - **Vertical** — every object except the last one printed has to pass under
///   the gantry while the *next* one prints, so anything taller than
///   [`SlicingParams::extruder_clearance_height_mm`] is a crash waiting to
///   happen.
/// - **Horizontal** — the hotend and its fan duct sweep a disc around the
///   nozzle, so two objects closer than
///   [`SlicingParams::extruder_clearance_radius_mm`] cannot both be reached
///   without the duct grazing the finished one.
///
/// Returned as warnings rather than errors: the clearances are machine
/// estimates, and refusing to slice would be worse than telling the user what
/// to check.
pub fn sequential_warnings(objects: &[ObjectIdentity], params: &SlicingParams) -> Vec<String> {
    let mut warnings = Vec::new();
    if objects.len() < 2 {
        return warnings;
    }

    let order = sequential_order(objects);
    let clearance_height = params.extruder_clearance_height_mm;
    if clearance_height > 0.0 {
        // The last object printed has nothing printed after it, so its height
        // is irrelevant — only the ones the gantry still has to reach over.
        for &index in order.iter().take(order.len() - 1) {
            let object = &objects[index];
            if object.height_mm > clearance_height {
                warnings.push(format!(
                    "sequential printing: '{}' is {:.1} mm tall, above the {:.1} mm gantry \
                     clearance — the carriage may hit it while printing a later object",
                    object.name, object.height_mm, clearance_height
                ));
            }
        }
    }

    let clearance_radius = params.extruder_clearance_radius_mm;
    if clearance_radius > 0.0 {
        for a in 0..objects.len() {
            for b in (a + 1)..objects.len() {
                let gap = objects[a].bbox_gap(&objects[b]);
                if gap < clearance_radius {
                    warnings.push(format!(
                        "sequential printing: '{}' and '{}' are {:.1} mm apart, closer than the \
                         {:.1} mm extruder clearance radius — space them further apart",
                        objects[a].name, objects[b].name, gap, clearance_radius
                    ));
                }
            }
        }
    }

    warnings
}

/// Build the identity record for one object from its baked mesh.
fn identity_for(index: usize, name: String, mesh: &Mesh) -> ObjectIdentity {
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut min_z = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    let mut max_z = f64::NEG_INFINITY;
    let mut points: Vec<(f64, f64)> = Vec::with_capacity(mesh.vertices.len());
    for v in &mesh.vertices {
        min_x = min_x.min(v.x);
        min_y = min_y.min(v.y);
        min_z = min_z.min(v.z);
        max_x = max_x.max(v.x);
        max_y = max_y.max(v.y);
        max_z = max_z.max(v.z);
        points.push((v.x, v.y));
    }
    if points.is_empty() {
        return ObjectIdentity {
            index,
            name,
            center: (0.0, 0.0),
            polygon: Vec::new(),
            bbox: (0.0, 0.0, 0.0, 0.0),
            height_mm: 0.0,
        };
    }

    let polygon = resample_ring(convex_hull(points), MAX_EXCLUSION_POLYGON_POINTS);
    ObjectIdentity {
        index,
        name,
        center: (0.5 * (min_x + max_x), 0.5 * (min_y + max_y)),
        polygon,
        bbox: (min_x, min_y, max_x, max_y),
        height_mm: (max_z - min_z).max(0.0),
    }
}

/// Andrew's monotone chain convex hull, returned counter-clockwise and without
/// a repeated closing vertex.
fn convex_hull(mut points: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    points.sort_by(|a, b| {
        a.0.partial_cmp(&b.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
    });
    points.dedup_by(|a, b| (a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9);
    if points.len() < 3 {
        return points;
    }

    let cross = |o: (f64, f64), a: (f64, f64), b: (f64, f64)| {
        (a.0 - o.0) * (b.1 - o.1) - (a.1 - o.1) * (b.0 - o.0)
    };

    let mut hull: Vec<(f64, f64)> = Vec::with_capacity(points.len() * 2);
    for &p in &points {
        while hull.len() >= 2 && cross(hull[hull.len() - 2], hull[hull.len() - 1], p) <= 0.0 {
            hull.pop();
        }
        hull.push(p);
    }
    let lower_len = hull.len() + 1;
    for &p in points.iter().rev().skip(1) {
        while hull.len() >= lower_len && cross(hull[hull.len() - 2], hull[hull.len() - 1], p) <= 0.0
        {
            hull.pop();
        }
        hull.push(p);
    }
    hull.pop();
    hull
}

/// Evenly resample a closed ring down to at most `max_points` vertices.
///
/// Keeps the first vertex so the polygon still starts where the hull did.
fn resample_ring(ring: Vec<(f64, f64)>, max_points: usize) -> Vec<(f64, f64)> {
    if ring.len() <= max_points || max_points == 0 {
        return ring;
    }
    let n = ring.len();
    (0..max_points).map(|k| ring[k * n / max_points]).collect()
}

/// Sanitise `name` for a firmware object marker and disambiguate duplicates.
///
/// Klipper parses `EXCLUDE_OBJECT_DEFINE NAME=…` as a G-code parameter, so
/// whitespace and `=` would split the token; a duplicate name would make two
/// parts cancel together.  Both are corrected here rather than at the call
/// sites, because every runtime feeds user-chosen filenames straight in.
fn unique_object_name(name: &str, index: usize, used: &[String]) -> String {
    let mut sanitized: String = name
        .trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | '#' | '+') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if sanitized.is_empty() {
        sanitized = "object".to_string();
    }
    if !used.iter().any(|u| u == &sanitized) {
        return sanitized;
    }
    format!("{sanitized}_id_{index}")
}

/// Copy `layer`, tagging every path as belonging to `object_index`.
fn tagged_copy(layer: &SliceLayer, object_index: usize) -> SliceLayer {
    let mut copy = layer.clone();
    copy.path_objects = vec![Some(object_index); copy.paths.len()];
    copy
}

/// Interleave per-object layer stacks into one plate-wide stack.
///
/// Layers are grouped into shared Z slots: two objects resting on the bed slice
/// onto an identical grid, while an object lifted off the bed keeps its own
/// (its bottom layers are its own bottom layers, not the plate's).  A slot
/// collects at most one layer per object and takes the mean Z of its members,
/// so the emitted Z sequence is strictly ascending.
fn merge_layers_by_z(per_object: &[Vec<SliceLayer>], layer_height: f64) -> Vec<SliceLayer> {
    // Quarter of a layer: comfortably absorbs the float noise of two baked
    // meshes resting on the same bed, while never welding two genuinely
    // different heights the nozzle would have to visit separately.
    let tolerance = (layer_height * 0.25).max(1e-6);
    let mut cursors = vec![0usize; per_object.len()];
    let mut out: Vec<SliceLayer> = Vec::new();

    loop {
        let mut slot_z = f64::INFINITY;
        for (object_index, layers) in per_object.iter().enumerate() {
            if let Some(layer) = layers.get(cursors[object_index]) {
                if layer.z < slot_z {
                    slot_z = layer.z;
                }
            }
        }
        if !slot_z.is_finite() {
            break;
        }

        let mut slot = SliceLayer::new(slot_z);
        let mut z_sum = 0.0;
        let mut z_count = 0usize;
        for (object_index, layers) in per_object.iter().enumerate() {
            let Some(layer) = layers.get(cursors[object_index]) else {
                continue;
            };
            if layer.z - slot_z > tolerance {
                continue;
            }
            z_sum += layer.z;
            z_count += 1;
            cursors[object_index] += 1;
            append_tagged(&mut slot, layer, object_index);
        }
        if z_count > 0 {
            slot.z = z_sum / z_count as f64;
        }

        // A graded plate keeps its overhang classes; an ungraded one keeps the
        // empty-vector sentinel that means "no dynamic overhang override".
        if slot.path_overhang.iter().all(|c| *c == OverhangClass::None) {
            slot.path_overhang.clear();
        }
        // Same sentinel for the per-path extrusion height: only combined sparse
        // infill sets one, and dropping it would print a stacked bead at the
        // layer height — a hard under-extrusion.
        if slot.path_heights.iter().all(|h| h.is_none()) {
            slot.path_heights.clear();
        }
        out.push(slot);
    }

    out
}

/// Append every path of `layer` to `slot`, tagged as `object_index`.
fn append_tagged(slot: &mut SliceLayer, layer: &SliceLayer, object_index: usize) {
    for (path_index, path) in layer.paths.iter().enumerate() {
        slot.paths.push(path.clone());
        slot.path_roles.push(layer.role_for_path(path_index));
        slot.path_widths.push(layer.width_for_path(path_index));
        slot.path_vertex_widths
            .push(layer.vertex_widths_for_path(path_index));
        slot.path_is_open.push(layer.is_path_open(path_index));
        slot.path_overhang.push(layer.overhang_for_path(path_index));
        slot.path_heights.push(layer.height_for_path(path_index));
        slot.path_objects.push(Some(object_index));
    }
    for region in layer.solid_regions.iter() {
        slot.solid_regions.push(region.clone());
    }
    for region in layer.unsupported_regions.iter() {
        slot.unsupported_regions.push(region.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logging::NullLogger;
    use crate::mesh::types::{Face, Vertex};

    /// Axis-aligned box mesh with its minimum corner at `(x, y, 0)`.
    fn box_mesh(x: f64, y: f64, size: f64, height: f64) -> Mesh {
        let v = |dx: f64, dy: f64, dz: f64| Vertex::new(x + dx, y + dy, dz);
        let c = [
            v(0.0, 0.0, 0.0),
            v(size, 0.0, 0.0),
            v(size, size, 0.0),
            v(0.0, size, 0.0),
            v(0.0, 0.0, height),
            v(size, 0.0, height),
            v(size, size, height),
            v(0.0, size, height),
        ];
        let quad = |a: usize, b: usize, cc: usize, d: usize| {
            [
                Face::new([c[a], c[b], c[cc]]),
                Face::new([c[a], c[cc], c[d]]),
            ]
        };
        let mut faces = Vec::new();
        faces.extend(quad(0, 3, 2, 1)); // bottom
        faces.extend(quad(4, 5, 6, 7)); // top
        faces.extend(quad(0, 1, 5, 4));
        faces.extend(quad(1, 2, 6, 5));
        faces.extend(quad(2, 3, 7, 6));
        faces.extend(quad(3, 0, 4, 7));
        Mesh {
            vertices: c.to_vec(),
            faces,
            aabb: None,
        }
    }

    fn two_object_params() -> SlicingParams {
        SlicingParams {
            layer_height: 0.5,
            wall_count: 1,
            infill_density: 0.0,
            top_layers: 0,
            bottom_layers: 0,
            ..Default::default()
        }
    }

    fn plate() -> Vec<ObjectInput> {
        vec![
            ObjectInput::new("cube a", box_mesh(0.0, 0.0, 10.0, 5.0)),
            ObjectInput::new("cube b", box_mesh(60.0, 60.0, 10.0, 3.0)),
        ]
    }

    #[test]
    fn merged_path_leaves_layers_untagged() {
        let params = two_object_params();
        assert!(!params.object_aware());
        let slice = slice_plate(&plate(), &params, &NullLogger);
        assert!(slice.objects.is_empty(), "no identities without a feature");
        assert!(slice
            .layers
            .iter()
            .all(|l| l.path_objects.is_empty() && l.object_for_path(0).is_none()));
    }

    #[test]
    fn merged_path_matches_a_plain_process_mesh() {
        let params = two_object_params();
        let objects = plate();
        let direct = process_mesh(&merge_meshes(&objects), &params, &NullLogger);
        let plate_slice = slice_plate(&objects, &params, &NullLogger);
        assert_eq!(direct.len(), plate_slice.layers.len());
        for (a, b) in direct.iter().zip(plate_slice.layers.iter()) {
            assert_eq!(a.paths.len(), b.paths.len());
            assert!((a.z - b.z).abs() < 1e-12);
        }
    }

    #[test]
    fn a_raft_still_suppresses_elephant_foot_when_slicing_object_by_object() {
        // Suppressing per-object adhesion clears `adhesion_type`, which is also
        // how elephant-foot compensation learns the object never meets the bed.
        // Without carrying that across, an object-aware raft print gets its base
        // shrunk for a squish that does not happen.
        let mut params = two_object_params();
        params.exclude_object = true;
        params.adhesion_type = AdhesionType::Raft;
        params.raft_layers = 3;
        params.elephant_foot_compensation_mm = 0.3;
        assert!(params.object_aware() && params.print_sequence != PrintSequence::ByObject);

        let on_raft = slice_plate(&plate(), &params, &NullLogger);

        let mut without = params.clone();
        without.elephant_foot_compensation_mm = 0.0;
        let baseline = slice_plate(&plate(), &without, &NullLogger);

        assert_eq!(on_raft.layers.len(), baseline.layers.len());
        for (i, (a, b)) in on_raft.layers.iter().zip(&baseline.layers).enumerate() {
            assert_eq!(
                a.paths, b.paths,
                "layer {i} was compensated despite printing on a raft"
            );
        }
    }

    #[test]
    fn exclude_object_tags_every_path_by_layer() {
        let mut params = two_object_params();
        params.exclude_object = true;
        let slice = slice_plate(&plate(), &params, &NullLogger);

        assert_eq!(slice.objects.len(), 2);
        assert_eq!(slice.objects[0].name, "cube_a");
        assert_eq!(slice.objects[1].name, "cube_b");

        // Both objects rest on the bed, so their grids coincide: the first
        // layers share one slot and carry both tags.
        let first = &slice.layers[0];
        let tags: std::collections::BTreeSet<_> = (0..first.paths.len())
            .filter_map(|i| first.object_for_path(i))
            .collect();
        assert_eq!(tags, [0, 1].into_iter().collect());

        // The taller object outlives the shorter one, so the top layers are
        // single-object.
        let last = slice.layers.last().unwrap();
        let tags: std::collections::BTreeSet<_> = (0..last.paths.len())
            .filter_map(|i| last.object_for_path(i))
            .collect();
        assert_eq!(tags, [0].into_iter().collect());
    }

    #[test]
    fn by_layer_z_is_ascending_and_slots_are_shared() {
        let mut params = two_object_params();
        params.exclude_object = true;
        let slice = slice_plate(&plate(), &params, &NullLogger);
        for pair in slice.layers.windows(2) {
            assert!(pair[1].z > pair[0].z, "layer Z must strictly ascend");
        }
        // 5 mm and 3 mm objects at 0.5 mm layers share the bottom 6 slots.
        assert_eq!(slice.layers.len(), 10);
    }

    #[test]
    fn by_object_emits_each_object_contiguously() {
        let mut params = two_object_params();
        params.print_sequence = PrintSequence::ByObject;
        let slice = slice_plate(&plate(), &params, &NullLogger);

        let sequence: Vec<usize> = slice
            .layers
            .iter()
            .map(|l| l.object_for_path(0).expect("every layer is owned"))
            .collect();
        // Front-most object (cube a at y=0) prints first, and each object's
        // layers form one unbroken run.
        let mut runs: Vec<usize> = Vec::new();
        for object in sequence {
            if runs.last() != Some(&object) {
                runs.push(object);
            }
        }
        assert_eq!(runs, vec![0, 1]);
        // Z restarts for the second object.
        let boundary = slice
            .layers
            .windows(2)
            .position(|p| p[1].z < p[0].z)
            .expect("Z restarts at the object boundary");
        assert_eq!(slice.layers[boundary + 1].z, slice.layers[0].z);
    }

    #[test]
    fn sequential_order_is_front_to_back() {
        let objects = vec![
            identity_for(0, "back".into(), &box_mesh(0.0, 80.0, 10.0, 5.0)),
            identity_for(1, "front".into(), &box_mesh(0.0, 0.0, 10.0, 5.0)),
            identity_for(2, "middle".into(), &box_mesh(0.0, 40.0, 10.0, 5.0)),
        ];
        assert_eq!(sequential_order(&objects), vec![1, 2, 0]);
    }

    #[test]
    fn sequential_warnings_flag_tall_and_close_objects() {
        let params = SlicingParams {
            extruder_clearance_height_mm: 10.0,
            extruder_clearance_radius_mm: 30.0,
            ..Default::default()
        };
        let objects = vec![
            identity_for(0, "tall".into(), &box_mesh(0.0, 0.0, 10.0, 40.0)),
            identity_for(1, "far".into(), &box_mesh(0.0, 100.0, 10.0, 5.0)),
        ];
        let warnings = sequential_warnings(&objects, &params);
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].contains("gantry clearance"));

        let close = vec![
            identity_for(0, "a".into(), &box_mesh(0.0, 0.0, 10.0, 5.0)),
            identity_for(1, "b".into(), &box_mesh(15.0, 0.0, 10.0, 5.0)),
        ];
        let warnings = sequential_warnings(&close, &params);
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].contains("clearance radius"));
    }

    #[test]
    fn the_last_object_printed_may_be_taller_than_the_gantry() {
        let params = SlicingParams {
            extruder_clearance_height_mm: 10.0,
            extruder_clearance_radius_mm: 0.0,
            ..Default::default()
        };
        // The tall one is at the back, so it prints last and nothing has to
        // reach over it.
        let objects = vec![
            identity_for(0, "short_front".into(), &box_mesh(0.0, 0.0, 10.0, 5.0)),
            identity_for(1, "tall_back".into(), &box_mesh(0.0, 100.0, 10.0, 60.0)),
        ];
        assert!(sequential_warnings(&objects, &params).is_empty());
    }

    #[test]
    fn duplicate_names_are_disambiguated_and_sanitised() {
        let used = vec!["part".to_string()];
        assert_eq!(unique_object_name("part", 1, &used), "part_id_1");
        assert_eq!(unique_object_name("my part.stl", 0, &[]), "my_part.stl");
        assert_eq!(unique_object_name("   ", 3, &[]), "object");
    }

    #[test]
    fn hull_of_a_box_is_its_four_corners() {
        let identity = identity_for(0, "box".into(), &box_mesh(5.0, 7.0, 10.0, 4.0));
        assert_eq!(identity.polygon.len(), 4);
        assert_eq!(identity.bbox, (5.0, 7.0, 15.0, 17.0));
        assert_eq!(identity.center, (10.0, 12.0));
        assert_eq!(identity.height_mm, 4.0);
    }

    #[test]
    fn long_hulls_are_resampled() {
        let ring: Vec<(f64, f64)> = (0..360)
            .map(|d| {
                let a = (d as f64).to_radians();
                (a.cos() * 20.0, a.sin() * 20.0)
            })
            .collect();
        let reduced = resample_ring(ring, MAX_EXCLUSION_POLYGON_POINTS);
        assert_eq!(reduced.len(), MAX_EXCLUSION_POLYGON_POINTS);
    }

    #[test]
    fn plate_adhesion_is_generated_once_and_owned_by_nobody() {
        let mut params = two_object_params();
        params.exclude_object = true;
        params.adhesion_type = AdhesionType::Skirt;
        params.skirt_loops = 1;
        params.skirt_distance = 2.0;
        params.skirt_height = 1;
        params.nozzle_diameter_mm = 0.4;

        let slice = slice_plate(&plate(), &params, &NullLogger);
        let first = &slice.layers[0];
        let skirts: Vec<usize> = (0..first.paths.len())
            .filter(|&i| first.role_for_path(i) == crate::core::ExtrusionRole::Skirt)
            .collect();
        // The two parts are far apart, so one plate-wide skirt pass still
        // yields one loop per island — what matters is that it ran **once**,
        // over the merged plate, rather than once inside each object's own
        // pipeline (which would also have drawn a loop where the parts are).
        assert_eq!(skirts.len(), 2);
        assert!(
            skirts.iter().all(|&i| first.object_for_path(i).is_none()),
            "adhesion must survive cancelling a single object"
        );
        // The object paths that follow keep their tags aligned after the
        // prepend.
        assert!((0..first.paths.len())
            .filter(|&i| first.role_for_path(i) != crate::core::ExtrusionRole::Skirt)
            .all(|i| first.object_for_path(i).is_some()));
    }
}
