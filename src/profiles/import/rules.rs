//! What every foreign setting becomes here — one table, read top to bottom.
//!
//! Each [`Rule`] names a setting as the INI family (`perimeters`, `fill_density`)
//! and the JSON family (`wall_loops`, `sparse_infill_density`) spell it, the
//! kind of preset the source keeps it in, and an [`Action`]:
//!
//! - **`Map`** — the same setting here, with a [`Conv`] for its value.
//! - **`Override`** — a filament's own value for a printer setting; it stays on
//!   the filament, which is the only place a per-spool override can live.
//! - **`Field`** — a typed profile field rather than a slice parameter.
//! - **`Group`** — worked out together with related settings by a handler in
//!   [`super::map`], because no one of them means anything alone.
//! - **`Ignore`** — recognised; changes nothing here.
//! - **`Unsupported`** — a feature this slicer doesn't have. It is reported as
//!   lost only when the file actually turns it on ([`Inactive`]) and only when
//!   it could have mattered ([`Gate`]) — an ironing pattern is no loss on a
//!   profile that never irons.
//!
//! A setting absent from the table is reported as unknown, never dropped
//! silently. Values are always read in the file's own vocabulary; the
//! conversions below are where the two vocabularies meet.

use super::read::Category::{self, Filament, Printer, Process};

/// How a value is read and what it becomes.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Conv {
    /// A number (the extruder's own element of a per-extruder list).
    Number,
    /// A whole number.
    Integer,
    /// A switch.
    Bool,
    /// One string.
    Text,
    /// A per-extruder string.
    TextList,
    /// One block of G-code — its placeholders are translated.
    Gcode,
    /// A per-extruder block of G-code.
    GcodeList,
    /// `15%` (or a bare fraction) → `0.15`.
    Fraction,
    /// `10%` → `10.0`, for settings this slicer keeps in percent.
    PercentNumber,
    /// A fan speed written 0–100 → `0.0`–`1.0`.
    FanPercent,
    /// mm/s → mm/min.
    PerMinute,
    /// Millimetres, or a percentage kept as one (`"110%"`) for the engine to
    /// resolve against its base setting at slice time.
    MmOrPercent,
    /// An overhang angle measured from the horizontal → from the vertical.
    FromVertical,
    /// A word from a fixed vocabulary.
    Choice(&'static [Choice]),
    /// mm/s, or a percentage of another of the file's settings.
    OrPercentOf(&'static str),
    /// A distance in mm → whole layers at the file's layer height.
    Layers,
    /// The gap between support lines in mm → a fill density.
    GapDensity,
    /// A percentage of the infill bead, or a length in mm worked back into one.
    AnchorPercent,
    /// A length in mm, or a percentage of the infill bead worked out into one.
    AnchorLength,
    /// Millimetres, or a percentage of the outer-wall bead worked out into mm.
    MmOrWidthPercent,
}

/// One word of a foreign vocabulary and its counterpart here.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Choice {
    pub from: &'static str,
    pub to: To,
    /// Set when the counterpart is only the nearest one: what will differ.
    pub note: Option<&'static str>,
}

/// A counterpart value.
#[derive(Debug, Clone, Copy)]
pub(crate) enum To {
    Str(&'static str),
    Bool(bool),
}

const fn exact(from: &'static str, to: &'static str) -> Choice {
    Choice {
        from,
        to: To::Str(to),
        note: None,
    }
}

const fn near(from: &'static str, to: &'static str, note: &'static str) -> Choice {
    Choice {
        from,
        to: To::Str(to),
        note: Some(note),
    }
}

const fn on(from: &'static str, value: bool) -> Choice {
    Choice {
        from,
        to: To::Bool(value),
        note: None,
    }
}

/// A typed profile field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Field {
    /// The preset's own name.
    Name,
    /// The printer model.
    PrinterModel,
    /// The printer's maker.
    PrinterVendor,
    /// The angle parts are turned to by default on this machine.
    PreferredOrientation,
    /// The filament brand.
    FilamentVendor,
    /// The filament colour.
    FilamentColor,
    /// Density in g/cm³.
    FilamentDensity,
    /// Price per kilogram.
    FilamentCost,
    /// Maximum print height.
    BedHeight,
}

/// Settings worked out together by a handler in [`super::map`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Group {
    BedShape,
    Flavor,
    Nozzle,
    Material,
    Adhesion,
    SupportStyle,
    Ironing,
    Widths,
    FanCurve,
    PlateTemps,
    Limits,
    Thumbnails,
    PressureAdvance,
    ShellThickness,
    Resolution,
    LayerGcode,
    PauseGcode,
    ChamberControl,
    WallOrder,
    FirstLayerSpeed,
    OverhangSpeeds,
    VolumetricSpeed,
    WipeTower,
    Connection,
}

/// When an unsupported setting changes nothing: the file leaves it off or at
/// the value that means "not in use".
#[derive(Debug, Clone, Copy)]
pub(crate) enum Inactive {
    /// Off, zero, empty or `none`.
    Off,
    /// One of these values (compared without case).
    Is(&'static [&'static str]),
    /// Never inactive: present means in use.
    Never,
}

/// When a setting could matter at all, given the rest of the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Gate {
    Always,
    Supports,
    TreeSupports,
    Raft,
    Brim,
    Skirt,
    Ironing,
    FuzzySkin,
    MultiExtruder,
}

impl Gate {
    /// Why a gated setting changes nothing when its gate is closed.
    pub(crate) fn reason(self) -> &'static str {
        match self {
            Gate::Always => "",
            Gate::Supports => "Supports are off, so this changes nothing.",
            Gate::TreeSupports => "Only used by tree supports, which this profile doesn't use.",
            Gate::Raft => "There is no raft, so this changes nothing.",
            Gate::Brim => "There is no brim, so this changes nothing.",
            Gate::Skirt => "There is no skirt, so this changes nothing.",
            Gate::Ironing => "Ironing is off, so this changes nothing.",
            Gate::FuzzySkin => "Fuzzy skin is off, so this changes nothing.",
            Gate::MultiExtruder => "Only matters with more than one extruder.",
        }
    }
}

/// What to do with a setting.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Action {
    Map(&'static str, Conv),
    Override(&'static str, Conv),
    Field(Field, Conv),
    Group(Group),
    Ignore(&'static str),
    Unsupported {
        /// What the feature is, as a person would name it.
        feature: &'static str,
        /// What happens instead here.
        effect: &'static str,
        inactive: Inactive,
        gate: Gate,
    },
}

/// One row of the table.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Rule {
    pub keys: &'static [&'static str],
    pub category: Category,
    pub action: Action,
}

const fn rule(keys: &'static [&'static str], category: Category, action: Action) -> Rule {
    Rule {
        keys,
        category,
        action,
    }
}

const fn map(setting: &'static str, conv: Conv) -> Action {
    Action::Map(setting, conv)
}

const fn unsupported(feature: &'static str, effect: &'static str, inactive: Inactive) -> Action {
    Action::Unsupported {
        feature,
        effect,
        inactive,
        gate: Gate::Always,
    }
}

const fn unsupported_when(
    feature: &'static str,
    effect: &'static str,
    inactive: Inactive,
    gate: Gate,
) -> Action {
    Action::Unsupported {
        feature,
        effect,
        inactive,
        gate,
    }
}

const fn gated(gate: Gate, feature: &'static str, effect: &'static str) -> Action {
    Action::Unsupported {
        feature,
        effect,
        inactive: Inactive::Never,
        gate,
    }
}

// ── Reasons shared by many ignored settings ──────────────────────────────────

const BOOKKEEPING: &str = "Bookkeeping for the source slicer's own preset list.";
const NOTES: &str = "Free-text notes aren't kept.";
const MULTI_MATERIAL: &str = "Part of multi-material printing, which only matters with \
                              several extruders or a filament changer.";
const DISPLAY_ONLY: &str = "How the source slicer draws the printer; no effect on printing.";
const GUIDANCE: &str = "Guidance the source slicer shows; no effect on printing.";
const OWN_MARKERS: &str = "Covered by the layer markers this slicer writes itself.";

// ── Vocabularies ─────────────────────────────────────────────────────────────

const WALL_GENERATORS: &[Choice] = &[exact("classic", "classic"), exact("arachne", "arachne")];

const SEAMS: &[Choice] = &[
    exact("aligned", "aligned"),
    exact("nearest", "nearest"),
    exact("random", "random"),
    exact("rear", "rear"),
    exact("back", "rear"),
    near(
        "cost",
        "aligned",
        "Seams are placed by this slicer's aligned rule rather than the source's cost weights.",
    ),
    near(
        "hidden",
        "sharpest_corner",
        "Seams are tucked into the sharpest corner, the nearest rule here to \"hidden\".",
    ),
];

