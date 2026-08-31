//! Travel-move routing that avoids crossing perimeter walls
//! (`avoid_crossing_perimeters` / OrcaSlicer `reduce_crossing_wall`).
//!
//! When enabled, the G-code generator asks a per-layer [`TravelPlanner`] to turn
//! a straight nozzle hop from A → B into a poly-line that never *crosses* an
//! outer-wall loop.  This keeps the oozing nozzle off the finished, visible
//! surface: instead of dragging a scar straight across the top of a part, the
//! move detours around the inside of the island's walls (or around the outside
//! of the island when it started there).
//!
//! ## Model
//!
//! The obstacles are the layer's **closed outer-wall loops** (an island's outer
//! contour plus any hole boundaries).  A travel segment is *blocked* only when
//! it **properly crosses** one of those loop edges — i.e. it passes from one
//! side of a wall to the other.  A segment that stays entirely inside a single
//! island (hidden interior travel) or entirely outside every island is fine and
//! is emitted unchanged.
//!
//! When a direct hop is blocked, the planner runs a **visibility-graph shortest
//! path**: nodes are the two endpoints plus every obstacle vertex, an edge joins
//! two nodes whose connecting segment crosses no wall, and Dijkstra returns the
//! shortest wall-free poly-line.  Touching a wall at a shared vertex is allowed,
//! so the route can hug convex corners.
//!
//! ## Bounds & fallbacks
//!
//! Visibility graphs are `O(V²)` to build.  To keep an enabled slice tractable
//! the planner:
//!   * only runs the graph search when the **direct** hop is actually blocked
//!     (the overwhelmingly common case is a short, clear hop that returns
//!     immediately);
//!   * simplifies obstacle loops and **caps** the working vertex count
//!     ([`MAX_OBSTACLE_VERTS`]); above the cap, or when no wall-free route
//!     exists (a fully enclosed pocket), it falls back to the original straight
//!     hop.  The retract/​z-hop the generator already performs still protects the
//!     surface in that fallback case.
//!
//! Only outer-wall loops are treated as obstacles: crossing an *inner* wall
//! during travel is hidden and not worth the extra routing cost.  This mirrors
//! the visible-surface intent of the feature.

use crate::core::{ExtrusionRole, SliceLayer};

/// Points closer than this (mm) are treated as coincident.
const EPS: f64 = 1e-6;

/// Simplification tolerance (mm) applied to obstacle loops before routing.
const SIMPLIFY_TOL_MM: f64 = 0.2;

/// Maximum obstacle vertices the planner will route around on one layer.
/// Above this the planner disables itself and travels stay straight.
const MAX_OBSTACLE_VERTS: usize = 400;

type Pt = (f64, f64);

/// Per-layer travel router.  Build once per layer with [`TravelPlanner::for_layer`].
pub struct TravelPlanner {
    /// Closed obstacle loops (outer-wall contours and hole boundaries).
    loops: Vec<Vec<Pt>>,
    /// Flattened obstacle vertices, used as visibility-graph waypoints.
    verts: Vec<Pt>,
    /// Axis-aligned bounds of all obstacles `(min_x, min_y, max_x, max_y)`.
    bounds: (f64, f64, f64, f64),
}

