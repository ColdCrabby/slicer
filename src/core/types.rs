use clipper2::*;

/// The role of an extrusion path, used to annotate G-code with `;TYPE:` comments
/// and enable firmware features like Klipper adaptive acceleration by role.
///
/// Each variant maps to a named type that is emitted in the G-code output and
/// carries a default extrusion width for that role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExtrusionRole {
    /// Outermost perimeter / wall contour (default role).
    #[default]
    OuterWall,
    /// Inner perimeter / wall contours.
    InnerWall,
    /// Perimeter (outer or inner) that crosses unsupported air below it.
    ///
    /// Treated as a bridge in the G-code generator (slow speed, reduced flow,
    /// high fan cooling) so the wall strand cools and tensions before the
    /// next layer lands on it.  Unlike [`Self::Bridge`] (which is bridge
    /// **infill** spanning a gap), this is a **wall** path printed in air.
    OverhangPerimeter,
    /// Sparse infill pattern (low-density interior fill).
    Infill,
    /// Bridge extrusion spanning a gap with no support below.
    Bridge,
    /// Solid top-surface infill.
    TopSurface,
    /// Solid bottom-surface infill.
    BottomSurface,
    /// Dense solid infill **inside** the part — the hidden floors
    /// [`crate::settings::params::SlicingParams::solid_infill_every_layers`]
    /// inserts to brace tall sparse regions. Not a visible surface, so it is
    /// tagged separately from top/bottom.
    InternalSolid,
    /// Variable-width gap fill: thin-wall medial beads laid into spaces too
    /// narrow for a full perimeter. Emitted as OrcaSlicer `;TYPE:Gap infill`.
    GapFill,
    /// Support structure material.
    Support,
    /// Skirt or brim line.
    Skirt,
    /// Near-dry smoothing pass over a finished top surface.
    ///
    /// Deposits almost no material — the nozzle re-melts what is already there
    /// and spreads it flat. A role of its own rather than a variant of
    /// [`Self::TopSurface`] because it needs its own speed, its own
    /// acceleration, its own (much reduced) flow, and its own path-ordering
    /// group so it cannot be interleaved with the fill it is meant to follow.
    Ironing,
}

impl ExtrusionRole {
    /// The `;TYPE:` label emitted in G-code comments for this role.
    ///
    /// Strings match the OrcaSlicer convention exactly so that G-code previews
    /// colour and classify paths correctly.  Any unrecognised string would be
    /// shown as *Undefined* in OrcaSlicer's G-code viewer.
    pub fn type_name(self) -> &'static str {
        match self {
            Self::OuterWall => "Outer wall",
            Self::InnerWall => "Inner wall",
            Self::OverhangPerimeter => "Overhang wall",
            Self::Infill => "Sparse infill",
            Self::Bridge => "Bridge",
            Self::TopSurface => "Top surface",
            Self::BottomSurface => "Bottom surface",
            Self::InternalSolid => "Internal solid infill",
            Self::GapFill => "Gap infill",
            Self::Support => "Support material",
            Self::Skirt => "Skirt",
            Self::Ironing => "Ironing",
        }
    }

    /// Default extrusion width in mm for this role.
    ///
    /// Used to populate the `;WIDTH:` annotation in the G-code output.
    pub fn default_width_mm(self) -> f64 {
        match self {
            Self::OuterWall
            | Self::InnerWall
            | Self::OverhangPerimeter
            | Self::Infill
            | Self::Bridge
            | Self::TopSurface
            | Self::BottomSurface
            | Self::InternalSolid => 0.4,
            Self::GapFill => 0.4,
            Self::Support => 0.4,
            Self::Skirt => 0.4,
            Self::Ironing => 0.4,
        }
    }
}