const INFILL_PATTERNS: &[Choice] = &[
    exact("rectilinear", "Rectilinear"),
    exact("zig-zag", "Rectilinear"),
    exact("alignedrectilinear", "AlignedRectilinear"),
    exact("grid", "Grid"),
    exact("triangles", "Triangles"),
    exact("tri-hexagon", "TriHexagon"),
    near(
        "stars",
        "TriHexagon",
        "Stars become tri-hexagon, the same three line sets with a slightly different offset.",
    ),
    exact("cubic", "Cubic"),
    exact("honeycomb", "Honeycomb"),
    exact("concentric", "Concentric"),
    exact("gyroid", "Gyroid"),
    exact("tpmsd", "TpmsD"),
    near(
        "line",
        "Rectilinear",
        "Line infill becomes rectilinear: the same lines, joined end to end.",
    ),
    near(
        "3dhoneycomb",
        "Honeycomb",
        "3D honeycomb becomes honeycomb, which doesn't shift its cells from layer to layer.",
    ),
    near(
        "adaptivecubic",
        "Cubic",
        "Adaptive cubic becomes cubic at one density throughout, without the denser cells \
         near the surface — more material and time inside the part.",
    ),
    near(
        "supportcubic",
        "Cubic",
        "Support cubic becomes cubic at one density throughout — more material and time \
         inside the part.",
    ),
    near(
        "lightning",
        "Rectilinear",
        "Lightning infill only props up top surfaces; rectilinear at the same density fills \
         the whole inside, which takes noticeably more material and time.",
    ),
    near(
        "hilbertcurve",
        "Rectilinear",
        "The Hilbert curve becomes rectilinear lines at the same density.",
    ),
    near(
        "archimedeanchords",
        "Concentric",
        "Archimedean chords become concentric loops at the same density.",
    ),
    near(
        "octagramspiral",
        "Rectilinear",
        "The octagram spiral becomes rectilinear lines at the same density.",
    ),
    near(
        "crosshatch",
        "Grid",
        "Cross hatch becomes grid, crossing lines in every layer rather than alternating blocks.",
    ),
    near(
        "tpmsfk",
        "TpmsD",
        "TPMS-FK becomes TPMS-D, the diamond surface of the same family.",
    ),
    near(
        "quartercubic",
        "Cubic",
        "Quarter cubic becomes cubic at the same density.",
    ),
];

const SURFACE_PATTERNS: &[Choice] = &[
    exact("rectilinear", "rectilinear"),
    exact("zig-zag", "rectilinear"),
    exact("alignedrectilinear", "aligned-rectilinear"),
    exact("monotonic", "monotonic"),
    exact("monotoniclines", "monotonic-line"),
    exact("monotonicline", "monotonic-line"),
    exact("concentric", "concentric"),
    near(
        "hilbertcurve",
        "monotonic",
        "The Hilbert-curve surface becomes monotonic lines.",
    ),
    near(
        "archimedeanchords",
        "concentric",
        "Archimedean chords become concentric loops.",
    ),
    near(
        "octagramspiral",
        "monotonic",
        "The octagram-spiral surface becomes monotonic lines.",
    ),
];

const IRONING_TYPES: &[Choice] = &[
    exact("top", "top_surfaces"),
    exact("topmost", "topmost_only"),
    exact("solid", "all_solid"),
];

const BRIM_TYPES: &[Choice] = &[
    exact("outer_only", "outer_only"),
    exact("inner_only", "inner_only"),
    exact("outer_and_inner", "outer_and_inner"),
    exact("brim_ears", "ears"),
    near(
        "auto_brim",
        "outer_only",
        "The source chose a brim per object automatically; here every object gets an outer \
         brim.",
    ),
    near(
        "painted",
        "outer_only",
        "Painted brims become an outer brim around every object.",
    ),
];

const FUZZY_SKIN: &[Choice] = &[
    on("none", false),
    on("disabled_fuzzy", false),
    on("external", true),
    Choice {
        from: "all",
        to: To::Bool(true),
        note: Some(
            "Only the outer wall is fuzzed here; the source fuzzed the inner walls of holes too.",
        ),
    },
    Choice {
        from: "allwalls",
        to: To::Bool(true),
        note: Some("Only the outer wall is fuzzed here; the source fuzzed every wall."),
    },
];

const VERTICAL_SHELLS: &[Choice] = &[
    on("ensure_all", true),
    Choice {
        from: "ensure_moderate",
        to: To::Bool(true),
        note: Some("Vertical shells are thickened wherever needed, not only in moderate cases."),
    },
    Choice {
        from: "ensure_critical_only",
        to: To::Bool(true),
        note: Some("Vertical shells are thickened wherever needed, not only in critical areas."),
    },
    on("none", false),
];

const PRINT_SEQUENCE: &[Choice] = &[
    exact("0", "by_layer"),
    exact("1", "by_object"),
    exact("by layer", "by_layer"),
    exact("by object", "by_object"),
];

const LABEL_OBJECTS: &[Choice] = &[
    on("firmware", true),
    on("1", true),
    on("disabled", false),
    on("0", false),
    Choice {
        from: "octoprint",
        to: To::Bool(true),
        note: Some(
            "Objects are labelled with firmware commands (M486 or EXCLUDE_OBJECT); the \
             OctoPrint-style comment labels aren't written.",
        ),
    },
];

const TOP_ONE_WALL: &[Choice] = &[
    on("none", false),
    on("not apply", false),
    on("all top", true),
    on("top", true),
    Choice {
        from: "topmost",
        to: To::Bool(true),
        note: Some("Every top surface gets the single wall, not only the topmost one."),
    },
    Choice {
        from: "topmost only",
        to: To::Bool(true),
        note: Some("Every top surface gets the single wall, not only the topmost one."),
    },
];

// ── The table ────────────────────────────────────────────────────────────────