impl TravelPlanner {
    /// Build a planner from a layer's closed outer-wall loops, or return `None`
    /// when there is nothing worth routing around (no walls, or too many
    /// vertices to plan within budget).
    pub fn for_layer(layer: &SliceLayer) -> Option<Self> {
        let mut loops: Vec<Vec<Pt>> = Vec::new();
        for (i, path) in layer.paths.iter().enumerate() {
            if layer.role_for_path(i) != ExtrusionRole::OuterWall || layer.is_path_open(i) {
                continue;
            }
            let raw: Vec<Pt> = path.iter().map(|p| (p.x(), p.y())).collect();
            let simplified = simplify_closed(&raw, SIMPLIFY_TOL_MM);
            if simplified.len() >= 3 {
                loops.push(simplified);
            }
        }
        if loops.is_empty() {
            return None;
        }
        let total: usize = loops.iter().map(|l| l.len()).sum();
        if total > MAX_OBSTACLE_VERTS {
            return None;
        }

        let mut verts = Vec::with_capacity(total);
        let (mut min_x, mut min_y) = (f64::INFINITY, f64::INFINITY);
        let (mut max_x, mut max_y) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
        for l in &loops {
            for &(x, y) in l {
                verts.push((x, y));
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
        Some(Self {
            loops,
            verts,
            bounds: (min_x, min_y, max_x, max_y),
        })
    }

    /// Route a travel from `from` to `to`, returning the ordered waypoints to
    /// move through (the destination is always the final element, `from` is
    /// never included).
    ///
    /// Returns `[to]` (a straight hop) when the direct segment crosses no wall,
    /// or when no wall-free detour can be found within budget.
    pub fn route(&self, from: Pt, to: Pt) -> Vec<Pt> {
        // Fast path: destination essentially coincident.
        if dist(from, to) < EPS {
            return vec![to];
        }
        // Cheap reject: a hop whose bounding box misses every obstacle can't
        // cross one.
        if !self.segment_bbox_touches_obstacles(from, to) || !self.crosses_any_wall(from, to) {
            return vec![to];
        }
        self.route_around(from, to).unwrap_or_else(|| vec![to])
    }

    /// Visibility-graph shortest path from `from` to `to` avoiding wall crossings.
    fn route_around(&self, from: Pt, to: Pt) -> Option<Vec<Pt>> {
        let n = self.verts.len();
        // Node indices: 0..n obstacle verts, n = from, n+1 = to.
        let start = n;
        let goal = n + 1;
        let node = |i: usize| -> Pt {
            if i == start {
                from
            } else if i == goal {
                to
            } else {
                self.verts[i]
            }
        };

        // Dijkstra with on-the-fly neighbour visibility (avoids an O(V²) matrix).
        let mut dist_to = vec![f64::INFINITY; n + 2];
        let mut prev = vec![usize::MAX; n + 2];
        let mut visited = vec![false; n + 2];
        dist_to[start] = 0.0;

        for _ in 0..(n + 2) {
            // Pick the closest unvisited node.
            let mut u = usize::MAX;
            let mut best = f64::INFINITY;
            for (i, &d) in dist_to.iter().enumerate() {
                if !visited[i] && d < best {
                    best = d;
                    u = i;
                }
            }
            if u == usize::MAX {
                break;
            }
            if u == goal {
                break;
            }
            visited[u] = true;
            let up = node(u);

            // Candidate neighbours: all obstacle verts plus the goal.
            for v in 0..(n + 2) {
                if v == start || visited[v] || v == u {
                    continue;
                }
                let vp = node(v);
                let step = dist(up, vp);
                if step <= EPS || dist_to[u] + step >= dist_to[v] {
                    continue;
                }
                if self.crosses_any_wall(up, vp) {
                    continue;
                }
                dist_to[v] = dist_to[u] + step;
                prev[v] = u;
            }
        }

        if !dist_to[goal].is_finite() {
            return None;
        }
        // Reconstruct (goal → start), then reverse and drop the start node.
        let mut chain = Vec::new();
        let mut cur = goal;
        while cur != start {
            chain.push(node(cur));
            let p = prev[cur];
            if p == usize::MAX {
                return None;
            }
            cur = p;
        }
        chain.reverse();
        Some(chain)
    }

    /// True when segment `a`–`b` properly crosses any obstacle loop edge.
    fn crosses_any_wall(&self, a: Pt, b: Pt) -> bool {
        for l in &self.loops {
            let m = l.len();
            for i in 0..m {
                let c = l[i];
                let d = l[(i + 1) % m];
                if segments_properly_cross(a, b, c, d) {
                    return true;
                }
            }
        }
        false
    }

    fn segment_bbox_touches_obstacles(&self, a: Pt, b: Pt) -> bool {
        let (min_x, min_y, max_x, max_y) = self.bounds;
        let seg_min_x = a.0.min(b.0);
        let seg_max_x = a.0.max(b.0);
        let seg_min_y = a.1.min(b.1);
        let seg_max_y = a.1.max(b.1);
        seg_max_x >= min_x && seg_min_x <= max_x && seg_max_y >= min_y && seg_min_y <= max_y
    }
}

fn dist(a: Pt, b: Pt) -> f64 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

/// Orientation sign of the ordered triple (p, q, r): >0 CCW, <0 CW, 0 colinear.
fn cross(p: Pt, q: Pt, r: Pt) -> f64 {
    (q.0 - p.0) * (r.1 - p.1) - (q.1 - p.1) * (r.0 - p.0)
}

/// True when open segments `p1p2` and `p3p4` cross *transversally* — they
/// intersect at a single interior point of both.  Touching only at shared
/// endpoints (or colinear overlap) is **not** a crossing, so a travel is allowed
/// to graze a wall vertex.
fn segments_properly_cross(p1: Pt, p2: Pt, p3: Pt, p4: Pt) -> bool {
    let d1 = cross(p3, p4, p1);
    let d2 = cross(p3, p4, p2);
    let d3 = cross(p1, p2, p3);
    let d4 = cross(p1, p2, p4);
    // Strict sign change on both segments ⇒ proper interior crossing.
    ((d1 > EPS && d2 < -EPS) || (d1 < -EPS && d2 > EPS))
        && ((d3 > EPS && d4 < -EPS) || (d3 < -EPS && d4 > EPS))
}

/// Ramer–Douglas–Peucker simplification of a *closed* loop.
fn simplify_closed(pts: &[Pt], tol: f64) -> Vec<Pt> {
    if pts.len() <= 4 || tol <= 0.0 {
        return pts.to_vec();
    }
    // Treat the loop as open by anchoring at the two extreme points so RDP has a
    // stable baseline, then simplify the resulting polyline.
    let mut open = pts.to_vec();
    open.push(pts[0]);
    let simplified = rdp(&open, tol);
    // Drop the duplicated closing vertex.
    let mut out = simplified;
    if out.len() > 1 && dist(out[0], *out.last().unwrap()) < EPS {
        out.pop();
    }
    out
}

fn rdp(pts: &[Pt], tol: f64) -> Vec<Pt> {
    if pts.len() < 3 {
        return pts.to_vec();
    }
    let a = pts[0];
    let b = *pts.last().unwrap();
    let mut idx = 0;
    let mut max_d = 0.0;
    for (i, &p) in pts.iter().enumerate().take(pts.len() - 1).skip(1) {
        let d = perp_dist(p, a, b);
        if d > max_d {
            max_d = d;
            idx = i;
        }
    }
    if max_d > tol {
        let mut left = rdp(&pts[..=idx], tol);
        let right = rdp(&pts[idx..], tol);
        left.pop();
        left.extend(right);
        left
    } else {
        vec![a, b]
    }
}

fn perp_dist(p: Pt, a: Pt, b: Pt) -> f64 {
    let dx = b.0 - a.0;
    let dy = b.1 - a.1;
    let len = dx.hypot(dy);
    if len < EPS {
        return dist(p, a);
    }
    ((p.0 - a.0) * dy - (p.1 - a.1) * dx).abs() / len
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::SliceLayer;
    use clipper2::Path;

    fn layer_with_outer_square(side: f64) -> SliceLayer {
        let mut layer = SliceLayer::new(0.2);
        let h = side / 2.0;
        let sq: Path = vec![(-h, -h), (h, -h), (h, h), (-h, h)].into();
        layer.paths.push(sq);
        layer.path_roles.push(ExtrusionRole::OuterWall);
        layer.path_widths.push(Some(0.4));
        layer.path_is_open.push(false);
        layer
    }

    #[test]
    fn direct_hop_outside_all_walls_is_unchanged() {
        let layer = layer_with_outer_square(10.0);
        let planner = TravelPlanner::for_layer(&layer).unwrap();
        // Both endpoints well outside the square, line does not clip it.
        let route = planner.route((-20.0, -20.0), (-20.0, 20.0));
        assert_eq!(route, vec![(-20.0, 20.0)]);
    }

    #[test]
    fn hop_through_a_wall_detours_around_it() {
        let layer = layer_with_outer_square(10.0);
        let planner = TravelPlanner::for_layer(&layer).unwrap();
        // Straight line from left to right passes through the solid square,
        // crossing its left and right walls → must detour.
        let from = (-20.0, 0.0);
        let to = (20.0, 0.0);
        let route = planner.route(from, to);
        assert!(
            route.len() > 1,
            "expected a multi-point detour, got {route:?}"
        );
        assert_eq!(*route.last().unwrap(), to, "route must end at destination");
        // No leg of the detour may cross a wall.
        let mut prev = from;
        for &wp in &route {
            assert!(
                !planner.crosses_any_wall(prev, wp),
                "detour leg {prev:?}->{wp:?} crosses a wall"
            );
            prev = wp;
        }
    }

    #[test]
    fn interior_hop_within_one_island_is_unchanged() {
        let layer = layer_with_outer_square(20.0);
        let planner = TravelPlanner::for_layer(&layer).unwrap();
        // Both endpoints inside the same island: straight interior travel is
        // hidden and allowed.
        let route = planner.route((-5.0, -5.0), (5.0, 5.0));
        assert_eq!(route, vec![(5.0, 5.0)]);
    }

    #[test]
    fn no_outer_walls_yields_no_planner() {
        let mut layer = SliceLayer::new(0.2);
        let sq: Path = vec![(0.0, 0.0), (5.0, 0.0), (5.0, 5.0), (0.0, 5.0)].into();
        layer.paths.push(sq);
        layer.path_roles.push(ExtrusionRole::Infill);
        layer.path_is_open.push(true);
        assert!(TravelPlanner::for_layer(&layer).is_none());
    }
}
