//! Outbound printer transports — the slicer → printer link.
//!
//! The engine prefers to talk to printers **from the native process** (CLI or
//! the `serve` WebSocket server) so that the request never leaves the same
//! trust boundary as the printer's LAN and, crucially, is **not subject to
//! browser CORS**. Moonraker (Klipper) ships no permissive `Access-Control-*`
//! headers by default, so a direct browser `fetch` from the Angular UI fails
//! for most users. Routing the probe/upload through the server sidesteps that
//! entirely.
//!
//! The wasm build has no native transport (this whole module is
//! `cfg(not(target_arch = "wasm32"))`); there the UI falls back to a direct
//! `fetch`, which is expected to fail on CORS for many hosts and is surfaced to
//! the user as a distinct, actionable state rather than a silent error.
//!
//! Only [`PrinterConnectionKind::Moonraker`] is implemented today. Other kinds
//! report `unsupported` so the UI can render an honest status instead of a
//! misleading green dot.

use std::path::Path;
use std::time::Duration;

use crate::profiles::printer::{BedShape, PrinterConnection, PrinterConnectionKind};

/// How long to wait for a printer to answer before declaring it offline.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

/// A point-in-time snapshot of a printer's reachability and job state.
#[derive(Debug, Clone, Default)]
pub struct PrinterStatusReport {
    /// The host answered a status query.
    pub online: bool,
    /// Firmware/host state (e.g. `ready`, `error`, `startup`, `shutdown`).
    pub state: Option<String>,
    /// Current job state (e.g. `standby`, `printing`, `paused`, `complete`).
    pub print_state: Option<String>,
    /// Print progress in `0.0..=1.0`, when a job is active.
    pub progress: Option<f32>,
    /// Human-readable detail — an error reason when offline, or a state note.
    pub message: Option<String>,
}

impl PrinterStatusReport {
    fn offline(message: impl Into<String>) -> Self {
        Self {
            online: false,
            message: Some(message.into()),
            ..Default::default()
        }
    }
}

/// Outcome of an upload/print request.
#[derive(Debug, Clone)]
pub struct SendOutcome {
    /// Human-readable summary for the UI.
    pub message: String,
    /// Whether the print was started (vs. only uploaded).
    pub started: bool,
}

/// Everything we could learn about a printer by probing a single URL.
///
/// Every hardware field is optional: detection is best-effort and degrades
/// gracefully. A `reachable: false` result still carries a `message` explaining
/// why. When `kind` is identified but hardware fields are absent (OctoPrint /
/// PrusaLink), the wizard can still pre-select the transport and host.
#[derive(Debug, Clone, Default)]
pub struct PrinterDetection {
    /// The host answered at least one probe.
    pub reachable: bool,
    /// Detected transport, or `None` when nothing answered.
    pub kind: PrinterConnectionKind,
    /// Human-readable summary (a success note or the failure reason).
    pub message: Option<String>,
    /// Friendly name (Klipper hostname), when known.
    pub name: Option<String>,
    /// Model designation, when known.
    pub model: Option<String>,
    /// Manufacturer / firmware family, when known.
    pub vendor: Option<String>,
    /// G-code dialect the firmware speaks (`marlin`, `klipper`).
    pub firmware: Option<String>,
    /// Bed shape (rectangular / circular), when known.
    pub bed_shape: Option<BedShape>,
    /// Bed width / diameter (mm), when known.
    pub bed_width: Option<f64>,
    /// Bed depth (mm), when known.
    pub bed_depth: Option<f64>,
    /// Max Z height (mm), when known.
    pub bed_height: Option<f64>,
    /// True for delta / center-origin machines, when known.
    pub origin_at_center: Option<bool>,
    /// Nozzle diameter (mm), when known.
    pub nozzle_diameter_mm: Option<f64>,
}

