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
}

impl WsLogger {
    fn new(tx: tokio::sync::mpsc::Sender<String>) -> Self {
        Self {
            global: StderrLogger,
            tx,
        }
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
        let server_msg = crate::ws_protocol::ServerMessage::PhaseMarker {
            phase: phase.to_string(),
            event: "start".to_string(),
            elapsed_ms: None,
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
        let server_msg = crate::ws_protocol::ServerMessage::PhaseMarker {
            phase: phase.to_string(),
            event: "end".to_string(),
            elapsed_ms: Some(elapsed_ms),
        };
        let json = serde_json::to_string(&server_msg).unwrap_or_else(|_| {
            format!(
                r#"{{"type":"PhaseMarker","phase":"{}","event":"end","elapsed_ms":{}}}"#,
                phase, elapsed_ms
            )
        });
        let _ = self.tx.blocking_send(json);
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

    actix_web::rt::spawn(handle_ws_session(
        session, msg_stream, db, work_dir, base_url,
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

    while let Some(Ok(msg)) = stream.next().await {
        use actix_ws::AggregatedMessage;
        match msg {
            AggregatedMessage::Text(text) => match serde_json::from_str::<ClientMessage>(&text) {
                Ok(ClientMessage::Slice {
                    request_uuid,
                    scene: scene_objects,
                    profiles,
                    settings,
                }) => {
                    logger.log_debug(&format!("[WS] Processing slice request: {}", request_uuid));
                    // Resolve the parameters: prefer the structured profile
                    // selection (engine-owned composition); fall back to the
                    // legacy pre-flattened settings, then to defaults.
                    let params = match resolve_slice_params(profiles, settings) {
                        Ok(p) => Box::new(p),
                        Err(e) => {
                            logger.log_warn(&format!("[WS] Invalid profile selection: {e}"));
                            let _ = send_msg(
                                &mut session,
                                &ServerMessage::error(format!("Invalid profile selection: {e}")),
                            )
                            .await;
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
/// engine defaults. Returns an error only when a provided profile selection
/// fails to resolve (e.g. a malformed override diff).
fn resolve_slice_params(
    profiles: Option<Box<crate::profiles::ProfileSelection>>,
    settings: Option<Box<crate::settings::params::SlicingParams>>,
) -> Result<crate::settings::params::SlicingParams, serde_json::Error> {
    if let Some(selection) = profiles {
        return selection.resolve();
    }
    Ok(settings.map(|b| *b).unwrap_or_default())
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

    let mut slice_inputs: Vec<(std::path::PathBuf, Transform, u64)> =
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
        slice_inputs.push((entry.file_path, transform, entry.file_size as u64));
    }

    let total_bytes: u64 = slice_inputs.iter().map(|(_, _, sz)| *sz).sum();
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

    let slice_handle = tokio::task::spawn_blocking(move || {
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

        // Load each scene object (auto-detecting format from its extension),
        // bake its transform, and concatenate faces into a single combined
        // mesh that the slicer pipeline sees.
        let t_load = PhaseTimer::start(phases::MESH_LOAD, &logger);
        let mut combined = crate::mesh::types::Mesh::new();
        for (path, transform, _) in &slice_inputs {
            let mesh = match crate::scene::load_path(path) {
                Ok(m) => m,
                Err(e) => {
                    let msg = ServerMessage::error(format!(
                        "Failed to load mesh {}: {}",
                        path.display(),
                        e
                    ));
                    let _ = tx.blocking_send(to_json(&msg));
                    return;
                }
            };
            // Bake the per-object transform exactly once, at the slicer
            // boundary — see the SSOT contract in src/scene/README.md.
            let baked = crate::scene::apply_transform(&mesh, transform);
            combined.vertices.extend(baked.vertices);
            combined.faces.extend(baked.faces);
        }
        if combined.faces.is_empty() {
            let msg = ServerMessage::error(
                "Combined scene has no triangles — nothing to slice".to_string(),
            );
            let _ = tx.blocking_send(to_json(&msg));
            return;
        }
        t_load.finish();

        for warning in params.unsupported_feature_warnings() {
            logger.log_warn(&format!("[slice] {warning}"));
        }

        let layers = crate::core::process_mesh(&combined, &params, &logger);
        let layer_count = layers.len();

        let progress = ServerMessage::Progress {
            current_layer: layer_count,
            total_layers: layer_count,
        };
        let _ = tx.blocking_send(to_json(&progress));

        let t_gcode = PhaseTimer::start(phases::GCODE_GENERATION, &logger);
        let gcode = crate::gcode::generate_gcode(&layers, &params);
        t_gcode.finish();

        // Write G-code to disk
        let t_write = PhaseTimer::start(phases::FILE_WRITE, &logger);
        if let Err(e) = std::fs::write(&gcode_output_path_clone, &gcode) {
            let msg = ServerMessage::error(format!("Failed to write G-code file: {e}"));
            let _ = tx.blocking_send(to_json(&msg));
            return;
        }
        t_write.finish();

        let complete = ServerMessage::SliceComplete {
            layer_count,
            download_url: format!("{}/api/download/{}", base_url, uuid),
        };
        let _ = tx.blocking_send(to_json(&complete));

        // Finish overall timing
        t_total.finish();
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
    if let Err(join_err) = slice_handle.await {
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
    }

    // Update database with G-code file info
    if let Ok(file_size) = std::fs::metadata(&gcode_output_path).map(|m| m.len()) {
        let _ = db
            .set_download_file(uuid, &gcode_output_path, file_size)
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
            })
        }
        SceneOpDto::Remove { id } => Ok(SceneOp::Remove {
            id: crate::scene::ObjectId(id),
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
