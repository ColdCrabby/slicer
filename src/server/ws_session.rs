//! WebSocket session management and message handling.

use crate::logging::{phases, PhaseTimer, ProcessLogger, StderrLogger};
use crate::scene::{BedConfig, SceneOp, SceneState};
use crate::ws_protocol::{BedConfigDto, ClientMessage, SceneObjectDto, SceneOpDto, ServerMessage};
use futures_util::StreamExt as _;
use std::sync::Arc;
use uuid::Uuid;

/// A [`ProcessLogger`] that relays every message to the global stderr logger
/// *and* sends a JSON [`ServerMessage::Log`] frame to the connected WebSocket
/// client.
///
/// This gives WebSocket clients the same level of pipeline verbosity that the
/// CLI exposes via `--verbose`, without any special-casing inside the slicing
/// pipeline itself.
struct WsLogger {
    global: StderrLogger,
    tx: tokio::sync::mpsc::Sender<String>,
    /// Current object scope as `(index, count)`, both 1-based, or `(0, 0)` for
    /// "no scope" (a single merged slice). Interior-mutable because the logger
    /// is shared immutably down the pipeline while `slice_plate` updates the
    /// scope between objects.
    object_scope: std::sync::atomic::AtomicU64,
}

impl WsLogger {
    fn new(tx: tokio::sync::mpsc::Sender<String>) -> Self {
        Self {
            global: StderrLogger,
            tx,
            object_scope: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// Read the current object scope as optional 1-based `(index, count)`.
    fn scope(&self) -> (Option<u32>, Option<u32>) {
        let packed = self.object_scope.load(std::sync::atomic::Ordering::Relaxed);
        if packed == 0 {
            return (None, None);
        }
        let index = (packed >> 32) as u32;
        let count = (packed & 0xFFFF_FFFF) as u32;
        (Some(index), Some(count))
    }

    fn send_log(&self, level: &str, msg: &str) {
        let server_msg = ServerMessage::Log {
            level: level.to_string(),
            message: msg.to_string(),
        };
        let json = serde_json::to_string(&server_msg).unwrap_or_else(|_| {
            format!(
                r#"{{"type":"log","level":"{}","message":"<serialization error>"}}"#,
                level
            )
        });
        // `WsLogger` is exclusively constructed and used inside
        // `tokio::task::spawn_blocking`, so `blocking_send` is safe here and
        // will not stall an async executor thread.
        let _ = self.tx.blocking_send(json);
    }
}

impl ProcessLogger for WsLogger {
    fn log_info(&self, msg: &str) {
        self.global.log_info(msg);
        self.send_log("info", msg);
    }

    fn log_debug(&self, msg: &str) {
        self.global.log_debug(msg);
        self.send_log("debug", msg);
    }

    fn log_warn(&self, msg: &str) {
        self.global.log_warn(msg);
        self.send_log("warn", msg);
    }

    fn log_phase_start(&self, phase: &str) {
        self.global.log_phase_start(phase);
        let (object, object_count) = self.scope();
        let server_msg = crate::ws_protocol::ServerMessage::PhaseMarker {
            phase: phase.to_string(),
            event: "start".to_string(),
            elapsed_ms: None,
            object,
            object_count,
        };
        let json = serde_json::to_string(&server_msg).unwrap_or_else(|_| {
            format!(
                r#"{{"type":"PhaseMarker","phase":"{}","event":"start"}}"#,
                phase
            )
        });
        let _ = self.tx.blocking_send(json);
    }

    fn log_phase_end(&self, phase: &str, elapsed_ms: u64) {
        self.global.log_phase_end(phase, elapsed_ms);
        let (object, object_count) = self.scope();
        let server_msg = crate::ws_protocol::ServerMessage::PhaseMarker {
            phase: phase.to_string(),
            event: "end".to_string(),
            elapsed_ms: Some(elapsed_ms),
            object,
            object_count,
        };
        let json = serde_json::to_string(&server_msg).unwrap_or_else(|_| {
            format!(
                r#"{{"type":"PhaseMarker","phase":"{}","event":"end","elapsed_ms":{}}}"#,
                phase, elapsed_ms
            )
        });
        let _ = self.tx.blocking_send(json);
    }

    fn set_object_scope(&self, index: usize, count: usize) {
        let packed = ((index as u64) << 32) | (count as u64 & 0xFFFF_FFFF);
        self.object_scope
            .store(packed, std::sync::atomic::Ordering::Relaxed);
    }

    fn clear_object_scope(&self) {
        self.object_scope
            .store(0, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Upgrade an HTTP GET to a WebSocket connection and hand off to the session handler.
pub async fn ws_handler(
    req: actix_web::HttpRequest,
    stream: actix_web::web::Payload,
    state: actix_web::web::Data<super::handlers::AppState>,
) -> Result<actix_web::HttpResponse, actix_web::Error> {
    // Derive the base URL from the request so download URLs are fully qualified
    let scheme = if req.connection_info().scheme() == "https" {
        "https"
    } else {
        "http"
    };
    let host = req.connection_info().host().to_string();
    let base_url = format!("{}://{}", scheme, host);

    let (response, session, msg_stream) = actix_ws::handle(&req, stream)?;

    let db = state.db.clone();
    let work_dir = state.work_dir.clone();
    let profiles_changed = state.profiles_changed.subscribe();

    actix_web::rt::spawn(handle_ws_session(
        session,
        msg_stream,
        db,
        work_dir,
        base_url,
        profiles_changed,
    ));

    Ok(response)
}

/// Drive a single WebSocket session: send the initial handshake message then
/// dispatch incoming [`ClientMessage`]s until the client disconnects.
async fn handle_ws_session(
    mut session: actix_ws::Session,
    msg_stream: actix_ws::MessageStream,
    db: Arc<crate::db::Database>,
    work_dir: std::path::PathBuf,
    base_url: String,
    mut profiles_changed: tokio::sync::broadcast::Receiver<String>,
) {
    let logger = StderrLogger;
    logger.log_info("[WS] New session started");

    // Per-session scene state — ephemeral, dropped on disconnect.
    let mut scene = SceneState::new(BedConfig::default());

    // Aggregate WebSocket continuation frames (limit: 64 MiB per message)
    let mut stream = msg_stream
        .aggregate_continuations()
        .max_continuation_size(64 * 1024 * 1024);

    // Announce server version on connect
    let hello = ServerMessage::Connected {
        version: crate::version::VERSION.to_string(),
    };
    if send_msg(&mut session, &hello).await.is_err() {
        logger.log_warn("[WS] Failed to send Connected message, closing session");
        return;
    }

    loop {
        // Multiplex the client's inbound frames with server-side profile-change
        // notifications so a `PUT /api/profiles/:kind` from any client nudges
        // every open session to refetch.
        let msg = tokio::select! {
            incoming = stream.next() => match incoming {
                Some(Ok(msg)) => msg,
                _ => break,
            },
            changed = profiles_changed.recv() => {
                if let Ok(kind) = changed {
                    let _ = send_msg(&mut session, &ServerMessage::ProfilesChanged { kind }).await;
                }
                // A `Closed` sender is unreachable (AppState holds it for the
                // server's lifetime); a `Lagged` receiver merely drops missed
                // notifications — clients refetch the whole category anyway.
                continue;
            },
        };

        use actix_ws::AggregatedMessage;
        match msg {
            AggregatedMessage::Text(text) => match serde_json::from_str::<ClientMessage>(&text) {
                Ok(ClientMessage::Slice {
                    request_uuid,
                    scene: scene_objects,
                    profiles,
                    settings,
                    thumbnail_png_base64,
                }) => {
                    logger.log_debug(&format!("[WS] Processing slice request: {}", request_uuid));
                    // Resolve the parameters: prefer the structured profile
                    // selection (engine-owned composition); fall back to the
                    // legacy pre-flattened settings, then to defaults.
                    let params =
                        match resolve_slice_params(profiles, settings, thumbnail_png_base64) {
                            Ok(p) => Box::new(p),
                            Err(e) => {
                                logger.log_warn(&format!("[WS] Invalid profile selection: {e}"));
                                let _ = send_msg(&mut session, &ServerMessage::error(e)).await;
                                continue;
                            }
                        };
                    handle_slice(
                        &mut session,
                        request_uuid,
                        scene_objects,
                        params,
                        db.clone(),
                        work_dir.clone(),
                        base_url.clone(),
                    )
                    .await;
                }
                Ok(ClientMessage::ListSessions) => {
                    logger.log_debug("[WS] Processing list sessions request");
                    handle_list_sessions(&mut session, db.clone(), base_url.clone()).await;
                }
                Ok(ClientMessage::Reset) => {
                    logger.log_debug("[WS] Processing reset request");
                    scene = SceneState::new(BedConfig::default());
                    let _ = send_msg(&mut session, &ServerMessage::log_info("Reset.")).await;
                    let _ = send_msg(&mut session, &snapshot_msg(&scene)).await;
                }
                Ok(ClientMessage::Scene { ops, options }) => {
                    logger.log_debug(&format!(
                        "[WS] Applying {} scene ops (gravity={})",
                        ops.len(),
                        options.gravity
                    ));
                    handle_scene_ops(&mut session, &mut scene, ops, options, &work_dir, &db).await;
                }
                Ok(ClientMessage::SceneSnapshot) => {
                    let _ = send_msg(&mut session, &snapshot_msg(&scene)).await;
                }
                Ok(ClientMessage::Ping) => {
                    let _ = send_msg(&mut session, &ServerMessage::Pong).await;
                }
                Ok(ClientMessage::CheckPrinter {
                    printer_id,
                    connection,
                }) => {
                    logger.log_debug(&format!("[WS] Checking printer {printer_id}"));
                    handle_check_printer(&mut session, printer_id, connection).await;
                }
                Ok(ClientMessage::DetectPrinter { host }) => {
                    logger.log_debug(&format!("[WS] Detecting printer at {host}"));
                    handle_detect_printer(&mut session, host).await;
                }
                Ok(ClientMessage::SendToPrinter {
                    request_uuid,
                    printer_id,
                    connection,
                    filename,
                    start,
                }) => {
                    logger.log_debug(&format!(
                        "[WS] Sending {request_uuid} to printer {printer_id} (start={start})"
                    ));
                    handle_send_to_printer(
                        &mut session,
                        request_uuid,
                        printer_id,
                        connection,
                        filename,
                        start,
                        db.clone(),
                        work_dir.clone(),
                    )
                    .await;
                }
                Err(e) => {
                    logger.log_warn(&format!("[WS] Failed to parse message: {}", e));
                    let _ = send_msg(
                        &mut session,
                        &ServerMessage::error(format!("Unrecognised message: {e}")),
                    )
                    .await;
                }
            },
            AggregatedMessage::Close(_) => {
                logger.log_debug("[WS] Close message received");
                break;
            }
            _ => {}
        }
    }

    logger.log_info("[WS] Session ended");
    let _ = session.close(None).await;
}

/// Resolve a slice request's parameters from either the structured profile
/// selection (preferred) or the legacy pre-flattened settings.
///
/// Precedence: `profiles` (engine-owned composition) → `settings` (legacy) →
/// engine defaults.
///
/// A selection normally names its profiles by id, so this is where the
/// server's own `profiles.toml` becomes the thing that is actually sliced from.
/// Reading it per request rather than caching it is deliberate: the library is
/// small, and a `PUT /api/profiles/:kind` from another tab must take effect on
/// the very next slice, not whenever a cache happened to be refreshed.
///
/// `thumbnail_png_base64` is folded in afterwards. It is a per-slice artifact
/// rendered from the browser's 3D view — the engine has no renderer and never
/// makes one — so it rides its own field rather than posing as a user override.
fn resolve_slice_params(
    profiles: Option<Box<crate::profiles::ProfileSelection>>,
    settings: Option<Box<crate::settings::params::SlicingParams>>,
    thumbnail_png_base64: Option<String>,
) -> Result<crate::settings::params::SlicingParams, String> {
    let mut params = match profiles {
        Some(selection) => {
            let library = crate::profiles::ProfileStore::new()
                .load()
                .map_err(|e| format!("could not read this slicer's profile library: {e}"))?;
            selection
                .resolve(Some(&library))
                .map_err(|e| e.to_string())?
        }
        None => settings.map(|b| *b).unwrap_or_default(),
    };
    if let Some(png) = thumbnail_png_base64 {
        params.thumbnail_png_base64 = Some(png);
    }
    Ok(params)
}

/// Process a slice request from the browser.
///
/// The slice path is now fully scene-driven: the client sends the workplate
/// `request_uuid` plus a non-empty `scene` of placed objects (each
/// referencing an uploaded file by `file_uuid`). The server resolves every
/// file via the database (so it picks the right loader from the on-disk
/// extension), bakes each transform exactly once, merges the results into a
/// single mesh, and runs the slicer pipeline. The legacy "slice the upload
/// as-is" fallback has been removed.
async fn handle_slice(
    session: &mut actix_ws::Session,
    request_uuid: String,
    scene_objects: Vec<crate::ws_protocol::SceneObjectSliceDto>,
    params: Box<crate::settings::params::SlicingParams>,
    db: Arc<crate::db::Database>,
    work_dir: std::path::PathBuf,
    base_url: String,
) {
    macro_rules! send_or_return {
        ($msg:expr) => {
            if send_msg(session, &$msg).await.is_err() {
                return;
            }
        };
    }

    // Parse request UUID
    let uuid = match Uuid::parse_str(&request_uuid) {
        Ok(u) => u,
        Err(e) => {
            send_or_return!(ServerMessage::error(format!("Invalid request UUID: {e}")));
            return;
        }
    };

    // Build the list of (file_path, format, transform, size) entries we will
    // bake and merge before slicing. Every file is resolved via the DB so we
    // get the correct on-disk extension — no `.stl` assumption, no format
    // hint baked into the wire protocol.
    use crate::scene::Transform;
    if scene_objects.is_empty() {
        send_or_return!(ServerMessage::error(
            "Slice request has an empty `scene` — add at least one object before slicing"
        ));
        return;
    }

    // Content-derived cache key: identical scene + settings + engine version →
    // identical G-code. Computed from the wire DTOs (file ids + transforms) so
    // it is cheap and does not require reading mesh bytes.
    let cache_key = compute_slice_cache_key(&scene_objects, &params);

    // (path, part index within that file, transform, file size, support paint)
    let mut slice_inputs: Vec<(std::path::PathBuf, usize, Transform, u64, Option<String>)> =
        Vec::with_capacity(scene_objects.len());

    for obj in scene_objects {
        let file_uuid = match Uuid::parse_str(&obj.file_id) {
            Ok(u) => u,
            Err(e) => {
                send_or_return!(ServerMessage::error(format!(
                    "Invalid scene file_id '{}': {}",
                    obj.file_id, e
                )));
                return;
            }
        };

        // Look the file up in the DB so we know both the on-disk path
        // (extension preserved) and its size. The slicer's loader picks the
        // right format from that extension automatically.
        let entry = match db.get_file(file_uuid).await {
            Ok(Some(e)) => e,
            Ok(None) => {
                send_or_return!(ServerMessage::error(format!(
                    "Scene references unknown file_id {}",
                    file_uuid
                )));
                return;
            }
            Err(e) => {
                send_or_return!(ServerMessage::error(format!("Database error: {e}")));
                return;
            }
        };

        let transform = Transform::from_euler_xyz_deg(
            obj.transform.translation,
            obj.transform.euler_xyz_deg,
            obj.transform.scale,
        );
        slice_inputs.push((
            entry.file_path,
            obj.part_index,
            transform,
            entry.file_size as u64,
            obj.support_paint,
        ));
    }

    let total_bytes: u64 = slice_inputs.iter().map(|(_, _, _, sz, _)| *sz).sum();
    send_or_return!(ServerMessage::log_info(format!(
        "Slicing {} object(s), {} bytes total…",
        slice_inputs.len(),
        total_bytes
    )));

    // Run blocking work (mesh parse + bake + merge + slice + G-code gen) on
    // the thread pool. Messages are forwarded to the WebSocket via mpsc.
    let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(64);

    let gcode_output_path = work_dir.join(format!("{}.gcode", uuid));
    let gcode_output_path_clone = gcode_output_path.clone();

    // No cache lookup here on purpose: every slice request runs the full
    // pipeline, even when `cache_key` matches a previous run byte-for-byte.
    // `put_cached_gcode` below still records the result under that key — the
    // table stays warm for whatever else might read it — it is just never
    // consulted to *skip* a slice.

    let slice_handle = tokio::task::spawn_blocking(move || -> Option<usize> {
        /// Serializes `msg` to JSON; returns a hard-coded error frame on failure.
        fn to_json(msg: &ServerMessage) -> String {
            serde_json::to_string(msg).unwrap_or_else(|_| {
                r#"{"type":"error","message":"Internal error: failed to serialize message"}"#
                    .to_owned()
            })
        }

        // Build the request-specific logger early so it can cover all phases
        // including mesh loading.  Every pipeline message is sent back to the
        // client as a Log/PhaseMarker frame and is also written to stderr.
        let logger = WsLogger::new(tx.clone());

        // Start overall timing for the entire process
        let t_total = PhaseTimer::start(phases::TOTAL, &logger);

        // Load each scene object (auto-detecting format from its extension) and
        // bake its transform.  The objects stay separate: `slice_plate` decides
        // whether the plate can be merged into one mesh (the default) or has to
        // keep per-object identity for exclude-object / sequential printing.
        let t_load = PhaseTimer::start(phases::MESH_LOAD, &logger);
        let mut plate_objects: Vec<crate::core::ObjectInput> = Vec::new();
        // A multi-part file (3MF) backs several plate objects, so cache its
        // parsed parts: re-reading and re-parsing the archive once per part
        // would cost the same work N times for no gain. Each part carries the
        // health report from its own validation pass.
        let mut parts_cache: std::collections::HashMap<
            std::path::PathBuf,
            Vec<crate::scene::LoadedPart>,
        > = std::collections::HashMap::new();

        for (path, part_index, transform, _, support_paint) in &slice_inputs {
            if !parts_cache.contains_key(path) {
                match crate::scene::load_path_multi_reporting(
                    path,
                    &crate::mesh::repair::RepairOptions::default(),
                ) {
                    Ok(parts) => {
                        // Tell the client what shape its models were in — the
                        // warning is relayed into the UI log alongside every
                        // other pipeline message. Reported here, on the first
                        // read, so a file backing several plate objects is
                        // still only reported once per part.
                        let file = path
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_else(|| path.display().to_string());
                        let multi = parts.len() > 1;
                        for (index, part) in parts.iter().enumerate() {
                            let label = match (&part.name, multi) {
                                (Some(name), _) => format!("{file} ({name})"),
                                (None, true) => format!("{file} #{}", index + 1),
                                (None, false) => file.clone(),
                            };
                            crate::mesh::repair::log_report(&logger, &label, &part.report);
                        }
                        parts_cache.insert(path.clone(), parts);
                    }
                    Err(e) => {
                        let msg = ServerMessage::error(format!(
                            "Failed to load mesh {}: {}",
                            path.display(),
                            e
                        ));
                        let _ = tx.blocking_send(to_json(&msg));
                        return None;
                    }
                }
            }
            let parts = &parts_cache[path];
            let Some(part) = parts.get(*part_index) else {
                let msg = ServerMessage::error(format!(
                    "{} has no object at index {} (it contains {})",
                    path.display(),
                    part_index,
                    parts.len()
                ));
                let _ = tx.blocking_send(to_json(&msg));
                return None;
            };
            // Bake the per-object transform exactly once, at the slicer
            // boundary — see the SSOT contract in src/scene/README.md.
            let baked = crate::scene::apply_transform(&part.mesh, transform);
            // Name the object after the part inside its file, falling back to
            // the file stem — this is what the firmware's cancel UI shows.
            let name = part
                .name
                .as_deref()
                .map(str::trim)
                .filter(|n| !n.is_empty())
                .map(str::to_string)
                .or_else(|| path.file_stem().map(|s| s.to_string_lossy().into_owned()))
                .unwrap_or_else(|| format!("object_{}", plate_objects.len()));
            let mut object_input = crate::core::ObjectInput::new(name, baked);
            if let Some(encoded) = support_paint.as_deref() {
                match crate::mesh::paint::FacetPaint::decode(encoded, object_input.mesh.faces.len())
                {
                    Ok(paint) => object_input = object_input.with_paint(paint),
                    Err(e) => {
                        let msg = ServerMessage::error(format!(
                            "Invalid support paint for '{}': {}",
                            object_input.name, e
                        ));
                        let _ = tx.blocking_send(to_json(&msg));
                        return None;
                    }
                }
            }
            plate_objects.push(object_input);
        }
        if plate_objects.iter().all(|o| o.mesh.faces.is_empty()) {
            let msg = ServerMessage::error(
                "Combined scene has no triangles — nothing to slice".to_string(),
            );
            let _ = tx.blocking_send(to_json(&msg));
            return None;
        }
        t_load.finish();

        for warning in params.unsupported_feature_warnings() {
            logger.log_warn(&format!("[slice] {warning}"));
        }

        let plate = crate::core::slice_plate(&plate_objects, &params, &logger);
        let layer_count = plate.layers.len();

        let progress = ServerMessage::Progress {
            current_layer: layer_count,
            total_layers: layer_count,
        };
        let _ = tx.blocking_send(to_json(&progress));

        let t_gcode = PhaseTimer::start(phases::GCODE_GENERATION, &logger);
        let gcode = crate::gcode::generate_gcode_for_plate(&plate, &params);
        t_gcode.finish();

        // Write G-code to disk
        let t_write = PhaseTimer::start(phases::FILE_WRITE, &logger);
        if let Err(e) = std::fs::write(&gcode_output_path_clone, &gcode) {
            let msg = ServerMessage::error(format!("Failed to write G-code file: {e}"));
            let _ = tx.blocking_send(to_json(&msg));
            return None;
        }
        t_write.finish();

        // Finish overall timing
        t_total.finish();

        Some(layer_count)
    });

    // Forward channel messages to the WebSocket until the task finishes
    while let Some(msg_str) = rx.recv().await {
        if session.text(msg_str).await.is_err() {
            break;
        }
    }

    // If the blocking task panicked the channel closes silently — the UI
    // would hang forever with no response. Detect the panic and send an
    // error frame so the client can surface a meaningful message.
    let sliced_layer_count = match slice_handle.await {
        Ok(count) => count,
        Err(join_err) => {
            let panic_msg = if join_err.is_panic() {
                let payload = join_err.into_panic();
                payload
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_else(|| "unknown panic".to_string())
            } else {
                join_err.to_string()
            };
            let _ = send_msg(
                session,
                &ServerMessage::error(format!("Slicing failed unexpectedly: {panic_msg}")),
            )
            .await;
            None
        }
    };

    // Update database with G-code file info before announcing completion, so
    // the client can fetch `/api/download/{uuid}` immediately without racing
    // a not-yet-populated `download_file_path` (blank viewer / 404).
    if let Some(layer_count) = sliced_layer_count {
        let file_size = match std::fs::metadata(&gcode_output_path).map(|m| m.len()) {
            Ok(size) => size,
            Err(e) => {
                let _ = send_msg(
                    session,
                    &ServerMessage::error(format!(
                        "Failed to inspect G-code output for download: {e}"
                    )),
                )
                .await;
                return;
            }
        };

        if let Err(e) = db
            .set_download_file(uuid, &gcode_output_path, file_size)
            .await
        {
            let _ = send_msg(
                session,
                &ServerMessage::error(format!("Failed to register G-code download: {e}")),
            )
            .await;
            return;
        }

        // Populate the content cache so an identical future scene skips slicing.
        let _ = db
            .put_cached_gcode(&cache_key, &gcode_output_path, file_size, layer_count)
            .await;

        let _ = send_msg(
            session,
            &ServerMessage::SliceComplete {
                layer_count,
                download_url: format!("{}/api/download/{}", base_url, uuid),
            },
        )
        .await;
    }
}

/// Fetch and send a list of previously completed slicing sessions.
async fn handle_list_sessions(
    session: &mut actix_ws::Session,
    db: std::sync::Arc<crate::db::Database>,
    base_url: String,
) {
    // Query database for completed sessions and their files (uploads now live
    // in a separate table — pull the first file's name as the workplate
    // label).
    let sessions = db.get_completed_sessions().await.unwrap_or_default();
    let mut sessions_with_files: Vec<(crate::db::RequestSession, Option<String>)> = Vec::new();
    for session in sessions {
        let ruuid = session.request_uuid;
        let filename = db
            .get_files_for_request(ruuid)
            .await
            .ok()
            .and_then(|files| files.into_iter().next())
            .map(|f| f.original_filename);
        sessions_with_files.push((session, filename));
    }

    let summaries = sessions_with_files
        .into_iter()
        .map(|(session, filename)| {
            use crate::ws_protocol::SessionSummary;
            SessionSummary {
                request_uuid: session.request_uuid.to_string(),
                original_filename: filename,
                layer_count: session.download_file_size.map(|size| size as usize),
                created_at: session.created_at.to_rfc3339(),
                download_url: format!("{}/api/download/{}", base_url, session.request_uuid),
            }
        })
        .collect::<Vec<_>>();

    let msg = ServerMessage::SessionsList {
        sessions: summaries,
    };
    let _ = send_msg(session, &msg).await;
}

/// Serialize a [`ServerMessage`] to JSON and send it as a WebSocket text frame.
///
/// Falls back to a hard-coded error JSON string in the (very unlikely) event
/// that serialization itself fails, ensuring the client always receives valid
/// JSON rather than an empty frame.
async fn send_msg(
    session: &mut actix_ws::Session,
    msg: &ServerMessage,
) -> Result<(), actix_ws::Closed> {
    const SERIALIZATION_ERROR: &str =
        r#"{"type":"error","message":"Internal error: failed to serialize message"}"#;
    let json = serde_json::to_string(msg).unwrap_or_else(|_| SERIALIZATION_ERROR.to_owned());
    session.text(json).await
}

/// Apply a sequence of [`SceneOpDto`]s to the per-session scene and send back
/// the resulting [`ServerMessage::SceneState`] snapshot.
///
/// Mesh data for `Add` is sourced from the DB by `file_id` — the `file_uuid`
/// returned in `ofids` from `POST /api/upload`.
async fn handle_scene_ops(
    session: &mut actix_ws::Session,
    scene: &mut SceneState,
    ops: Vec<SceneOpDto>,
    options: crate::ws_protocol::SceneOptionsDto,
    work_dir: &std::path::Path,
    db: &Arc<crate::db::Database>,
) {
    let scene_options = crate::scene::SceneOptions {
        gravity: options.gravity,
    };
    for dto in ops {
        let op = match dto_to_op(dto, work_dir, db).await {
            Ok(op) => op,
            Err(e) => {
                let _ = send_msg(session, &ServerMessage::error(e)).await;
                return;
            }
        };
        if let Err(e) = scene.apply_with_options(op, scene_options) {
            let _ = send_msg(session, &ServerMessage::error(e.to_string())).await;
            return;
        }
    }
    let _ = send_msg(session, &snapshot_msg(scene)).await;
}

/// Translate a wire-format [`SceneOpDto`] into the internal [`SceneOp`].
///
/// For `Add` the mesh bytes are read from disk based on the upload `file_id`
/// (a `file_uuid` from `ofids`). The DB lookup gives us the actual on-disk
/// path including its extension.
async fn dto_to_op(
    dto: SceneOpDto,
    work_dir: &std::path::Path,
    db: &crate::db::Database,
) -> Result<SceneOp, String> {
    use crate::scene::Transform;
    let _ = work_dir; // path now comes from the DB; arg kept for signature symmetry
    match dto {
        SceneOpDto::Add {
            name,
            format,
            file_id,
        } => {
            let uuid = Uuid::parse_str(&file_id)
                .map_err(|e| format!("invalid file_id '{}': {}", file_id, e))?;
            let entry = db
                .get_file(uuid)
                .await
                .map_err(|e| format!("database error: {e}"))?
                .ok_or_else(|| format!("unknown file_id {}", uuid))?;
            let bytes = std::fs::read(&entry.file_path).map_err(|e| {
                format!("failed to read upload {}: {}", entry.file_path.display(), e)
            })?;
            Ok(SceneOp::Add {
                name,
                format,
                bytes,
                // Remember which upload this object came from so a later
                // slice resolves it without positional guesswork.
                source_id: Some(file_id),
            })
        }
        SceneOpDto::Remove { id } => Ok(SceneOp::Remove {
            id: crate::scene::ObjectId(id),
        }),
        SceneOpDto::RemoveMany { ids } => Ok(SceneOp::RemoveMany {
            ids: ids.into_iter().map(crate::scene::ObjectId).collect(),
        }),
        SceneOpDto::Duplicate { id, offset } => Ok(SceneOp::Duplicate {
            id: crate::scene::ObjectId(id),
            offset,
        }),
        SceneOpDto::Translate { id, delta } => Ok(SceneOp::Translate {
            id: crate::scene::ObjectId(id),
            delta,
        }),
        SceneOpDto::SetTransform {
            id,
            translation,
            euler_xyz_deg,
            scale,
        } => Ok(SceneOp::SetTransform {
            id: crate::scene::ObjectId(id),
            transform: Transform::from_euler_xyz_deg(translation, euler_xyz_deg, scale),
        }),
        SceneOpDto::Rotate { id, axis, degrees } => Ok(SceneOp::Rotate {
            id: crate::scene::ObjectId(id),
            axis,
            radians: degrees.to_radians(),
        }),
        SceneOpDto::Scale { id, factors } => Ok(SceneOp::Scale {
            id: crate::scene::ObjectId(id),
            factors,
        }),
        SceneOpDto::CenterOnBed { id } => Ok(SceneOp::CenterOnBed {
            id: crate::scene::ObjectId(id),
        }),
        SceneOpDto::DropToFloor { id } => Ok(SceneOp::DropToFloor {
            id: crate::scene::ObjectId(id),
        }),
        SceneOpDto::PlaceFaceOnFloor { id, face_index } => Ok(SceneOp::PlaceFaceOnFloor {
            id: crate::scene::ObjectId(id),
            face_index,
        }),
        SceneOpDto::AutoOrient { id, options } => Ok(SceneOp::AutoOrient {
            id: crate::scene::ObjectId(id),
            options,
        }),
        SceneOpDto::ArrangeOnBed { ids, options } => Ok(SceneOp::ArrangeOnBed {
            ids: ids.into_iter().map(crate::scene::ObjectId).collect(),
            options,
        }),
        SceneOpDto::PaintSupport {
            id,
            seed_face,
            center,
            radius,
            state,
        } => Ok(SceneOp::PaintSupport {
            id: crate::scene::ObjectId(id),
            seed_face,
            center,
            radius,
            state,
        }),
        SceneOpDto::SetSupportPaint { id, encoded } => Ok(SceneOp::SetSupportPaint {
            id: crate::scene::ObjectId(id),
            encoded,
        }),
    }
}

/// Build a [`ServerMessage::SceneState`] snapshot from the current scene.
fn snapshot_msg(scene: &SceneState) -> ServerMessage {
    let objects = scene
        .objects
        .iter()
        .map(|o| {
            let world = o.world_aabb();
            SceneObjectDto {
                id: o.id.0,
                name: o.name.clone(),
                translation: o.transform.translation,
                euler_xyz_deg: o.transform.to_euler_xyz_deg(),
                scale: o.transform.scale,
                triangle_count: o.mesh.faces.len(),
                world_aabb: [
                    [world.min.x, world.min.y, world.min.z],
                    [world.max.x, world.max.y, world.max.z],
                ],
                support_paint: o.paint.encode(),
                painted_facets: o.paint.painted_count(),
            }
        })
        .collect();
    ServerMessage::SceneState {
        objects,
        bed: BedConfigDto {
            width: scene.bed.width,
            depth: scene.bed.depth,
            height: scene.bed.height,
            origin_offset_x: scene.bed.origin_offset_x,
            origin_offset_y: scene.bed.origin_offset_y,
        },
    }
}

/// Derive a stable content key for a slice request.
///
/// Two requests share a key iff they would produce equivalent G-code: same
/// engine version, same resolved [`SlicingParams`], and the same ordered list
/// of placed objects (file id + transform). Object order is preserved because
/// the merge order affects the combined mesh and therefore the output. The key
/// is a hex FNV-1a 64-bit hash — collision-free enough for a best-effort cache
/// and dependency-free (no crypto hash crate).
///
/// The params are fingerprinted via [`SlicingParams::cache_fingerprint`], which
/// omits the ephemeral, camera-derived thumbnail PNG payload — so a fresh
/// render's bytes never bust the cache (issue #106).
fn compute_slice_cache_key(
    scene: &[crate::ws_protocol::SceneObjectSliceDto],
    params: &crate::settings::params::SlicingParams,
) -> String {
    let mut canonical = String::new();
    canonical.push_str("v=");
    canonical.push_str(crate::version::VERSION);
    canonical.push_str(";params=");
    canonical.push_str(&params.cache_fingerprint());
    canonical.push_str(";scene=");
    for obj in scene {
        let t = &obj.transform;
        // `part_index` is part of the identity: two objects can share a
        // file_id yet be different parts of it, and omitting it would let
        // distinct plates collide on one cached G-code. `support_paint` is
        // included the same way — a repaint has to bust the cache, or the
        // server would keep serving G-code sliced before the stroke.
        canonical.push_str(&format!(
            "[{}#{}|{:?}|{:?}|{:?}|paint={:?}]",
            obj.file_id,
            obj.part_index,
            t.translation,
            t.euler_xyz_deg,
            t.scale,
            obj.support_paint.as_deref().unwrap_or("")
        ));
    }
    format!("{:016x}", fnv1a_64(canonical.as_bytes()))
}

/// FNV-1a 64-bit hash — deterministic across runs and platforms (unlike
/// `std::hash::DefaultHasher`, whose output is not stability-guaranteed).
fn fnv1a_64(bytes: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = OFFSET;
    for &b in bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

/// Probe a printer connection and reply with its live status.
///
/// The HTTP request runs server-side precisely so it is **not** subject to
/// browser CORS — Moonraker ships no permissive CORS headers, so a direct
/// browser `fetch` would fail for most users.
async fn handle_check_printer(
    session: &mut actix_ws::Session,
    printer_id: String,
    connection: crate::profiles::PrinterConnection,
) {
    let report = crate::printer::check_status(&connection).await;
    let msg = ServerMessage::PrinterStatus {
        printer_id,
        online: report.online,
        state: report.state,
        print_state: report.print_state,
        progress: report.progress,
        message: report.message,
    };
    let _ = send_msg(session, &msg).await;
}

/// Probe a URL to identify a printer and prefill the setup wizard.
///
/// Like [`handle_check_printer`], the HTTP request runs server-side so it is
/// **not** subject to browser CORS.
async fn handle_detect_printer(session: &mut actix_ws::Session, host: String) {
    let detection = crate::printer::detect_printer(&host).await;
    let msg = ServerMessage::PrinterDetected {
        host,
        reachable: detection.reachable,
        kind: detection.kind,
        message: detection.message,
        name: detection.name,
        model: detection.model,
        vendor: detection.vendor,
        firmware: detection.firmware,
        bed_shape: detection.bed_shape,
        bed_width: detection.bed_width,
        bed_depth: detection.bed_depth,
        bed_height: detection.bed_height,
        origin_at_center: detection.origin_at_center,
        nozzle_diameter_mm: detection.nozzle_diameter_mm,
        params: detection.params,
        findings: detection.findings,
        questions: detection.questions,
    };
    let _ = send_msg(session, &msg).await;
}

/// Upload a previously-sliced G-code file to a printer and reply with the
/// outcome.
#[allow(clippy::too_many_arguments)]
async fn handle_send_to_printer(
    session: &mut actix_ws::Session,
    request_uuid: String,
    printer_id: String,
    connection: crate::profiles::PrinterConnection,
    filename: Option<String>,
    start: bool,
    db: Arc<crate::db::Database>,
    work_dir: std::path::PathBuf,
) {
    let uuid = match Uuid::parse_str(&request_uuid) {
        Ok(u) => u,
        Err(e) => {
            let _ = send_msg(
                session,
                &ServerMessage::PrinterSendResult {
                    printer_id,
                    request_uuid,
                    ok: false,
                    message: format!("Invalid request UUID: {e}"),
                    started: false,
                },
            )
            .await;
            return;
        }
    };

    // Prefer the DB-recorded download path (authoritative), then fall back to
    // the conventional `{uuid}.gcode` in the work dir.
    let gcode_path = match db.get_request(uuid).await {
        Ok(Some(req)) => req.download_file_path,
        _ => None,
    }
    .unwrap_or_else(|| work_dir.join(format!("{}.gcode", uuid)));

    if !gcode_path.exists() {
        let _ = send_msg(
            session,
            &ServerMessage::PrinterSendResult {
                printer_id,
                request_uuid,
                ok: false,
                message: "No sliced G-code found for this scene — slice it first".to_string(),
                started: false,
            },
        )
        .await;
        return;
    }

    let default_name = format!("{}.gcode", uuid);
    let name = filename.unwrap_or(default_name);

    let result = crate::printer::send_gcode(&connection, &gcode_path, &name, start).await;
    let msg = match result {
        Ok(outcome) => ServerMessage::PrinterSendResult {
            printer_id,
            request_uuid,
            ok: true,
            message: outcome.message,
            started: outcome.started,
        },
        Err(e) => ServerMessage::PrinterSendResult {
            printer_id,
            request_uuid,
            ok: false,
            message: e,
            started: false,
        },
    };
    let _ = send_msg(session, &msg).await;
}

#[cfg(test)]
mod cache_key_tests {
    use super::*;
    use crate::settings::params::SlicingParams;
    use crate::ws_protocol::{SceneObjectSliceDto, TransformDto};

    fn object(paint: Option<&str>) -> SceneObjectSliceDto {
        SceneObjectSliceDto {
            file_id: "11111111-1111-1111-1111-111111111111".to_string(),
            part_index: 0,
            transform: TransformDto::default(),
            support_paint: paint.map(str::to_string),
        }
    }

    #[test]
    fn the_cache_key_changes_when_support_paint_is_added() {
        let params = SlicingParams::default();
        let unpainted = compute_slice_cache_key(&[object(None)], &params);
        let painted = compute_slice_cache_key(&[object(Some("abc"))], &params);
        assert_ne!(
            unpainted, painted,
            "a repaint must bust the cache, or the server would keep serving \
             G-code sliced before the stroke"
        );
    }

    #[test]
    fn the_cache_key_distinguishes_two_different_paint_payloads() {
        let params = SlicingParams::default();
        let a = compute_slice_cache_key(&[object(Some("aaa"))], &params);
        let b = compute_slice_cache_key(&[object(Some("bbb"))], &params);
        assert_ne!(a, b, "different paint must not collide on one cache entry");
    }

    #[test]
    fn the_cache_key_is_stable_for_identical_input() {
        let params = SlicingParams::default();
        let a = compute_slice_cache_key(&[object(Some("abc"))], &params);
        let b = compute_slice_cache_key(&[object(Some("abc"))], &params);
        assert_eq!(a, b);
    }
}
