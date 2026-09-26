//! HTTP request handlers for upload, download, and configuration operations.

use actix_web::web;
use std::sync::Arc;

/// Response from `POST /api/upload`.
///
/// `ruuid` is the workplate / scene identifier; `ofids` is the list of file
/// identifiers that have been placed in that scene. Today there is exactly
/// one file per upload, but the protocol intentionally supports multiple so
/// the slice path doesn't have to change when multi-file UX lands.
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct UploadResponse {
    pub ruuid: String,
    pub ofids: Vec<String>,
}

pub struct AppState {
    pub db: Arc<crate::db::Database>,
    pub work_dir: std::path::PathBuf,
    /// Broadcasts a profile-category token whenever the on-disk profile library
    /// changes, so every open WebSocket session can tell its client to refetch.
    pub profiles_changed: tokio::sync::broadcast::Sender<String>,
    /// Broadcasts a plate whenever one is written, so a second person working
    /// on the same plate is *told* rather than silently overwritten.
    pub workplates_changed: tokio::sync::broadcast::Sender<WorkplateChange>,
    /// The object library every upload is recorded into.
    pub library: crate::library::LibraryStore,
}

/// One plate having been written, on its way to every other open session.
#[derive(Clone, Debug)]
pub struct WorkplateChange {
    /// The plate's `request_uuid`.
    pub request_uuid: String,
    /// When the change was recorded, RFC 3339.
    pub updated_at: Option<String>,
    /// The client that made it, from `X-Client-Id`.
    ///
    /// Carried so a session can skip its own writes. Without it every save
    /// would bounce straight back as "someone changed this plate" — which is
    /// exactly the prompt the feature exists to make meaningful.
    pub client: Option<String>,
}

/// The `X-Client-Id` a request identified itself with, when it sent one.
///
/// Opaque and self-assigned: it exists only to tell "this browser tab" from
/// "some other browser tab", never to say who anyone is.
pub fn client_id_of(req: &actix_web::HttpRequest) -> Option<String> {
    req.headers()
        .get("X-Client-Id")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty() && value.len() <= 128)
        .map(str::to_string)
}

/// `Cache-Control` for a body that can never change under its own URL.
///
/// A year is the conventional "forever"; `immutable` is what stops the browser
/// revalidating on a plain reload, which is the case that matters here.
pub const IMMUTABLE_CACHE: &str = "private, max-age=31536000, immutable";

// ── Config handlers ───────────────────────────────────────────────────────────

/// Request body for `GET /api/config`.
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct ConfigResponse {
    pub config: crate::config::AppConfig,
}

/// Request body for `PATCH /api/config`.
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct PatchConfigRequest {
    /// Dot-separated key, e.g. `"slicing.layer_height"` or `"server.port"`.
    pub key: String,
    /// New value (JSON-typed).
    pub value: serde_json::Value,
}

/// `GET /api/config` — return the fully-merged runtime configuration.
pub async fn get_config_handler() -> actix_web::HttpResponse {
    match crate::config::load_and_merge_config(None) {
        Ok(config) => actix_web::HttpResponse::Ok().json(ConfigResponse { config }),
        Err(e) => actix_web::HttpResponse::InternalServerError()
            .json(serde_json::json!({ "error": e.to_string() })),
    }
}

/// `PATCH /api/config` — update a single config key and persist to `slicer.toml`.
pub async fn patch_config_handler(body: web::Json<PatchConfigRequest>) -> actix_web::HttpResponse {
    use crate::cli::commands::config::apply_config_field;
    use crate::config::{config_file, load_config, save_config};

    let toml_path = config_file();

    let mut config = match load_config(&toml_path) {
        Ok(c) => c,
        Err(e) => {
            return actix_web::HttpResponse::InternalServerError()
                .json(serde_json::json!({ "error": e.to_string() }));
        }
    };

    if let Err(e) = apply_config_field(&mut config, &body.key, &body.value) {
        return actix_web::HttpResponse::BadRequest()
            .json(serde_json::json!({ "error": e.to_string() }));
    }

    if let Err(e) = save_config(&config, &toml_path) {
        return actix_web::HttpResponse::InternalServerError()
            .json(serde_json::json!({ "error": e.to_string() }));
    }

    actix_web::HttpResponse::Ok().json(serde_json::json!({
        "key": body.key,
        "value": body.value,
        "message": "Configuration updated and persisted to slicer.toml",
    }))
}

