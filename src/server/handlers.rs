//! HTTP request handlers for upload, download, and configuration operations.

use actix_web::web;
use std::sync::Arc;

/// Response from `POST /api/upload`.
///
/// `ruuid` is the workplate / scene identifier; `ofids` is the list of file
/// identifiers that have been placed in that scene. Today there is exactly
/// one file per upload, but the protocol intentionally supports multiple so
/// the slice path doesn't have to change when multi-file UX lands.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct UploadResponse {
    pub ruuid: String,
    pub ofids: Vec<String>,
}

pub struct AppState {
    pub db: Arc<crate::db::Database>,
    pub work_dir: std::path::PathBuf,
}

// ── Config handlers ───────────────────────────────────────────────────────────

/// Request body for `GET /api/config`.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct ConfigResponse {
    pub config: crate::config::AppConfig,
}

/// Request body for `PATCH /api/config`.
#[derive(serde::Serialize, serde::Deserialize)]
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
) -> actix_web::HttpResponse {
    let Some(kind) = crate::profiles::ProfileKind::parse(&path.into_inner()) else {
        return actix_web::HttpResponse::NotFound()
            .json(serde_json::json!({ "error": "unknown profile category" }));
    };

    match crate::profiles::ProfileStore::new().replace_category(kind, body.into_inner()) {
        Ok(library) => actix_web::HttpResponse::Ok().json(library),
        Err(e) => actix_web::HttpResponse::BadRequest()
            .json(serde_json::json!({ "error": e.to_string() })),
    }
}

/// Handle file upload: save the file with its original extension and return
/// `{ ruuid, ofids: [file_uuid] }`. The workplate UUID and the file UUID are
/// distinct — the slice protocol references files by `file_uuid` and never by
/// `request_uuid` (the legacy "request UUID is also the file ID" convention
/// is gone).
pub async fn upload_handler(
    state: web::Data<AppState>,
    mut multipart: actix_multipart::Multipart,
) -> Result<actix_web::HttpResponse, actix_web::Error> {
    use futures_util::StreamExt as _;
    use uuid::Uuid;

    // Generate workplate UUID and a separate file UUID.
    let request_uuid = Uuid::new_v4();
    let file_uuid = Uuid::new_v4();

    // Create database record
    state
        .db
        .create_request(request_uuid)
        .await
        .map_err(actix_web::error::ErrorInternalServerError)?;

    const MAX_FILE_SIZE: u64 = 500 * 1024 * 1024; // 500 MB limit
    let mut file_size: u64 = 0;
    let mut original_filename: Option<String> = None;
    let mut file_path: Option<std::path::PathBuf> = None;

    // Process multipart fields
    while let Some(field_result) = multipart.next().await {
        let mut field = field_result.map_err(actix_web::error::ErrorBadRequest)?;

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

    // Update database with file info
    let filename = original_filename.unwrap_or_else(|| format!("{}.stl", file_uuid));
    state
        .db
        .add_upload_file(request_uuid, file_uuid, &filename, &file_path, file_size)
        .await
        .map_err(actix_web::error::ErrorInternalServerError)?;

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
#[derive(serde::Serialize)]
pub struct RequestFileSummary {
    pub file_uuid: String,
    pub original_filename: String,
}

/// Response body for `GET /api/request/:request_uuid`.
///
/// Returns the workplate's status, the G-code download status, and the list
/// of file IDs (`ofids`-style) so the UI can rebuild a slice payload after a
/// page reload without having to re-upload anything.
#[derive(serde::Serialize)]
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

    Ok(actix_web::HttpResponse::Ok().json(RequestMetaResponse {
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

    let content = tokio::fs::read(&entry.file_path)
        .await
        .map_err(|_| actix_web::error::ErrorNotFound("File could not be read"))?;

    Ok(actix_web::HttpResponse::Ok()
        .content_type("application/octet-stream")
        .insert_header((
            "Content-Disposition",
            format!("attachment; filename=\"{}\"", entry.original_filename),
        ))
        .body(content))
}
