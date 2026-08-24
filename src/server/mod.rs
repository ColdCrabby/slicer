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
            eprintln!(
                "\nServing Slicer Engine UI on all interfaces (port {})",
                port
            );
            eprintln!("  Local:   http://localhost:{}/", port);
            eprintln!("  Network: http://<your-ip>:{}/", port);
            eprintln!("WebSocket endpoint: ws://<host>:{}/ws", port);
        } else {
            eprintln!("\nServing Slicer Engine UI at http://{}:{}/", host, port);
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
            ])
            .supports_credentials();

        App::new()
            .app_data(app_state.clone())
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
                        "/profiles/{kind}",
                        web::put().to(handlers::put_profiles_category_handler),
                    )
                    .route("/history", web::delete().to(handlers::delete_history_handler)),
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
                            actix_files::NamedFile::open_async(path)
                                .await
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

#[cfg(test)]
mod tests {
    use super::*;

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
