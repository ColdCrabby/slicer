//! Serve command – starts a local HTTP + WebSocket server to host the Angular UI.
//!
//! This module provides the UI server for development and Tauri integration.
//! It is separate from the core slicing engine and can be easily swapped for other frontends.
//!
//! ## Upload/Download Flow
//!
//! Files are handled via HTTP (not WebSocket) for efficient streaming:
//!
//! 1. Browser uploads file: `POST /api/upload` → returns `{ ruuid, ofids: [...] }`
//!    where `ruuid` is the workplate / scene identifier and `ofids` are the
//!    file identifiers placed in that scene.
//! 2. Browser sends slice request: WebSocket with `request_uuid` (= `ruuid`),
//!    a `scene` referencing files by `file_uuid` (from `ofids`), and settings.
//! 3. Server processes files from disk (extension preserved from upload)
//! 4. Server saves G-code to disk
//! 5. Browser downloads G-code: `GET /api/download/:request_uuid`
//! 6. Browser fetches an uploaded file by id: `GET /api/file/:file_uuid`

pub mod handlers;
pub mod openapi;
pub mod ws_session;

use clap::Parser;
use std::sync::Arc;

pub use handlers::AppState;

/// Serve the bundled Angular UI over a local HTTP server
#[derive(Parser, Debug)]
pub struct ServeCommand {
    /// Port to listen on
    #[arg(short, long, default_value_t = 5201)]
    pub port: u16,

    /// Directory containing the built Angular app
    /// (defaults to `./ui/dist/slicer-ui/browser`)
    #[arg(long, default_value = "./ui/dist/slicer-ui/browser")]
    pub ui_dir: String,

    /// Host address to bind (use 0.0.0.0 to listen on all network interfaces)
    #[arg(long, default_value = "0.0.0.0")]
    pub host: String,

    /// Directory to store temporary session files
    /// (defaults to system temp directory)
    #[arg(long)]
    pub work_dir: Option<String>,
}

impl ServeCommand {
    /// Execute the serve command
    pub fn execute(&self) -> Result<(), Box<dyn std::error::Error>> {
        let ui_dir = std::path::PathBuf::from(&self.ui_dir);

        if !ui_dir.exists() {
            return Err(format!(
                "UI directory not found: {}\n\
                 Build the Angular app first:\n\
                 \n  cd ui && npm run build\n",
                ui_dir.display()
            )
            .into());
        }

        // Ensure global config exists, writing defaults if not
        let global_config = crate::config::io::config_file();
        if !global_config.exists() {
            crate::config::io::save_config(&Default::default(), &global_config)?;
        }

        let project_config = crate::config::io::find_project_config_toml();

        eprintln!("Loading configuration:");
        eprintln!("  Global config: {}", global_config.display());
        if let Some(ref p) = project_config {
            eprintln!("  Project config: {}", p.display());
        }

        let host = self.host.clone();
        let port = self.port;
        let ui_dir = self.ui_dir.clone();
        let work_dir = self.work_dir.clone();

        if host == "0.0.0.0" {
            eprintln!("\nServing Cold Crabby UI on all interfaces (port {})", port);
            eprintln!("  Local:   http://localhost:{}/", port);
            eprintln!("  Network: http://<your-ip>:{}/", port);
            eprintln!("WebSocket endpoint: ws://<host>:{}/ws", port);
        } else {
            eprintln!("\nServing Cold Crabby UI at http://{}:{}/", host, port);
            eprintln!("WebSocket endpoint:        ws://{}:{}/ws", host, port);
        }
        eprintln!("Serving files from: {}", ui_dir);
        eprintln!("Press Ctrl+C to stop.");

        tokio::runtime::Runtime::new()?.block_on(run_server(host, port, ui_dir, work_dir))?;
        Ok(())
    }
}