// ── Profile library handlers ──────────────────────────────────────────────────

/// `GET /api/profiles` — return the whole user-owned profile library
/// (printers, filaments, processes, labels) persisted beside the engine.
///
/// This is what the UI hydrates from on startup in cloud mode, so the user's
/// printers/filaments/profiles live with the slicer and survive a browser
/// cache wipe.
pub async fn get_profiles_handler() -> actix_web::HttpResponse {
    match crate::profiles::ProfileStore::new().load() {
        Ok(library) => actix_web::HttpResponse::Ok().json(library),
        Err(e) => actix_web::HttpResponse::InternalServerError()
            .json(serde_json::json!({ "error": e.to_string() })),
    }
}

/// `PUT /api/profiles/{kind}` — replace one category (`printers`, `filaments`,
/// `processes`, or `labels`) with the posted JSON array and persist to
/// `profiles.toml`.
///
/// Whole-category, last-writer-wins: the UI sends the full list for a category
/// on any add / edit / delete, mirroring how it used to overwrite the whole
/// localStorage blob. Returns the updated library.
pub async fn put_profiles_category_handler(
    path: web::Path<String>,
    body: web::Json<serde_json::Value>,
    state: web::Data<AppState>,
) -> actix_web::HttpResponse {
    let Some(kind) = crate::profiles::ProfileKind::parse(&path.into_inner()) else {
        return actix_web::HttpResponse::NotFound()
            .json(serde_json::json!({ "error": "unknown profile category" }));
    };

    match crate::profiles::ProfileStore::new().replace_category(kind, body.into_inner()) {
        Ok(library) => {
            // Nudge every open WebSocket session to refetch this category.
            // `send` errors only when there are no subscribers, which is fine.
            let _ = state.profiles_changed.send(kind.as_str().to_string());
            actix_web::HttpResponse::Ok().json(library)
        }
        Err(e) => actix_web::HttpResponse::BadRequest()
            .json(serde_json::json!({ "error": e.to_string() })),
    }
}

/// `GET /api/workplates/{request_uuid}` — the saved setup for one plate.
///
/// Returns `{}` for a plate nobody has configured, rather than 404: "this plate
/// has no saved setup" and "this plate does not exist" are the same thing to the
/// caller, which is about to render the defaults either way.
pub async fn get_workplate_handler(
    path: web::Path<String>,
    state: web::Data<AppState>,
) -> actix_web::HttpResponse {
    let Ok(uuid) = uuid::Uuid::parse_str(&path.into_inner()) else {
        return actix_web::HttpResponse::BadRequest()
            .json(serde_json::json!({ "error": "invalid workplate uuid" }));
    };
    match state.db.get_workplate_setup(uuid).await {
        Ok(setup) => actix_web::HttpResponse::Ok()
            // Never cached. This is the document a plate is rebuilt from, and
            // it changes whenever anyone touches the plate — a cached copy is
            // how one person's arrangement quietly replaces another's.
            .insert_header((actix_web::http::header::CACHE_CONTROL, "no-store"))
            .json(setup.unwrap_or_else(crate::workplate::WorkplateSetup::default)),
        Err(e) => actix_web::HttpResponse::InternalServerError()
            .json(serde_json::json!({ "error": e.to_string() })),
    }
}

/// `PUT /api/workplates/{request_uuid}` — replace one plate's saved setup.
///
/// Whole-document, last writer wins, matching how a profile category is
/// written. The body is a [`WorkplateSetup`](crate::workplate::WorkplateSetup):
/// three profile ids, the user's sparse override diff, and where each object
/// sits. Never mesh bytes, and never a copy of a profile.
pub async fn put_workplate_handler(
    req: actix_web::HttpRequest,
    path: web::Path<String>,
    body: web::Json<crate::workplate::WorkplateSetup>,
    state: web::Data<AppState>,
) -> actix_web::HttpResponse {
    let Ok(uuid) = uuid::Uuid::parse_str(&path.into_inner()) else {
        return actix_web::HttpResponse::BadRequest()
            .json(serde_json::json!({ "error": "invalid workplate uuid" }));
    };
    let mut setup = body.into_inner();
    setup.updated_at = Some(chrono::Utc::now().to_rfc3339());

    match state.db.save_workplate_setup(uuid, &setup).await {
        Ok(()) => {
            // Tell everyone else looking at this plate. `send` errors only when
            // there are no subscribers, which is the ordinary single-user case.
            let _ = state.workplates_changed.send(WorkplateChange {
                request_uuid: uuid.to_string(),
                updated_at: setup.updated_at.clone(),
                client: client_id_of(&req),
            });
            actix_web::HttpResponse::Ok().json(setup)
        }
        Err(e) => actix_web::HttpResponse::InternalServerError()
            .json(serde_json::json!({ "error": e.to_string() })),
    }
}