/// Every setting the importer recognises.
pub(crate) const RULES: &[Rule] = &[
    // ── Identity ─────────────────────────────────────────────────────────────
    rule(&["print_settings_id"], Process, Action::Field(Field::Name, Conv::Text)),
    rule(&["filament_settings_id"], Filament, Action::Field(Field::Name, Conv::TextList)),
    rule(&["printer_settings_id"], Printer, Action::Field(Field::Name, Conv::Text)),
    rule(&["printer_model"], Printer, Action::Field(Field::PrinterModel, Conv::Text)),
    rule(&["printer_vendor"], Printer, Action::Field(Field::PrinterVendor, Conv::Text)),
    rule(
        &["preferred_orientation"],
        Printer,
        Action::Field(Field::PreferredOrientation, Conv::Number),
    ),
    rule(&["filament_vendor"], Filament, Action::Field(Field::FilamentVendor, Conv::TextList)),
    rule(&["filament_colour"], Filament, Action::Field(Field::FilamentColor, Conv::TextList)),
    rule(&["filament_density"], Filament, Action::Field(Field::FilamentDensity, Conv::Number)),
    rule(&["filament_cost"], Filament, Action::Field(Field::FilamentCost, Conv::Number)),
    rule(&["filament_type"], Filament, Action::Group(Group::Material)),
    rule(
        &[
            "name", "inherits", "from", "type", "version", "setting_id", "filament_id",
            "instantiation", "is_custom_defined", "print_settings_id_parent", "base_id",
            "compatible_printers", "compatible_printers_condition", "compatible_prints",
            "compatible_prints_condition", "printer_variant", "print_compatible_printers",
            "default_print_profile", "default_filament_profile",
            "printer_settings_url", "filament_settings_url", "print_settings_url",
            "printer_technology", "printer_structure", "printer_agent", "upward_compatible_machine",
            "default_bed_type", "renamed_from", "different_settings_to_system",
            "inherits_group", "filament_ids", "physical_printer_settings_id",
            "nozzle_volume_type", "printer_extruder_id", "printer_extruder_variant",
            "extruder_variant_list", "filament_extruder_variant", "print_extruder_id",
            "print_extruder_variant", "extruder_type", "nozzle_type",
        ],
        Printer,
        Action::Ignore(BOOKKEEPING),
    ),
    rule(
        &["notes", "printer_notes", "filament_notes", "print_notes"],
        Printer,
        Action::Ignore(NOTES),
    ),
    // ── Printer: bed, nozzle, firmware ───────────────────────────────────────
    rule(&["bed_shape", "printable_area"], Printer, Action::Group(Group::BedShape)),
    rule(
        &["max_print_height", "printable_height"],
        Printer,
        Action::Field(Field::BedHeight, Conv::Number),
    ),
    rule(
        &[
            "bed_custom_texture", "bed_custom_model", "bed_model", "bed_texture",
            "thumbnails_custom_color", "extruder_colour", "printer_color", "bed_temperature_formula",
            "best_object_pos", "head_wrap_detect_zone", "scan_first_layer", "auxiliary_fan",
            "support_air_filtration", "nozzle_hrc",
            "machine_load_filament_time", "machine_unload_filament_time", "machine_tool_change_time",
            "printer_volume", "bed_mesh_min", "bed_mesh_max", "bed_mesh_probe_distance",
            "adaptive_bed_mesh_margin", "fan_direction", "use_firmware_retraction_values",
        ],
        Printer,
        Action::Ignore(DISPLAY_ONLY),
    ),
    rule(&["bed_exclude_area"], Printer, unsupported(
        "Excluded bed areas",
        "Parts can be placed anywhere on the bed; keep them clear of the excluded areas yourself.",
        Inactive::Is(&["", "0x0"]),
    )),
    rule(&["nozzle_diameter"], Printer, Action::Group(Group::Nozzle)),
    rule(&["support_chamber_temp_control"], Printer, map("heated_chamber", Conv::Bool)),
    rule(&["gcode_flavor"], Printer, Action::Group(Group::Flavor)),
    rule(&["z_offset"], Printer, map("z_offset_mm", Conv::Number)),
    rule(&["silent_mode"], Printer, Action::Ignore(
        "Selects the source's quiet-mode machine limits; the normal-mode limits are imported.",
    )),
    rule(&["variable_layer_height"], Printer, Action::Ignore(
        "Says whether the printer allows variable layer height, which this slicer doesn't do yet.",
    )),
    rule(&["min_layer_height", "max_layer_height"], Printer, Action::Ignore(
        "Limits for variable layer height, which this slicer doesn't do yet.",
    )),
    rule(&["extruder_offset", "extruder_clearance_height_to_lid"], Printer, Action::Ignore(
        "Only matters with more than one extruder.",
    )),
    rule(
        &["extruder_clearance_height", "extruder_clearance_height_to_rod"],
        Process,
        map("extruder_clearance_height_mm", Conv::Number),
    ),
    rule(&["extruder_clearance_radius"], Process, map("extruder_clearance_radius_mm", Conv::Number)),
    rule(
        &[
            "machine_limits_usage", "emit_machine_limits_to_gcode",
            "machine_max_acceleration_x", "machine_max_acceleration_y",
            "machine_max_acceleration_z", "machine_max_acceleration_e",
            "machine_max_acceleration_extruding", "machine_max_acceleration_retracting",
            "machine_max_acceleration_travel", "machine_max_feedrate_x", "machine_max_feedrate_y",
            "machine_max_feedrate_z", "machine_max_feedrate_e", "machine_max_speed_x",
            "machine_max_speed_y", "machine_max_speed_z", "machine_max_speed_e",
            "machine_max_jerk_x", "machine_max_jerk_y", "machine_max_jerk_z", "machine_max_jerk_e",
            "machine_min_extruding_rate", "machine_min_travel_rate", "machine_max_junction_deviation",
        ],
        Printer,
        Action::Group(Group::Limits),
    ),
    rule(&["thumbnails", "thumbnails_format", "thumbnail_size"], Printer, Action::Group(Group::Thumbnails)),
    rule(
        &["host_type", "print_host", "printhost_apikey", "printhost_cafile", "print_host_webui",
          "printhost_port", "printhost_authorization_type", "printhost_user", "printhost_password",
          "printhost_ssl_ignore_revoke"],
        Printer,
        Action::Group(Group::Connection),
    ),
    // ── Printer: custom G-code ───────────────────────────────────────────────
    rule(&["start_gcode", "machine_start_gcode"], Printer, map("start_gcode", Conv::Gcode)),
    rule(&["end_gcode", "machine_end_gcode"], Printer, map("end_gcode", Conv::Gcode)),
    rule(
        &["layer_gcode", "layer_change_gcode", "before_layer_gcode", "before_layer_change_gcode"],
        Printer,
        Action::Group(Group::LayerGcode),
    ),
    rule(
        &["between_objects_gcode", "printing_by_object_gcode"],
        Printer,
        map("between_objects_gcode", Conv::Gcode),
    ),
    rule(
        &["color_change_gcode", "pause_print_gcode", "machine_pause_gcode", "template_custom_gcode"],
        Printer,
        Action::Group(Group::PauseGcode),
    ),
    rule(
        &["toolchange_gcode", "change_filament_gcode", "change_extrusion_role_gcode",
          "filament_change_gcode"],
        Printer,
        unsupported_when(
            "Tool-change G-code",
            "Multi-material printing isn't supported yet, so nothing runs on a tool change.",
            Inactive::Off,
            Gate::MultiExtruder,
        ),
    ),
    rule(&["time_lapse_gcode", "timelapse_gcode"], Printer, unsupported(
        "Timelapse G-code",
        "Nothing is inserted for timelapse photos; set timelapse up on the printer instead.",
        Inactive::Off,
    )),
    rule(&["wrapping_detection_gcode", "wipe_tower_type"], Printer, Action::Ignore(BOOKKEEPING)),
    // ── Printer: retraction ──────────────────────────────────────────────────
    rule(&["retract_length", "retraction_length"], Printer, map("retract_mm", Conv::Number)),
    rule(&["retract_speed", "retraction_speed"], Printer, map("retract_speed_mm_min", Conv::PerMinute)),
    rule(&["deretract_speed", "deretraction_speed"], Printer, unsupported(
        "A separate un-retract speed",
        "Un-retraction runs at the retraction speed.",
        Inactive::Is(&["0"]),
    )),
    rule(&["retract_restart_extra"], Printer, map("retract_restart_extra_mm", Conv::Number)),
    rule(
        &["retract_before_travel", "retraction_minimum_travel"],
        Printer,
        map("retract_before_travel_mm", Conv::Number),
    ),
    rule(
        &["retract_layer_change", "retract_when_changing_layer"],
        Printer,
        map("retract_on_layer_change", Conv::Bool),
    ),
    rule(&["retract_lift", "z_hop"], Printer, map("z_hop_mm", Conv::Number)),
    rule(&["retract_lift_above", "retract_lift_below"], Printer, unsupported(
        "Height-limited Z-hop",
        "Z-hop applies at every height.",
        Inactive::Is(&["0"]),
    )),
    rule(&["retract_lift_top", "retract_lift_enforce"], Printer, unsupported(
        "Surface-limited Z-hop",
        "Z-hop applies over every surface.",
        Inactive::Is(&["All surfaces", "All Surfaces"]),
    )),
    rule(&["z_hop_types"], Printer, unsupported(
        "Slope, spiral and automatic Z-hop",
        "Z-hop is a straight lift.",
        Inactive::Is(&["Normal Lift"]),
    )),
    rule(
        &["travel_ramping_lift", "travel_max_lift", "travel_slope", "travel_lift_before_obstacle"],
        Printer,
        unsupported("Ramping lift", "Travel moves lift straight up.", Inactive::Is(&["0", "0%"])),
    ),
    rule(&["retract_before_wipe"], Printer, map("retract_before_wipe_percent", Conv::Fraction)),
    rule(&["wipe"], Printer, map("wipe", Conv::Bool)),
    rule(&["wipe_distance"], Printer, map("wipe_distance_mm", Conv::Number)),
    rule(&["use_firmware_retraction"], Printer, map("use_firmware_retraction", Conv::Bool)),
    rule(&["use_relative_e_distances"], Printer, map("use_relative_e_distances", Conv::Bool)),
    rule(
        &["retract_length_toolchange", "retract_restart_extra_toolchange",
          "retraction_length_toolchange", "long_retractions_when_cut",
          "retraction_distances_when_cut", "enable_long_retraction_when_cut"],
        Printer,
        Action::Ignore("Only used when changing tools or cutting filament."),
    ),
    // ── Printer: multi-material hardware ─────────────────────────────────────
    rule(
        &["single_extruder_multi_material", "single_extruder_multi_material_priming",
          "high_current_on_filament_swap", "extra_loading_move", "purge_in_prime_tower",
          "enable_filament_ramming", "manual_filament_change", "standby_temperature_delta",
          "ooze_prevention", "interface_shells", "wipe_tower_extruder", "bed_temperature_difference"],
        Printer,
        Action::Ignore(MULTI_MATERIAL),
    ),
    // ── Process: layers ──────────────────────────────────────────────────────
    rule(&["layer_height"], Process, map("layer_height", Conv::Number)),
    rule(
        &["first_layer_height", "initial_layer_print_height"],
        Process,
        map("first_layer_height", Conv::MmOrPercent),
    ),
    rule(&["adaptive_layer_height"], Process, unsupported(
        "Adaptive layer height",
        "Every layer is printed at the base layer height.",
        Inactive::Off,
    )),
    // ── Process: walls ───────────────────────────────────────────────────────
    rule(&["perimeters", "wall_loops"], Process, map("wall_count", Conv::Integer)),
    rule(
        &["perimeter_generator", "wall_generator"],
        Process,
        map("wall_generator", Conv::Choice(WALL_GENERATORS)),
    ),
    rule(&["wall_transition_angle"], Process, map("wall_transition_angle", Conv::Number)),
    rule(&["wall_distribution_count"], Process, map("wall_distribution_count", Conv::Integer)),
    rule(&["min_bead_width"], Process, map("wall_line_width_min", Conv::Fraction)),
    rule(
        &["wall_transition_length", "wall_transition_filter_deviation", "min_feature_size"],
        Process,
        unsupported(
            "Fine-tuning for the source's variable-width walls",
            "This slicer's wall generator uses its own transition settings.",
            Inactive::Is(&["100%", "25%", "0.4", "0.1"]),
        ),
    ),
    rule(&["thin_walls", "detect_thin_wall"], Process, map("thin_walls", Conv::Bool)),
    rule(&["seam_position"], Process, map("seam_position", Conv::Choice(SEAMS))),
    rule(&["seam_gap", "seam_gap_mm"], Process, unsupported(
        "Seam gap",
        "Wall loops are closed without shortening them.",
        Inactive::Is(&["0", "0%", "10%", "15%"]),
    )),
    rule(&["staggered_inner_seams"], Process, unsupported(
        "Staggered inner seams",
        "Inner-wall seams line up with the outer one.",
        Inactive::Off,
    )),
    rule(&["seam_slope_type"], Process, unsupported(
        "Scarf seams",
        "Seams are ordinary butt joints rather than a sloped scarf.",
        Inactive::Is(&["none"]),
    )),
    rule(&["external_perimeters_first", "wall_sequence", "wall_infill_order", "is_infill_first",
           "infill_first"],
         Process, Action::Group(Group::WallOrder)),
    rule(&["extra_perimeters"], Process, map("extra_perimeters", Conv::Bool)),
    rule(&["extra_perimeters_on_overhangs"], Process, unsupported(
        "Extra perimeters on overhangs",
        "Overhangs get the normal number of walls.",
        Inactive::Off,
    )),
    rule(
        &["ensure_vertical_shell_thickness"],
        Process,
        map("ensure_vertical_shell_thickness", Conv::Choice(VERTICAL_SHELLS)),
    ),
    rule(
        &["avoid_crossing_perimeters", "reduce_crossing_wall"],
        Process,
        map("avoid_crossing_perimeters", Conv::Bool),
    ),
    rule(
        &["avoid_crossing_perimeters_max_detour", "max_travel_detour_distance"],
        Process,
        unsupported("Detour limit", "Travel detours aren't capped.", Inactive::Is(&["0", "0%"])),
    ),
    rule(&["avoid_crossing_curled_overhangs"], Process, unsupported(
        "Avoiding curled overhangs while travelling",
        "Travel moves don't steer around curled overhangs.",
        Inactive::Off,
    )),
    rule(&["spiral_vase", "spiral_mode"], Process, map("spiral_vase", Conv::Bool)),
    rule(&["spiral_mode_smooth", "spiral_mode_max_xy_smoothing", "spiral_starting_flow_ratio",
           "spiral_finishing_flow_ratio"], Process, unsupported(
        "Smoothed spiral vase",
        "The spiral follows the sliced outline without extra smoothing.",
        Inactive::Is(&["0", "0%", "200%"]),
    )),
    rule(&["fuzzy_skin"], Process, map("fuzzy_skin", Conv::Choice(FUZZY_SKIN))),
    rule(&["fuzzy_skin_thickness"], Process, map("fuzzy_skin_thickness_mm", Conv::Number)),
    rule(
        &["fuzzy_skin_point_dist", "fuzzy_skin_point_distance"],
        Process,
        map("fuzzy_skin_point_dist_mm", Conv::Number),
    ),
    rule(
        &["fuzzy_skin_first_layer", "fuzzy_skin_noise_type", "fuzzy_skin_mode", "fuzzy_skin_scale",
          "fuzzy_skin_octaves", "fuzzy_skin_persistence"],
        Process,
        unsupported_when(
            "Fuzzy-skin noise options",
            "Fuzzy skin uses plain random displacement on every layer but the first.",
            Inactive::Is(&["0", "classic", "displacement", "1", "4", "0.5"]),
            Gate::FuzzySkin,
        ),
    ),
    rule(
        &["only_one_wall_top", "top_one_perimeter_type", "top_one_wall_type", "only_one_perimeter_top"],
        Process,
        map("only_one_wall_top", Conv::Choice(TOP_ONE_WALL)),
    ),
    rule(
        &["only_one_wall_first_layer", "only_one_perimeter_first_layer"],
        Process,
        map("only_one_wall_first_layer", Conv::Bool),
    ),
    rule(&["precise_outer_wall", "precise_z_height"], Process, unsupported(
        "Precise wall and Z-height compensation",
        "Walls and layer heights are printed as sliced.",
        Inactive::Off,
    )),
    rule(&["thick_bridges", "thick_internal_bridges"], Process, unsupported(
        "Thick bridges",
        "Bridges use the bridge flow ratio at the normal layer height.",
        Inactive::Off,
    )),
    rule(&["overhangs", "detect_overhang_wall"], Process, Action::Ignore(
        "Overhanging walls are always detected here.",
    )),
    rule(
        &["enable_dynamic_overhang_speeds", "enable_overhang_speed", "overhang_speed_0",
          "overhang_speed_1", "overhang_speed_2", "overhang_speed_3", "overhang_1_4_speed",
          "overhang_2_4_speed", "overhang_3_4_speed", "overhang_4_4_speed"],
        Process,
        Action::Group(Group::OverhangSpeeds),
    ),
    rule(
        &["slowdown_for_curled_perimeters", "slow_down_for_curled_perimeters"],
        Process,
        map("slowdown_for_curled_perimeters", Conv::Bool),
    ),
    rule(&["overhang_speed_classic"], Process, Action::Ignore(
        "Selects the source's older overhang classifier; overhang speeds are graded by degree here.",
    )),
    // ── Process: infill ──────────────────────────────────────────────────────
    rule(&["fill_density", "sparse_infill_density"], Process, map("infill_density", Conv::Fraction)),
    rule(
        &["fill_pattern", "sparse_infill_pattern"],
        Process,
        map("infill_pattern", Conv::Choice(INFILL_PATTERNS)),
    ),
    rule(
        &["top_fill_pattern", "top_surface_pattern"],
        Process,
        map("top_surface_pattern", Conv::Choice(SURFACE_PATTERNS)),
    ),
    rule(
        &["bottom_fill_pattern", "bottom_surface_pattern"],
        Process,
        map("bottom_surface_pattern", Conv::Choice(SURFACE_PATTERNS)),
    ),
    rule(
        &["solid_fill_pattern", "internal_solid_infill_pattern"],
        Process,
        map("internal_solid_infill_pattern", Conv::Choice(SURFACE_PATTERNS)),
    ),
    rule(&["fill_angle", "infill_direction"], Process, map("infill_base_angle", Conv::Number)),
    rule(&["solid_infill_direction"], Process, map("surface_infill_angle", Conv::Number)),
    rule(&["bridge_angle"], Process, map("bridge_angle", Conv::Number)),
    rule(&["infill_anchor", "sparse_infill_anchor"], Process, map("infill_anchor_percent", Conv::AnchorPercent)),
    rule(&["infill_anchor_max", "sparse_infill_anchor_max"], Process, map("infill_anchor_max_mm", Conv::AnchorLength)),
    rule(&["infill_every_layers"], Process, map("infill_every_layers", Conv::Integer)),
    rule(&["solid_infill_every_layers"], Process, map("solid_infill_every_layers", Conv::Integer)),
    rule(&["infill_combination"], Process, unsupported(
        "Automatic infill combination",
        "Infill is printed every layer unless \"infill every N layers\" says otherwise.",
        Inactive::Off,
    )),
    rule(
        &["infill_combination_max_layer_height"],
        Process,
        Action::Ignore("Only used by automatic infill combination."),
    ),
    rule(&["infill_overlap", "top_bottom_infill_wall_overlap"], Process, map("infill_overlap_percent", Conv::Fraction)),
    rule(&["infill_wall_overlap"], Process, unsupported(
        "Sparse-infill overlap with the walls",
        "Sparse infill stops just short of the inner wall instead of overlapping it.",
        Inactive::Is(&["0", "0%"]),
    )),
    rule(&["infill_only_where_needed"], Process, unsupported(
        "Infill only where needed",
        "Infill fills the whole inside of the part.",
        Inactive::Off,
    )),
    rule(&["solid_infill_below_area", "minimum_sparse_infill_area"], Process, unsupported(
        "Solid infill for small areas",
        "Small areas get sparse infill like the rest of the part.",
        Inactive::Is(&["0"]),
    )),
    rule(&["filter_out_gap_fill"], Process, map("gap_fill_min_length_mm", Conv::Number)),
    rule(&["gap_fill_target", "gap_fill_enabled"], Process, unsupported(
        "Restricting gap fill",
        "Gaps are filled everywhere they occur.",
        Inactive::Is(&["everywhere", "1"]),
    )),
    rule(&["align_infill_direction_to_model", "fill_multiline", "infill_shift_step",
           "infill_rotate_step", "symmetric_infill_y_axis", "infill_lock_depth", "skin_infill_depth",
           "skin_infill_density", "skeleton_infill_density", "skin_infill_line_width",
           "skeleton_infill_line_width", "lateral_lattice_angle_1", "lateral_lattice_angle_2",
           "infill_overhang_angle", "sparse_infill_rotate_template", "solid_infill_rotate_template"],
         Process, unsupported(
        "Infill fine-tuning",
        "Infill keeps one direction scheme and single lines.",
        Inactive::Is(&["0", "1", "0%", "", "45", "-45", "30%", "25%", "60"]),
    )),
    rule(&["bridge_flow_ratio", "bridge_flow"], Process, map("bridge_flow_ratio", Conv::Number)),
    rule(&["internal_bridge_flow", "internal_bridge_speed"], Process, unsupported(
        "Separate internal-bridge flow and speed",
        "Bridges inside the part use the same flow and speed as visible ones.",
        Inactive::Is(&["1", "150%", "100%"]),
    )),
    rule(
        &["top_solid_infill_flow_ratio", "bottom_solid_infill_flow_ratio", "print_flow_ratio",
          "fill_top_flow_ratio", "first_layer_flow_ratio"],
        Process,
        unsupported(
            "Per-feature flow ratios",
            "Every extrusion uses the filament's flow ratio.",
            Inactive::Is(&["1", "100%"]),
        ),
    ),
    // ── Process: surfaces ────────────────────────────────────────────────────
    rule(
        &["top_solid_layers", "top_shell_layers", "bottom_solid_layers", "bottom_shell_layers",
          "top_solid_min_thickness", "bottom_solid_min_thickness", "top_shell_thickness",
          "bottom_shell_thickness"],
        Process,
        Action::Group(Group::ShellThickness),
    ),
    rule(&["ironing", "ironing_type"], Process, Action::Group(Group::Ironing)),
    rule(&["ironing_flowrate", "ironing_flow"], Process, map("ironing_flow", Conv::PercentNumber)),
    rule(&["ironing_spacing"], Process, map("ironing_spacing", Conv::Number)),
    rule(&["ironing_speed"], Process, map("ironing_speed", Conv::Number)),
    rule(&["ironing_angle", "ironing_direction"], Process, map("ironing_angle", Conv::Number)),
    rule(&["ironing_pattern", "ironing_inset", "ironing_angle_fixed"], Process, unsupported_when(
        "Ironing pattern options",
        "Ironing runs back and forth across the whole surface.",
        Inactive::Is(&["zig-zag", "rectilinear", "0", "0%"]),
        Gate::Ironing,
    )),
    // ── Process: speeds ──────────────────────────────────────────────────────
    rule(&["perimeter_speed"], Process, map("inner_wall_speed", Conv::Number)),
    rule(&["inner_wall_speed"], Process, map("inner_wall_speed", Conv::Number)),
    rule(
        &["external_perimeter_speed"],
        Process,
        map("perimeter_speed", Conv::OrPercentOf("perimeter_speed")),
    ),
    rule(&["outer_wall_speed"], Process, map("perimeter_speed", Conv::Number)),
    rule(&["infill_speed", "sparse_infill_speed"], Process, map("infill_speed", Conv::Number)),
    rule(&["solid_infill_speed"], Process, map("solid_infill_speed", Conv::OrPercentOf("infill_speed"))),
    rule(&["internal_solid_infill_speed"], Process, map("solid_infill_speed", Conv::Number)),
    rule(
        &["top_solid_infill_speed"],
        Process,
        map("top_surface_speed", Conv::OrPercentOf("solid_infill_speed")),
    ),
    rule(&["top_surface_speed"], Process, map("top_surface_speed", Conv::Number)),
    rule(&["bridge_speed"], Process, map("bridge_speed", Conv::Number)),
    rule(&["gap_fill_speed", "gap_infill_speed"], Process, map("gap_fill_speed", Conv::Number)),
    rule(&["support_material_speed", "support_speed"], Process, map("support_speed", Conv::Number)),
    rule(
        &["support_material_interface_speed", "support_interface_speed"],
        Process,
        unsupported_when(
            "A separate support-interface speed",
            "Support interfaces print at the support speed.",
            Inactive::Is(&["100%"]),
            Gate::Supports,
        ),
    ),
    rule(&["small_perimeter_speed", "small_perimeter_threshold"], Process, unsupported(
        "Small-perimeter speed",
        "Small holes and details print at the normal wall speed.",
        Inactive::Is(&["0", "100%"]),
    )),
    rule(
        &["first_layer_speed", "initial_layer_speed", "initial_layer_infill_speed"],
        Process,
        Action::Group(Group::FirstLayerSpeed),
    ),
    rule(&["first_layer_speed_over_raft"], Process, gated(
        Gate::Raft,
        "First-layer speed over a raft",
        "The first layer on a raft prints at the normal first-layer speed.",
    )),
    rule(&["travel_speed"], Process, map("travel_speed_mm_min", Conv::PerMinute)),
    rule(&["travel_speed_z"], Process, unsupported(
        "Z travel speed",
        "Z moves run at this slicer's fixed Z feedrate.",
        Inactive::Is(&["0"]),
    )),
    rule(&["max_print_speed", "autospeed"], Process, Action::Ignore(
        "Only used to pick speeds automatically where the source left them at zero.",
    )),
    rule(&["max_volumetric_speed", "filament_max_volumetric_speed"], Filament, Action::Group(Group::VolumetricSpeed)),
    rule(&["max_volumetric_extrusion_rate_slope", "max_volumetric_extrusion_rate_slope_segment_length",
           "extrusion_rate_smoothing_external_perimeter_only"], Process, unsupported(
        "Pressure equalizer",
        "Extrusion rate changes are not smoothed between moves.",
        Inactive::Is(&["0"]),
    )),
    rule(&["dont_slow_down_outer_wall"], Filament, unsupported(
        "Keeping the outer wall at full speed while cooling",
        "Layer-time slowdown slows every move, the outer wall included.",
        Inactive::Off,
    )),
    rule(&["wipe_speed", "role_based_wipe_speed"], Printer, unsupported(
        "A separate wipe speed",
        "Wiping moves at the travel speed.",
        Inactive::Is(&["80%", "1", "0"]),
    )),
    // ── Process: acceleration ────────────────────────────────────────────────
    rule(&["default_acceleration"], Process, map("acceleration", Conv::Number)),
    rule(&["perimeter_acceleration", "inner_wall_acceleration"], Process, map("inner_wall_acceleration", Conv::Number)),
    rule(
        &["external_perimeter_acceleration", "outer_wall_acceleration"],
        Process,
        map("outer_wall_acceleration", Conv::Number),
    ),
    rule(&["infill_acceleration"], Process, map("sparse_infill_acceleration", Conv::Number)),
    rule(
        &["sparse_infill_acceleration"],
        Process,
        map("sparse_infill_acceleration", Conv::OrPercentOf("default_acceleration")),
    ),
    rule(&["solid_infill_acceleration"], Process, map("solid_infill_acceleration", Conv::Number)),
    rule(
        &["internal_solid_infill_acceleration"],
        Process,
        map("solid_infill_acceleration", Conv::OrPercentOf("default_acceleration")),
    ),
    rule(
        &["top_solid_infill_acceleration", "top_surface_acceleration"],
        Process,
        map("top_surface_acceleration", Conv::Number),
    ),
    rule(&["bridge_acceleration"], Process, map("bridge_acceleration", Conv::Number)),
    rule(
        &["first_layer_acceleration", "initial_layer_acceleration"],
        Process,
        map("first_layer_acceleration", Conv::Number),
    ),
    rule(&["first_layer_acceleration_over_raft"], Process, gated(
        Gate::Raft,
        "First-layer acceleration over a raft",
        "The first layer on a raft uses the normal first-layer acceleration.",
    )),
    rule(&["travel_acceleration"], Process, map("travel_acceleration", Conv::Number)),
    rule(&["accel_to_decel_enable", "accel_to_decel_factor"], Process, unsupported(
        "Klipper's accel-to-decel limit",
        "The firmware's own acceleration-to-deceleration setting applies.",
        Inactive::Is(&["0", "50%"]),
    )),
    rule(&["default_jerk"], Process, map("square_corner_velocity", Conv::Number)),
    rule(
        &["outer_wall_jerk", "inner_wall_jerk", "infill_jerk", "top_surface_jerk",
          "initial_layer_jerk", "travel_jerk", "default_junction_deviation"],
        Process,
        unsupported(
            "Per-feature jerk",
            "One cornering limit, the square-corner velocity, applies to every move.",
            Inactive::Is(&["0"]),
        ),
    ),
    // ── Process: cooling and temperatures (filament) ─────────────────────────
    rule(
        &["fan_always_on", "cooling", "min_fan_speed", "fan_min_speed", "fan_below_layer_time",
          "fan_cooling_layer_time", "max_fan_speed", "fan_max_speed", "slowdown_below_layer_time",
          "slow_down_layer_time", "slow_down_for_layer_cooling"],
        Filament,
        Action::Group(Group::FanCurve),
    ),
    rule(&["bridge_fan_speed"], Filament, map("bridge_fan_speed", Conv::FanPercent)),
    rule(&["overhang_fan_speed"], Filament, map("overhang_fan_speed", Conv::FanPercent)),
    rule(&["overhang_fan_threshold"], Filament, map("overhang_fan_threshold", Conv::Fraction)),
    rule(&["enable_overhang_bridge_fan"], Filament, unsupported(
        "Switching the overhang fan off",
        "Overhangs and bridges always get their extra cooling.",
        Inactive::Is(&["1"]),
    )),
    rule(
        &["disable_fan_first_layers", "close_fan_the_first_x_layers"],
        Filament,
        map("disable_fan_first_layers", Conv::Integer),
    ),
    rule(&["full_fan_speed_layer"], Filament, unsupported(
        "Ramping the fan up over the first layers",
        "The fan goes from off to its normal speed at once.",
        Inactive::Is(&["0", "1"]),
    )),

    rule(&["min_print_speed", "slow_down_min_speed"], Filament, map("min_print_speed", Conv::Number)),

    rule(
        &["enable_dynamic_fan_speeds", "overhang_fan_speed_0", "overhang_fan_speed_1",
          "overhang_fan_speed_2", "overhang_fan_speed_3"],
        Filament,
        unsupported(
            "Fan speed graded by overhang",
            "Overhangs past the threshold get the one overhang fan speed.",
            Inactive::Off,
        ),
    ),
    rule(
        &["reduce_fan_stop_start_freq", "fan_kickstart", "fan_speedup_time",
          "fan_speedup_overhangs"],
        Printer,
        unsupported(
            "Fan start-up tuning",
            "Fan speed changes are sent as they happen.",
            Inactive::Off,
        ),
    ),
    rule(&["additional_cooling_fan_speed", "aux_fan_speed"], Filament, unsupported(
        "Auxiliary fan speed",
        "The auxiliary fan isn't set by this filament; configure it under the printer's fans.",
        Inactive::Is(&["0"]),
    )),
    rule(&["temperature", "nozzle_temperature"], Filament, map("nozzle_temp", Conv::Number)),
    rule(
        &["first_layer_temperature", "nozzle_temperature_initial_layer"],
        Filament,
        map("nozzle_temp_first_layer", Conv::Number),
    ),
    rule(&["bed_temperature"], Filament, map("bed_temp", Conv::Number)),
    rule(&["first_layer_bed_temperature"], Filament, map("bed_temp_first_layer", Conv::Number)),
    rule(
        &["hot_plate_temp", "hot_plate_temp_initial_layer", "cool_plate_temp",
          "cool_plate_temp_initial_layer", "eng_plate_temp", "eng_plate_temp_initial_layer",
          "textured_plate_temp", "textured_plate_temp_initial_layer", "supertack_plate_temp",
          "supertack_plate_temp_initial_layer", "textured_cool_plate_temp",
          "textured_cool_plate_temp_initial_layer", "curr_bed_type"],
        Filament,
        Action::Group(Group::PlateTemps),
    ),
    rule(
        &["chamber_temperature", "chamber_temperatures", "activate_chamber_temp_control"],
        Filament,
        Action::Group(Group::ChamberControl),
    ),
    rule(
        &["chamber_temperature_initial_layer"],
        Filament,
        map("chamber_temp_first_layer", Conv::Number),
    ),
    rule(&["chamber_minimal_temperature"], Filament, unsupported(
        "Minimum chamber temperature",
        "The print starts once the chamber target is reached; there is no separate minimum.",
        Inactive::Is(&["0"]),
    )),
    rule(
        &["idle_temperature", "filament_idle_temperature"],
        Filament,
        unsupported_when(
            "Idle temperature",
            "A parked tool keeps its print temperature.",
            Inactive::Is(&["0", "nil"]),
            Gate::MultiExtruder,
        ),
    ),
    rule(
        &["temperature_vitrification", "nozzle_temperature_range_low",
          "nozzle_temperature_range_high", "required_nozzle_HRC", "filament_minimal_purge_on_wipe_tower"],
        Filament,
        Action::Ignore(GUIDANCE),
    ),
    // ── Filament: material ───────────────────────────────────────────────────
    rule(&["filament_diameter"], Filament, map("filament_diameter_mm", Conv::Number)),
    rule(&["extrusion_multiplier", "filament_flow_ratio"], Filament, map("flow_ratio", Conv::Number)),
    rule(
        &["enable_pressure_advance", "pressure_advance", "adaptive_pressure_advance",
          "adaptive_pressure_advance_model", "adaptive_pressure_advance_overhangs",
          "adaptive_pressure_advance_bridges"],
        Filament,
        Action::Group(Group::PressureAdvance),
    ),
    rule(
        &["start_filament_gcode", "filament_start_gcode"],
        Filament,
        map("start_filament_gcode", Conv::GcodeList),
    ),
    rule(
        &["end_filament_gcode", "filament_end_gcode"],
        Filament,
        map("end_filament_gcode", Conv::GcodeList),
    ),
    rule(
        &["filament_shrink", "filament_shrinkage_compensation_xy", "filament_shrinkage_compensation_z",
          "filament_shrinkage_compensation"],
        Filament,
        unsupported(
            "Shrinkage compensation",
            "Parts print at their modelled size; scale them up yourself if this material shrinks.",
            Inactive::Is(&["100%", "0%", "0"]),
        ),
    ),
    rule(
        &["filament_spool_weight", "filament_soluble", "filament_is_support", "filament_abrasive",
          "filament_printable", "filament_scarf_seam_type", "filament_scarf_height",
          "filament_scarf_gap", "filament_scarf_length", "filament_adaptive_volumetric_speed",
          "filament_max_volumetric_speed_coef"],
        Filament,
        Action::Ignore("Describes the spool or its use with several materials; no effect on a single-material print."),
    ),
    // Retraction a filament overrides for itself. `nil` means "not overridden".
    rule(&["filament_retract_length", "filament_retraction_length"], Filament,
         Action::Override("retract_mm", Conv::Number)),
    rule(&["filament_retract_speed", "filament_retraction_speed"], Filament,
         Action::Override("retract_speed_mm_min", Conv::PerMinute)),
    rule(&["filament_retract_restart_extra"], Filament,
         Action::Override("retract_restart_extra_mm", Conv::Number)),
    rule(&["filament_retract_before_travel", "filament_retraction_minimum_travel"], Filament,
         Action::Override("retract_before_travel_mm", Conv::Number)),
    rule(&["filament_retract_layer_change", "filament_retract_when_changing_layer"], Filament,
         Action::Override("retract_on_layer_change", Conv::Bool)),
    rule(&["filament_retract_lift", "filament_z_hop"], Filament,
         Action::Override("z_hop_mm", Conv::Number)),
    rule(&["filament_wipe"], Filament, Action::Override("wipe", Conv::Bool)),
    rule(&["filament_wipe_distance"], Filament, Action::Override("wipe_distance_mm", Conv::Number)),
    rule(&["filament_retract_before_wipe"], Filament,
         Action::Override("retract_before_wipe_percent", Conv::Fraction)),
    rule(
        &["filament_deretract_speed", "filament_deretraction_speed", "filament_retract_lift_above",
          "filament_retract_lift_below", "filament_retract_lift_enforce", "filament_retract_lift_top",
          "filament_z_hop_types", "filament_travel_ramping_lift", "filament_travel_max_lift",
          "filament_travel_slope", "filament_travel_lift_before_obstacle",
          "filament_long_retractions_when_cut", "filament_retraction_distances_when_cut",
          "filament_retract_length_toolchange", "filament_retract_restart_extra_toolchange"],
        Filament,
        unsupported(
            "A filament's own Z-hop limits and un-retract speed",
            "The printer's Z-hop and retraction behaviour applies to this filament.",
            Inactive::Is(&["nil", ""]),
        ),
    ),
    // ── Process: supports ────────────────────────────────────────────────────
    rule(&["support_material", "enable_support"], Process, map("support_enabled", Conv::Bool)),
    rule(&["support_material_auto"], Process, map("support_auto", Conv::Bool)),
    rule(
        &["support_type", "support_material_style", "support_style"],
        Process,
        Action::Group(Group::SupportStyle),
    ),
    rule(
        &["support_material_threshold", "support_threshold_angle"],
        Process,
        map("support_threshold_angle", Conv::FromVertical),
    ),
    rule(
        &["support_material_buildplate_only", "support_on_build_plate_only"],
        Process,
        map("support_on_build_plate_only", Conv::Bool),
    ),
    rule(
        &["support_material_spacing", "support_base_pattern_spacing"],
        Process,
        map("support_density", Conv::GapDensity),
    ),
    rule(
        &["support_material_interface_layers", "support_interface_top_layers"],
        Process,
        map("support_interface_layers", Conv::Integer),
    ),
    rule(
        &["support_material_bottom_interface_layers", "support_interface_bottom_layers"],
        Process,
        unsupported_when(
            "A separate bottom-interface layer count",
            "Bottom interfaces use the same number of layers as the top ones.",
            Inactive::Is(&["-1"]),
            Gate::Supports,
        ),
    ),
    rule(
        &["support_material_interface_spacing", "support_interface_spacing"],
        Process,
        map("support_interface_density", Conv::GapDensity),
    ),
    rule(
        &["support_material_contact_distance", "support_top_z_distance"],
        Process,
        map("support_z_gap_layers", Conv::Layers),
    ),
    rule(
        &["support_material_bottom_contact_distance", "support_bottom_z_distance"],
        Process,
        unsupported_when(
            "A separate bottom contact distance",
            "Support resting on the model uses the top contact distance.",
            Inactive::Is(&["0"]),
            Gate::Supports,
        ),
    ),
    rule(
        &["support_material_xy_spacing", "support_object_xy_distance"],
        Process,
        map("support_xy_distance_mm", Conv::MmOrWidthPercent),
    ),
    rule(
        &["support_material_angle", "support_angle", "support_material_pattern", "support_base_pattern",
          "support_material_interface_pattern", "support_interface_pattern",
          "support_material_with_sheath", "support_material_interface_contact_loops",
          "support_interface_loop_pattern", "support_expansion", "support_material_closing_radius",
          "support_closing_radius", "support_material_enforce_layers", "enforce_support_layers",
          "support_critical_regions_only", "support_remove_small_overhang", "support_ironing",
          "support_ironing_pattern", "support_ironing_flow", "support_ironing_spacing",
          "support_object_first_layer_gap", "support_interface_not_for_body",
          "raft_first_layer_density", "raft_first_layer_expansion"],
        Process,
        unsupported_when(
            "Support pattern options",
            "Support is built from this slicer's own column pattern and interface layout.",
            Inactive::Is(&["0", "default", "rectilinear", "rectilinear-grid", "auto", "2", "1", "",
                            "zig-zag", "10%", "100%"]),
            Gate::Supports,
        ),
    ),
    rule(&["dont_support_bridges", "bridge_no_support"], Process, unsupported_when(
        "Leaving bridges unsupported",
        "Bridges get support like any other overhang steeper than the threshold.",
        Inactive::Off,
        Gate::Supports,
    )),
    rule(&["support_material_synchronize_layers", "independent_support_layer_height"], Process,
         Action::Ignore("Only matters with soluble supports printed by a second extruder.")),
    rule(&["support_threshold_overlap", "max_bridge_length", "support_filament",
           "support_interface_filament", "support_material_extruder",
           "support_material_interface_extruder"], Process, Action::Ignore(
        "Only matters with several extruders or the source's overhang classifier.",
    )),
    rule(
        &["support_tree_angle", "tree_support_branch_angle", "tree_support_branch_angle_organic"],
        Process,
        map("support_tree_branch_angle", Conv::Number),
    ),
    rule(
        &["support_tree_angle_slow", "tree_support_angle_slow"],
        Process,
        map("support_tree_preferred_angle", Conv::Number),
    ),
    rule(
        &["support_tree_tip_diameter", "tree_support_tip_diameter"],
        Process,
        map("support_tree_tip_diameter", Conv::Number),
    ),
    rule(
        &["support_tree_branch_diameter", "tree_support_branch_diameter",
          "tree_support_branch_diameter_organic"],
        Process,
        map("support_tree_branch_diameter", Conv::Number),
    ),
    rule(
        &["support_tree_branch_diameter_angle", "tree_support_branch_diameter_angle"],
        Process,
        map("support_tree_branch_diameter_angle", Conv::Number),
    ),
    rule(
        &["support_tree_branch_diameter_double_wall", "support_tree_branch_distance",
          "support_tree_top_rate", "tree_support_branch_distance",
          "tree_support_branch_distance_organic", "tree_support_top_rate", "tree_support_wall_count",
          "tree_support_adaptive_layer_height", "tree_support_auto_brim", "tree_support_brim_width",
          "tree_support_with_infill", "tree_support_angle_slow_organic"],
        Process,
        unsupported_when(
            "Tree branch spacing and walls",
            "Trees use this slicer's own branch spacing and wall count.",
            Inactive::Never,
            Gate::TreeSupports,
        ),
    ),
    // ── Process: adhesion ────────────────────────────────────────────────────
    rule(
        &["skirts", "skirt_loops", "brim_width", "raft_layers", "brim_type"],
        Process,
        Action::Group(Group::Adhesion),
    ),
    rule(&["skirt_distance"], Process, map("skirt_distance", Conv::Number)),
    rule(&["skirt_height"], Process, map("skirt_height", Conv::Integer)),
    rule(&["skirt_type", "skirt_speed", "skirt_start_angle", "min_skirt_length", "draft_shield"],
         Process, unsupported_when(
        "Skirt options",
        "The skirt is one combined loop set around everything, as long as its loops make it.",
        Inactive::Is(&["combined", "0", "disabled", "-135"]),
        Gate::Skirt,
    )),
    rule(&["brim_separation", "brim_object_gap"], Process, map("brim_separation", Conv::Number)),
    rule(&["brim_ears_max_angle", "brim_ears_detection_length", "brim_use_efc_outline"], Process,
         unsupported_when(
        "Brim-ear fine-tuning",
        "Brim ears are placed by this slicer's own corner detection.",
        Inactive::Never,
        Gate::Brim,
    )),
    rule(&["raft_contact_distance"], Process, map("raft_air_gap", Conv::Number)),
    rule(&["raft_expansion"], Process, unsupported_when(
        "Raft expansion",
        "The raft follows the part's outline without growing past it.",
        Inactive::Is(&["0"]),
        Gate::Raft,
    )),
    // ── Process: dimensional accuracy ────────────────────────────────────────
    rule(
        &["xy_size_compensation", "xy_contour_compensation"],
        Process,
        map("xy_size_compensation", Conv::Number),
    ),
    rule(&["xy_hole_compensation"], Process, map("xy_hole_compensation", Conv::Number)),
    rule(&["elefant_foot_compensation"], Process, map("elephant_foot_compensation_mm", Conv::Number)),
    rule(&["elefant_foot_compensation_layers"], Process, map("elephant_foot_layers", Conv::Integer)),
    rule(&["hole_to_polyhole", "hole_to_polyhole_threshold", "hole_to_polyhole_twisted"], Process,
         unsupported("Polyholes", "Round holes stay round.", Inactive::Off)),
    rule(&["slice_closing_radius"], Process, unsupported(
        "Gap-closing radius",
        "Small gaps in the mesh are closed by this slicer's own repair.",
        Inactive::Is(&["0.049", "0.05", "0"]),
    )),
    rule(&["slicing_mode"], Process, unsupported(
        "Even-odd and hole-closing slicing",
        "Every closed outline is sliced as a solid with its holes.",
        Inactive::Is(&["regular"]),
    )),
    rule(&["resolution", "gcode_resolution"], Process, Action::Group(Group::Resolution)),
    rule(&["arc_fitting", "enable_arc_fitting"], Process, unsupported(
        "Arc fitting",
        "Curves are written as many short straight moves rather than G2/G3 arcs — a larger \
         file, the same shape.",
        Inactive::Is(&["0", "disabled", "false"]),
    )),
    // ── Process: objects and output ──────────────────────────────────────────
    rule(&["complete_objects", "print_sequence"], Process, map("print_sequence", Conv::Choice(PRINT_SEQUENCE))),
    rule(
        &["gcode_label_objects", "exclude_object", "label_printed_objects"],
        Process,
        map("exclude_object", Conv::Choice(LABEL_OBJECTS)),
    ),
    rule(&["gcode_comments", "gcode_add_line_number", "output_filename_format", "filename_format",
           "gcode_flavor_comment", "disable_m73", "timelapse_type", "post_process_scripts_output"],
         Process, Action::Ignore("Affects how the file is written or named, not what prints.")),
    rule(&["reduce_infill_retraction"], Process, Action::Ignore(
        "Travel that stays inside the part already skips the retraction here.",
    )),
    rule(&["wipe_on_loops", "wipe_before_external_loop"], Process, unsupported(
        "Extra wipes around wall loops",
        "Wiping happens on retraction only, along the path just printed.",
        Inactive::Off,
    )),
    rule(&["automatic_extrusion_widths"], Process, Action::Ignore(
        "Widths left at zero already follow the nozzle diameter here.",
    )),
    rule(&["prefer_clockwise_movements", "wall_direction"], Process, unsupported(
        "A fixed wall direction",
        "Walls are printed in whichever direction this slicer's path order picks.",
        Inactive::Is(&["0", "auto"]),
    )),
    rule(
        &["make_overhang_printable", "make_overhang_printable_angle",
          "make_overhang_printable_hole_size"],
        Process,
        unsupported(
            "Making overhangs printable",
            "The model is sliced as it is, without reshaping steep overhangs into cones.",
            Inactive::Is(&["0", "55", "0%"]),
        ),
    ),
    rule(
        &["dont_filter_internal_bridges", "counterbore_hole_bridging", "bridge_density",
          "internal_bridge_density"],
        Process,
        unsupported(
            "Bridge detection options",
            "Bridges are detected and filled with this slicer's own rules.",
            Inactive::Is(&["0", "disabled", "none", "100%"]),
        ),
    ),
    rule(&["pellet_modded_printer", "pellet_flow_coefficient"], Printer, unsupported(
        "Pellet extruders",
        "Extrusion is computed for filament of the stated diameter.",
        Inactive::Off,
    )),
    rule(&["binary_gcode"], Printer, unsupported(
        "Binary G-code",
        "Files are written as plain-text G-code, which is larger but readable everywhere.",
        Inactive::Off,
    )),
    rule(&["remaining_times"], Printer, unsupported(
        "Remaining-time updates (M73)",
        "The printer's display estimates the remaining time itself.",
        Inactive::Off,
    )),
    rule(&["post_process"], Process, unsupported(
        "Post-processing scripts",
        "Scripts are never run on the output. Apply them yourself if you still need them.",
        Inactive::Off,
    )),
    rule(&["gcode_substitutions"], Process, unsupported(
        "G-code substitutions",
        "The output is written as generated, without find-and-replace.",
        Inactive::Off,
    )),
    rule(&["autoemit_temperature_commands"], Printer, Action::Ignore(
        "Checked against the start G-code instead: heating it relied on is added there.",
    )),
    // ── Line widths ──────────────────────────────────────────────────────────
    rule(
        &["extrusion_width", "line_width", "perimeter_extrusion_width", "inner_wall_line_width",
          "external_perimeter_extrusion_width", "outer_wall_line_width", "infill_extrusion_width",
          "sparse_infill_line_width", "solid_infill_extrusion_width",
          "internal_solid_infill_line_width", "top_infill_extrusion_width", "top_surface_line_width",
          "support_material_extrusion_width", "support_line_width", "first_layer_extrusion_width",
          "initial_layer_line_width"],
        Process,
        Action::Group(Group::Widths),
    ),
    // ── Multi-material and wipe tower ────────────────────────────────────────
    rule(
        &["wipe_tower", "enable_prime_tower", "prime_tower_width", "prime_tower_brim_width",
          "prime_volume", "wipe_tower_x", "wipe_tower_y", "wipe_tower_width",
          "wipe_tower_rotation_angle", "wipe_tower_brim_width", "wipe_tower_bridging",
          "wipe_tower_no_sparse_layers", "wipe_tower_extra_spacing", "wipe_tower_cone_angle",
          "wipe_tower_extra_flow", "wipe_tower_acceleration", "wiping_volumes_matrix",
          "wiping_volumes_extruders", "flush_volumes_matrix", "flush_volumes_vector",
          "flush_multiplier", "flush_into_infill", "flush_into_objects", "flush_into_support",
          "wipe_into_infill", "wipe_into_objects", "mmu_segmented_region_max_width",
          "mmu_segmented_region_interlocking_depth", "interlocking_beam", "interlocking_depth",
          "interlocking_orientation", "interlocking_beam_layer_count", "interlocking_beam_width",
          "interlocking_boundary_avoidance"],
        Process,
        Action::Group(Group::WipeTower),
    ),
    rule(
        &["perimeter_extruder", "infill_extruder", "solid_infill_extruder", "wall_filament",
          "sparse_infill_filament", "solid_infill_filament", "extruder", "first_layer_extruder"],
        Process,
        Action::Ignore("Chooses an extruder per feature; only matters with several extruders."),
    ),
];

