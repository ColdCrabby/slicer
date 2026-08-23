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

use crate::profiles::printer::{PrinterConnection, PrinterConnectionKind};

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

    let mut url = if host.starts_with("http://") || host.starts_with("https://") {
        host.to_string()
    } else {
        format!("http://{host}")
    };
    while url.ends_with('/') {
        url.pop();
    }

    // Append the explicit port only when the authority lacks one.
    if let Some(port) = conn.port {
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
}