/// `GET /api/openapi.json` — this server's OpenAPI 3.1 document.
///
/// Generated from the engine's own Rust types on every request, so it describes
/// what *this build* accepts rather than what a checked-in spec last claimed.
/// Cheap enough to build per call, and building it per call is what removes any
/// possibility of serving a stale one.
pub async fn openapi_handler() -> actix_web::HttpResponse {
    actix_web::HttpResponse::Ok().json(super::openapi::document())
}

/// `GET /api/docs` — a browsable reference rendered from that document.
///
/// Self-contained: no CDN, no bundled viewer library. A self-hosted slicer is
/// routinely a machine on a workshop network with no route out, and a reference
/// page that goes blank without internet is worse than no page at all.
pub async fn api_docs_handler() -> actix_web::HttpResponse {
    actix_web::HttpResponse::Ok()
        .content_type("text/html; charset=utf-8")
        .body(include_str!("docs.html"))
}

/// Query for `GET /api/profiles/export`.
#[derive(serde::Deserialize)]
pub struct ProfileExportQuery {
    /// `bundle` (ZIP of one TOML per profile) or `toml` (single
    /// `profiles.toml`). Defaults to `bundle`.
    pub format: Option<String>,
}

/// `GET /api/profiles/export` — download the profile library as TOML.
///
/// Exports what is actually persisted beside the engine, so the artifact is the
/// same data the CLI would read on this machine. See
/// [`crate::profiles::export`] for the two shapes.
pub async fn export_profiles_handler(
    query: web::Query<ProfileExportQuery>,
) -> actix_web::HttpResponse {
    let token = query.format.as_deref().unwrap_or("bundle");
    let Some(format) = crate::profiles::ProfileExportFormat::parse(token) else {
        return actix_web::HttpResponse::BadRequest()
            .json(serde_json::json!({ "error": format!("unknown export format '{token}'") }));
    };

    let library = match crate::profiles::ProfileStore::new().load() {
        Ok(library) => library,
        Err(e) => {
            return actix_web::HttpResponse::InternalServerError()
                .json(serde_json::json!({ "error": e.to_string() }))
        }
    };

    match crate::profiles::export_library(&library, format) {
        Ok(artifact) => actix_web::HttpResponse::Ok()
            .content_type(artifact.mime)
            .insert_header((
                actix_web::http::header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{}\"", artifact.filename),
            ))
            .body(artifact.bytes),
        Err(e) => actix_web::HttpResponse::InternalServerError()
            .json(serde_json::json!({ "error": e.to_string() })),
    }
}

/// `DELETE /api/history` — drop every slicing session, uploaded-file row, and
/// cached G-code entry (and their on-disk artifacts). Backs the settings Danger
/// Zone "Clear slice history" action. Profiles and configuration are untouched.
pub async fn delete_history_handler(state: web::Data<AppState>) -> actix_web::HttpResponse {
    match state.db.clear_history().await {
        Ok(removed) => actix_web::HttpResponse::Ok().json(serde_json::json!({
            "removed": removed,
            "message": "Slice history and G-code cache cleared",
        })),
        Err(e) => actix_web::HttpResponse::InternalServerError()
            .json(serde_json::json!({ "error": e.to_string() })),
    }
}

