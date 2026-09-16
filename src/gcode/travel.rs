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

/// Maximum footprint vertices [`MaterialRouter`] will hold for one layer. Far
/// larger than [`MAX_OBSTACLE_VERTS`] because an extrusion footprint traces every
/// bead rather than the outer walls alone, and because each *query* narrows to a
/// local window first — the cap only has to keep a pathological layer from
/// holding an unbounded vector.
const MAX_MATERIAL_VERTS: usize = 20_000;

/// Maximum waypoints in one routing query's local window. A window this crowded
/// means the hop is in filigree the `O(V²)` search cannot afford; the hop stays
/// straight.
const MAX_MATERIAL_ROUTE_VERTS: usize = 240;

type Pt = (f64, f64);

/// Per-layer travel router.  Build once per layer with [`TravelPlanner::for_layer`].
pub struct TravelPlanner {
    /// Closed obstacle loops (outer-wall contours and hole boundaries).
    loops: Vec<Vec<Pt>>,
    /// Per-loop flag: this loop is an island **outline** — it is not contained
    /// in any other loop, so its interior is the part's footprint rather than a
    /// hole. Parallel to `loops`.
    outline: Vec<bool>,
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
        // Outermost = whose first vertex lies inside no other loop. A hole's
        // boundary sits inside its island outline; separate islands are
        // disjoint, so neither contains the other.
        let outline: Vec<bool> = loops
            .iter()
            .enumerate()
            .map(|(i, l)| {
                !loops
                    .iter()
                    .enumerate()
                    .any(|(j, other)| j != i && point_in_loop(l[0], other))
            })
            .collect();