/// Overhang severity class for a single wall path, used by the **dynamic
/// overhang speed & cooling** feature ([`crate::settings::params::SlicingParams::enable_overhang_speed`]).
///
/// A wall bead is centred on its centreline and spans one bead width; the class
/// records how much of that width overhangs unsupported air below it, measured
/// against the previous layer's material footprint (`inflate(perimeters[i-1],
/// +d/2)`).  The four degrees mirror the OrcaSlicer / PrusaSlicer 4-band model:
///
/// | Class   | Unsupported fraction | OrcaSlicer speed field |
/// |---------|----------------------|------------------------|
/// | `None`  | ≤ 0 % (fully on material) | (normal perimeter speed) |
/// | `Deg1`  | 0 – 25 %             | `overhang_1_4_speed`   |
/// | `Deg2`  | 25 – 50 %            | `overhang_2_4_speed`   |
/// | `Deg3`  | 50 – 75 %            | `overhang_3_4_speed`   |
/// | `Deg4`  | 75 – 100 %           | `overhang_4_4_speed`   |
///
/// `Deg3` / `Deg4` (majority in air) coincide with the segments the binary
/// [`ExtrusionRole::OverhangPerimeter`] classifier already isolates, so the
/// role and the class stay consistent: any path tagged `OverhangPerimeter`
/// carries a class of `Deg3` or `Deg4`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OverhangClass {
    /// Fully supported — no dynamic overhang override applies.
    #[default]
    None,
    /// 0–25 % of the bead width unsupported.
    Deg1,
    /// 25–50 % unsupported.
    Deg2,
    /// 50–75 % unsupported.
    Deg3,
    /// 75–100 % unsupported.
    Deg4,
}

impl OverhangClass {
    /// Band index `0..=4` (`0` = fully supported, `4` = fully in air).
    pub fn band(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Deg1 => 1,
            Self::Deg2 => 2,
            Self::Deg3 => 3,
            Self::Deg4 => 4,
        }
    }

    /// Construct from a band index; values `≥ 4` saturate to [`Self::Deg4`].
    pub fn from_band(band: u8) -> Self {
        match band {
            0 => Self::None,
            1 => Self::Deg1,
            2 => Self::Deg2,
            3 => Self::Deg3,
            _ => Self::Deg4,
        }
    }

    /// `true` when this class represents an actual overhang (`Deg1`–`Deg4`).
    pub fn is_overhang(self) -> bool {
        self != Self::None
    }
}

/// Plugin-owned scratch space attached to one path.
///
/// Keyed by plugin id so two plugins can both annotate the same path without
/// knowing about each other — the alternative, a single slot, makes the second
/// plugin to run silently clobber the first.
///
/// An empty `PathData` allocates nothing, which is what every path on an
/// ordinary print carries.
#[derive(Clone, Default)]
pub struct PathData(
    Vec<(
        &'static str,
        std::sync::Arc<dyn std::any::Any + Send + Sync>,
    )>,
);

impl PathData {
    /// Whether any plugin has attached anything.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Attach `value` under `owner`, replacing whatever that plugin stored
    /// before.
    pub fn set<T: std::any::Any + Send + Sync>(&mut self, owner: &'static str, value: T) {
        let value = std::sync::Arc::new(value);
        match self.0.iter_mut().find(|(id, _)| *id == owner) {
            Some(slot) => slot.1 = value,
            None => self.0.push((owner, value)),
        }
    }

    /// Read back what `owner` attached, if it is of type `T`.
    pub fn get<T: std::any::Any + Send + Sync>(&self, owner: &str) -> Option<&T> {
        self.0
            .iter()
            .find(|(id, _)| *id == owner)
            .and_then(|(_, value)| value.downcast_ref::<T>())
    }

    /// The plugin ids with data attached here.
    pub fn owners(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.0.iter().map(|(id, _)| *id)
    }
}

impl std::fmt::Debug for PathData {
    /// `dyn Any` is not `Debug`, so this names the owners rather than their
    /// values — enough to see who attached something when reading a dump.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_set().entries(self.owners()).finish()
    }
}

/// How a picked path's vertices are re-walked when a layer is rebuilt.
///
/// Whatever is chosen here is applied to the path's vertices **and** to every
/// per-vertex array in step, which is the part that is easy to get wrong by
/// hand: a rotated loop whose widths were not rotated with it prints the wrong
/// bead width at every vertex.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VertexOrder {
    /// Keep the vertex order as it is.
    #[default]
    AsIs,
    /// Walk the vertices backwards.
    Reversed,
    /// Rotate a closed loop to start at this vertex index (its seam).
    RotatedTo(usize),
}

/// One entry in a layer rebuild: which existing path to take, and how.
#[derive(Debug, Clone, Copy)]
pub struct PathPick {
    /// Index into the layer's *current* paths.
    pub index: usize,
    /// How that path's vertices are re-walked.
    pub order: VertexOrder,
}

impl PathPick {
    /// Take path `index` unchanged.
    pub fn keep(index: usize) -> Self {
        Self {
            index,
            order: VertexOrder::AsIs,
        }
    }

    /// Take path `index`, walking its vertices backwards.
    pub fn reversed(index: usize) -> Self {
        Self {
            index,
            order: VertexOrder::Reversed,
        }
    }

