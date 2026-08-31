/// Extrusion role for a GCode move.
///
/// Derived from `;TYPE:` comment lines emitted by our slicer and by
/// OrcaSlicer-compatible slicers.
///
/// Role ID mapping (used by the TypeScript viewer):
/// - 0  OuterWall
/// - 1  InnerWall
/// - 2  Infill
/// - 3  TopSurface
/// - 4  BottomSurface
/// - 5  Travel
/// - 6  Other
/// - 7  Bridge
/// - 8  Skirt
/// - 9  Support
/// - 10 Seam  (synthetic — point marker at the outer-wall seam/start)
/// - 11 OverhangPerimeter
/// - 12 GapFill
/// - 13 SolidInfill
/// - 14 SupportInterface
/// - 15 Brim
/// - 16 PrimeTower
/// - 17 InternalBridge
/// - 18 Ironing
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Role {
    OuterWall,
    InnerWall,
    Infill,
    TopSurface,
    BottomSurface,
    Travel,
    Other,
    /// Bridge extrusion spanning an unsupported gap.
    Bridge,
    /// Skirt line printed around the model.
    Skirt,
    /// Support structure material.
    Support,
    /// Support interface / roof / floor material.
    SupportInterface,
    /// Synthetic point-marker at the seam (start/end) of each outer-wall loop.
    /// Stored as a degenerate zero-length segment so the viewer can render it
    /// as a white dot without special-casing the block data format.
    Seam,
    /// Outer wall that is printed as an overhang perimeter.
    OverhangPerimeter,
    /// Variable-width gap infill.
    GapFill,
    /// Dense internal solid infill.
    SolidInfill,
    /// Brim adhesion lines around the part.
    Brim,
    /// Prime / wipe tower extrusion.
    PrimeTower,
    /// Internal bridge infill.
    InternalBridge,
    /// Near-dry smoothing pass over a finished top surface.
    Ironing,
}

impl Role {
    pub(super) fn from_type_comment(s: &str) -> Self {
        let lower = s.to_ascii_lowercase();
        // Ahead of the "top" test below: ironing sweeps a top surface but is a
        // role of its own, and its label shares no substring with one.
        if lower.contains("ironing") {
            return Self::Ironing;
        }
        if lower.contains("internal bridge") {
            return Self::InternalBridge;
        }
        // Check bridge / overhang before any "bottom" or "outer" test so
        // "Bridge" isn't confused with "Bottom surface", and "Overhang wall"
        // isn't confused with a normal outer/inner wall.
        if lower == "bridge" || lower.contains("bridge infill") {
            return Self::Bridge;
        }
        // Match OrcaSlicer's exact `;TYPE:Overhang wall` so generic strings
        // like "non-overhang" or "overhang setting" cannot accidentally
        // promote a normal perimeter to bridge colouring.
        if lower == "overhang wall" || lower == "overhang perimeter" {
            return Self::OverhangPerimeter;
        }
        if lower.contains("gap infill") || lower.contains("gap fill") {
            return Self::GapFill;
        }
        if lower.contains("support interface")
            || lower.contains("support roof")
            || lower.contains("support floor")
        {
            return Self::SupportInterface;
        }
        if lower.contains("prime tower") || lower.contains("wipe tower") {
            return Self::PrimeTower;
        }
        if lower.contains("brim") && !lower.contains("skirt") {
            return Self::Brim;
        }
        if lower.contains("internal solid infill") {
            return Self::SolidInfill;
        }
        if lower.contains("skirt") || lower.contains("brim") {
            return Self::Skirt;
        }
        if lower.contains("support") {
            return Self::Support;
        }
        if lower.contains("outer") || lower.contains("perimeter") && !lower.contains("inner") {
            return Self::OuterWall;
        }
        if lower.contains("inner") || lower.contains("inner perimeter") {
            return Self::InnerWall;
        }
        if lower.contains("top") {
            return Self::TopSurface;
        }
        if lower.contains("bottom") {
            return Self::BottomSurface;
        }
        if lower.contains("solid infill") {
            return Self::SolidInfill;
        }
        if lower.contains("infill") || lower.contains("sparse") {
            return Self::Infill;
        }
        Self::Other
    }

    pub(super) fn id(self) -> u8 {
        match self {
            Role::OuterWall => 0,
            Role::InnerWall => 1,
            Role::Infill => 2,
            Role::TopSurface => 3,
            Role::BottomSurface => 4,
            Role::Travel => 5,
            Role::Other => 6,
            Role::Bridge => 7,
            Role::Skirt => 8,
            Role::Support => 9,
            Role::Seam => 10,
            Role::OverhangPerimeter => 11,
            Role::GapFill => 12,
            Role::SolidInfill => 13,
            Role::SupportInterface => 14,
            Role::Brim => 15,
            Role::PrimeTower => 16,
            Role::InternalBridge => 17,
            // Appended, never inserted: this ordinal is the wire format the
            // TypeScript viewer decodes, so renumbering would recolour every
            // previously-rendered role.
            Role::Ironing => 18,
        }
    }
}

// ── Internal layer representation ──────────────────────────────────────────

#[derive(Debug, Clone)]
pub(super) struct Block {
    pub(super) role: Role,
    pub(super) data: Vec<f32>,
}

/// One layer's geometry, composed of sequential segment blocks to preserve timeline order.
#[derive(Debug, Default)]
pub(super) struct InternalLayer {
    pub(super) z: f32,
    pub(super) blocks: Vec<Block>,
    /// Per-layer machine state (fan speeds, nozzle temp, active tool, layer
    /// time) captured for the non-geometric "Color by" view modes.
    pub(super) meta: LayerMeta,
}

/// Sticky machine state active during a layer, snapshotted for the viewer's
/// per-layer color channels (fan / temperature / tool / layer time).
#[derive(Debug, Clone, Default)]
pub(super) struct LayerMeta {
    /// Nozzle target temperature in °C, captured from temperature commands
    /// (`M104`/`M109`, `SET_HEATER_TEMPERATURE`) and common start-macro
    /// key/value args (`EXTRUDER_TEMP=`, `EXTRUDER=`, `HOTEND=`...).
    pub(super) nozzle_temp: Option<f32>,
    /// Active tool/extruder index (`T0` by default).
    pub(super) tool: u32,
    /// Layer print time in seconds, if a `;LAYER_TIME:` marker was seen.
    pub(super) layer_time_s: Option<f32>,
    /// Per-fan speeds active on this layer, in first-seen order.
    pub(super) fans: Vec<FanSample>,
}

/// One fan's speed on a layer. `key` is a stable correlation id across layers
/// (`"P0"`, `"P2"`, or a Klipper fan name); `speed` is a `0.0..=1.0` fraction.
#[derive(Debug, Clone)]
pub(super) struct FanSample {
    pub(super) key: String,
    pub(super) speed: f32,
}

impl InternalLayer {
    pub(super) fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }

    pub(super) fn new(z: f32) -> Self {
        Self {
            z,
            blocks: Vec::new(),
            meta: LayerMeta::default(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn push_segment(
        &mut self,
        role: Role,
        x0: f32,
        y0: f32,
        z0: f32,
        x1: f32,
        y1: f32,
        z1: f32,
        width: f32,
        height: f32,
        speed: f32,
        accel: f32,
    ) {
        let segment_data = [x0, y0, z0, x1, y1, z1, width, height, speed, accel];
        if let Some(last) = self.blocks.last_mut() {
            if last.role == role {
                last.data.extend_from_slice(&segment_data);
                return;
            }
        }
        self.blocks.push(Block {
            role,
            data: segment_data.to_vec(),
        });
    }
}