/// Handle file upload: save the file with its original extension and return
/// `{ ruuid, ofids: [file_uuid] }`. The workplate UUID and the file UUID are
/// distinct — the slice protocol references files by `file_uuid` and never by
/// `request_uuid` (the legacy "request UUID is also the file ID" convention
/// is gone).
///
/// An optional `ruuid` text field attaches the upload to an **existing**
/// workplate instead of creating a new one. That is how a plate accumulates
/// several models: every object's file hangs off the same `request_uuid`, so
/// `GET /api/request/:ruuid` lists them all and reopening the plate restores
/// every object rather than just the one it started from. Clients must send
/// `ruuid` *before* the `file` field, since multipart fields stream in order.
pub async fn upload_handler(
    req: actix_web::HttpRequest,
    state: web::Data<AppState>,
    mut multipart: actix_multipart::Multipart,
) -> Result<actix_web::HttpResponse, actix_web::Error> {
    use futures_util::StreamExt as _;
    use uuid::Uuid;

    let file_uuid = Uuid::new_v4();

    const MAX_FILE_SIZE: u64 = 500 * 1024 * 1024; // 500 MB limit
    let mut file_size: u64 = 0;
    let mut original_filename: Option<String> = None;
    let mut file_path: Option<std::path::PathBuf> = None;
    let mut existing_request: Option<Uuid> = None;

    // Process multipart fields
    while let Some(field_result) = multipart.next().await {
        let mut field = field_result.map_err(actix_web::error::ErrorBadRequest)?;

        if field.name() == Some("ruuid") {
            let mut raw = Vec::new();
            while let Some(chunk) = field.next().await {
                raw.extend_from_slice(&chunk.map_err(actix_web::error::ErrorBadRequest)?);
            }
            let text = String::from_utf8_lossy(&raw).trim().to_string();
            if !text.is_empty() {
                let uuid = Uuid::parse_str(&text)
                    .map_err(|_| actix_web::error::ErrorBadRequest("Invalid ruuid"))?;
                // Only adopt a workplate that actually exists; a stale id from
                // the client must not silently orphan the upload.
                if state
                    .db
                    .get_request(uuid)
                    .await
                    .map_err(actix_web::error::ErrorInternalServerError)?
                    .is_some()
                {
                    existing_request = Some(uuid);
                }
            }
            continue;
        }

        // Only process the "file" field
        if field.name() != Some("file") {
            continue;
        }

        // Extract original filename from Content-Disposition header
        if let Some(filename) = field
            .content_disposition()
            .and_then(|cd| cd.get_filename().map(|f| f.to_string()))
        {
            original_filename = Some(filename);
        }

        // Preserve the original extension on disk so the slicer can pick the
        // right loader without anyone having to re-encode the format hint
        // into the URL or the wire protocol.
        let ext = original_filename
            .as_deref()
            .and_then(|f| std::path::Path::new(f).extension())
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .unwrap_or_else(|| "stl".to_string());
        let path = state.work_dir.join(format!("{}.{}", file_uuid, ext));
        file_path = Some(path.clone());

        let mut file = tokio::fs::File::create(&path)
            .await
            .map_err(actix_web::error::ErrorInternalServerError)?;

        // Stream the file field data directly to disk
        while let Some(chunk_result) = field.next().await {
            let chunk = chunk_result.map_err(actix_web::error::ErrorBadRequest)?;
            file_size += chunk.len() as u64;

            if file_size > MAX_FILE_SIZE {
                let _ = tokio::fs::remove_file(&path).await;
                return Err(actix_web::error::ErrorPayloadTooLarge(
                    "File exceeds 500 MB limit",
                ));
            }

            tokio::io::AsyncWriteExt::write_all(&mut file, &chunk)
                .await
                .map_err(actix_web::error::ErrorInternalServerError)?;
        }

        break; // Only process first file field
    }

    let file_path = match file_path {
        Some(p) if file_size > 0 => p,
        Some(p) => {
            let _ = tokio::fs::remove_file(&p).await;
            return Err(actix_web::error::ErrorBadRequest("No file uploaded"));
        }
        None => return Err(actix_web::error::ErrorBadRequest("No file uploaded")),
    };

    // Reuse the caller's workplate when it named a live one, else start a new
    // one. Creating it only now keeps a rejected upload from leaving an empty
    // workplate behind.
    let request_uuid = match existing_request {
        Some(uuid) => uuid,
        None => {
            let uuid = Uuid::new_v4();
            state
                .db
                .create_request(uuid)
                .await
                .map_err(actix_web::error::ErrorInternalServerError)?;
            uuid
        }
    };

    // Update database with file info
    let filename = original_filename.unwrap_or_else(|| format!("{}.stl", file_uuid));
    state
        .db
        .add_upload_file(request_uuid, file_uuid, &filename, &file_path, file_size)
        .await
        .map_err(actix_web::error::ErrorInternalServerError)?;

    // Every model that reaches a plate reaches the library, and counts as a
    // use of it. Recorded in the background: hashing and measuring a large
    // model is slow, and the user is waiting on the plate, not on the library.
    {
        let store = state.library.clone();
        let path = file_path.clone();
        let name = filename.clone();
        tokio::task::spawn_blocking(move || {
            let recorded = std::fs::read(&path)
                .map_err(anyhow::Error::from)
                .and_then(|bytes| store.import_bytes(&name, &bytes))
                .and_then(|outcome| store.touch(&outcome.entry_id));
            if let Err(e) = recorded {
                eprintln!("library: could not record '{name}': {e}");
            }
        });
    }

    // A model landing on a plate someone else has open is a change to that
    // plate, same as moving one. A brand-new plate has nobody to tell.
    if existing_request.is_some() {
        let _ = state.workplates_changed.send(WorkplateChange {
            request_uuid: request_uuid.to_string(),
            updated_at: Some(chrono::Utc::now().to_rfc3339()),
            client: client_id_of(&req),
        });
    }

    Ok(actix_web::HttpResponse::Ok().json(UploadResponse {
        ruuid: request_uuid.to_string(),
        ofids: vec![file_uuid.to_string()],
    }))
}

