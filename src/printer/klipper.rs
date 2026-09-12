//! Reading a Klipper machine out of its own configuration.
//!
//! Moonraker will hand us a parsed copy of the user's `printer.cfg`, and almost
//! everything the setup wizard used to ask for is already in it. This module is
//! the one place that turns those responses into a [`PrinterDetection`]: pure
//! functions over `serde_json::Value`, no transport, no I/O — which is what
//! lets the browser build reach the same conclusions through wasm instead of
//! maintaining a second copy of these rules in TypeScript.
//!
//! # Facts apply, preferences ask
//!
//! A value the config states outright — bed volume, nozzle, filament diameter,
//! the machine's own velocity and acceleration limits — is a **fact**. It lands
//! in [`PrinterDetection::params`] with a [`DetectionFinding`] recording where
//! it came from, and the user is never asked about it.
//!
//! A value that is a **choice** — which start-macro convention to target, what
//! an auxiliary fan is for, whether to re-probe the bed every print — becomes a
//! [`DetectionQuestion`]. Every question ships a `suggested` answer that is
//! applied up front, so the profile is finished and usable before the user has
//! answered anything; the questions only refine it.
//!
//! # Probe stages
//!
//! The absorb methods mirror the order the transport probes in, cheapest and
//! most load-bearing first, so a host whose heavy `configfile` query times out
//! still yields a nearly complete profile:
//!
//! 1. [`KlipperProbe::absorb_info`] — `/printer/info`
//! 2. [`KlipperProbe::absorb_objects`] — `/printer/objects/query?toolhead`
//! 3. [`KlipperProbe::absorb_object_list`] — `/printer/objects/list`
//! 4. [`KlipperProbe::absorb_objects`] — `/printer/objects/query?configfile`
//!
//! Derivation runs once, in [`KlipperProbe::into_detection`], because the
//! conclusions cross stages: identifying the machine needs the kinematics from
//! the config *and* the bed span from the toolhead.

use serde_json::{json, Map, Value};

use super::detection::{DetectionFinding, DetectionOption, DetectionQuestion, PrinterDetection};
use crate::profiles::printer::{BedShape, PrinterConnectionKind};

