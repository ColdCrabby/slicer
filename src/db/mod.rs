//! Local SQLite database for tracking upload/download sessions and file metadata.
//!
//! Backed by [SeaORM](https://www.sea-ql.org/SeaORM/) with the `sqlx-sqlite`
//! driver. All public methods are `async`; callers running inside a Tokio
//! runtime can `.await` them directly — no `spawn_blocking` wrapper needed.
//!
//! Schema evolution is handled by [`crate::db::migrator::Migrator`].
//! [`Database::open`] runs pending migrations automatically on every startup,
//! so the database is always up-to-date before the first query executes.

use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter,
    QueryOrder,
};
use sea_orm_migration::MigratorTrait as _;
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub mod entities;
pub mod migrations;
pub mod migrator;

use entities::{files, requests};

// ── RequestStatus ─────────────────────────────────────────────────────────────

/// Request lifecycle status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestStatus {
    /// Awaiting file upload.
    AwaitingUpload,
    /// Upload in progress.
    Uploading,
    /// Upload complete, ready to slice.
    UploadComplete,
    /// Slicing in progress.
    Slicing,
    /// Slicing complete, G-code ready for download.
    SliceComplete,
    /// Error occurred during processing.
    Error,
}

impl RequestStatus {
    /// Convert to the TEXT value stored in the database.
    fn to_db(self) -> &'static str {
        match self {
            RequestStatus::AwaitingUpload => "awaiting_upload",
            RequestStatus::Uploading => "uploading",
            RequestStatus::UploadComplete => "upload_complete",
            RequestStatus::Slicing => "slicing",
            RequestStatus::SliceComplete => "slice_complete",
            RequestStatus::Error => "error",
        }
    }

    /// Parse from the TEXT value stored in the database.
    fn from_db(s: &str) -> Result<Self> {
        Ok(match s {
            "awaiting_upload" => RequestStatus::AwaitingUpload,
            "uploading" => RequestStatus::Uploading,
            "upload_complete" => RequestStatus::UploadComplete,
            "slicing" => RequestStatus::Slicing,
            "slice_complete" => RequestStatus::SliceComplete,
            "error" => RequestStatus::Error,
            _ => return Err(anyhow!("Unknown status: {}", s)),
        })
    }
}

// ── Domain types ──────────────────────────────────────────────────────────────

/// A workplate session.
///
/// Per-file metadata (uploaded model paths, original filenames, sizes) lives
/// on the [`FileEntry`] rows in the `files` table. The `requests` table only
/// tracks the workplate's lifecycle and the single G-code output produced
/// when the slice completes.
#[derive(Debug, Clone)]
pub struct RequestSession {
    pub request_uuid: Uuid,
    pub status: RequestStatus,
    pub download_file_path: Option<PathBuf>,
    pub download_file_size: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A single uploaded file belonging to a workplate (request).
///
/// Files are addressed by their own `file_uuid` (distinct from the workplate's
/// `request_uuid`). `file_path` retains the original extension so the slicer
/// can pick the right loader without re-encoding any format hints into the URL
/// or the wire protocol.
#[derive(Debug, Clone)]
pub struct FileEntry {
    pub file_uuid: Uuid,
    pub request_uuid: Uuid,
    pub original_filename: String,
    pub file_path: PathBuf,
    pub file_size: i64,
    pub created_at: DateTime<Utc>,
}

// ── Conversion helpers ────────────────────────────────────────────────────────

fn model_to_session(m: requests::Model) -> Result<RequestSession> {
    Ok(RequestSession {
        request_uuid: Uuid::parse_str(&m.request_uuid)?,
        status: RequestStatus::from_db(&m.status)?,
        download_file_path: m.download_file_path.map(PathBuf::from),
        download_file_size: m.download_file_size,
        created_at: DateTime::parse_from_rfc3339(&m.created_at)?.with_timezone(&Utc),
        updated_at: DateTime::parse_from_rfc3339(&m.updated_at)?.with_timezone(&Utc),
    })
}

fn model_to_file_entry(m: files::Model) -> Result<FileEntry> {
    Ok(FileEntry {
        file_uuid: Uuid::parse_str(&m.file_uuid)?,
        request_uuid: Uuid::parse_str(&m.request_uuid)?,
        original_filename: m.original_filename,
        file_path: PathBuf::from(m.file_path),
        file_size: m.file_size,
        created_at: DateTime::parse_from_rfc3339(&m.created_at)?.with_timezone(&Utc),
    })
}

// ── Database ──────────────────────────────────────────────────────────────────

/// Database connection manager.
pub struct Database {
    conn: DatabaseConnection,
}

impl Database {
    /// Open (or create) the SQLite database at `db_path` and run any pending
    /// migrations. Uses `?mode=rwc` so the file is created automatically on
    /// first startup.
    pub async fn open(db_path: impl AsRef<Path>) -> Result<Self> {
        let url = format!("sqlite://{}?mode=rwc", db_path.as_ref().display());
        let conn = sea_orm::Database::connect(&url).await?;

        // Apply all pending migrations (idempotent — already-applied ones are
        // skipped via the `seaql_migrations` bookkeeping table).
        migrator::Migrator::up(&conn, None).await?;

        Ok(Self { conn })
    }