async fn run_server(
    host: String,
    port: u16,
    ui_dir: String,
    work_dir: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    use actix_cors::Cors;
    use actix_files::Files;
    use actix_web::dev::Service as _;
    use actix_web::{http, web, App, HttpServer};

    // Initialize work directory
    let work_path = if let Some(dir) = work_dir {
        std::path::PathBuf::from(dir)
    } else {
        let temp_dir = std::env::temp_dir();
        temp_dir.join("slicer-engine")
    };
    std::fs::create_dir_all(&work_path)?;
    eprintln!("Work directory: {}", work_path.display());

    // Initialize database
    let db_path = work_path.join("slicer.db");
    eprintln!("Database path:  {}", db_path.display());
    let db = Arc::new(crate::db::Database::open(&db_path).await?);
    eprintln!("Database initialized successfully.");

    let app_state = web::Data::new(AppState {
        db,
        work_dir: work_path.clone(),
        // Retained for the server's lifetime; sessions each hold a subscriber.
        profiles_changed: tokio::sync::broadcast::channel(16).0,
        workplates_changed: tokio::sync::broadcast::channel(64).0,
        library: crate::library::LibraryStore::new(),
    });

    HttpServer::new(move || {
        let fallback_dir = ui_dir.clone();

        // CORS configuration for HTTP API routes only
        // Note: WebSocket connections do not support CORS and bypass this middleware
        let cors = Cors::default()
            .allow_any_origin()
            .allowed_methods(vec![
                http::Method::GET,
                http::Method::POST,
                http::Method::PUT,
                http::Method::PATCH,
                http::Method::DELETE,
                http::Method::OPTIONS,
            ])
            .allowed_headers(vec![
                http::header::CONTENT_TYPE,
                http::header::AUTHORIZATION,
                // Self-assigned, opaque, and only ever compared for equality:
                // it is how a client's own edit is kept out of the "someone
                // else changed this plate" prompt it would otherwise trigger.
                http::header::HeaderName::from_static("x-client-id"),
            ])
            // `Content-Disposition` is not CORS-safelisted, so a cross-origin
            // client — any UI served from another origin — cannot read the
            // filename the engine chose for a download unless it is explicitly
            // exposed. (The Angular dev server proxies `/api`, so it is
            // same-origin and does not rely on this.)
            .expose_headers(vec![http::header::CONTENT_DISPOSITION])
            .supports_credentials();

        App::new()
            .app_data(app_state.clone())
            // Decide how long the browser may keep each static asset. Handlers
            // set their own `Cache-Control`, so this only ever fills in the
            // blank the file server leaves — see `static_cache_control`.
            .wrap_fn(|req, srv| {
                let policy = static_cache_control(req.path());
                let fut = srv.call(req);
                async move {
                    let mut res = fut.await?;
                    let headers = res.headers_mut();
                    match policy {
                        Some(value) if !headers.contains_key(http::header::CACHE_CONTROL) => {
                            headers.insert(
                                http::header::CACHE_CONTROL,
                                http::header::HeaderValue::from_static(value),
                            );
                        }
                        _ => {}
                    }
                    Ok(res)
                }
            })
            // Apply CORS only to API scope, not to WebSocket
            .service(
                web::scope("/api")
                    .wrap(cors)
                    .route("/upload", web::post().to(handlers::upload_handler))
                    .route(
                        "/download/{request_uuid}",
                        web::get().to(handlers::download_handler),
                    )
                    .route(
                        "/request/{request_uuid}",
                        web::get().to(handlers::get_request_handler),
                    )
                    .route(
                        "/file/{file_uuid}",
                        web::get().to(handlers::download_file_handler),
                    )
                    .route("/config", web::get().to(handlers::get_config_handler))
                    .route("/config", web::patch().to(handlers::patch_config_handler))
                    .route("/profiles", web::get().to(handlers::get_profiles_handler))
                    .route(
                        "/profiles/export",
                        web::get().to(handlers::export_profiles_handler),
                    )
                    .route(
                        "/profiles/{kind}",
                        web::put().to(handlers::put_profiles_category_handler),
                    )
                    .route(
                        "/workplates/{request_uuid}",
                        web::get().to(handlers::get_workplate_handler),
                    )
                    .route(
                        "/workplates/{request_uuid}",
                        web::put().to(handlers::put_workplate_handler),
                    )
                    .route("/library", web::get().to(handlers::get_library_handler))
                    .route(
                        "/library/settings",
                        web::put().to(handlers::put_library_settings_handler),
                    )
                    .route(
                        "/library/scan",
                        web::post().to(handlers::scan_library_handler),
                    )
                    .route(
                        "/library/import",
                        web::post().to(handlers::import_library_handler),
                    )
                    .route(
                        "/library/{id}",
                        web::patch().to(handlers::rename_library_handler),
                    )
                    .route(
                        "/library/{id}",
                        web::delete().to(handlers::delete_library_handler),
                    )
                    .route(
                        "/library/{id}/file",
                        web::get().to(handlers::get_library_file_handler),
                    )
                    .route(
                        "/library/{id}/thumbnail",
                        web::get().to(handlers::get_library_thumbnail_handler),
                    )
                    .route(
                        "/library/{id}/thumbnail",
                        web::put().to(handlers::put_library_thumbnail_handler),
                    )
                    .route(
                        "/library/{id}/place",
                        web::post().to(handlers::place_library_handler),
                    )
                    .route("/openapi.json", web::get().to(handlers::openapi_handler))
                    .route("/docs", web::get().to(handlers::api_docs_handler))
                    .route(
                        "/history",
                        web::delete().to(handlers::delete_history_handler),
                    ),
            )
            // WebSocket endpoint
            .route("/ws", web::get().to(ws_session::ws_handler))
            // Serve static assets; fall back to index.html for SPA navigation
            .service(
                Files::new("/", &ui_dir)
                    .index_file("index.html")
                    .default_handler(web::to(move || {
                        let path = format!("{}/index.html", fallback_dir);
                        async move {
                            actix_files::NamedFile::open(path)
                                .map_err(actix_web::error::ErrorNotFound)
                        }
                    })),
            )
    })
    .bind((host.as_str(), port))?
    .run()
    .await?;

    Ok(())
}