/// Handle file download: stream G-code file to browser
pub async fn download_handler(
    state: web::Data<AppState>,
    request_uuid: web::Path<String>,
) -> Result<actix_web::HttpResponse, actix_web::Error> {
    let uuid_str = request_uuid.into_inner();
    let uuid = uuid::Uuid::parse_str(&uuid_str)
        .map_err(|_| actix_web::error::ErrorBadRequest("Invalid UUID"))?;

    // Look up session in database
    let session = state
        .db
        .get_request(uuid)
        .await
        .map_err(actix_web::error::ErrorInternalServerError)?
        .ok_or_else(|| actix_web::error::ErrorNotFound("Request not found"))?;

    let download_path = session
        .download_file_path
        .ok_or_else(|| actix_web::error::ErrorNotFound("G-code not ready"))?;

    // Generate download filename from the workplate's first uploaded file
    // (replace its extension with `.gcode`). Falls back to a generic name if
    // the request somehow has no associated file row.
    let files = state
        .db
        .get_files_for_request(uuid)
        .await
        .map_err(actix_web::error::ErrorInternalServerError)?;
    let download_filename = files
        .first()
        .map(|f| {
            let stem = std::path::Path::new(&f.original_filename)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("output");
            format!("{}.gcode", stem)
        })
        .unwrap_or_else(|| "output.gcode".to_string());

    // Read file and stream as response
    let content = tokio::fs::read(&download_path)
        .await
        .map_err(|_| actix_web::error::ErrorNotFound("G-code file not found"))?;

    Ok(actix_web::HttpResponse::Ok()
        .content_type("text/plain")
        .insert_header((
            "Content-Disposition",
            format!("attachment; filename=\"{}\"", download_filename),
        ))
        .body(content))
}

/// One file entry returned by `GET /api/request/:request_uuid`.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct RequestFileSummary {
    pub file_uuid: String,
    pub original_filename: String,
}

/// Response body for `GET /api/request/:request_uuid`.
///
/// Returns the workplate's status, the G-code download status, and the list
/// of file IDs (`ofids`-style) so the UI can rebuild a slice payload after a
/// page reload without having to re-upload anything.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct RequestMetaResponse {
    pub ruuid: String,
    pub status: String,
    pub has_gcode: bool,
    pub ofids: Vec<RequestFileSummary>,
}