    // ── Write helpers ─────────────────────────────────────────────────────────

    /// Create a new request session in the `AwaitingUpload` state.
    pub async fn create_request(&self, request_uuid: Uuid) -> Result<RequestSession> {
        let now = Utc::now();
        let now_str = now.to_rfc3339();

        let model = requests::ActiveModel {
            request_uuid: Set(request_uuid.to_string()),
            status: Set(RequestStatus::AwaitingUpload.to_db().to_owned()),
            download_file_path: Set(None),
            download_file_size: Set(None),
            created_at: Set(now_str.clone()),
            updated_at: Set(now_str),
        };

        model.insert(&self.conn).await?;

        Ok(RequestSession {
            request_uuid,
            status: RequestStatus::AwaitingUpload,
            download_file_path: None,
            download_file_size: None,
            created_at: now,
            updated_at: now,
        })
    }

    /// Update the lifecycle status of a request.
    pub async fn update_status(&self, request_uuid: Uuid, new_status: RequestStatus) -> Result<()> {
        let now = Utc::now().to_rfc3339();

        let model = requests::ActiveModel {
            request_uuid: Set(request_uuid.to_string()),
            status: Set(new_status.to_db().to_owned()),
            updated_at: Set(now),
            ..Default::default()
        };

        let result = requests::Entity::update(model)
            .filter(requests::Column::RequestUuid.eq(request_uuid.to_string()))
            .exec(&self.conn)
            .await;

        match result {
            Ok(_) => Ok(()),
            Err(sea_orm::DbErr::RecordNotUpdated) => {
                Err(anyhow!("Request not found: {}", request_uuid))
            }
            Err(e) => Err(e.into()),
        }
    }

    /// Add an uploaded file row for a workplate and promote it to
    /// `UploadComplete`. The single source of truth for "which files belong to
    /// which request" — slicing references files by `file_uuid`, never by
    /// `request_uuid`.
    pub async fn add_upload_file(
        &self,
        request_uuid: Uuid,
        file_uuid: Uuid,
        original_filename: &str,
        file_path: impl AsRef<Path>,
        file_size: u64,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let path_str = file_path.as_ref().to_string_lossy().to_string();

        let file_model = files::ActiveModel {
            file_uuid: Set(file_uuid.to_string()),
            request_uuid: Set(request_uuid.to_string()),
            original_filename: Set(original_filename.to_owned()),
            file_path: Set(path_str),
            file_size: Set(file_size as i64),
            created_at: Set(now.clone()),
        };

        file_model.insert(&self.conn).await?;

        // Promote the workplate to UploadComplete on first file.
        let req_model = requests::ActiveModel {
            request_uuid: Set(request_uuid.to_string()),
            status: Set(RequestStatus::UploadComplete.to_db().to_owned()),
            updated_at: Set(now),
            ..Default::default()
        };

        requests::Entity::update(req_model)
            .filter(requests::Column::RequestUuid.eq(request_uuid.to_string()))
            .exec(&self.conn)
            .await?;

        Ok(())
    }