/// Probe a printer connection and report its live status.
///
/// Never returns an error: an unreachable or misconfigured printer is reported
/// as `online: false` with an explanatory `message`.
pub async fn check_status(conn: &PrinterConnection) -> PrinterStatusReport {
    match conn.kind {
        PrinterConnectionKind::Moonraker => moonraker_status(conn).await,
        PrinterConnectionKind::None => PrinterStatusReport::offline("No connection configured"),
        other => PrinterStatusReport {
            online: false,
            message: Some(format!(
                "{} connections are not supported yet",
                kind_name(other)
            )),
            ..Default::default()
        },
    }
}

/// Probe a single URL and report everything we can learn about the printer.
///
/// Detection is best-effort and never errors: an unreachable host yields
/// `reachable: false` with an explanatory `message`. The probe tries Moonraker
/// first (richest metadata — bed volume, nozzle, kinematics), then falls back
/// to identifying OctoPrint / PrusaLink from their `/api/version` banner.
pub async fn detect_printer(host: &str) -> PrinterDetection {
    let base = match base_url_from_parts(host, None) {
        Ok(b) => b,
        Err(e) => {
            return PrinterDetection {
                message: Some(e),
                ..Default::default()
            }
        }
    };

    let client = http_client();

    // 1) Moonraker (Klipper) — the only transport we can deeply introspect.
    if let Some(detection) = detect_moonraker(&client, &base).await {
        return detection;
    }

    // 2) OctoPrint / PrusaLink share the `/api/version` banner shape.
    if let Some(detection) = detect_api_version(&client, &base).await {
        return detection;
    }

    PrinterDetection {
        reachable: false,
        message: Some(
            "Could not identify a printer at that address. Check the host and that the printer is on."
                .to_string(),
        ),
        ..Default::default()
    }
}

