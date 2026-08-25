#[cfg(test)]
mod history_integration_tests {
    use crate::db::{Database, RequestStatus};
    use crate::ws_protocol::{ClientMessage, ServerMessage, SessionSummary};
    use tempfile::TempDir;
    use uuid::Uuid;

    #[test]
    fn test_list_sessions_message_serialization() {
        let summaries = vec![SessionSummary {
            request_uuid: "test-uuid".to_string(),
            original_filename: Some("model.stl".to_string()),
            layer_count: Some(100),
            created_at: "2026-04-26T00:00:00Z".to_string(),
            download_url: "/api/download/test-uuid".to_string(),
        }];

        let msg = ServerMessage::SessionsList {
            sessions: summaries,
        };

        // Verify message can be serialized to JSON
        let json = serde_json::to_string(&msg).expect("Should serialize");
        assert!(json.contains("SessionsList"));
        assert!(json.contains("model.stl"));
        assert!(json.contains("100"));
    }

    #[test]
    fn test_list_sessions_request_message() {
        let msg = ClientMessage::ListSessions;

        // Verify message can be serialized to JSON
        let json = serde_json::to_string(&msg).expect("Should serialize");
        assert!(json.contains("ListSessions"));
    }

    #[tokio::test]
    async fn test_completed_sessions_query() -> Result<(), Box<dyn std::error::Error>> {
        let dir = TempDir::new()?;
        let db_path = dir.path().join("test.db");
        let db = Database::open(&db_path).await?;

        let uuid1 = Uuid::new_v4();
        let uuid2 = Uuid::new_v4();

        // Create two sessions
        db.create_request(uuid1).await?;
        db.create_request(uuid2).await?;

        // Mark both as complete
        db.update_status(uuid1, RequestStatus::SliceComplete)
            .await?;
        db.update_status(uuid2, RequestStatus::SliceComplete)
            .await?;

        // Query completed sessions
        let completed = db.get_completed_sessions().await?;

        // Should have at least 2 completed sessions
        assert!(completed.len() >= 2);

        // Verify sessions are SliceComplete status
        for session in &completed {
            assert_eq!(session.status, RequestStatus::SliceComplete);
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_gcode_cache_round_trip() -> Result<(), Box<dyn std::error::Error>> {
        let dir = TempDir::new()?;
        let db = Database::open(dir.path().join("test.db")).await?;

        // Miss on an unknown key.
        assert!(db.get_cached_gcode("deadbeef").await?.is_none());

        // Store a cache entry pointing at a real file.
        let gcode = dir.path().join("cached.gcode");
        std::fs::write(&gcode, b"G28\n")?;
        db.put_cached_gcode("deadbeef", &gcode, 4, 42).await?;

        // Hit returns the path, size, and layer count.
        let hit = db.get_cached_gcode("deadbeef").await?.expect("cache hit");
        assert_eq!(hit.0, gcode);
        assert_eq!(hit.1, 4);
        assert_eq!(hit.2, 42);

        // A missing file evicts the row and reports a miss.
        std::fs::remove_file(&gcode)?;
        assert!(db.get_cached_gcode("deadbeef").await?.is_none());

        Ok(())
    }

    #[tokio::test]
    async fn test_gcode_cache_upsert_replaces() -> Result<(), Box<dyn std::error::Error>> {
        let dir = TempDir::new()?;
        let db = Database::open(dir.path().join("test.db")).await?;

        let a = dir.path().join("a.gcode");
        let b = dir.path().join("b.gcode");
        std::fs::write(&a, b"a")?;
        std::fs::write(&b, b"bb")?;

        db.put_cached_gcode("k", &a, 1, 1).await?;
        db.put_cached_gcode("k", &b, 2, 9).await?;

        let hit = db.get_cached_gcode("k").await?.expect("cache hit");
        assert_eq!(hit.0, b);
        assert_eq!(hit.1, 2);
        assert_eq!(hit.2, 9);

        Ok(())
    }
}