/// How long the browser may keep the asset at `path`.
///
/// `None` leaves the decision to whoever produced the response — every `/api`
/// handler answers for itself, because only it knows whether its body is a
/// plate that someone else may already have changed.
///
/// For the app's own files the rule is the usual one, and the reason to state
/// it explicitly is that the default — no header at all — lets the browser
/// guess, and a browser that guesses wrong about `index.html` serves a build
/// the user cannot get rid of by reloading:
///
/// - **A file whose name carries a build hash never changes**, because a new
///   build gives it a new name. Keep it for a year.
/// - **Everything else revalidates.** `index.html` is the one file that names
///   the current build, and `scene_engine_bg.wasm` ships under a fixed name
///   beside content-hashed glue — a browser reusing a stale copy of either
///   pairs the wrong halves of the app together.
fn static_cache_control(path: &str) -> Option<&'static str> {
    if path.starts_with("/api") || path == "/ws" {
        return None;
    }
    if is_build_hashed(path) {
        return Some(handlers::IMMUTABLE_CACHE);
    }
    Some("no-cache")
}

/// Whether a filename carries a build hash — `main-6T6X7SIP.js`, not `main.js`.
///
/// Matches the bundler's `<name>-<hash>.<ext>` shape: at least eight characters
/// of base64url after the last dash, and at least one digit, so an ordinary
/// hyphenated name (`apple-touch-icon.png`) is never mistaken for one.
fn is_build_hashed(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    let Some((stem, ext)) = name.rsplit_once('.') else {
        return false;
    };
    if !matches!(ext, "js" | "css" | "mjs") {
        return false;
    }
    let Some((_, hash)) = stem.rsplit_once('-') else {
        return false;
    };
    hash.len() >= 8
        && hash.chars().any(|c| c.is_ascii_digit())
        && hash
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashed_bundles_are_kept_forever() {
        assert_eq!(
            static_cache_control("/main-6T6X7SIP.js"),
            Some(handlers::IMMUTABLE_CACHE)
        );
        assert_eq!(
            static_cache_control("/media/chunk-3MtePz_z.css"),
            Some(handlers::IMMUTABLE_CACHE)
        );
    }

    #[test]
    fn the_shell_and_the_wasm_always_revalidate() {
        // These two name the current build; a stale copy of either is a user
        // stuck on an old app with no way to reload out of it.
        assert_eq!(static_cache_control("/"), Some("no-cache"));
        assert_eq!(static_cache_control("/index.html"), Some("no-cache"));
        assert_eq!(
            static_cache_control("/scene_engine_bg.wasm"),
            Some("no-cache")
        );
    }

    #[test]
    fn an_ordinary_hyphenated_name_is_not_a_hash() {
        assert_eq!(
            static_cache_control("/apple-touch-icon.png"),
            Some("no-cache")
        );
        assert_eq!(static_cache_control("/proxy-conf.js"), Some("no-cache"));
    }

    #[test]
    fn handlers_answer_for_their_own_bodies() {
        assert_eq!(static_cache_control("/api/workplates/abc"), None);
        assert_eq!(static_cache_control("/api/file/abc"), None);
        assert_eq!(static_cache_control("/ws"), None);
    }

    #[test]
    fn test_serve_command_defaults() {
        let cmd = ServeCommand {
            port: 5201,
            ui_dir: "./ui/dist/slicer-ui/browser".to_string(),
            host: "0.0.0.0".to_string(),
            work_dir: None,
        };
        assert_eq!(cmd.port, 5201);
        assert_eq!(cmd.host, "0.0.0.0");
    }

    #[test]
    fn test_serve_command_missing_dir_error() {
        let cmd = ServeCommand {
            port: 4200,
            ui_dir: "/nonexistent/path/that/does/not/exist".to_string(),
            host: "127.0.0.1".to_string(),
            work_dir: None,
        };
        let result = cmd.execute();
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("UI directory not found"));
    }
}