/// Probe Moonraker and, on success, harvest bed volume, nozzle, and kinematics.
async fn detect_moonraker(client: &reqwest::Client, base: &str) -> Option<PrinterDetection> {
    let info_url = format!("{base}/printer/info");
    let resp = client.get(&info_url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let info: serde_json::Value = resp.json().await.ok()?;
    let result = &info["result"];
    // `/printer/info` on a real Moonraker always carries a `state` field.
    if result.get("state").is_none() && result.get("hostname").is_none() {
        return None;
    }

    let mut detection = PrinterDetection {
        reachable: true,
        kind: PrinterConnectionKind::Moonraker,
        vendor: Some("Klipper".to_string()),
        firmware: Some("klipper".to_string()),
        ..Default::default()
    };
    detection.name = result["hostname"].as_str().map(str::to_string);

    // Enrich with bed volume / nozzle from the config + toolhead objects.
    let query_url = format!("{base}/printer/objects/query?configfile&toolhead");
    if let Ok(resp) = client.get(&query_url).send().await {
        if let Ok(body) = resp.json::<serde_json::Value>().await {
            enrich_from_moonraker_objects(&mut detection, &body["result"]["status"]);
        }
    }

    detection.message = Some(match &detection.name {
        Some(name) if !name.is_empty() => format!("Found Klipper printer “{name}”."),
        _ => "Found a Klipper (Moonraker) printer.".to_string(),
    });
    Some(detection)
}

/// Pull bed dimensions, kinematics, and nozzle diameter out of a Moonraker
/// `printer/objects/query?configfile&toolhead` status payload.
fn enrich_from_moonraker_objects(detection: &mut PrinterDetection, status: &serde_json::Value) {
    let settings = &status["configfile"]["settings"];

    // Kinematics decides bed shape: deltas are circular / center-origin.
    let kinematics = settings["printer"]["kinematics"].as_str().unwrap_or("");
    let is_delta = kinematics.eq_ignore_ascii_case("delta");
    detection.bed_shape = Some(if is_delta {
        BedShape::Circular
    } else {
        BedShape::Rectangular
    });
    detection.origin_at_center = Some(is_delta);

    // Bed volume from the toolhead's reachable axis limits. `axis_maximum` and
    // `axis_minimum` are `[x, y, z, e]`; the span covers center-origin deltas
    // (negative minima) as well as 0-origin cartesians.
    let max = &status["toolhead"]["axis_maximum"];
    let min = &status["toolhead"]["axis_minimum"];
    let span = |i: usize| -> Option<f64> {
        let hi = max.get(i)?.as_f64()?;
        let lo = min.get(i).and_then(serde_json::Value::as_f64).unwrap_or(0.0);
        let span = if lo < 0.0 { hi - lo } else { hi };
        (span > 0.0).then_some((span * 10.0).round() / 10.0)
    };
    if let Some(w) = span(0) {
        detection.bed_width = Some(w);
    }
    if let Some(d) = span(1) {
        detection.bed_depth = Some(d);
    }
    if let Some(h) = max.get(2).and_then(serde_json::Value::as_f64) {
        detection.bed_height = Some((h * 10.0).round() / 10.0);
    }

    // Nozzle diameter lives on the primary extruder config.
    if let Some(nozzle) = settings["extruder"]["nozzle_diameter"].as_f64() {
        if nozzle > 0.0 {
            detection.nozzle_diameter_mm = Some(nozzle);
        }
    }
}

/// Identify an OctoPrint / PrusaLink host from its `/api/version` banner.
///
/// These transports aren't slicer-driven yet, so we only report the kind and
/// reachability — enough for the wizard to pre-select the connection and host.
async fn detect_api_version(client: &reqwest::Client, base: &str) -> Option<PrinterDetection> {
    let url = format!("{base}/api/version");
    let resp = client.get(&url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let body: serde_json::Value = resp.json().await.ok()?;
    let banner = [body["text"].as_str(), body["server"].as_str()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase();

    if banner.contains("prusa") {
        Some(PrinterDetection {
            reachable: true,
            kind: PrinterConnectionKind::Prusalink,
            vendor: Some("Prusa".to_string()),
            firmware: Some("marlin".to_string()),
            message: Some("Found a PrusaLink printer.".to_string()),
            name: body["hostname"].as_str().map(str::to_string),
            ..Default::default()
        })
    } else if banner.contains("octoprint") {
        Some(PrinterDetection {
            reachable: true,
            kind: PrinterConnectionKind::Octoprint,
            message: Some(
                "Found an OctoPrint host. Add its API key to finish setup.".to_string(),
            ),
            ..Default::default()
        })
    } else {
        None
    }
}

/// Upload `gcode_path` to the printer under `filename`, optionally starting the
/// print immediately.
pub async fn send_gcode(
    conn: &PrinterConnection,
    gcode_path: &Path,
    filename: &str,
    start: bool,
) -> Result<SendOutcome, String> {
    match conn.kind {
        PrinterConnectionKind::Moonraker => {
            moonraker_upload(conn, gcode_path, filename, start).await
        }
        PrinterConnectionKind::None => Err("No connection configured for this printer".to_string()),
        other => Err(format!(
            "Sending to {} printers is not supported yet",
            kind_name(other)
        )),
    }
}

fn kind_name(kind: PrinterConnectionKind) -> &'static str {
    match kind {
        PrinterConnectionKind::None => "unconnected",
        PrinterConnectionKind::Octoprint => "OctoPrint",
        PrinterConnectionKind::Moonraker => "Moonraker",
        PrinterConnectionKind::Bambu => "Bambu Lab",
        PrinterConnectionKind::Prusalink => "PrusaLink",
    }
}

// ── Moonraker (Klipper) ─────────────────────────────────────────────────────

/// Build a normalized base URL (`http://host[:port]`) from a connection.
///
/// Accepts a bare host, a `host:port`, or a full `http(s)://…` URL. The
/// explicit `port` field is only applied when the host doesn't already carry
/// one.
fn base_url(conn: &PrinterConnection) -> Result<String, String> {
    let host = conn
        .host
        .as_deref()
        .map(str::trim)
        .filter(|h| !h.is_empty())
        .ok_or_else(|| "No host configured".to_string())?;

    base_url_from_parts(host, conn.port)
}

/// Build a normalized base URL (`http://host[:port]`) from a raw host string
/// and an optional explicit port.
///
/// Accepts a bare host, a `host:port`, or a full `http(s)://…` URL. The
/// explicit `port` is only applied when the host doesn't already carry one.
fn base_url_from_parts(host: &str, port: Option<u16>) -> Result<String, String> {
    let host = host.trim();
    if host.is_empty() {
        return Err("No host configured".to_string());
    }

    let mut url = if host.starts_with("http://") || host.starts_with("https://") {
        host.to_string()
    } else {
        format!("http://{host}")
    };
    while url.ends_with('/') {
        url.pop();
    }

    // Append the explicit port only when the authority lacks one.
    if let Some(port) = port {
        let authority = url.split("://").nth(1).unwrap_or("");
        let authority_has_port = authority.split('/').next().unwrap_or("").contains(':');
        if !authority_has_port {
            url = format!("{url}:{port}");
        }
    }
    Ok(url)
}

fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .unwrap_or_default()
}

/// Attach the `X-Api-Key` header when an API key is configured.
fn with_auth(
    mut req: reqwest::RequestBuilder,
    conn: &PrinterConnection,
) -> reqwest::RequestBuilder {
    if let Some(key) = conn.api_key.as_deref().filter(|k| !k.trim().is_empty()) {
        req = req.header("X-Api-Key", key);
    }
    req
}

async fn moonraker_status(conn: &PrinterConnection) -> PrinterStatusReport {
    let base = match base_url(conn) {
        Ok(b) => b,
        Err(e) => return PrinterStatusReport::offline(e),
    };

    let url = format!("{base}/printer/objects/query?webhooks&print_stats&display_status");
    let client = http_client();
    let resp = match with_auth(client.get(&url), conn).send().await {
        Ok(r) => r,
        Err(e) => return PrinterStatusReport::offline(friendly_error(&e)),
    };

    if !resp.status().is_success() {
        let code = resp.status();
        // 401/403 → the host is up but the API key is missing/wrong.
        return PrinterStatusReport::offline(format!("Printer responded with HTTP {code}"));
    }

    let body: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => return PrinterStatusReport::offline(format!("Invalid response: {e}")),
    };

    let status = &body["result"]["status"];
    let state = status["webhooks"]["state"].as_str().map(str::to_string);
    let state_message = status["webhooks"]["state_message"]
        .as_str()
        .map(str::to_string);
    let print_state = status["print_stats"]["state"].as_str().map(str::to_string);
    let progress = status["display_status"]["progress"]
        .as_f64()
        .map(|p| p as f32);

    PrinterStatusReport {
        online: true,
        state,
        print_state,
        progress,
        message: state_message,
    }
}

