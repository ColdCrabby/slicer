use clipper2::Paths;

use crate::core::ExtrusionRole;

/// Identifies which pipeline stage a captured geometry snapshot belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DebugStage {
    /// Raw mesh cross-section contours produced by `slice_mesh`, before any
    /// further processing.
    RawContours,
    /// The Clipper2 EvenOdd-union-normalised contours fed into the wall
    /// generator — the input after winding is resolved but before any beads
    /// are placed.
    WallNormalisedInput,
    /// One inward-offset (`shrink`) intermediate produced while placing bead
    /// number `bead_k` (0-based, outermost first).
    WallOffsetStep { bead_k: usize },
    /// Final wall bead centerline paths after the full bead algorithm.
    WallBeads,
    /// Interior region (inside the innermost wall) for infill/surface placement.
    InteriorRegion,
    /// Solid top/bottom surface regions (`layer.solid_regions`).
    SolidSurface,
    /// Sparse infill, top-surface, and bottom-surface fill paths.
    Infill,
    /// Support island contours and fill strands (`ExtrusionRole::Support`).
    Support,
}

impl DebugStage {
    /// Short ASCII identifier used in OBJ group names and SVG element ids.
    pub fn id(&self) -> String {
        match self {
            Self::RawContours => "raw_contours".to_string(),
            Self::WallNormalisedInput => "wall_norm_input".to_string(),
            Self::WallOffsetStep { bead_k } => format!("wall_offset_k{}", bead_k),
            Self::WallBeads => "wall_beads".to_string(),
            Self::InteriorRegion => "interior_region".to_string(),
            Self::SolidSurface => "solid_surface".to_string(),
            Self::Infill => "infill".to_string(),
            Self::Support => "support".to_string(),
        }
    }

    /// Human-readable label for legend annotations.
    pub fn label(&self) -> String {
        match self {
            Self::RawContours => "Raw contours".to_string(),
            Self::WallNormalisedInput => "Wall normalised input".to_string(),
            Self::WallOffsetStep { bead_k } => {
                format!("Wall offset (bead {})", bead_k)
            }
            Self::WallBeads => "Wall beads".to_string(),
            Self::InteriorRegion => "Interior region".to_string(),
            Self::SolidSurface => "Solid surface".to_string(),
            Self::Infill => "Infill".to_string(),
            Self::Support => "Support".to_string(),
        }
    }

    /// SVG stroke color for this stage.
    pub fn svg_color(&self) -> &'static str {
        match self {
            Self::RawContours => "#888888",
            Self::WallNormalisedInput => "#ff8800",
            Self::WallOffsetStep { .. } => "#ffdd00",
            Self::WallBeads => "#2266ff",
            Self::InteriorRegion => "#22bb44",
            Self::SolidSurface => "#cc22cc",
            Self::Infill => "#ee2222",
            Self::Support => "#00a3a3",
        }
    }

    /// OBJ material name for this stage.
    pub fn mtl_name(&self) -> String {
        format!("mat_{}", self.id())
    }
}

/// A single geometry snapshot captured at one pipeline stage for one layer.
pub struct DebugPath {
    /// The pipeline stage this snapshot was taken from.
    pub stage: DebugStage,
    /// Zero-based index of the layer within the full layer stack.
    pub layer_index: usize,
    /// Z coordinate of the layer in mm.
    pub z: f64,
    /// The captured polygon paths (Clipper2 `Paths`).
    pub paths: Paths,
    /// Optional extrusion role annotation (for bead paths).
    pub role: Option<ExtrusionRole>,
}

/// Accumulates all debug geometry snapshots for a single pipeline run.
///
/// Passed (mutably) through the debug variants of the pipeline functions.
/// At the end of the run, call `write_svgs` to flush to disk.
#[derive(Default)]
pub struct DebugGeometry {
    pub records: Vec<DebugPath>,
}

impl DebugGeometry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Push a snapshot for a specific layer.
    pub fn push(&mut self, stage: DebugStage, layer_index: usize, z: f64, paths: Paths) {
        if paths.is_empty() {
            return;
        }
        self.records.push(DebugPath {
            stage,
            layer_index,
            z,
            paths,
            role: None,
        });
    }

    /// Push a snapshot with an extrusion role annotation.
    pub fn push_with_role(
        &mut self,
        stage: DebugStage,
        layer_index: usize,
        z: f64,
        paths: Paths,
        role: ExtrusionRole,
    ) {
        if paths.is_empty() {
            return;
        }
        self.records.push(DebugPath {
            stage,
            layer_index,
            z,
            paths,
            role: Some(role),
        });
    }

    /// Total number of captured records.
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// True when no snapshots have been captured yet.
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}