        Some(Self {
            loops,
            outline,
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

    /// True when the hop `a`→`b` is **interior** to the part: it crosses no
    /// wall and runs inside one island's outline.
    ///
    /// The G-code generator uses this to decide whether a hop can skip the
    /// retract → z-hop → travel → lower → un-retract ceremony. Whatever such a
    /// hop oozes lands inside the part — in a pocket, over infill, or on the
    /// wall it just printed — where the ceremony costs more time than the ooze
    /// costs quality. A hop that crosses a wall, leaves the footprint, or runs
    /// between two islands fails the test and retracts as usual.
    ///
    /// Testing the segment's midpoint is sufficient: a segment that crosses no
    /// loop lies wholly on one side of every loop.
    pub fn hop_is_interior(&self, a: Pt, b: Pt) -> bool {
        if self.crosses_any_wall(a, b) {
            return false;
        }
        let mid = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
        self.loops
            .iter()
            .zip(&self.outline)
            .any(|(l, &is_outline)| is_outline && point_in_loop(mid, l))
    }

    /// Visibility-graph shortest path from `from` to `to` avoiding wall crossings.
    fn route_around(&self, from: Pt, to: Pt) -> Option<Vec<Pt>> {
        visibility_route(&self.verts, from, to, f64::INFINITY, &|a, b| {
            !self.crosses_any_wall(a, b)
        })
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

/// Keeps a short travel hop **over printed material** instead of letting it cut
/// across open air.
///
/// A hop that leaves the part drools into the void it crosses — the strings
/// between the dividers of a card caddy, the webs across a fan grille. Those
/// hops are unavoidable in *number* (a field of thin ribs is one short bead per
/// rib), but not in *route*: the ribs share a wall band, so a hop can go back
/// into the band, along it, and out again without ever leaving material.
///
/// # What "material" means
///
/// The layer's extrusion footprint — every bead's physical area, walls and
/// medial beads alike ([`crate::core::compute_wall_bead_footprint`]). Its
/// boundary is the containment test. Waypoints, by contrast, come from that
/// footprint **eroded by half a bead**, so a shortest path hugs bead interiors
/// rather than running along the outer edge of the wall it just printed, which
/// is the surface a dragged nozzle would scar. Start and end still only have to
/// satisfy the full-footprint test, so a bead centerline is always a legal
/// endpoint even where erosion has collapsed its own bead.
pub struct MaterialRouter {
    /// Boundary loops of the extrusion footprint.
    loops: Vec<Vec<Pt>>,
    /// Waypoint candidates — vertices of the eroded footprint.
    verts: Vec<Pt>,
    bounds: (f64, f64, f64, f64),
}

impl MaterialRouter {
    /// Build a router from a layer's extrusion footprint, or `None` when there
    /// is nothing to route over or too much of it to plan within budget.
    pub fn for_layer(layer: &SliceLayer, nozzle_diameter_mm: f64) -> Option<Self> {
        if nozzle_diameter_mm <= 0.0 {
            return None;
        }
        let footprint = crate::core::compute_wall_bead_footprint(layer, nozzle_diameter_mm);
        if footprint.is_empty() {
            return None;
        }
        let to_loops = |paths: &clipper2::Paths| -> Vec<Vec<Pt>> {
            paths
                .iter()
                .map(|p| {
                    let raw: Vec<Pt> = p.iter().map(|q| (q.x(), q.y())).collect();
                    simplify_closed(&raw, SIMPLIFY_TOL_MM)
                })
                .filter(|l| l.len() >= 3)
                .collect()
        };

        let loops = to_loops(&footprint);
        if loops.is_empty() {
            return None;
        }
        let total: usize = loops.iter().map(|l| l.len()).sum();
        if total > MAX_MATERIAL_VERTS {
            return None;
        }

        // Waypoints sit half a bead inside the material, so a route turns in the
        // middle of the beads it crosses rather than along the outer edge of the
        // wall just printed — the surface a dragged nozzle would scar. Eroding
        // also keeps the waypoint count an order of magnitude below the raw
        // centerlines, which is what keeps the O(V²) search affordable.
        let inner = clipper2::inflate(
            footprint.clone(),
            -0.5 * nozzle_diameter_mm,
            clipper2::JoinType::Miter,
            clipper2::EndType::Polygon,
            2.0,
        );
        let mut verts: Vec<Pt> = to_loops(&inner).into_iter().flatten().collect();

        // Plus the ends of every open bead. Erosion deletes a bead only one
        // nozzle wide — a rib, a fin, a medial gap bead — so a rib meeting a
        // wall leaves no corner to turn at, and the one route that matters (down
        // the rib, along the wall, out the next rib) has nowhere to turn. A
        // bead's own ends are exactly those turning points, and there are two
        // per bead rather than one per vertex.
        for (i, path) in layer.paths.iter().enumerate() {
            if !layer.is_path_open(i) {
                continue;
            }
            let mut ends = path.iter();
            if let Some(first) = ends.next() {
                verts.push((first.x(), first.y()));
            }
            if let Some(last) = path.iter().last() {
                verts.push((last.x(), last.y()));
            }
        }

        let (mut min_x, mut min_y) = (f64::INFINITY, f64::INFINITY);
        let (mut max_x, mut max_y) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
        for l in &loops {
            for &(x, y) in l {
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

    /// True when the whole segment `a`–`b` lies on material.
    pub fn covers(&self, a: Pt, b: Pt) -> bool {
        let (min_x, min_y, max_x, max_y) = self.bounds;
        if a.0.min(b.0) > max_x
            || a.0.max(b.0) < min_x
            || a.1.min(b.1) > max_y
            || a.1.max(b.1) < min_y
        {
            return false;
        }
        for l in &self.loops {
            let m = l.len();
            for i in 0..m {
                if segments_properly_cross(a, b, l[i], l[(i + 1) % m]) {
                    return false;
                }
            }
        }
        // Crossing nothing, the segment is wholly inside or wholly outside every
        // loop, so its midpoint settles which.
        let mid = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
        self.loops.iter().filter(|l| point_in_loop(mid, l)).count() % 2 == 1
    }

    /// Route `from` → `to` without ever leaving material, in at most `max_len`
    /// of travel. Returns the waypoints to move through (`to` last, `from` never
    /// included), or `None` when no such route fits.
    ///
    /// A hop that is already on material returns `[to]` — the straight line.
    pub fn route(&self, from: Pt, to: Pt, max_len: f64) -> Option<Vec<Pt>> {
        if self.covers(from, to) {
            return Some(vec![to]);
        }
        // Only waypoints that could serve a route this short are worth scoring.
        let (lo_x, hi_x) = (from.0.min(to.0) - max_len, from.0.max(to.0) + max_len);
        let (lo_y, hi_y) = (from.1.min(to.1) - max_len, from.1.max(to.1) + max_len);
        let local: Vec<Pt> = self
            .verts
            .iter()
            .copied()
            .filter(|&(x, y)| x >= lo_x && x <= hi_x && y >= lo_y && y <= hi_y)
            .collect();
        if local.is_empty() || local.len() > MAX_MATERIAL_ROUTE_VERTS {
            return None;
        }
        visibility_route(&local, from, to, max_len, &|a, b| self.covers(a, b))
    }
}

/// Visibility-graph shortest path from `from` to `to` over `verts`, admitting
/// only the edges `edge_ok` accepts, and giving up once the best route exceeds
/// `max_len`.
///
/// Node indices are `0..verts.len()` for the waypoints, then `from`, then `to`.
/// Dijkstra evaluates neighbour visibility on the fly rather than building an
/// `O(V²)` matrix, because the overwhelmingly common call is a short hop whose
/// direct edge is already admissible.
///
/// Returns the ordered waypoints to move through; `from` is never included, and
/// `to` is always last.
fn visibility_route(
    verts: &[Pt],
    from: Pt,
    to: Pt,
    max_len: f64,
    edge_ok: &dyn Fn(Pt, Pt) -> bool,
) -> Option<Vec<Pt>> {
    let n = verts.len();
    let start = n;
    let goal = n + 1;
    let node = |i: usize| -> Pt {
        if i == start {
            from
        } else if i == goal {
            to
        } else {
            verts[i]
        }
    };

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
        if u == usize::MAX || best > max_len {
            break;
        }
        if u == goal {
            break;
        }
        visited[u] = true;
        let up = node(u);

        // Candidate neighbours: all waypoints plus the goal.
        for v in 0..(n + 2) {
            if v == start || visited[v] || v == u {
                continue;
            }
            let vp = node(v);
            let step = dist(up, vp);
            if step <= EPS || dist_to[u] + step >= dist_to[v] || dist_to[u] + step > max_len {
                continue;
            }
            if !edge_ok(up, vp) {
                continue;
            }
            dist_to[v] = dist_to[u] + step;
            prev[v] = u;
        }
    }

    if !dist_to[goal].is_finite() || dist_to[goal] > max_len {
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

/// Even-odd ray-cast point-in-polygon test (winding-independent).
fn point_in_loop(p: Pt, poly: &[Pt]) -> bool {
    let n = poly.len();
    if n < 3 {
        return false;
    }
    let (px, py) = p;
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = poly[i];
        let (xj, yj) = poly[j];
        if ((yi > py) != (yj > py)) && (px < (xj - xi) * (py - yi) / (yj - yi) + xi) {
            inside = !inside;
        }
        j = i;
    }
    inside
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
    fn interior_hop_inside_an_island_is_interior() {
        let layer = layer_with_outer_square(10.0);
        let planner = TravelPlanner::for_layer(&layer).unwrap();
        assert!(planner.hop_is_interior((-2.0, -2.0), (2.0, 2.0)));
    }

    #[test]
    fn hop_outside_every_island_is_not_interior() {
        let layer = layer_with_outer_square(10.0);
        let planner = TravelPlanner::for_layer(&layer).unwrap();
        // Alongside the square, crossing nothing — but over bare bed, where a
        // drool is a loose strand rather than material landing on the part.
        assert!(!planner.hop_is_interior((-20.0, -20.0), (-20.0, 20.0)));
    }

    #[test]
    fn hop_that_leaves_through_a_wall_is_not_interior() {
        let layer = layer_with_outer_square(10.0);
        let planner = TravelPlanner::for_layer(&layer).unwrap();
        assert!(!planner.hop_is_interior((0.0, 0.0), (20.0, 0.0)));
    }

    #[test]
    fn hop_across_a_hole_stays_interior() {
        // A rib field lives in a pocket: the hop from one rib to the next runs
        // through the hole, crossing no wall, still inside the island outline.
        let mut layer = layer_with_outer_square(20.0);
        let hole: Path = vec![(-5.0, -5.0), (-5.0, 5.0), (5.0, 5.0), (5.0, -5.0)].into();
        layer.paths.push(hole);
        layer.path_roles.push(ExtrusionRole::OuterWall);
        layer.path_widths.push(Some(0.4));
        layer.path_is_open.push(false);

        let planner = TravelPlanner::for_layer(&layer).unwrap();
        assert!(planner.hop_is_interior((-3.0, 0.0), (3.0, 0.0)));
    }

    /// Two ribs off a shared spine, the card-divider shape. The hop between
    /// their far ends crosses open air; the router sends it back down one rib,
    /// along the spine and out the other.
    fn rib_field_layer() -> SliceLayer {
        let mut layer = SliceLayer::new(0.2);
        let push = |layer: &mut SliceLayer, pts: Vec<(f64, f64)>, w: f64| {
            let path: Path = pts.into();
            layer.paths.push(path);
            layer.path_roles.push(ExtrusionRole::OuterWall);
            layer.path_widths.push(Some(w));
            layer.path_is_open.push(true);
        };
        // A 1.2 mm-wide spine up x = 10, with two 0.4 mm ribs reaching left.
        push(&mut layer, vec![(10.0, 0.0), (10.0, 20.0)], 1.2);
        push(&mut layer, vec![(10.0, 5.0), (6.0, 5.0)], 0.4);
        push(&mut layer, vec![(10.0, 8.0), (6.0, 8.0)], 0.4);
        layer
    }

    #[test]
    fn a_hop_along_one_bead_is_already_on_material() {
        let layer = rib_field_layer();
        let router = MaterialRouter::for_layer(&layer, 0.4).unwrap();
        assert!(router.covers((10.0, 2.0), (10.0, 6.0)));
        assert_eq!(
            router.route((10.0, 2.0), (10.0, 6.0), 8.0),
            Some(vec![(10.0, 6.0)])
        );
    }

    #[test]
    fn a_hop_across_open_air_is_not_on_material() {
        let layer = rib_field_layer();
        let router = MaterialRouter::for_layer(&layer, 0.4).unwrap();
        // Straight from one rib tip to the other, through the empty slot.
        assert!(!router.covers((6.0, 5.0), (6.0, 8.0)));
    }

    #[test]
    fn a_rib_to_rib_hop_reroutes_over_the_spine() {
        let layer = rib_field_layer();
        let router = MaterialRouter::for_layer(&layer, 0.4).unwrap();
        // Straight line 3 mm; the way round is ~4+3+4, so give it room.
        let route = router
            .route((8.0, 5.0), (8.0, 8.0), 12.0)
            .expect("a route over the ribs and spine exists");
        assert!(route.len() > 1, "expected a detour, got {route:?}");
        assert_eq!(route.last().copied(), Some((8.0, 8.0)));
        // Every leg stays on material — that is the whole point.
        let mut from = (8.0, 5.0);
        for &wp in &route {
            assert!(
                router.covers(from, wp),
                "leg {from:?}->{wp:?} left the part"
            );
            from = wp;
        }
    }

    #[test]
    fn a_hop_with_no_route_within_budget_gives_up() {
        let layer = rib_field_layer();
        let router = MaterialRouter::for_layer(&layer, 0.4).unwrap();
        // The way round is far longer than the 3 mm straight line; a budget that
        // tight must be refused rather than stretched.
        assert_eq!(router.route((6.0, 5.0), (6.0, 8.0), 3.5), None);
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