async fn moonraker_upload(
    conn: &PrinterConnection,
    gcode_path: &Path,
    filename: &str,
    start: bool,
) -> Result<SendOutcome, String> {
    let base = base_url(conn)?;
    let bytes = std::fs::read(gcode_path)
        .map_err(|e| format!("Failed to read G-code {}: {e}", gcode_path.display()))?;

    let safe_name = sanitize_filename(filename);
    let part = reqwest::multipart::Part::bytes(bytes)
        .file_name(safe_name.clone())
        .mime_str("application/octet-stream")
        .map_err(|e| format!("Failed to build upload: {e}"))?;
    let mut form = reqwest::multipart::Form::new()
        .text("root", "gcodes")
        .part("file", part);
    if start {
        form = form.text("print", "true");
    }

    let url = format!("{base}/server/files/upload");
    let client = http_client();
    let resp = with_auth(client.post(&url), conn)
        .multipart(form)
        .send()
        .await
        .map_err(|e| friendly_error(&e))?;

    let code = resp.status();
    if !code.is_success() {
        let detail = resp.text().await.unwrap_or_default();
        let hint = moonraker_error_hint(code.as_u16(), &detail);
        return Err(format!("Upload rejected (HTTP {code}){hint}"));
    }

    Ok(SendOutcome {
        message: if start {
            format!("Uploaded {safe_name} and started the print")
        } else {
            format!("Uploaded {safe_name} to the printer")
        },
        started: start,
    })
}

