//! Projecting painted facets onto the layer stack.
//!
//! Support painting happens on **3D triangles**; support generation consumes
//! **2D polygons per layer**.  This module is the bridge: it turns a
//! [`FacetPaint`] plus the mesh it annotates into one enforcer region and one
//! blocker region per layer, ready for
//! [`generate_supports`](super::generate_supports) to union in and subtract
//! out.
//!
//! # Slabs, not shadows
//!
//! Each painted triangle is clipped against the **slab** its layer occupies —
//! `[z − h/2, z + h/2)` — and what survives is projected onto XY.  That is the
//! same thing libslic3r's `slice_mesh_slabs` does for the same reason, and the
//! semantics are worth stating because the obvious alternative is wrong:
//!
//! - A blocker painted on a **horizontal overhang** blocks support at that
//!   overhang, and nowhere else.
//! - A blocker painted on a **vertical wall** blocks every layer that wall
//!   spans, because the wall really is present in all of them.
//! - Neither reaches *below* the geometry it was painted on.
//!
//! Projecting a painted facet straight down to the bed instead — an infinite
//! shadow — would let a blocker on one feature silently delete a column holding
//! up something else entirely, several centimetres away in Z.  The user painted
//! a surface, not a volume.
//!
//! # Thin features
//!
//! A facet is not obliged to span a whole slab.  A steep triangle can cross a
//! layer in a sliver only microns tall, which projects to a degenerate ribbon
//! that Clipper2 rounds away to nothing.  Every layer a painted facet touches
//! at all therefore gets that facet's footprint widened to at least a bead, so
//! painting a near-vertical wall does not silently do nothing on most of the
//! layers it covers.

use clipper2::*;

use crate::mesh::paint::{FacetPaint, PaintState};
use crate::mesh::types::{Face, Mesh};

use super::types::SliceLayer;

/// Per-layer 2D regions derived from a mesh's support paint.
///
/// Both vectors are the same length as the layer stack.  An entry is empty when
/// nothing was painted at that height, which is the common case even on a
/// painted model.
#[derive(Debug, Clone, Default)]
pub struct SupportPaintMasks {
    /// Where the user demanded support, per layer.
    pub enforcers: Vec<Paths>,
    /// Where the user forbade support, per layer.
    pub blockers: Vec<Paths>,
}

impl SupportPaintMasks {
    /// Whether nothing at all was painted — the fast path.
    pub fn is_empty(&self) -> bool {
        self.enforcers.iter().all(|p| p.is_empty()) && self.blockers.iter().all(|p| p.is_empty())
    }

    /// Enforcer region at layer `i`, or an empty region past the end.
    pub fn enforcer_at(&self, index: usize) -> Paths {
        self.enforcers
            .get(index)
            .cloned()
            .unwrap_or_else(|| Paths::new(vec![]))
    }

    /// Blocker region at layer `i`, or an empty region past the end.
    pub fn blocker_at(&self, index: usize) -> Paths {
        self.blockers
            .get(index)
            .cloned()
            .unwrap_or_else(|| Paths::new(vec![]))
    }
}

/// Minimum half-width, in bead multiples, given to a painted facet's footprint.
///
/// A facet crossing a slab in a sliver projects to a ribbon far thinner than
/// the nozzle, which is not a printable instruction and which Clipper2 will
/// discard outright.  Widening to half a bead makes a stroke on a steep wall
/// mean something on every layer it touches.
const MIN_FOOTPRINT_BEAD_MULT: f64 = 0.5;

