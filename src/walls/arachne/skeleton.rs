//! Interior medial-axis skeleton extracted from the segment Voronoi diagram.
//!
//! This is **Phase 2a** of the Arachne generator.  Given the raw Voronoi
//! [`Diagram`](boostvoronoi::prelude::Diagram) of a shell polygon's boundary
//! segments (see [`super::voronoi`]), it keeps only the edges that form the
//! polygon's *interior medial axis* and annotates every surviving node with its
//! **distance to the boundary** — the local inradius, i.e. *half* the local
//! wall thickness.  That width field is what the bead-count function (Phase 2b)
//! consumes to decide how many beads cross each point and how wide they are.
//!
//! ## What is filtered out
//!
//! * **Secondary edges** — `boost::polygon::voronoi` emits an edge between a
//!   segment site and its own endpoint point-site; these are construction
//!   artifacts, not medial axis. Kept edges satisfy [`Edge::is_primary`].
//! * **Infinite edges** — rays to infinity (a missing endpoint vertex).
//! * **Exterior edges** — edges whose midpoint lies outside the polygon
//!   material (this also removes edges inside holes), via an even-odd
//!   point-in-polygon test that is winding-independent and therefore correct
//!   for the Clipper2 EvenOdd-normalised input.
//!
//! ## Known approximation
//!
//! Curved (parabolic) Voronoi edges — those between a convex vertex point-site
//! and a segment — are currently treated as straight chords between their
//! endpoints. Cura discretises these arcs; that refinement is deferred.

use std::collections::{HashMap, HashSet};

use boostvoronoi::prelude::Diagram;
use clipper2::Paths;

use super::voronoi::VORONOI_SCALE;

/// A node on the medial axis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SkeletonNode {
    /// X coordinate in millimetres.
    pub x: f64,
    /// Y coordinate in millimetres.
    pub y: f64,
    /// Distance to the nearest polygon boundary in mm (local inradius =
    /// half the local wall thickness at this point).
    pub radius: f64,
}

/// An undirected medial-axis edge between two [`SkeletonNode`]s (indices into
/// [`Skeleton::nodes`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SkeletonEdge {
    pub a: usize,
    pub b: usize,
}

/// The interior medial-axis graph of one shell polygon.
#[derive(Debug, Default)]
pub struct Skeleton {
    pub nodes: Vec<SkeletonNode>,
    pub edges: Vec<SkeletonEdge>,
}

impl Skeleton {
    /// Decompose the graph into polylines (node-index chains), splitting at
    /// every junction or endpoint (a node whose degree is not exactly 2).
    ///
    /// Degree-2 runs become one chain each; any remaining pure cycles (every
    /// node degree 2, e.g. the medial loop of an annulus) are emitted as a
    /// single closed chain whose first and last node coincide.
    pub fn chains(&self) -> Vec<Vec<usize>> {
        let n = self.nodes.len();
        let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
        for (ei, e) in self.edges.iter().enumerate() {
            adj[e.a].push(ei);
            adj[e.b].push(ei);
        }
        let other = |ei: usize, from: usize| -> usize {
            let e = self.edges[ei];
            if e.a == from {
                e.b
            } else {
                e.a
            }
        };

        let mut used = vec![false; self.edges.len()];
        let mut chains: Vec<Vec<usize>> = Vec::new();

        // 1. Open chains anchored at junctions / endpoints (degree != 2).
        for start in 0..n {
            if adj[start].len() == 2 {
                continue;
            }
            for idx in 0..adj[start].len() {
                let ei0 = adj[start][idx];
                if used[ei0] {
                    continue;
                }
                used[ei0] = true;
                let mut chain = vec![start];
                let mut cur = other(ei0, start);
                chain.push(cur);
                while adj[cur].len() == 2 {
                    let Some(&nei) = adj[cur].iter().find(|&&e| !used[e]) else {
                        break;
                    };
                    used[nei] = true;
                    cur = other(nei, cur);
                    chain.push(cur);
                }
                chains.push(chain);
            }
        }

        // 2. Remaining pure cycles (all nodes degree 2).
        for ei0 in 0..self.edges.len() {
            if used[ei0] {
                continue;
            }
            let start = self.edges[ei0].a;
            used[ei0] = true;
            let mut chain = vec![start];
            let mut cur = other(ei0, start);
            chain.push(cur);
            while cur != start {
                let Some(&nei) = adj[cur].iter().find(|&&e| !used[e]) else {
                    break;
                };
                used[nei] = true;
                cur = other(nei, cur);
                chain.push(cur);
            }
            chains.push(chain);
        }

        chains
    }
}