    /// Take closed loop `index`, rotated to start at vertex `seam`.
    pub fn rotated(index: usize, seam: usize) -> Self {
        Self {
            index,
            order: VertexOrder::RotatedTo(seam),
        }
    }
}

/// Apply a [`VertexOrder`] to a per-vertex array.
fn reorder_vertex_values<T: Clone>(values: Vec<T>, order: VertexOrder) -> Vec<T> {
    match order {
        VertexOrder::AsIs => values,
        VertexOrder::Reversed => {
            let mut v = values;
            v.reverse();
            v
        }
        VertexOrder::RotatedTo(seam) => {
            let n = values.len();
            if n == 0 || seam == 0 {
                return values;
            }
            (0..n).map(|k| values[(seam + k) % n].clone()).collect()
        }
    }
}

/// Represents a slice layer in the 3D model
#[derive(Debug, Clone)]
pub struct SliceLayer {
    /// Z-coordinate of this layer
    pub z: f64,
    /// Paths that make up this layer (closed contours in XY)
    pub paths: Paths,
    /// Extrusion role for each path in [`SliceLayer::paths`].
    ///
    /// `path_roles[i]` is the role of `paths[i]`.  If shorter than `paths`,
    /// the remaining paths default to [`ExtrusionRole::OuterWall`].
    pub path_roles: Vec<ExtrusionRole>,
    /// Per-path extrusion width override in mm.
    ///
    /// `path_widths[i]` is the extrusion width for `paths[i]`.  `None` means
    /// use the role's default width ([`ExtrusionRole::default_width_mm`]).
    /// This is set by the Arachne variable-width perimeter generator.
    pub path_widths: Vec<Option<f64>>,
    /// Per-path **per-vertex** extrusion width overrides in mm.
    ///
    /// `path_vertex_widths[i]`, when `Some`, holds one width per vertex of
    /// `paths[i]` (same length and order); the width of the segment between two
    /// vertices is the mean of its endpoints.  `None` (or a short vector) falls
    /// back to the scalar [`SliceLayer::path_widths`] entry.  Set by the Arachne
    /// medial gap-fill beads so their width tapers along the bead.
    pub path_vertex_widths: Vec<Option<Vec<f64>>>,
    /// The union of top and bottom solid-surface regions on this layer.
    ///
    /// Populated by [`generate_top_bottom_surfaces`] and used by
    /// [`add_infill_to_layers`] to prevent sparse infill from being placed on
    /// areas already filled with solid top/bottom surface infill.
    pub solid_regions: Paths,
    /// The unsupported area on this layer — portions of the layer footprint
    /// that have no solid material directly below them in the previous layer.
    ///
    /// Populated by [`generate_top_bottom_surfaces`] (its surface-detection
    /// pass already computes this).  Used after surface generation to
    /// classify wall paths that cross air as
    /// [`ExtrusionRole::OverhangPerimeter`].
    ///
    /// This is the *raw* unsupported area — it includes the area covered by
    /// the perimeter walls themselves, **before** clipping to the wall
    /// interior.  This is intentional: an overhanging wall path lies on the
    /// perimeter of the layer, so detecting it requires the full footprint
    /// view rather than just the inside-the-walls interior.
    pub unsupported_regions: Paths,
    /// Per-path open-arc flag.
    ///
    /// Set to `true` for wall paths that are **open polyline segments** —
    /// i.e. sub-arcs produced when [`classify_overhang_perimeters`] splits a
    /// closed loop at the air/support boundary.  `false` (or absent) means
    /// the path is a genuine closed loop and the G-code generator should
    /// append a closing move back to the first vertex.
    ///
    /// Indexed parallel to [`SliceLayer::paths`] / [`SliceLayer::path_roles`].
    /// Shorter-than-paths vectors default to `false` (closed).
    pub path_is_open: Vec<bool>,
    /// Per-path overhang severity class for dynamic overhang speed & cooling.
    ///
    /// `path_overhang[i]` is the [`OverhangClass`] of `paths[i]`.  Populated by
    /// [`classify_overhang_perimeters`] **only** when
    /// [`crate::settings::params::SlicingParams::enable_overhang_speed`] is set;
    /// otherwise it is left empty and every path resolves to
    /// [`OverhangClass::None`] via [`SliceLayer::overhang_for_path`].
    ///
    /// Indexed parallel to [`SliceLayer::paths`].  Shorter-than-paths vectors
    /// default to [`OverhangClass::None`] (no override).
    pub path_overhang: Vec<OverhangClass>,
    /// Per-path extrusion **height** override in mm.
    ///
    /// `path_heights[i]` is the layer height the G-code generator should charge
    /// `paths[i]` at. `None` (or a missing entry) means the print's normal layer
    /// height.
    ///
    /// Set by sparse-infill layer combining
    /// ([`crate::settings::params::SlicingParams::infill_every_layers`]), where
    /// the top layer of a group prints the infill it stood in for at the group's
    /// full stacked height. Nothing else overrides it, so this vector is empty
    /// on an ordinary print.
    pub path_heights: Vec<Option<f64>>,
    /// Per-path owning **print object** index, for firmware object exclusion
    /// (`EXCLUDE_OBJECT` / `M486`) and sequential printing.
    ///
    /// `path_objects[i]` identifies which object on the plate produced
    /// `paths[i]`, indexing into the plate's object list
    /// ([`crate::core::PlateSlice::objects`]).  `None` — and any index past the
    /// end of the vector — means the path belongs to **no** object: bed
    /// adhesion (skirt / brim / raft) covers the whole plate and must never be
    /// cancelled with a single part.
    ///
    /// Left empty by the single-mesh pipeline; populated only when the plate is
    /// sliced object by object (see [`crate::core::slice_plate`]).
    pub path_objects: Vec<Option<usize>>,
    /// Per-path **per-vertex** Z offsets in mm, relative to [`SliceLayer::z`].
    ///
    /// `path_vertex_z[i]`, when `Some`, holds one offset per vertex of
    /// `paths[i]` (same length and order). The Z of the segment between two
    /// vertices ramps linearly between their offsets, exactly as
    /// [`SliceLayer::path_vertex_widths`] interpolates width.
    ///
    /// This is what makes non-planar extrusion expressible at all: without it a
    /// layer is one flat plane, and a feature like a wavy overhang — which
    /// needs the bead to rise and fall *within* a layer — cannot be described
    /// no matter where a plugin is allowed to run.
    ///
    /// Offsets rather than absolute heights, deliberately: `z` moves when a
    /// raft is prepended or a first layer is made thicker, and an offset stays
    /// correct across both. Empty (the default) means every path is flat, which
    /// is what an ordinary print produces.
    pub path_vertex_z: Vec<Option<Vec<f64>>>,
    /// Per-path scratch space owned by plugins.
    ///
    /// A plugin that computes something about a path in one stage and consumes
    /// it in a later one needs somewhere to put it that survives the stages in
    /// between — including the ones that reorder, split and prepend paths.
    /// Keyed by plugin id so two plugins cannot collide.
    ///
    /// Empty (the default) means no plugin attached anything, which costs an
    /// ordinary print nothing.
    pub path_data: Vec<PathData>,
}