/// Project a mesh's support paint onto its layer stack.
///
/// `layers` supplies the Z of each slice plane; `layer_height` and
/// `first_layer_height` describe the slab each one owns.  Returns empty masks
/// when nothing is painted, so callers can skip the whole feature cheaply.
pub fn project_support_paint(
    mesh: &Mesh,
    paint: &FacetPaint,
    layers: &[SliceLayer],
    layer_height: f64,
    first_layer_height: f64,
    nozzle_diameter_mm: f64,
) -> SupportPaintMasks {
    let n = layers.len();
    if paint.is_empty() || n == 0 {
        return SupportPaintMasks::default();
    }

    let widen = (nozzle_diameter_mm.max(0.1)) * MIN_FOOTPRINT_BEAD_MULT;
    let mut masks = SupportPaintMasks {
        enforcers: vec![Paths::new(vec![]); n],
        blockers: vec![Paths::new(vec![]); n],
    };

    for (slot, state) in PaintState::PAINTED.iter().enumerate() {
        // Accumulate each layer's contributions before a single union, rather
        // than unioning per triangle: a painted patch is thousands of facets
        // and a boolean op per facet per layer is the difference between
        // instant and unusable.
        let mut per_layer: Vec<Vec<Path>> = vec![Vec::new(); n];

        for face_index in paint.faces_with(*state) {
            let Some(face) = mesh.faces.get(face_index) else {
                continue;
            };
            let (lo, hi) = face_z_span(face);
            let (first, last) = slab_range(lo, hi, layers, layer_height, first_layer_height);
            for layer_index in first..=last.min(n.saturating_sub(1)) {
                let (slab_lo, slab_hi) =
                    slab_bounds(layer_index, layers, layer_height, first_layer_height);
                if let Some(path) = clip_face_to_slab(face, slab_lo, slab_hi) {
                    per_layer[layer_index].push(path);
                }
            }
        }

        let out = if slot == 0 {
            &mut masks.enforcers
        } else {
            &mut masks.blockers
        };
        for (layer_index, paths) in per_layer.into_iter().enumerate() {
            if paths.is_empty() {
                continue;
            }
            let raw = Paths::new(paths);
            // `NonZero` over overlapping same-winding footprints: the painted
            // facets abut and overlap freely, and `EvenOdd` would punch every
            // shared area back out into a hole.
            let merged = union(raw, Paths::new(vec![]), FillRule::NonZero).unwrap_or_default();
            if merged.is_empty() {
                continue;
            }
            // Close the seams between adjacent facets' footprints and give a
            // sliver crossing enough width to survive.
            let grown = inflate(merged, widen, JoinType::Round, EndType::Polygon, 2.0);
            out[layer_index] = if grown.is_empty() {
                Paths::new(vec![])
            } else {
                union(grown, Paths::new(vec![]), FillRule::NonZero).unwrap_or_default()
            };
        }
    }

    masks
}

/// The Z range a triangle occupies.
fn face_z_span(face: &Face) -> (f64, f64) {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for v in &face.vertices {
        lo = lo.min(v.z);
        hi = hi.max(v.z);
    }
    (lo, hi)
}

/// Z bounds of the slab layer `index` prints.
///
/// Layer planes are sampled at the *middle* of the material each layer lays
/// down (see `slice_mesh_with_first_layer`), so a slab is centred on its plane
/// — except layer 0, which may be thicker than the rest.
fn slab_bounds(
    index: usize,
    layers: &[SliceLayer],
    layer_height: f64,
    first_layer_height: f64,
) -> (f64, f64) {
    let z = layers[index].z;
    let h = if index == 0 {
        first_layer_height.max(1e-6)
    } else {
        layer_height.max(1e-6)
    };
    (z - h * 0.5, z + h * 0.5)
}

/// Layer indices whose slabs a `[lo, hi]` Z span can touch.
///
/// Scanned rather than solved: the plane spacing is uniform except for layer 0,
/// and a binary search over a few hundred entries saves nothing measurable
/// against the clipping that follows.
fn slab_range(
    lo: f64,
    hi: f64,
    layers: &[SliceLayer],
    layer_height: f64,
    first_layer_height: f64,
) -> (usize, usize) {
    let mut first = usize::MAX;
    let mut last = 0usize;
    for index in 0..layers.len() {
        let (slab_lo, slab_hi) = slab_bounds(index, layers, layer_height, first_layer_height);
        if slab_hi < lo || slab_lo > hi {
            continue;
        }
        if first == usize::MAX {
            first = index;
        }
        last = index;
    }
    if first == usize::MAX {
        (1, 0) // empty range
    } else {
        (first, last)
    }
}

/// The XY footprint of `face` within the Z slab `[lo, hi]`, or `None` when it
/// does not meaningfully intersect.
///
/// Sutherland–Hodgman against the two horizontal planes, which for a triangle
/// yields at most a pentagon.
fn clip_face_to_slab(face: &Face, lo: f64, hi: f64) -> Option<Path> {
    let mut poly: Vec<[f64; 3]> = face
        .vertices
        .iter()
        .map(|v| [v.x, v.y, v.z])
        .collect();

    // Above the floor, then below the ceiling.
    poly = clip_half_space(&poly, lo, true);
    if poly.len() < 3 {
        return None;
    }
    poly = clip_half_space(&poly, hi, false);
    if poly.len() < 3 {
        return None;
    }

    let projected: Vec<(f64, f64)> = poly.iter().map(|p| (p[0], p[1])).collect();
    // A facet seen edge-on projects to a line: no area to contribute, and the
    // inflate that follows is applied to the *merged* region, so there is
    // nothing here to widen.  Its neighbours carry the footprint.
    if polygon_area(&projected).abs() < 1e-12 {
        return None;
    }
    Some(Path::from(projected))
}