/// Build the interior medial-axis skeleton from a polygon set and its Voronoi
/// diagram.
///
/// `paths` must be the same EvenOdd-normalised `Paths` that produced
/// `diagram` (see [`super::voronoi::build_segment_voronoi`]); `offset` is the
/// translation that builder returned.  Only nodes referenced by a surviving
/// interior edge are emitted, with indices remapped to a compact
/// `0..nodes.len()` range.
pub fn build_skeleton(paths: &Paths, diagram: &Diagram, offset: [f64; 2]) -> Skeleton {
    let verts = diagram.vertices();

    // 1. Collect interior, primary, finite edges as raw diagram-vertex pairs.
    let mut raw_edges: Vec<(usize, usize)> = Vec::new();
    let mut seen: HashSet<(usize, usize)> = HashSet::new();
    for edge in diagram.edges() {
        if !edge.is_primary() {
            continue;
        }
        let Some(v0) = edge.vertex0() else {
            continue; // infinite edge (no start vertex)
        };
        let v1 = match diagram.edge_get_vertex1(edge.id()) {
            Ok(Some(v)) => v,
            _ => continue, // infinite edge (no end vertex)
        };
        let (i0, i1) = (v0.usize(), v1.usize());
        // Each undirected edge appears twice (edge + twin); keep one.
        let key = (i0.min(i1), i0.max(i1));
        if !seen.insert(key) {
            continue;
        }
        let mx = (verts[i0].x() + verts[i1].x()) / 2.0 / VORONOI_SCALE + offset[0];
        let my = (verts[i0].y() + verts[i1].y()) / 2.0 / VORONOI_SCALE + offset[1];
        if !point_inside(paths, mx, my) {
            continue; // exterior edge or inside a hole
        }
        raw_edges.push((i0, i1));
    }

    // 2. Compact the referenced diagram vertices into skeleton nodes.
    let mut remap: HashMap<usize, usize> = HashMap::new();
    let mut nodes: Vec<SkeletonNode> = Vec::new();
    let mut edges: Vec<SkeletonEdge> = Vec::new();

    for (i0, i1) in raw_edges {
        let a = intern_node(i0, verts, paths, offset, &mut nodes, &mut remap);
        let b = intern_node(i1, verts, paths, offset, &mut nodes, &mut remap);
        edges.push(SkeletonEdge { a, b });
    }

    Skeleton { nodes, edges }
}

/// Intern diagram vertex `i` as a compact skeleton node, computing its radius
/// once.  `offset` converts the translated Voronoi frame back to world mm.
fn intern_node(
    i: usize,
    verts: &[boostvoronoi::prelude::Vertex],
    paths: &Paths,
    offset: [f64; 2],
    nodes: &mut Vec<SkeletonNode>,
    remap: &mut HashMap<usize, usize>,
) -> usize {
    *remap.entry(i).or_insert_with(|| {
        let x = verts[i].x() / VORONOI_SCALE + offset[0];
        let y = verts[i].y() / VORONOI_SCALE + offset[1];
        nodes.push(SkeletonNode {
            x,
            y,
            radius: dist_to_boundary(paths, x, y),
        });
        nodes.len() - 1
    })
}

/// Even-odd point-in-polygon test over every contour in `paths`.
///
/// Winding-independent, so it is correct for Clipper2 EvenOdd output: a point
/// inside the outer ring but also inside a hole is toggled twice and reported
/// as outside the material.
fn point_inside(paths: &Paths, x: f64, y: f64) -> bool {
    let mut inside = false;
    for path in paths.iter() {
        let pts: Vec<(f64, f64)> = path.iter().map(|p| (p.x(), p.y())).collect();
        let n = pts.len();
        if n < 3 {
            continue;
        }
        let mut j = n - 1;
        for i in 0..n {
            let (xi, yi) = pts[i];
            let (xj, yj) = pts[j];
            if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
                inside = !inside;
            }
            j = i;
        }
    }
    inside
}