/// Families of settings named by a shared prefix, checked when no exact key
/// matches.
pub(crate) const PREFIX_RULES: &[(&str, Category, Action)] = &[
    ("seam_slope_", Process, Action::Ignore("Only used by scarf seams.")),
    ("wipe_tower_", Process, Action::Group(Group::WipeTower)),
    ("prime_tower_", Process, Action::Group(Group::WipeTower)),
    ("flush_", Process, Action::Group(Group::WipeTower)),
    ("mmu_", Process, Action::Ignore(MULTI_MATERIAL)),
    ("filament_ramming", Filament, Action::Ignore(MULTI_MATERIAL)),
    ("filament_multitool_ramming", Filament, Action::Ignore(MULTI_MATERIAL)),
    ("filament_cooling_", Filament, Action::Ignore(MULTI_MATERIAL)),
    ("filament_load", Filament, Action::Ignore(MULTI_MATERIAL)),
    ("filament_unload", Filament, Action::Ignore(MULTI_MATERIAL)),
    ("filament_toolchange", Filament, Action::Ignore(MULTI_MATERIAL)),
    ("filament_stamping_", Filament, Action::Ignore(MULTI_MATERIAL)),
    ("filament_purge_", Filament, Action::Ignore(MULTI_MATERIAL)),
    ("filament_minimal_purge", Filament, Action::Ignore(MULTI_MATERIAL)),
    ("cooling_tube_", Printer, Action::Ignore(MULTI_MATERIAL)),
    ("parking_pos_", Printer, Action::Ignore(MULTI_MATERIAL)),
    ("small_area_infill_flow_compensation", Process, Action::Ignore(
        "Fine-tuning for very small infill areas; this slicer meters them with the normal flow.",
    )),
    ("bed_mesh_", Printer, Action::Ignore(
        "The source's adaptive-mesh bookkeeping; choose bed meshing under the printer's hardware settings.",
    )),
    ("thumbnails_", Printer, Action::Group(Group::Thumbnails)),
    ("printhost_", Printer, Action::Group(Group::Connection)),
    ("print_host_", Printer, Action::Group(Group::Connection)),
    ("overhang_fan_speed_", Filament, Action::Ignore(
        "Fan speed graded by overhang isn't supported; overhangs get the one overhang fan speed.",
    )),
];