/// `GET /api/request/:request_uuid` — return metadata for a workplate.
pub async fn get_request_handler(
    state: web::Data<AppState>,
    request_uuid: web::Path<String>,
) -> Result<actix_web::HttpResponse, actix_web::Error> {
    let uuid_str = request_uuid.into_inner();
    let uuid = uuid::Uuid::parse_str(&uuid_str)
        .map_err(|_| actix_web::error::ErrorBadRequest("Invalid UUID"))?;

    let session = state
        .db
        .get_request(uuid)
        .await
        .map_err(actix_web::error::ErrorInternalServerError)?
        .ok_or_else(|| actix_web::error::ErrorNotFound("Request not found"))?;

    let files = state
        .db
        .get_files_for_request(uuid)
        .await
        .map_err(actix_web::error::ErrorInternalServerError)?;

    let has_gcode = session
        .download_file_path
        .as_ref()
        .map(|p| p.exists())
        .unwrap_or(false);

    let status_str = format!("{:?}", session.status).to_lowercase();

    Ok(actix_web::HttpResponse::Ok()
        // Which files are on the plate changes whenever anyone adds one, and
        // it is what a restore iterates. Small, and never worth a stale copy.
        .insert_header((actix_web::http::header::CACHE_CONTROL, "no-store"))
        .json(RequestMetaResponse {
            ruuid: session.request_uuid.to_string(),
            status: status_str,
            has_gcode,
            ofids: files
                .into_iter()
                .map(|f| RequestFileSummary {
                    file_uuid: f.file_uuid.to_string(),
                    original_filename: f.original_filename,
                })
                .collect(),
        }))
}

/// `GET /api/file/:file_uuid` — stream an uploaded file back to the browser.
///
/// Replaces the legacy `/api/stl/:request_uuid` endpoint. The file's actual
/// extension is preserved in `original_filename` so the browser sees the
/// right name regardless of format.
pub async fn download_file_handler(
    req: actix_web::HttpRequest,
    state: web::Data<AppState>,
    file_uuid: web::Path<String>,
) -> Result<actix_web::HttpResponse, actix_web::Error> {
    let uuid_str = file_uuid.into_inner();
    let uuid = uuid::Uuid::parse_str(&uuid_str)
        .map_err(|_| actix_web::error::ErrorBadRequest("Invalid UUID"))?;

    let entry = state
        .db
        .get_file(uuid)
        .await
        .map_err(actix_web::error::ErrorInternalServerError)?
        .ok_or_else(|| actix_web::error::ErrorNotFound("File not found"))?;

    if !entry.file_path.exists() {
        return Err(actix_web::error::ErrorNotFound("File not found on disk"));
    }

    // An uploaded model never changes: a second upload of the same file is a
    // second `file_uuid`. So the id *is* the validator, and the browser may
    // keep the bytes for as long as it likes — which is what makes switching
    // back to a plate instant instead of a fresh megabyte download.
    //
    // `private`, not `public`: these are one person's models, and a shared
    // proxy has no business holding them.
    let etag = format!("\"{}\"", uuid);
    if req
        .headers()
        .get(actix_web::http::header::IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.split(',').any(|tag| tag.trim() == etag))
    {
        return Ok(actix_web::HttpResponse::NotModified()
            .insert_header((actix_web::http::header::ETAG, etag))
            .insert_header((actix_web::http::header::CACHE_CONTROL, IMMUTABLE_CACHE))
            .finish());
    }

    let content = tokio::fs::read(&entry.file_path)
        .await
        .map_err(|_| actix_web::error::ErrorNotFound("File could not be read"))?;

    Ok(actix_web::HttpResponse::Ok()
        .content_type("application/octet-stream")
        .insert_header((actix_web::http::header::ETAG, etag))
        .insert_header((actix_web::http::header::CACHE_CONTROL, IMMUTABLE_CACHE))
        .insert_header((
            "Content-Disposition",
            format!("attachment; filename=\"{}\"", entry.original_filename),
        ))
        .body(content))
}

// ── Object library ────────────────────────────────────────────────────────────
//
// Every model uploaded to any plate lands in the library too (see
// `upload_handler`), deduplicated by content and by shape. These routes read
// it, keep its thumbnails, and put an entry back on a plate without the browser
// uploading the bytes a second time.

/// Largest model the library will take in one request — the upload limit.
const MAX_LIBRARY_IMPORT: usize = 500 * 1024 * 1024;

/// Query for `POST /api/library/import`.
#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct LibraryImportQuery {
    /// The file's name, with its extension — which is how its format is known.
    pub name: String,
}

/// Body for `PATCH /api/library/{id}`.
#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct LibraryRenameRequest {
    pub name: String,
}

/// Body for `POST /api/library/{id}/place`.
#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct LibraryPlaceRequest {
    /// The plate to add the model to. Omitted, a new plate is started.
    #[serde(default)]
    pub ruuid: Option<String>,
}