impl SliceLayer {
    /// Create a new slice layer at the given Z coordinate
    pub fn new(z: f64) -> Self {
        Self {
            z,
            paths: Paths::default(),
            path_roles: Vec::new(),
            path_widths: Vec::new(),
            path_vertex_widths: Vec::new(),
            solid_regions: Paths::default(),
            unsupported_regions: Paths::default(),
            path_is_open: Vec::new(),
            path_overhang: Vec::new(),
            path_heights: Vec::new(),
            path_objects: Vec::new(),
            path_vertex_z: Vec::new(),
            path_data: Vec::new(),
        }
    }

    /// Return the extrusion role for path index `i`.
    ///
    /// Falls back to [`ExtrusionRole::OuterWall`] when `path_roles` has no
    /// entry for the given index.
    pub fn role_for_path(&self, i: usize) -> ExtrusionRole {
        self.path_roles.get(i).copied().unwrap_or_default()
    }

    /// Return the extrusion width in mm for path index `i`.
    ///
    /// Returns the per-path override when set, otherwise falls back to the
    /// role's default width via [`ExtrusionRole::default_width_mm`].
    pub fn width_for_path(&self, i: usize) -> Option<f64> {
        self.path_widths.get(i).copied().flatten()
    }

    /// Return the per-vertex widths for path index `i`, if any.
    ///
    /// `None` when unset or the index is out of range; callers then fall back
    /// to [`SliceLayer::width_for_path`].
    pub fn vertex_widths_for_path(&self, i: usize) -> Option<Vec<f64>> {
        self.path_vertex_widths.get(i).cloned().flatten()
    }