/// Minimum distance from `(x, y)` to any boundary edge in `paths` (mm).
fn dist_to_boundary(paths: &Paths, x: f64, y: f64) -> f64 {
    let mut best = f64::INFINITY;
    for path in paths.iter() {
        let pts: Vec<(f64, f64)> = path.iter().map(|p| (p.x(), p.y())).collect();
        let n = pts.len();
        if n < 2 {
            continue;
        }
        for i in 0..n {
            let (ax, ay) = pts[i];
            let (bx, by) = pts[(i + 1) % n];
            let d = dist_point_segment(x, y, ax, ay, bx, by);
            if d < best {
                best = d;
            }
        }
    }
    best
}

/// Distance from point `p` to line segment `a`–`b`.
fn dist_point_segment(px: f64, py: f64, ax: f64, ay: f64, bx: f64, by: f64) -> f64 {
    let dx = bx - ax;
    let dy = by - ay;
    let len_sq = dx * dx + dy * dy;
    let t = if len_sq <= f64::EPSILON {
        0.0
    } else {
        (((px - ax) * dx + (py - ay) * dy) / len_sq).clamp(0.0, 1.0)
    };
    let cx = ax + t * dx;
    let cy = ay + t * dy;
    ((px - cx).powi(2) + (py - cy).powi(2)).sqrt()
}

#[cfg(test)]
mod tests {
    use super::super::voronoi::build_segment_voronoi;
    use super::*;
    use clipper2::{Path, Paths};

    fn square(side: f64) -> Paths {
        let h = side / 2.0;
        let sq: Path = vec![(-h, -h), (h, -h), (h, h), (-h, h)].into();
        Paths::new(vec![sq])
    }

    #[test]
    fn square_skeleton_has_centre_with_correct_radius() {
        let paths = square(20.0);
        let (diagram, off) = build_segment_voronoi(&paths).unwrap();
        let skel = build_skeleton(&paths, &diagram, off);

        assert!(!skel.edges.is_empty(), "skeleton should have edges");
        // The centre of a 20 mm square is 10 mm from every edge.
        let centre = skel
            .nodes
            .iter()
            .find(|n| n.x.abs() < 0.5 && n.y.abs() < 0.5);
        let centre = centre.expect("expected a node at the square centre");
        assert!(
            (centre.radius - 10.0).abs() < 0.3,
            "centre radius should be ~10 mm, got {}",
            centre.radius
        );
    }

    #[test]
    fn all_skeleton_nodes_are_interior() {
        let paths = square(20.0);
        let (diagram, off) = build_segment_voronoi(&paths).unwrap();
        let skel = build_skeleton(&paths, &diagram, off);
        for n in &skel.nodes {
            assert!(
                n.x.abs() <= 10.01 && n.y.abs() <= 10.01,
                "node ({}, {}) should be inside the square",
                n.x,
                n.y
            );
            assert!(n.radius >= 0.0, "radius must be non-negative");
        }
    }

    #[test]
    fn annulus_skeleton_runs_through_the_wall_band() {
        // 40 mm outer square, 20 mm square hole → 10 mm-thick wall band whose
        // medial axis sits 5 mm from both boundaries.
        let outer: Path = vec![(-20.0, -20.0), (20.0, -20.0), (20.0, 20.0), (-20.0, 20.0)].into();
        let hole: Path = vec![(-10.0, -10.0), (-10.0, 10.0), (10.0, 10.0), (10.0, -10.0)].into();
        let paths = Paths::new(vec![outer, hole]);

        let (diagram, off) = build_segment_voronoi(&paths).unwrap();
        let skel = build_skeleton(&paths, &diagram, off);

        assert!(!skel.edges.is_empty(), "annulus skeleton should have edges");
        // A band medial node has radius ~5 mm and never sits inside the hole.
        let band = skel
            .nodes
            .iter()
            .find(|n| (n.radius - 5.0).abs() < 0.6)
            .expect("expected a wall-band medial node with radius ~5 mm");
        assert!(
            band.x.abs() > 9.5 || band.y.abs() > 9.5,
            "band node ({}, {}) must lie outside the hole",
            band.x,
            band.y
        );
    }
}