fn library_error(e: impl std::fmt::Display) -> actix_web::HttpResponse {
    actix_web::HttpResponse::BadRequest().json(serde_json::json!({ "error": e.to_string() }))
}

/// Run blocking library work — hashing, measuring, file copies — off the
/// async executor.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
) -> anyhow::Result<T> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|e| anyhow::anyhow!("library task failed: {e}"))?
}

/// Read a request body up to `limit` bytes.
async fn read_body(mut payload: web::Payload, limit: usize) -> Result<Vec<u8>, actix_web::Error> {
    use futures_util::StreamExt as _;
    let mut body = Vec::new();
    while let Some(chunk) = payload.next().await {
        let chunk = chunk?;
        if body.len() + chunk.len() > limit {
            return Err(actix_web::error::ErrorPayloadTooLarge("body too large"));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// `GET /api/library` — every entry, with missing files and thumbnails marked.
pub async fn get_library_handler(state: web::Data<AppState>) -> actix_web::HttpResponse {
    let store = state.library.clone();
    match blocking(move || store.load()).await {
        Ok(library) => actix_web::HttpResponse::Ok()
            .insert_header((actix_web::http::header::CACHE_CONTROL, "no-store"))
            .json(library),
        Err(e) => actix_web::HttpResponse::InternalServerError()
            .json(serde_json::json!({ "error": e.to_string() })),
    }
}

/// `PUT /api/library/settings` — replace the storage mode and watched folders.
pub async fn put_library_settings_handler(
    body: web::Json<crate::library::LibrarySettings>,
    state: web::Data<AppState>,
) -> actix_web::HttpResponse {
    let store = state.library.clone();
    let settings = body.into_inner();
    match blocking(move || store.save_settings(settings)).await {
        Ok(library) => actix_web::HttpResponse::Ok().json(library),
        Err(e) => library_error(e),
    }
}

/// `POST /api/library/scan` — pick up files added to the library's folders.
pub async fn scan_library_handler(state: web::Data<AppState>) -> actix_web::HttpResponse {
    let store = state.library.clone();
    match blocking(move || store.scan()).await {
        Ok(report) => actix_web::HttpResponse::Ok().json(report),
        Err(e) => library_error(e),
    }
}

/// `POST /api/library/import?name=…` — add a model to the library without
/// putting it on a plate. The body is the file's raw bytes.
pub async fn import_library_handler(
    query: web::Query<LibraryImportQuery>,
    payload: web::Payload,
    state: web::Data<AppState>,
) -> Result<actix_web::HttpResponse, actix_web::Error> {
    let bytes = read_body(payload, MAX_LIBRARY_IMPORT).await?;
    let store = state.library.clone();
    let name = query.into_inner().name;
    Ok(
        match blocking(move || store.import_bytes(&name, &bytes)).await {
            Ok(outcome) => actix_web::HttpResponse::Ok().json(outcome),
            Err(e) => library_error(e),
        },
    )
}

/// `PATCH /api/library/{id}` — rename an entry.
pub async fn rename_library_handler(
    path: web::Path<String>,
    body: web::Json<LibraryRenameRequest>,
    state: web::Data<AppState>,
) -> actix_web::HttpResponse {
    let store = state.library.clone();
    let id = path.into_inner();
    let name = body.into_inner().name;
    match blocking(move || store.rename(&id, &name)).await {
        Ok(true) => actix_web::HttpResponse::NoContent().finish(),
        Ok(false) => actix_web::HttpResponse::NotFound().finish(),
        Err(e) => library_error(e),
    }
}

/// `DELETE /api/library/{id}` — forget an entry and delete the library's copy.
pub async fn delete_library_handler(
    path: web::Path<String>,
    state: web::Data<AppState>,
) -> actix_web::HttpResponse {
    let store = state.library.clone();
    let id = path.into_inner();
    match blocking(move || store.remove(&id)).await {
        Ok(true) => actix_web::HttpResponse::NoContent().finish(),
        Ok(false) => actix_web::HttpResponse::NotFound().finish(),
        Err(e) => library_error(e),
    }
}

/// `GET /api/library/{id}/file` — the model's bytes, from its first readable
/// location.
pub async fn get_library_file_handler(
    path: web::Path<String>,
    state: web::Data<AppState>,
) -> Result<actix_web::HttpResponse, actix_web::Error> {
    let store = state.library.clone();
    let id = path.into_inner();
    let (entry, file) = blocking(move || store.resolve(&id))
        .await
        .map_err(actix_web::error::ErrorInternalServerError)?
        .ok_or_else(|| actix_web::error::ErrorNotFound("No readable file for this entry"))?;
    let content = tokio::fs::read(&file)
        .await
        .map_err(|_| actix_web::error::ErrorNotFound("File could not be read"))?;
    Ok(actix_web::HttpResponse::Ok()
        .content_type("application/octet-stream")
        // The entry id is stable, but which of its files answers is not — a
        // copy can be deleted and a reference take over.
        .insert_header((actix_web::http::header::CACHE_CONTROL, "private, no-cache"))
        .insert_header((
            "Content-Disposition",
            format!("attachment; filename=\"{}.{}\"", entry.name, entry.format),
        ))
        .body(content))
}

/// `GET /api/library/{id}/thumbnail` — the stored PNG.
pub async fn get_library_thumbnail_handler(
    path: web::Path<String>,
    state: web::Data<AppState>,
) -> actix_web::HttpResponse {
    match state.library.thumbnail(&path.into_inner()) {
        Some(png) => actix_web::HttpResponse::Ok()
            .content_type("image/png")
            // Re-rendered when the viewer's look changes, so revalidate.
            .insert_header((actix_web::http::header::CACHE_CONTROL, "private, no-cache"))
            .body(png),
        None => actix_web::HttpResponse::NotFound().finish(),
    }
}

/// `PUT /api/library/{id}/thumbnail` — store a PNG the browser rendered.
pub async fn put_library_thumbnail_handler(
    path: web::Path<String>,
    payload: web::Payload,
    state: web::Data<AppState>,
) -> Result<actix_web::HttpResponse, actix_web::Error> {
    let png = read_body(payload, 2 * 1024 * 1024).await?;
    Ok(
        match state.library.set_thumbnail(&path.into_inner(), &png) {
            Ok(()) => actix_web::HttpResponse::NoContent().finish(),
            Err(e) => library_error(e),
        },
    )
}

/// `POST /api/library/{id}/place` — put a library entry on a plate.
///
/// Answers exactly like an upload, so the client adds the object the way it
/// adds any other: the model gets its own `file_uuid` in the work dir and the
/// slice path never learns the library exists.
pub async fn place_library_handler(
    path: web::Path<String>,
    body: web::Json<LibraryPlaceRequest>,
    state: web::Data<AppState>,
) -> Result<actix_web::HttpResponse, actix_web::Error> {
    use uuid::Uuid;

    let store = state.library.clone();
    let id = path.into_inner();
    let (entry, source) = blocking({
        let id = id.clone();
        move || store.resolve(&id)
    })
    .await
    .map_err(actix_web::error::ErrorInternalServerError)?
    .ok_or_else(|| actix_web::error::ErrorNotFound("No readable file for this entry"))?;

    let existing = match body.into_inner().ruuid {
        Some(text) => {
            let uuid = Uuid::parse_str(&text)
                .map_err(|_| actix_web::error::ErrorBadRequest("Invalid ruuid"))?;
            state
                .db
                .get_request(uuid)
                .await
                .map_err(actix_web::error::ErrorInternalServerError)?
                .map(|_| uuid)
        }
        None => None,
    };

    let file_uuid = Uuid::new_v4();
    let target = state
        .work_dir
        .join(format!("{}.{}", file_uuid, entry.format));
    let size = tokio::fs::copy(&source, &target)
        .await
        .map_err(actix_web::error::ErrorInternalServerError)?;

    let request_uuid = match existing {
        Some(uuid) => uuid,
        None => {
            let uuid = Uuid::new_v4();
            state
                .db
                .create_request(uuid)
                .await
                .map_err(actix_web::error::ErrorInternalServerError)?;
            uuid
        }
    };
    let filename = format!("{}.{}", entry.name, entry.format);
    state
        .db
        .add_upload_file(request_uuid, file_uuid, &filename, &target, size)
        .await
        .map_err(actix_web::error::ErrorInternalServerError)?;

    let store = state.library.clone();
    let _ = blocking(move || store.touch(&id)).await;

    Ok(actix_web::HttpResponse::Ok().json(UploadResponse {
        ruuid: request_uuid.to_string(),
        ofids: vec![file_uuid.to_string()],
    }))
}