/// Round a millimetre reading to 0.1 mm.
///
/// Axis limits are floats that routinely carry a trailing `0.00000001`; a bed
/// reported as `350.00000000000006` mm should read `350`.
fn mm(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

/// Accumulates what the successive Moonraker probes report.
///
/// Each `absorb_*` is independently optional — a probe that failed simply
/// contributes nothing, and [`Self::into_detection`] derives whatever the
/// remaining evidence supports.
#[derive(Debug, Default)]
pub struct KlipperProbe {
    hostname: Option<String>,
    /// `[x, y, z, e]` reachable limits from the `toolhead` object.
    axis_minimum: Vec<f64>,
    axis_maximum: Vec<f64>,
    /// Parsed config sections, keyed by lowercase section name
    /// (`extruder`, `gcode_macro print_start`, `fan_generic rscs`, …).
    settings: Map<String, Value>,
    /// Registered Klipper object names, lowercased. Answers *presence*
    /// questions without downloading the whole config.
    objects: Vec<String>,
    /// Saved bed mesh profile names, when `bed_mesh` was queried.
    mesh_profiles: Vec<String>,
}

impl KlipperProbe {
    /// Absorb the `result` of `/printer/info`.
    pub fn absorb_info(&mut self, result: &Value) {
        self.hostname = non_empty(result["hostname"].as_str());
    }

    /// Absorb the `result.status` of any `/printer/objects/query?…`.
    ///
    /// Takes whichever of `toolhead`, `configfile` and `bed_mesh` the payload
    /// happens to carry, so one method serves every query stage.
    pub fn absorb_objects(&mut self, status: &Value) {
        if let Some(axes) = status["toolhead"]["axis_minimum"].as_array() {
            self.axis_minimum = axes.iter().filter_map(Value::as_f64).collect();
        }
        if let Some(axes) = status["toolhead"]["axis_maximum"].as_array() {
            self.axis_maximum = axes.iter().filter_map(Value::as_f64).collect();
        }
        if let Some(settings) = status["configfile"]["settings"].as_object() {
            for (section, body) in settings {
                self.settings
                    .insert(section.to_ascii_lowercase(), body.clone());
            }
            // Config sections are also the most complete object list there is.
            self.absorb_names(settings.keys().map(String::as_str));
        }
        if let Some(profiles) = status["bed_mesh"]["profiles"].as_object() {
            self.mesh_profiles = profiles.keys().cloned().collect();
            self.mesh_profiles.sort();
        }
    }

    /// Absorb the `result` of `/printer/objects/list`.
    pub fn absorb_object_list(&mut self, result: &Value) {
        if let Some(objects) = result["objects"].as_array() {
            self.absorb_names(objects.iter().filter_map(Value::as_str));
        }
    }

    fn absorb_names<'a>(&mut self, names: impl Iterator<Item = &'a str>) {
        for name in names {
            let lowered = name.to_ascii_lowercase();
            if !self.objects.contains(&lowered) {
                self.objects.push(lowered);
            }
        }
    }

    /// True when a section or object of exactly this (lowercase) name exists.
    fn has(&self, name: &str) -> bool {
        self.objects.iter().any(|object| object == name)
    }

    /// Names of every section whose first word is `prefix`, e.g. every
    /// `fan_generic <name>`. Returns the `<name>` part, original case lost.
    fn named(&self, prefix: &str) -> Vec<&str> {
        let lead = format!("{prefix} ");
        self.objects
            .iter()
            .filter_map(|object| object.strip_prefix(&lead))
            .filter(|name| !name.is_empty())
            .collect()
    }

    /// A scalar out of a config section, e.g. `setting("extruder",
    /// "nozzle_diameter")`. `None` when the section or key is absent, or the
    /// value is not a positive, finite number.
    fn positive(&self, section: &str, key: &str) -> Option<f64> {
        let value = self.settings.get(section)?.get(key)?.as_f64()?;
        (value.is_finite() && value > 0.0).then_some(value)
    }

    /// Reachable span of an axis, covering center-origin machines (negative
    /// minima) as well as 0-origin cartesians.
    fn axis_span(&self, index: usize) -> Option<f64> {
        let hi = *self.axis_maximum.get(index)?;
        let lo = self.axis_minimum.get(index).copied().unwrap_or(0.0);
        let span = if lo < 0.0 { hi - lo } else { hi };
        (span > 0.0).then_some(mm(span))
    }

    fn kinematics(&self) -> Option<&str> {
        self.settings
            .get("printer")?
            .get("kinematics")?
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }

    /// Turn everything absorbed into a detection result.
    pub fn into_detection(self) -> PrinterDetection {
        let mut detection = PrinterDetection {
            reachable: true,
            kind: PrinterConnectionKind::Moonraker,
            firmware: Some("klipper".to_string()),
            name: self.hostname.clone(),
            ..Default::default()
        };
        let mut params = Map::new();

        self.derive_geometry(&mut detection);
        self.derive_extruder(&mut detection, &mut params);
        self.derive_motion(&mut detection, &mut params);
        self.derive_retraction(&mut detection, &mut params);
        self.derive_capabilities(&mut detection, &mut params);
        self.derive_identity(&mut detection);
        self.derive_questions(&mut detection);

        if !params.is_empty() {
            detection.params = Value::Object(params);
        }
        detection.message = Some(match detection.name.as_deref() {
            Some(name) if !name.is_empty() => format!("Found Klipper printer “{name}”."),
            _ => "Found a Klipper (Moonraker) printer.".to_string(),
        });
        detection
    }

    /// Bed shape, origin and build volume.
    ///
    /// Kinematics decides the shape — deltas are circular and center-origin —
    /// and is only stamped when present, so a toolhead-only payload reports the
    /// volume without guessing the shape.
    fn derive_geometry(&self, detection: &mut PrinterDetection) {
        if let Some(kinematics) = self.kinematics() {
            let is_delta = kinematics.eq_ignore_ascii_case("delta");
            detection.bed_shape = Some(if is_delta {
                BedShape::Circular
            } else {
                BedShape::Rectangular
            });
            detection.origin_at_center = Some(is_delta);
            detection
                .findings
                .push(DetectionFinding::new("Kinematics", kinematics, "[printer]"));
        }

        detection.bed_width = self.axis_span(0);
        detection.bed_depth = self.axis_span(1);
        detection.bed_height = self.axis_maximum.get(2).copied().map(mm);

        if let (Some(width), Some(height)) = (detection.bed_width, detection.bed_height) {
            let area = match (detection.bed_shape, detection.bed_depth) {
                (Some(BedShape::Circular), _) => format!("⌀ {width}"),
                (_, Some(depth)) => format!("{width} × {depth}"),
                _ => format!("{width}"),
            };
            detection.findings.push(DetectionFinding::new(
                "Build volume",
                format!("{area} × {height} mm"),
                "toolhead axis limits",
            ));
        }
    }

    /// Nozzle, filament and extruder count.
    fn derive_extruder(&self, detection: &mut PrinterDetection, params: &mut Map<String, Value>) {
        if let Some(nozzle) = self.positive("extruder", "nozzle_diameter") {
            detection.nozzle_diameter_mm = Some(nozzle);
            params.insert("nozzle_diameter_mm".into(), json!(nozzle));
            detection.findings.push(DetectionFinding::new(
                "Nozzle",
                format!("{nozzle} mm"),
                "[extruder]",
            ));
        }
        if let Some(filament) = self.positive("extruder", "filament_diameter") {
            params.insert("filament_diameter_mm".into(), json!(filament));
            detection.findings.push(DetectionFinding::new(
                "Filament diameter",
                format!("{filament} mm"),
                "[extruder]",
            ));
        }
        // Pressure advance is only meaningful once tuned; an untuned machine
        // reports 0, which is also our default, so there is nothing to say.
        if let Some(advance) = self.positive("extruder", "pressure_advance") {
            params.insert("pressure_advance".into(), json!(advance));
            detection.findings.push(DetectionFinding::new(
                "Pressure advance",
                format!("{advance}"),
                "[extruder]",
            ));
        }

        // Klipper names additional tools `extruder1`, `extruder2`, … — the
        // first one is simply `extruder`, so the count is that plus the rest.
        let extras = (1..=8)
            .take_while(|n| self.has(&format!("extruder{n}")))
            .count();
        if extras > 0 {
            params.insert("extruder_count".into(), json!(extras + 1));
            detection.findings.push(DetectionFinding::new(
                "Extruders",
                format!("{}", extras + 1),
                "[extruder1…]",
            ));
        }
    }

    /// The machine's own velocity and acceleration limits.
    ///
    /// Worth reading even though the firmware clamps anyway: the slicer emits
    /// `SET_VELOCITY_LIMIT` from these params and estimates print time from
    /// them, so a profile that never learned the machine's real ceiling both
    /// raises the limit the machine was configured with and mis-predicts the
    /// ETA.
    ///
    /// Note what is *not* derived here: role speeds, and travel in particular.
    /// `max_velocity` is already the ceiling for every move the machine makes,
    /// in the firmware and in our own estimate, so writing a travel speed from
    /// it would restate the cap in a second place and quietly override whatever
    /// the user's profile asked for.
    fn derive_motion(&self, detection: &mut PrinterDetection, params: &mut Map<String, Value>) {
        if let Some(velocity) = self.positive("printer", "max_velocity") {
            params.insert("max_velocity".into(), json!(mm(velocity)));
            detection.findings.push(DetectionFinding::new(
                "Max velocity",
                format!("{} mm/s", mm(velocity)),
                "[printer]",
            ));
        }
        if let Some(accel) = self.positive("printer", "max_accel") {
            params.insert("acceleration".into(), json!(mm(accel)));
            detection.findings.push(DetectionFinding::new(
                "Max acceleration",
                format!("{} mm/s²", mm(accel)),
                "[printer]",
            ));
        }
        if let Some(corner) = self.positive("printer", "square_corner_velocity") {
            params.insert("square_corner_velocity".into(), json!(mm(corner)));
            detection.findings.push(DetectionFinding::new(
                "Square corner velocity",
                format!("{} mm/s", mm(corner)),
                "[printer]",
            ));
        }
    }

    /// Firmware retraction, when the machine has the module configured.
    ///
    /// `[firmware_retraction]` means the printer owns these numbers; handing
    /// them back to it through `G10`/`G11` keeps one source of truth and lets a
    /// user retune retraction without re-slicing.
    fn derive_retraction(&self, detection: &mut PrinterDetection, params: &mut Map<String, Value>) {
        if !self.settings.contains_key("firmware_retraction") {
            return;
        }
        params.insert("use_firmware_retraction".into(), json!(true));
        detection.findings.push(DetectionFinding::new(
            "Firmware retraction",
            "On — the printer's own retraction settings are used",
            "[firmware_retraction]",
        ));

        if let Some(length) = self.positive("firmware_retraction", "retract_length") {
            params.insert("retract_mm".into(), json!(length));
            detection.findings.push(DetectionFinding::new(
                "Retraction length",
                format!("{length} mm"),
                "[firmware_retraction]",
            ));
        }
        if let Some(speed) = self.positive("firmware_retraction", "retract_speed") {
            params.insert("retract_speed_mm_min".into(), json!(mm(speed * 60.0)));
            detection.findings.push(DetectionFinding::new(
                "Retraction speed",
                format!("{} mm/s", mm(speed)),
                "[firmware_retraction]",
            ));
        }
        if let Some(extra) = self.positive("firmware_retraction", "unretract_extra_length") {
            params.insert("retract_restart_extra_mm".into(), json!(extra));
        }
    }

    /// Optional Klipper modules that change what the slicer may emit.
    fn derive_capabilities(
        &self,
        detection: &mut PrinterDetection,
        params: &mut Map<String, Value>,
    ) {
        // `[exclude_object]` is what makes cancelling one object mid-print
        // possible. Off by default because emitting the markers to a printer
        // that lacks the module aborts the print.
        if self.has("exclude_object") {
            params.insert("exclude_object".into(), json!(true));
            detection.findings.push(DetectionFinding::new(
                "Cancel individual objects",
                "Supported",
                "[exclude_object]",
            ));
        }

        // A chamber the firmware can *heat* is a `heater_generic`; a bare
        // `temperature_sensor` only reports one, which is not something the
        // slicer can target.
        if self.has("heater_generic chamber") {
            params.insert("heated_chamber".into(), json!(true));
            detection.findings.push(DetectionFinding::new(
                "Heated chamber",
                "Yes",
                "[heater_generic chamber]",
            ));
        }
    }

    /// Name the machine, when its configuration is distinctive enough.
    ///
    /// Deliberately does **not** fall back to "Klipper" as the vendor: Klipper
    /// runs on hundreds of printers, and writing the firmware into the
    /// manufacturer field makes every detected profile claim the same maker.
    fn derive_identity(&self, detection: &mut PrinterDetection) {
        let Some((vendor, model)) = self.fingerprint() else {
            return;
        };
        detection.vendor = Some(vendor.to_string());
        detection.model = Some(model.to_string());
    }

    /// Match the machine against a handful of structural signatures.
    ///
    /// Keyed on levelling hardware and build volume — the two things that
    /// differ between otherwise identical CoreXY designs — because those are
    /// what a `printer.cfg` states plainly. A miss leaves vendor and model
    /// empty for the user to fill in, which is the honest answer.
    fn fingerprint(&self) -> Option<(&'static str, &'static str)> {
        let kinematics = self.kinematics()?.to_ascii_lowercase();
        let width = self.axis_span(0)?;
        // Within a centimetre: a machine trimmed to 349 mm of travel is still a
        // 350 mm machine.
        let about = |nominal: f64| (width - nominal).abs() <= 10.0;

        match kinematics.as_str() {
            // Four independently-levelled Z motors is the 2.x gantry.
            "corexy" if self.has("quad_gantry_level") => Some((
                "Voron",
                match width {
                    _ if about(250.0) => "Voron 2.4 250",
                    _ if about(300.0) => "Voron 2.4 300",
                    _ if about(350.0) => "Voron 2.4 350",
                    _ => "Voron 2.4",
                },
            )),
            // Three-point bed tilt with a fixed gantry is Trident / V1.
            "corexy" if self.has("z_tilt") => Some((
                "Voron",
                match width {
                    _ if about(250.0) => "Voron Trident 250",
                    _ if about(300.0) => "Voron Trident 300",
                    _ if about(350.0) => "Voron Trident 350",
                    _ => "Voron Trident",
                },
            )),
            "delta" if self.has("delta_calibrate") => Some(("Custom", "Delta")),
            _ => None,
        }
    }

    /// Everything the config hints at but cannot decide.
    fn derive_questions(&self, detection: &mut PrinterDetection) {
        if let Some(question) = self.macro_question() {
            detection.questions.push(question);
        }
        if let Some(question) = self.identity_question(detection) {
            detection.questions.push(question);
        }
        if let Some(question) = self.bed_mesh_question() {
            detection.questions.push(question);
        }
        if let Some(question) = self.aux_fan_question() {
            detection.questions.push(question);
        }
        if let Some(question) = self.orientation_question() {
            detection.questions.push(question);
        }
    }

    /// Which start/end macro convention this host follows.
    ///
    /// Returns `None` — no question at all — for the overwhelmingly common case
    /// of a host that defines exactly one of the two conventions. That is a
    /// fact, not a preference, and asking it was the single step every user had
    /// to answer before a profile could be created.
    fn macro_question(&self) -> Option<DetectionQuestion> {
        let standard = self.has("gcode_macro print_start");
        let klippain = self.has("gcode_macro start_print");

        let (suggested, certain, evidence) = match (standard, klippain) {
            (true, false) => (
                MACRO_STANDARD,
                true,
                "This printer defines a PRINT_START macro.",
            ),
            (false, true) => (
                MACRO_KLIPPAIN,
                true,
                "This printer defines a START_PRINT macro.",
            ),
            (true, true) => (
                MACRO_STANDARD,
                false,
                "This printer defines both PRINT_START and START_PRINT.",
            ),
            (false, false) => (
                MACRO_KEEP,
                false,
                "We couldn't find a PRINT_START or START_PRINT macro on this printer.",
            ),
        };

        Some(DetectionQuestion {
            id: "macro_convention".to_string(),
            options: ordered(
                suggested,
                vec![
                    DetectionOption::passive(MACRO_STANDARD),
                    DetectionOption::passive(MACRO_KLIPPAIN),
                    DetectionOption::passive(MACRO_KEEP),
                ],
            ),
            suggested: suggested.to_string(),
            evidence: Some(evidence.to_string()),
            certain,
        })
    }

    /// Confirm a fingerprinted machine, since a signature is a strong hint and
    /// never a certainty — plenty of custom builds borrow a Voron gantry.
    fn identity_question(&self, detection: &PrinterDetection) -> Option<DetectionQuestion> {
        let model = detection.model.clone()?;
        let vendor = detection.vendor.clone()?;
        Some(DetectionQuestion {
            id: "machine_identity".to_string(),
            options: vec![
                DetectionOption::passive(IDENTITY_CONFIRM).labelled(model),
                DetectionOption::passive(IDENTITY_OTHER),
            ],
            suggested: IDENTITY_CONFIRM.to_string(),
            certain: false,
            evidence: Some(format!(
                "Its levelling hardware and build volume match a {vendor} configuration."
            )),
        })
    }

    /// Whether to drive bed levelling from the slice.
    ///
    /// Suggests leaving it alone: a host with a start macro almost certainly
    /// calls `BED_MESH_*` there already, and emitting a second probe cycle
    /// wastes minutes on every print.
    fn bed_mesh_question(&self) -> Option<DetectionQuestion> {
        if !self.has("bed_mesh") {
            return None;
        }
        let mut options = vec![
            DetectionOption::with_params(MESH_LEAVE, json!({ "bed_mesh_mode": "off" })),
            DetectionOption::with_params(
                MESH_CALIBRATE,
                json!({ "bed_mesh_mode": "calibrate", "bed_mesh_adaptive": true }),
            ),
        ];
        for profile in &self.mesh_profiles {
            options.push(
                DetectionOption::with_params(
                    format!("profile:{profile}"),
                    json!({ "bed_mesh_mode": "load_profile", "bed_mesh_profile_name": profile }),
                )
                .labelled(profile.clone())
                .detail("Saved mesh profile"),
            );
        }

        let evidence = if self.mesh_profiles.is_empty() {
            "This printer can probe a bed mesh.".to_string()
        } else {
            format!(
                "This printer can probe a bed mesh, and has {} saved.",
                self.mesh_profiles.len()
            )
        };
        Some(DetectionQuestion {
            id: "bed_mesh".to_string(),
            options,
            suggested: MESH_LEAVE.to_string(),
            certain: false,
            evidence: Some(evidence),
        })
    }

    /// What a `[fan_generic]` is for.
    ///
    /// Klipper cannot tell us: `fan_generic` is the catch-all for any fan the
    /// firmware does not drive itself, which covers auxiliary part cooling,
    /// filtration, electronics bays and nozzle-side blowers alike. Suggests
    /// leaving it alone, because spinning up an exhaust fan as part cooling
    /// would quietly ruin prints.
    fn aux_fan_question(&self) -> Option<DetectionQuestion> {
        let fans = self.named("fan_generic");
        if fans.is_empty() {
            return None;
        }
        let mut options = vec![DetectionOption::passive(FAN_UNUSED)];
        for fan in &fans {
            options.push(
                DetectionOption::with_params(
                    format!("cooling:{fan}"),
                    json!({
                        "fan_configs": [
                            { "fan_index": 0, "min_speed": 0.35, "max_speed": 1.0,
                              "layer_time_fast_s": 10.0, "layer_time_slow_s": 30.0 },
                            { "fan_index": 3, "klipper_name": fan, "min_speed": 0.0,
                              "max_speed": 1.0, "layer_time_fast_s": 10.0,
                              "layer_time_slow_s": 30.0 }
                        ]
                    }),
                )
                .labelled((*fan).to_string())
                .detail(format!("[fan_generic {fan}]")),
            );
        }
        Some(DetectionQuestion {
            id: "aux_fan".to_string(),
            options,
            suggested: FAN_UNUSED.to_string(),
            certain: false,
            evidence: Some(format!(
                "This printer has {} that Klipper doesn't drive on its own: {}.",
                if fans.len() == 1 { "a fan" } else { "fans" },
                fans.join(", ")
            )),
        })
    }

    /// Whether to print everything rotated on a CoreXY.
    ///
    /// A CoreXY moves fastest along its diagonals, so many owners rotate every
    /// part 45° to keep long walls off the belt axes. A real preference — it
    /// changes how parts sit on the plate — so it is asked, and suggested off.
    fn orientation_question(&self) -> Option<DetectionQuestion> {
        if !self.kinematics()?.eq_ignore_ascii_case("corexy") {
            return None;
        }
        Some(DetectionQuestion {
            id: "preferred_orientation".to_string(),
            options: vec![
                DetectionOption::passive(ORIENT_KEEP),
                DetectionOption::passive(ORIENT_DIAGONAL),
            ],
            suggested: ORIENT_KEEP.to_string(),
            certain: false,
            evidence: Some("This is a CoreXY machine, which moves fastest diagonally.".to_string()),
        })
    }
}