/// Moonraker filenames may not contain path separators.
fn sanitize_filename(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name).trim();
    let cleaned: String = base
        .chars()
        .map(|c| if c.is_control() { '_' } else { c })
        .collect();
    if cleaned.is_empty() {
        "print.gcode".to_string()
    } else {
        cleaned
    }
}

/// Turn a transport error into a short, user-facing reason.
fn friendly_error(e: &reqwest::Error) -> String {
    if e.is_timeout() {
        "Timed out — printer did not respond".to_string()
    } else if e.is_connect() {
        "Could not connect — check the host address and that the printer is on".to_string()
    } else {
        format!("Connection failed: {e}")
    }
}

fn moonraker_error_hint(code: u16, detail: &str) -> String {
    match code {
        401 | 403 => " — an API key is required or incorrect".to_string(),
        _ if detail.trim().is_empty() => String::new(),
        _ => format!(" — {}", detail.trim()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn(host: &str, port: Option<u16>) -> PrinterConnection {
        PrinterConnection {
            kind: PrinterConnectionKind::Moonraker,
            host: Some(host.to_string()),
            port,
            api_key: None,
            connected: false,
        }
    }

    #[test]
    fn base_url_bare_host_defaults_to_http() {
        assert_eq!(
            base_url(&conn("192.168.1.50", None)).unwrap(),
            "http://192.168.1.50"
        );
    }

    #[test]
    fn base_url_appends_explicit_port() {
        assert_eq!(
            base_url(&conn("printer.local", Some(7125))).unwrap(),
            "http://printer.local:7125"
        );
    }

    #[test]
    fn base_url_respects_host_port_over_field() {
        assert_eq!(
            base_url(&conn("printer.local:7125", Some(80))).unwrap(),
            "http://printer.local:7125"
        );
    }

    #[test]
    fn base_url_keeps_explicit_scheme() {
        assert_eq!(
            base_url(&conn("https://printer.local/", None)).unwrap(),
            "https://printer.local"
        );
    }

    #[test]
    fn base_url_rejects_empty_host() {
        assert!(base_url(&conn("   ", None)).is_err());
    }

    #[test]
    fn sanitize_filename_strips_paths() {
        assert_eq!(sanitize_filename("/tmp/foo/bar.gcode"), "bar.gcode");
        assert_eq!(sanitize_filename(""), "print.gcode");
    }

    #[test]
    fn enrich_reads_cartesian_bed_and_nozzle() {
        let status = serde_json::json!({
            "configfile": {
                "settings": {
                    "printer": { "kinematics": "cartesian" },
                    "extruder": { "nozzle_diameter": 0.6 }
                }
            },
            "toolhead": {
                "axis_minimum": [0.0, 0.0, 0.0, 0.0],
                "axis_maximum": [250.0, 210.0, 220.0, 0.0]
            }
        });
        let mut d = PrinterDetection::default();
        enrich_from_moonraker_objects(&mut d, &status);
        assert_eq!(d.bed_shape, Some(BedShape::Rectangular));
        assert_eq!(d.origin_at_center, Some(false));
        assert_eq!(d.bed_width, Some(250.0));
        assert_eq!(d.bed_depth, Some(210.0));
        assert_eq!(d.bed_height, Some(220.0));
        assert_eq!(d.nozzle_diameter_mm, Some(0.6));
    }

    #[test]
    fn enrich_treats_delta_as_circular_center_origin() {
        let status = serde_json::json!({
            "configfile": { "settings": { "printer": { "kinematics": "delta" } } },
            "toolhead": {
                "axis_minimum": [-100.0, -100.0, 0.0, 0.0],
                "axis_maximum": [100.0, 100.0, 300.0, 0.0]
            }
        });
        let mut d = PrinterDetection::default();
        enrich_from_moonraker_objects(&mut d, &status);
        assert_eq!(d.bed_shape, Some(BedShape::Circular));
        assert_eq!(d.origin_at_center, Some(true));
        assert_eq!(d.bed_width, Some(200.0));
    }
}