    /// Record the generated G-code file path and advance the request to
    /// `SliceComplete`.
    pub async fn set_download_file(
        &self,
        request_uuid: Uuid,
        file_path: impl AsRef<Path>,
        file_size: u64,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let path_str = file_path.as_ref().to_string_lossy().to_string();

        let model = requests::ActiveModel {
            request_uuid: Set(request_uuid.to_string()),
            download_file_path: Set(Some(path_str)),
            download_file_size: Set(Some(file_size as i64)),
            status: Set(RequestStatus::SliceComplete.to_db().to_owned()),
            updated_at: Set(now),
            ..Default::default()
        };

        requests::Entity::update(model)
            .filter(requests::Column::RequestUuid.eq(request_uuid.to_string()))
            .exec(&self.conn)
            .await?;

        Ok(())
    }

    // ── G-code cache ──────────────────────────────────────────────────────────

    /// Look up a cached G-code slice by its content key.
    ///
    /// Returns `(file_path, file_size, layer_count)` when a row exists **and**
    /// its file is still present on disk; a stale row (file deleted by cleanup)
    /// is removed and treated as a miss so the caller re-slices.
    pub async fn get_cached_gcode(&self, cache_key: &str) -> Result<Option<(PathBuf, u64, usize)>> {
        let Some(model) = entities::gcode_cache::Entity::find_by_id(cache_key)
            .one(&self.conn)
            .await?
        else {
            return Ok(None);
        };

        let path = PathBuf::from(&model.file_path);
        if !path.exists() {
            // Evict the dangling entry so it doesn't shadow a fresh slice.
            let _ = entities::gcode_cache::Entity::delete_by_id(cache_key)
                .exec(&self.conn)
                .await;
            return Ok(None);
        }

        Ok(Some((
            path,
            model.file_size.max(0) as u64,
            model.layer_count.max(0) as usize,
        )))
    }

    /// Record (or replace) a cached G-code slice for `cache_key`.
    pub async fn put_cached_gcode(
        &self,
        cache_key: &str,
        file_path: impl AsRef<Path>,
        file_size: u64,
        layer_count: usize,
    ) -> Result<()> {
        use sea_orm::sea_query::OnConflict;

        let now = Utc::now().to_rfc3339();
        let model = entities::gcode_cache::ActiveModel {
            cache_key: Set(cache_key.to_owned()),
            file_path: Set(file_path.as_ref().to_string_lossy().to_string()),
            file_size: Set(file_size as i64),
            layer_count: Set(layer_count as i64),
            created_at: Set(now),
        };

        entities::gcode_cache::Entity::insert(model)
            .on_conflict(
                OnConflict::column(entities::gcode_cache::Column::CacheKey)
                    .update_columns([
                        entities::gcode_cache::Column::FilePath,
                        entities::gcode_cache::Column::FileSize,
                        entities::gcode_cache::Column::LayerCount,
                        entities::gcode_cache::Column::CreatedAt,
                    ])
                    .to_owned(),
            )
            .exec(&self.conn)
            .await?;

        Ok(())
    }

    // ── Read helpers ──────────────────────────────────────────────────────────

    /// Retrieve a request session by its UUID, or `None` if not found.
    pub async fn get_request(&self, request_uuid: Uuid) -> Result<Option<RequestSession>> {
        let model = requests::Entity::find_by_id(request_uuid.to_string())
            .one(&self.conn)
            .await?;

        model.map(model_to_session).transpose()
    }

    /// Look up a single file row by its `file_uuid`.
    pub async fn get_file(&self, file_uuid: Uuid) -> Result<Option<FileEntry>> {
        let model = files::Entity::find_by_id(file_uuid.to_string())
            .one(&self.conn)
            .await?;

        model.map(model_to_file_entry).transpose()
    }

    /// All files belonging to a workplate, ordered by upload time (oldest first).
    pub async fn get_files_for_request(&self, request_uuid: Uuid) -> Result<Vec<FileEntry>> {
        let rows = files::Entity::find()
            .filter(files::Column::RequestUuid.eq(request_uuid.to_string()))
            .order_by_asc(files::Column::CreatedAt)
            .all(&self.conn)
            .await?;

        rows.into_iter().map(model_to_file_entry).collect()
    }