/// Option ids. Shared with the wizard's copy table, so they are spelled once.
const MACRO_STANDARD: &str = "standard";
const MACRO_KLIPPAIN: &str = "klippain";
const MACRO_KEEP: &str = "keep";
const IDENTITY_CONFIRM: &str = "confirm";
const IDENTITY_OTHER: &str = "other";
const MESH_LEAVE: &str = "leave";
const MESH_CALIBRATE: &str = "calibrate";
const FAN_UNUSED: &str = "unused";
const ORIENT_KEEP: &str = "keep";
const ORIENT_DIAGONAL: &str = "diagonal";

/// Move the suggested option to the front, so a UI that renders options in
/// order leads with the recommendation.
fn ordered(suggested: &str, mut options: Vec<DetectionOption>) -> Vec<DetectionOption> {
    if let Some(index) = options.iter().position(|option| option.id == suggested) {
        options.swap(0, index);
    }
    options
}

fn non_empty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// `true` when a `/printer/info` result looks like a real Moonraker host.
///
/// A real Moonraker always carries `state`; `hostname` alone is accepted
/// because some proxies trim the reply.
pub fn looks_like_moonraker(result: &Value) -> bool {
    result.get("state").is_some() || result.get("hostname").is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A macro-heavy CoreXY host: Voron 2.4 350 running Klippain, with
    /// firmware retraction, object exclusion, an auxiliary fan and a saved
    /// mesh. Shaped exactly as Moonraker replies.
    fn corexy_probe() -> KlipperProbe {
        let mut probe = KlipperProbe::default();
        probe.absorb_info(&json!({
            "state": "ready",
            "hostname": "voron",
        }));
        probe.absorb_objects(&json!({
            "toolhead": {
                "axis_minimum": [0.0, 0.0, 0.0, 0.0],
                "axis_maximum": [350.00000000000006, 350.0, 340.0, 0.0],
            },
        }));
        probe.absorb_object_list(&json!({
            "objects": [
                "webhooks", "configfile", "toolhead", "extruder", "exclude_object",
                "bed_mesh", "quad_gantry_level", "fan_generic rscs",
                "gcode_macro START_PRINT", "gcode_macro END_PRINT",
            ],
        }));
        probe.absorb_objects(&json!({
            "bed_mesh": { "profiles": { "default": {}, "abs": {} } },
            "configfile": { "settings": {
                "printer": {
                    "kinematics": "corexy",
                    "max_velocity": 300.0,
                    "max_accel": 6000.0,
                    "square_corner_velocity": 8.0,
                },
                "extruder": {
                    "nozzle_diameter": 0.4,
                    "filament_diameter": 1.75,
                    "pressure_advance": 0.032,
                },
                "firmware_retraction": {
                    "retract_length": 0.8,
                    "retract_speed": 35.0,
                    "unretract_extra_length": 0.02,
                },
                "exclude_object": {},
                "bed_mesh": {},
                "quad_gantry_level": {},
                "fan_generic rscs": {},
                "gcode_macro start_print": {},
                "gcode_macro end_print": {},
            }},
        }));
        probe
    }

    /// A plain cartesian whose heavy `configfile` query never answered.
    fn toolhead_only_probe() -> KlipperProbe {
        let mut probe = KlipperProbe::default();
        probe.absorb_info(&json!({ "state": "ready", "hostname": "ender" }));
        probe.absorb_objects(&json!({
            "toolhead": {
                "axis_minimum": [0.0, 0.0, 0.0, 0.0],
                "axis_maximum": [235.0, 235.0, 250.0, 0.0],
            },
        }));
        probe
    }

    fn params_of(detection: &PrinterDetection) -> Map<String, Value> {
        detection.params.as_object().cloned().unwrap_or_default()
    }

    fn question<'a>(detection: &'a PrinterDetection, id: &str) -> Option<&'a DetectionQuestion> {
        detection.questions.iter().find(|q| q.id == id)
    }

    #[test]
    fn reads_the_whole_machine_off_a_corexy_config() {
        let detection = corexy_probe().into_detection();
        let params = params_of(&detection);

        assert_eq!(detection.bed_width, Some(350.0));
        assert_eq!(detection.bed_depth, Some(350.0));
        assert_eq!(detection.bed_height, Some(340.0));
        assert_eq!(detection.bed_shape, Some(BedShape::Rectangular));
        assert_eq!(detection.origin_at_center, Some(false));
        assert_eq!(detection.nozzle_diameter_mm, Some(0.4));

        assert_eq!(params["nozzle_diameter_mm"], json!(0.4));
        assert_eq!(params["filament_diameter_mm"], json!(1.75));
        assert_eq!(params["pressure_advance"], json!(0.032));
        assert_eq!(params["max_velocity"], json!(300.0));
        assert_eq!(params["acceleration"], json!(6000.0));
        assert_eq!(params["square_corner_velocity"], json!(8.0));
        assert_eq!(params["use_firmware_retraction"], json!(true));
        assert_eq!(params["retract_mm"], json!(0.8));
        assert_eq!(params["retract_speed_mm_min"], json!(2100.0));
        assert_eq!(params["exclude_object"], json!(true));
    }

    #[test]
    fn a_single_macro_convention_is_decided_not_asked() {
        // The one question every user used to have to answer before a profile
        // could be created — settled outright whenever the config is
        // unambiguous, so the wizard applies it instead of asking.
        let detection = corexy_probe().into_detection();
        let settled = question(&detection, "macro_convention").expect("reported either way");
        assert!(settled.certain);
        assert_eq!(settled.suggested, MACRO_KLIPPAIN);
    }

    #[test]
    fn ambiguous_macros_ask_and_lead_with_the_suggestion() {
        let mut probe = corexy_probe();
        probe.absorb_object_list(&json!({ "objects": ["gcode_macro PRINT_START"] }));

        let detection = probe.into_detection();
        let asked = question(&detection, "macro_convention").expect("both conventions present");
        assert!(!asked.certain);
        assert_eq!(asked.suggested, MACRO_STANDARD);
        assert_eq!(asked.options[0].id, MACRO_STANDARD);
    }

    #[test]
    fn a_host_with_no_start_macro_is_left_alone() {
        let mut probe = KlipperProbe::default();
        probe.absorb_object_list(&json!({ "objects": ["toolhead", "extruder"] }));

        let detection = probe.into_detection();
        let asked = question(&detection, "macro_convention").expect("neither convention present");
        assert!(!asked.certain);
        assert_eq!(asked.suggested, MACRO_KEEP);
    }

    #[test]
    fn fingerprints_the_machine_and_asks_for_confirmation() {
        let detection = corexy_probe().into_detection();
        assert_eq!(detection.vendor.as_deref(), Some("Voron"));
        assert_eq!(detection.model.as_deref(), Some("Voron 2.4 350"));
        // Never the firmware in the manufacturer field.
        assert_ne!(detection.vendor.as_deref(), Some("Klipper"));
        assert_eq!(detection.firmware.as_deref(), Some("klipper"));

        let asked = question(&detection, "machine_identity").expect("fingerprint matched");
        assert_eq!(asked.suggested, IDENTITY_CONFIRM);
    }

    #[test]
    fn bed_mesh_offers_saved_profiles_but_suggests_leaving_it_alone() {
        let detection = corexy_probe().into_detection();
        let asked = question(&detection, "bed_mesh").expect("bed_mesh module present");
        assert_eq!(asked.suggested, MESH_LEAVE);
        assert!(asked.options.iter().any(|o| o.id == "profile:abs"));
        assert!(asked.options.iter().any(|o| o.id == "profile:default"));
    }

    #[test]
    fn a_generic_fan_is_asked_about_never_assumed() {
        let detection = corexy_probe().into_detection();
        let asked = question(&detection, "aux_fan").expect("fan_generic present");
        assert_eq!(asked.suggested, FAN_UNUSED);
        assert!(asked.options.iter().any(|o| o.id == "cooling:rscs"));
    }

    #[test]
    fn the_machine_ceiling_is_stated_once_not_spread_across_role_speeds() {
        // `max_velocity` caps every move already; deriving a travel speed too
        // would restate it and override the user's own profile.
        let params = params_of(&corexy_probe().into_detection());
        assert_eq!(params["max_velocity"], json!(300.0));
        assert!(!params.contains_key("travel_speed_mm_min"));
        assert!(!params.contains_key("print_speed"));
    }

    #[test]
    fn a_missing_config_still_yields_the_build_volume() {
        let detection = toolhead_only_probe().into_detection();
        assert_eq!(detection.bed_width, Some(235.0));
        assert_eq!(detection.bed_depth, Some(235.0));
        assert_eq!(detection.bed_height, Some(250.0));
        // Nothing was claimed that the toolhead cannot know.
        assert_eq!(detection.bed_shape, None);
        assert_eq!(detection.origin_at_center, None);
        assert_eq!(detection.nozzle_diameter_mm, None);
        assert_eq!(detection.vendor, None);
        assert!(params_of(&detection).is_empty());
    }

    #[test]
    fn deltas_are_circular_and_center_origin() {
        let mut probe = KlipperProbe::default();
        probe.absorb_objects(&json!({
            "configfile": { "settings": { "printer": { "kinematics": "delta" } } },
            "toolhead": {
                "axis_minimum": [-100.0, -100.0, 0.0, 0.0],
                "axis_maximum": [100.0, 100.0, 300.0, 0.0],
            },
        }));

        let detection = probe.into_detection();
        assert_eq!(detection.bed_shape, Some(BedShape::Circular));
        assert_eq!(detection.origin_at_center, Some(true));
        assert_eq!(detection.bed_width, Some(200.0));
    }

    #[test]
    fn multiple_extruders_are_counted() {
        let mut probe = corexy_probe();
        probe.absorb_object_list(&json!({ "objects": ["extruder1"] }));
        assert_eq!(
            params_of(&probe.into_detection())["extruder_count"],
            json!(2)
        );
    }

    #[test]
    fn every_applied_value_can_be_traced_back_to_a_config_section() {
        let detection = corexy_probe().into_detection();
        assert!(!detection.findings.is_empty());
        for finding in &detection.findings {
            assert!(
                !finding.source.trim().is_empty(),
                "{finding:?} has no source"
            );
        }
    }
}
