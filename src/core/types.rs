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
    /// Variable-width gap fill: thin-wall medial beads laid into spaces too
    /// narrow for a full perimeter. Emitted as OrcaSlicer `;TYPE:Gap infill`.
    GapFill,
    /// Support structure material.
    Support,
    /// Skirt or brim line.
    Skirt,
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
            Self::GapFill => "Gap infill",
            Self::Support => "Support material",
            Self::Skirt => "Skirt",
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
            | Self::BottomSurface => 0.4,
            Self::GapFill => 0.4,
            Self::Support => 0.4,
            Self::Skirt => 0.4,
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
}