    /// All sessions with a specific status, ordered by most-recently updated first.
    pub async fn get_sessions_by_status(
        &self,
        status: RequestStatus,
    ) -> Result<Vec<RequestSession>> {
        let rows = requests::Entity::find()
            .filter(requests::Column::Status.eq(status.to_db()))
            .order_by_desc(requests::Column::UpdatedAt)
            .all(&self.conn)
            .await?;

        rows.into_iter().map(model_to_session).collect()
    }

    /// All completed slicing sessions, ordered by most recently updated first.
    pub async fn get_completed_sessions(&self) -> Result<Vec<RequestSession>> {
        self.get_sessions_by_status(RequestStatus::SliceComplete)
            .await
    }

    // ── Cleanup ───────────────────────────────────────────────────────────────

    /// Delete sessions (and their associated file rows) that have not been
    /// updated within the last `hours_old` hours. On-disk files referenced by
    /// deleted rows are removed from the filesystem as well.
    ///
    /// Returns the number of request rows deleted.
    pub async fn cleanup_old_sessions(&self, hours_old: i64) -> Result<usize> {
        let cutoff = Utc::now()
            .checked_sub_signed(chrono::Duration::hours(hours_old))
            .ok_or_else(|| anyhow!("Invalid duration"))?
            .to_rfc3339();

        // Collect on-disk paths before deleting the rows.
        let expired_requests = requests::Entity::find()
            .filter(requests::Column::UpdatedAt.lt(cutoff.clone()))
            .all(&self.conn)
            .await?;

        let expired_uuids: Vec<String> = expired_requests
            .iter()
            .map(|r| r.request_uuid.clone())
            .collect();

        // Collect uploaded-file paths for all expired requests.
        let expired_files = files::Entity::find()
            .filter(files::Column::RequestUuid.is_in(expired_uuids.clone()))
            .all(&self.conn)
            .await?;

        // Delete on-disk artifacts (best-effort; errors are silently ignored).
        for r in &expired_requests {
            if let Some(ref p) = r.download_file_path {
                let _ = std::fs::remove_file(p);
            }
        }
        for f in &expired_files {
            let _ = std::fs::remove_file(&f.file_path);
        }

        // Delete child rows first to satisfy the FK constraint, then requests.
        files::Entity::delete_many()
            .filter(files::Column::RequestUuid.is_in(expired_uuids))
            .exec(&self.conn)
            .await?;

        let result = requests::Entity::delete_many()
            .filter(requests::Column::UpdatedAt.lt(cutoff))
            .exec(&self.conn)
            .await?;

        Ok(result.rows_affected as usize)
    }

    /// Delete **every** session, uploaded-file row, and cached G-code entry,
    /// removing their on-disk artifacts too. This is the "drop the history
    /// database" action behind the settings Danger Zone: after it runs the
    /// history list is empty and the G-code cache is cold, but the schema (and
    /// the user's profiles, which live in `profiles.toml`, not the DB) are
    /// untouched.
    ///
    /// On-disk file removal is best-effort — a missing artifact never fails the
    /// clear. Returns the number of request rows deleted.
    pub async fn clear_history(&self) -> Result<usize> {
        // Collect on-disk paths before dropping the rows so we can unlink them.
        let all_requests = requests::Entity::find().all(&self.conn).await?;
        let all_files = files::Entity::find().all(&self.conn).await?;
        let all_cache = entities::gcode_cache::Entity::find()
            .all(&self.conn)
            .await?;

        for r in &all_requests {
            if let Some(ref p) = r.download_file_path {
                let _ = std::fs::remove_file(p);
            }
        }
        for f in &all_files {
            let _ = std::fs::remove_file(&f.file_path);
        }
        for c in &all_cache {
            let _ = std::fs::remove_file(&c.file_path);
        }

        // Delete child rows first (FK), then the cache, then the requests.
        files::Entity::delete_many().exec(&self.conn).await?;
        entities::gcode_cache::Entity::delete_many()
            .exec(&self.conn)
            .await?;
        let result = requests::Entity::delete_many().exec(&self.conn).await?;

        Ok(result.rows_affected as usize)
    }

    // ── Test helpers ──────────────────────────────────────────────────────────