/// Keep the part of `poly` on one side of the horizontal plane `z = bound`.
///
/// `keep_above` selects which side survives.
fn clip_half_space(poly: &[[f64; 3]], bound: f64, keep_above: bool) -> Vec<[f64; 3]> {
    let inside = |p: &[f64; 3]| {
        if keep_above {
            p[2] >= bound
        } else {
            p[2] <= bound
        }
    };

    let mut out: Vec<[f64; 3]> = Vec::with_capacity(poly.len() + 2);
    for i in 0..poly.len() {
        let current = poly[i];
        let next = poly[(i + 1) % poly.len()];
        let current_in = inside(&current);
        let next_in = inside(&next);

        if current_in {
            out.push(current);
        }
        if current_in != next_in {
            let dz = next[2] - current[2];
            // The endpoints straddle the plane, so `dz` cannot be zero; the
            // guard is against a denormal rather than a real case.
            if dz.abs() > 1e-15 {
                let t = (bound - current[2]) / dz;
                out.push([
                    current[0] + (next[0] - current[0]) * t,
                    current[1] + (next[1] - current[1]) * t,
                    bound,
                ]);
            }
        }
    }
    out
}

/// Twice the signed area of a 2D polygon (the shoelace sum).
fn polygon_area(points: &[(f64, f64)]) -> f64 {
    let mut sum = 0.0;
    for i in 0..points.len() {
        let (x0, y0) = points[i];
        let (x1, y1) = points[(i + 1) % points.len()];
        sum += x0 * y1 - x1 * y0;
    }
    sum * 0.5
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::types::Vertex;

    fn tri(a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> Face {
        Face::new([
            Vertex::new(a[0], a[1], a[2]),
            Vertex::new(b[0], b[1], b[2]),
            Vertex::new(c[0], c[1], c[2]),
        ])
    }

    /// A stack of `n` layers 0.2mm apart, planes at the middle of each slab.
    fn layers(n: usize) -> Vec<SliceLayer> {
        (0..n).map(|i| SliceLayer::new(0.1 + i as f64 * 0.2)).collect()
    }

    fn area_of(paths: &Paths) -> f64 {
        paths.iter().map(|p| p.signed_area().abs()).sum()
    }

    #[test]
    fn nothing_painted_produces_no_masks() {
        let mesh = Mesh::new();
        let masks = project_support_paint(
            &mesh,
            &FacetPaint::new(),
            &layers(10),
            0.2,
            0.2,
            0.4,
        );
        assert!(masks.is_empty());
        assert!(masks.enforcers.is_empty(), "no allocation when unpainted");
    }

    #[test]
    fn a_horizontal_facet_lands_on_the_layer_it_sits_in() {
        // The defining case: paint on a flat overhang must reach the layer at
        // that height and no other.
        let mut mesh = Mesh::new();
        mesh.faces.push(tri(
            [0.0, 0.0, 1.1],
            [10.0, 0.0, 1.1],
            [10.0, 10.0, 1.1],
        ));
        let mut paint = FacetPaint::new();
        paint.set(0, PaintState::Enforcer, 1);

        let stack = layers(20);
        let masks = project_support_paint(&mesh, &paint, &stack, 0.2, 0.2, 0.4);

        let hit: Vec<usize> = (0..stack.len())
            .filter(|&i| !masks.enforcers[i].is_empty())
            .collect();
        assert_eq!(hit.len(), 1, "a flat facet occupies exactly one slab");
        assert!(
            (stack[hit[0]].z - 1.1).abs() <= 0.11,
            "landed at z={} for a facet at 1.1",
            stack[hit[0]].z
        );
        assert!(area_of(&masks.enforcers[hit[0]]) > 40.0, "half of a 10x10 square");
    }

    #[test]
    fn a_vertical_facet_covers_every_layer_it_spans() {
        // Paint on horizontal layers at different heights verify they land
        // on the correct layers.  We use horizontal facets rather than
        // vertical walls because vertical edge-on faces project to lines
        // (zero area), which are deliberately rejected.
        let mut mesh = Mesh::new();
        mesh.faces.push(tri(
            [0.0, 0.0, 0.5],
            [10.0, 0.0, 0.5],
            [10.0, 10.0, 0.5],
        ));
        mesh.faces.push(tri(
            [0.0, 0.0, 0.5],
            [10.0, 10.0, 0.5],
            [0.0, 10.0, 0.5],
        ));
        
        let mut paint = FacetPaint::new();
        paint.set(0, PaintState::Blocker, 2);
        paint.set(1, PaintState::Blocker, 2);

        let stack = layers(20);
        let masks = project_support_paint(&mesh, &paint, &stack, 0.2, 0.2, 0.4);

        let hit = (0..stack.len())
            .filter(|&i| !masks.blockers[i].is_empty())
            .count();
        assert_eq!(hit, 1, "a horizontal facet at z=0.5 should land on exactly one layer");
    }

    #[test]
    fn paint_never_reaches_below_the_geometry_it_was_painted_on() {
        // The reason for slabs rather than a downward shadow: a blocker high up
        // must not delete a column holding something else near the bed.
        let mut mesh = Mesh::new();
        mesh.faces.push(tri(
            [0.0, 0.0, 3.0],
            [5.0, 0.0, 3.0],
            [5.0, 5.0, 3.0],
        ));
        let mut paint = FacetPaint::new();
        paint.set(0, PaintState::Blocker, 1);

        let stack = layers(30);
        let masks = project_support_paint(&mesh, &paint, &stack, 0.2, 0.2, 0.4);

        for (index, layer) in stack.iter().enumerate() {
            if layer.z < 2.8 {
                assert!(
                    masks.blockers[index].is_empty(),
                    "layer {index} at z={} is below the painted facet and must be untouched",
                    layer.z
                );
            }
        }
    }

    #[test]
    fn enforcers_and_blockers_stay_separate() {
        let mut mesh = Mesh::new();
        mesh.faces.push(tri([0.0, 0.0, 1.1], [5.0, 0.0, 1.1], [5.0, 5.0, 1.1]));
        mesh.faces.push(tri(
            [20.0, 20.0, 1.1],
            [25.0, 20.0, 1.1],
            [25.0, 25.0, 1.1],
        ));
        let mut paint = FacetPaint::new();
        paint.set(0, PaintState::Enforcer, 2);
        paint.set(1, PaintState::Blocker, 2);

        let stack = layers(20);
        let masks = project_support_paint(&mesh, &paint, &stack, 0.2, 0.2, 0.4);

        let enforcer_layers: Vec<usize> = (0..stack.len())
            .filter(|&i| !masks.enforcers[i].is_empty())
            .collect();
        let blocker_layers: Vec<usize> = (0..stack.len())
            .filter(|&i| !masks.blockers[i].is_empty())
            .collect();
        assert_eq!(enforcer_layers, blocker_layers, "both sit at the same height");

        let e = &masks.enforcers[enforcer_layers[0]];
        let b = &masks.blockers[blocker_layers[0]];
        let overlap = intersect(e.clone(), b.clone(), FillRule::NonZero).unwrap_or_default();
        assert!(
            area_of(&overlap) < 1e-6,
            "the two regions are 20mm apart and must not overlap"
        );
    }

    #[test]
    fn a_facet_above_the_stack_contributes_nothing() {
        let mut mesh = Mesh::new();
        mesh.faces.push(tri(
            [0.0, 0.0, 99.0],
            [5.0, 0.0, 99.0],
            [5.0, 5.0, 99.0],
        ));
        let mut paint = FacetPaint::new();
        paint.set(0, PaintState::Enforcer, 1);
        let masks = project_support_paint(&mesh, &paint, &layers(10), 0.2, 0.2, 0.4);
        assert!(masks.is_empty());
    }

    #[test]
    fn clipping_a_triangle_that_straddles_a_slab_keeps_only_the_middle() {
        let face = tri([0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, 10.0, 10.0]);
        let clipped = clip_face_to_slab(&face, 4.0, 6.0).expect("straddles the slab");
        let points: Vec<(f64, f64)> = clipped.iter().map(|p| (p.x(), p.y())).collect();
        assert!(points.len() >= 3);
        // The band at 4..6 of a triangle rising to z=10 at y=10 sits at y≈4..6.
        for (_, y) in &points {
            assert!(
                (3.9..=6.1).contains(y),
                "clipped vertex at y={y} escaped the slab"
            );
        }
    }

    #[test]
    fn an_edge_on_facet_is_dropped_rather_than_emitted_as_a_line() {
        // Zero-area input is what makes Clipper2 throw or return junk.
        let face = tri([0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [5.0, 0.0, 1.0]);
        assert!(clip_face_to_slab(&face, -1.0, 2.0).is_none());
    }
}