    /// Returns `true` when path index `i` is an open arc (a sub-segment
    /// produced by splitting a closed loop at an air/support boundary).
    ///
    /// Falls back to `false` (closed loop) when the index is out of range.
    pub fn is_path_open(&self, i: usize) -> bool {
        self.path_is_open.get(i).copied().unwrap_or(false)
    }

    /// Return the overhang severity class for path index `i`.
    ///
    /// Falls back to [`OverhangClass::None`] (fully supported, no dynamic
    /// overhang override) when `path_overhang` has no entry for the index —
    /// which is the case for every path when
    /// [`crate::settings::params::SlicingParams::enable_overhang_speed`] is off.
    pub fn overhang_for_path(&self, i: usize) -> OverhangClass {
        self.path_overhang.get(i).copied().unwrap_or_default()
    }

    /// Return the extrusion height override in mm for path index `i`.
    ///
    /// `None` means "use the print's layer height"; only combined sparse infill
    /// sets it.
    pub fn height_for_path(&self, i: usize) -> Option<f64> {
        self.path_heights.get(i).copied().flatten()
    }

    /// Return the owning print-object index for path `i`, if any.
    ///
    /// `None` for plate-wide geometry (bed adhesion) and for every path of a
    /// layer that was not sliced object-aware.
    pub fn object_for_path(&self, i: usize) -> Option<usize> {
        self.path_objects.get(i).copied().flatten()
    }

    /// Return the per-vertex Z offsets for path index `i`, if any.
    ///
    /// `None` means the path is flat at [`SliceLayer::z`].
    pub fn vertex_z_for_path(&self, i: usize) -> Option<&[f64]> {
        self.path_vertex_z.get(i)?.as_deref()
    }

    /// Return the plugin scratch space attached to path `i`, if any.
    pub fn data_for_path(&self, i: usize) -> Option<&PathData> {
        self.path_data.get(i)
    }

    /// Pad every per-path array out to `paths.len()` with its own default.
    ///
    /// The arrays that use an **empty vector as a sentinel** — `path_overhang`
    /// ("not graded"), `path_heights` ("no override"), `path_objects` ("not
    /// object-aware"), `path_vertex_z` ("flat") and `path_data` ("nothing
    /// attached") — are left empty if they already are. Padding one of those
    /// would turn "this feature never ran" into "it ran and found nothing",
    /// which is a different statement.
    pub fn pad_per_path_arrays(&mut self) {
        let n = self.paths.len();
        self.path_roles.resize(n, ExtrusionRole::OuterWall);
        self.path_widths.resize(n, None);
        self.path_vertex_widths.resize(n, None);
        self.path_is_open.resize(n, false);
        if !self.path_overhang.is_empty() {
            self.path_overhang.resize(n, OverhangClass::None);
        }
        if !self.path_heights.is_empty() {
            self.path_heights.resize(n, None);
        }
        if !self.path_objects.is_empty() {
            self.path_objects.resize(n, None);
        }
        if !self.path_vertex_z.is_empty() {
            self.path_vertex_z.resize(n, None);
        }
        if !self.path_data.is_empty() {
            self.path_data.resize(n, PathData::default());
        }
    }