/// The rule for `key`, if any — exact name first, then prefix family.
pub(crate) fn lookup(key: &str) -> Option<(Category, Action)> {
    RULES
        .iter()
        .find(|rule| rule.keys.contains(&key))
        .map(|rule| (rule.category, rule.action))
        .or_else(|| {
            PREFIX_RULES
                .iter()
                .find(|(prefix, _, _)| key.starts_with(prefix))
                .map(|(_, category, action)| (*category, *action))
        })
}

/// The foreign kind of preset a key belongs to, defaulting to the print profile
/// for keys no rule knows.
pub(crate) fn category_of(key: &str) -> Category {
    lookup(key).map(|(c, _)| c).unwrap_or(Process)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::params::SlicingParams;

    /// Every key appears in exactly one rule, or the first match would silently
    /// shadow the second.
    #[test]
    fn no_foreign_key_is_claimed_twice() {
        let mut seen = std::collections::HashSet::new();
        for rule in RULES {
            for key in rule.keys {
                assert!(
                    seen.insert(*key),
                    "`{key}` is claimed by more than one rule"
                );
            }
        }
    }

    /// A mapping to a setting that doesn't exist would write a key the engine
    /// ignores — an import that looks successful and changes nothing.
    #[test]
    fn every_mapped_setting_is_a_real_slicing_parameter() {
        let params = serde_json::to_value(SlicingParams::default()).unwrap();
        let known = params.as_object().unwrap();
        // Text settings that serialize as `null` when unset.
        let nullable = [
            "start_gcode",
            "end_gcode",
            "layer_gcode",
            "start_filament_gcode",
            "end_filament_gcode",
            "between_objects_gcode",
        ];
        for rule in RULES {
            if let Action::Map(setting, _) | Action::Override(setting, _) = rule.action {
                assert!(
                    known.contains_key(setting) || nullable.contains(&setting),
                    "{:?} maps to unknown setting `{setting}`",
                    rule.keys
                );
            }
        }
    }

    #[test]
    fn lookup_falls_back_to_prefix_families() {
        assert!(matches!(
            lookup("wipe_tower_x"),
            Some((_, Action::Group(Group::WipeTower)))
        ));
        assert!(matches!(
            lookup("seam_slope_steps"),
            Some((_, Action::Ignore(_)))
        ));
        assert!(lookup("definitely_not_a_setting").is_none());
    }

    #[test]
    fn every_unsupported_effect_reads_as_a_sentence() {
        for rule in RULES {
            if let Action::Unsupported { effect, .. } = rule.action {
                assert!(effect.ends_with('.'), "{:?}: {effect}", rule.keys);
            }
        }
    }
}