    /// Directly set `updated_at` on a request row to an arbitrary timestamp.
    ///
    /// **Only compiled in `#[cfg(test)]` mode.** Used by cleanup tests to age
    /// rows without sleeping.
    #[cfg(test)]
    pub async fn set_updated_at_for_test(&self, request_uuid: Uuid, timestamp: &str) -> Result<()> {
        use sea_orm::ConnectionTrait;
        self.conn
            .execute(sea_orm::Statement::from_sql_and_values(
                sea_orm::DatabaseBackend::Sqlite,
                "UPDATE requests SET updated_at = $1 WHERE request_uuid = $2",
                [timestamp.into(), request_uuid.to_string().into()],
            ))
            .await?;
        Ok(())
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_create_and_retrieve_request() -> Result<()> {
        let dir = TempDir::new()?;
        let db = Database::open(dir.path().join("test.db")).await?;

        let uuid = Uuid::new_v4();
        let session = db.create_request(uuid).await?;

        assert_eq!(session.request_uuid, uuid);
        assert_eq!(session.status, RequestStatus::AwaitingUpload);

        let retrieved = db.get_request(uuid).await?.unwrap();
        assert_eq!(retrieved.request_uuid, uuid);
        assert_eq!(retrieved.status, RequestStatus::AwaitingUpload);

        Ok(())
    }

    #[tokio::test]
    async fn test_update_status() -> Result<()> {
        let dir = TempDir::new()?;
        let db = Database::open(dir.path().join("test.db")).await?;

        let uuid = Uuid::new_v4();
        db.create_request(uuid).await?;
        db.update_status(uuid, RequestStatus::Slicing).await?;

        let retrieved = db.get_request(uuid).await?.unwrap();
        assert_eq!(retrieved.status, RequestStatus::Slicing);

        Ok(())
    }

    /// `add_upload_file` should write a row to `files` keyed by `file_uuid` and
    /// promote the request to `UploadComplete`. `get_file` and
    /// `get_files_for_request` should return what was just written.
    #[tokio::test]
    async fn test_add_and_get_files() -> Result<()> {
        let dir = TempDir::new()?;
        let db = Database::open(dir.path().join("test.db")).await?;
        let request_uuid = Uuid::new_v4();
        db.create_request(request_uuid).await?;

        let file_uuid = Uuid::new_v4();
        let file_path = dir.path().join(format!("{}.obj", file_uuid));
        std::fs::write(&file_path, b"dummy")?;
        db.add_upload_file(request_uuid, file_uuid, "model.obj", &file_path, 5)
            .await?;

        let entry = db.get_file(file_uuid).await?.expect("file row exists");
        assert_eq!(entry.file_uuid, file_uuid);
        assert_eq!(entry.request_uuid, request_uuid);
        assert_eq!(entry.original_filename, "model.obj");
        assert_eq!(entry.file_path, file_path);
        assert_eq!(entry.file_size, 5);

        let files = db.get_files_for_request(request_uuid).await?;
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].file_uuid, file_uuid);

        // Status should advance.
        let session = db.get_request(request_uuid).await?.unwrap();
        assert_eq!(session.status, RequestStatus::UploadComplete);

        Ok(())
    }

    /// Cleanup must delete `files` rows and their on-disk artifacts together
    /// with the workplate's G-code download (if any).
    #[tokio::test]
    async fn test_cleanup_removes_files_table_rows() -> Result<()> {
        let dir = TempDir::new()?;
        let db = Database::open(dir.path().join("test.db")).await?;
        let request_uuid = Uuid::new_v4();
        db.create_request(request_uuid).await?;
        let file_uuid = Uuid::new_v4();
        let file_path = dir.path().join(format!("{}.stl", file_uuid));
        std::fs::write(&file_path, b"dummy")?;
        db.add_upload_file(request_uuid, file_uuid, "m.stl", &file_path, 5)
            .await?;

        // Force the row to be older than the cutoff.
        let old = (Utc::now() - chrono::Duration::hours(48)).to_rfc3339();
        db.set_updated_at_for_test(request_uuid, &old).await?;

        let removed = db.cleanup_old_sessions(24).await?;
        assert_eq!(removed, 1);
        assert!(db.get_file(file_uuid).await?.is_none());
        assert!(!file_path.exists());

        Ok(())
    }
}

#[cfg(test)]
mod history_tests;