    /// Rebuild the layer as the given selection of its current paths.
    ///
    /// One call replaces the paths **and every per-path array** together, so a
    /// site that reorders, filters or re-seams paths cannot leave one array
    /// behind and shift somebody's tags onto the wrong path. That failure is
    /// silent and has bitten this codebase before, which is why the rebuild is
    /// here rather than open-coded per caller.
    ///
    /// Per-*vertex* arrays are re-walked with the same [`VertexOrder`] as the
    /// path's own vertices, so a rotated or reversed loop keeps its widths and
    /// Z offsets aligned to the points they describe.
    ///
    /// Picks may repeat an index (duplicating a path) or omit one (dropping
    /// it). An out-of-range index is skipped.
    pub fn rebuild_paths(&mut self, picks: &[PathPick]) {
        self.pad_per_path_arrays();

        let mut paths = Paths::default();
        let mut roles = Vec::with_capacity(picks.len());
        let mut widths = Vec::with_capacity(picks.len());
        let mut vertex_widths = Vec::with_capacity(picks.len());
        let mut is_open = Vec::with_capacity(picks.len());
        let mut overhang = Vec::new();
        let mut heights = Vec::new();
        let mut objects = Vec::new();
        let mut vertex_z = Vec::new();
        let mut data = Vec::new();

        let graded = !self.path_overhang.is_empty();
        let has_heights = !self.path_heights.is_empty();
        let has_objects = !self.path_objects.is_empty();
        let has_vertex_z = !self.path_vertex_z.is_empty();
        let has_data = !self.path_data.is_empty();

        for pick in picks {
            let Some(source) = self.paths.iter().nth(pick.index) else {
                continue;
            };
            let points: Vec<_> = source.iter().copied().collect();
            let mut path = Path::default();
            for point in reorder_vertex_values(points, pick.order) {
                path.push(point);
            }
            paths.push(path);

            roles.push(self.path_roles[pick.index]);
            widths.push(self.path_widths[pick.index]);
            vertex_widths.push(
                self.path_vertex_widths[pick.index]
                    .clone()
                    .map(|v| reorder_vertex_values(v, pick.order)),
            );
            is_open.push(self.path_is_open[pick.index]);
            if graded {
                overhang.push(self.path_overhang[pick.index]);
            }
            if has_heights {
                heights.push(self.path_heights[pick.index]);
            }
            if has_objects {
                objects.push(self.path_objects[pick.index]);
            }
            if has_vertex_z {
                vertex_z.push(
                    self.path_vertex_z[pick.index]
                        .clone()
                        .map(|v| reorder_vertex_values(v, pick.order)),
                );
            }
            if has_data {
                data.push(self.path_data[pick.index].clone());
            }
        }

        self.paths = paths;
        self.path_roles = roles;
        self.path_widths = widths;
        self.path_vertex_widths = vertex_widths;
        self.path_is_open = is_open;
        self.path_overhang = overhang;
        self.path_heights = heights;
        self.path_objects = objects;
        self.path_vertex_z = vertex_z;
        self.path_data = data;
    }

    /// Insert `additions`' paths in front of this layer's own.
    ///
    /// Used by bed adhesion, whose skirt/brim loops must print before the
    /// object. Every per-path array is carried through together; the sentinel
    /// arrays keep *this* layer's emptiness as the answer, and the inserted
    /// paths take the default — an adhesion loop is not graded for overhang,
    /// prints at the layer height, is flat, and belongs to no single object, so
    /// cancelling one part never takes the plate's skirt with it.
    pub fn prepend_paths(&mut self, mut additions: SliceLayer) {
        if additions.paths.is_empty() {
            return;
        }
        self.pad_per_path_arrays();
        additions.pad_per_path_arrays();
        let added = additions.paths.len();

        let mut paths = additions.paths;
        for path in self.paths.iter() {
            paths.push(path.clone());
        }
        self.paths = paths;

        let mut roles = additions.path_roles;
        roles.extend(self.path_roles.iter().copied());
        self.path_roles = roles;

        let mut widths = additions.path_widths;
        widths.extend(self.path_widths.iter().copied());
        self.path_widths = widths;

        let mut vertex_widths = additions.path_vertex_widths;
        vertex_widths.extend(self.path_vertex_widths.iter().cloned());
        self.path_vertex_widths = vertex_widths;

        let mut is_open = additions.path_is_open;
        is_open.extend(self.path_is_open.iter().copied());
        self.path_is_open = is_open;

        if !self.path_overhang.is_empty() {
            let mut v = vec![OverhangClass::None; added];
            v.extend(self.path_overhang.iter().copied());
            self.path_overhang = v;
        }
        if !self.path_heights.is_empty() {
            let mut v = vec![None; added];
            v.extend(self.path_heights.iter().copied());
            self.path_heights = v;
        }
        if !self.path_objects.is_empty() {
            let mut v = vec![None; added];
            v.extend(self.path_objects.iter().copied());
            self.path_objects = v;
        }
        if !self.path_vertex_z.is_empty() {
            let mut v = vec![None; added];
            v.extend(self.path_vertex_z.iter().cloned());
            self.path_vertex_z = v;
        }
        if !self.path_data.is_empty() {
            let mut v = vec![PathData::default(); added];
            v.extend(self.path_data.iter().cloned());
            self.path_data = v;
        }
    }

    /// Keep only the paths `keep` accepts, by their current index.
    ///
    /// A thin wrapper over [`SliceLayer::rebuild_paths`], and the reason to
    /// prefer it over filtering `paths` directly: it carries every parallel
    /// array through the filter with it.
    pub fn retain_paths(&mut self, mut keep: impl FnMut(usize) -> bool) {
        let picks: Vec<PathPick> = (0..self.paths.len())
            .filter(|i| keep(*i))
            .map(PathPick::keep)
            .collect();
        if picks.len() == self.paths.len() {
            return;
        }
        self.rebuild_paths(&picks);
    }
}
