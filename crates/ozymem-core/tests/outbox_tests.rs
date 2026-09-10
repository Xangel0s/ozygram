use ozymem_core::graph_backend::GraphBackend;
use ozymem_core::McpBackend;
use tempfile::tempdir;

#[tokio::test]
async fn test_outbox_lesson_insert_and_mark_processed() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let backend = GraphBackend::open_for_project(root).unwrap();

    // 1. Initially outbox is empty
    let pending = backend.fetch_pending_outbox(50).unwrap();
    assert!(pending.is_empty(), "Outbox should be initially empty");

    // 2. Record a lesson
    backend
        .record_lesson(
            "src/auth.rs",
            Some("verify_jwt"),
            "JWT expired panic",
            "Handle expired token gracefully by returning AuthError",
        )
        .await
        .unwrap();

    // 3. Verify outbox has the UPSERT event
    let events = backend.fetch_pending_outbox(50).unwrap();
    assert_eq!(events.len(), 1, "Should have 1 pending outbox event");
    let event = &events[0];
    assert_eq!(event.entity_type, "lesson");
    assert_eq!(event.operation, "UPSERT");
    assert!(event.processed_at.is_none());
    assert!(event.payload.contains("verify_jwt"));
    assert!(event.payload.contains("JWT expired panic"));

    // 4. Mark outbox processed
    backend.mark_outbox_processed(&[event.id]).unwrap();

    // 5. Verify outbox is now empty
    let pending_after = backend.fetch_pending_outbox(50).unwrap();
    assert!(pending_after.is_empty(), "Processed events should not be returned as pending");
}

#[tokio::test]
async fn test_outbox_observation_lifecycle() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let backend = GraphBackend::open_for_project(root).unwrap();

    // 1. Start memory session
    backend
        .memory_session_start("sess_100", "test_proj", &root.to_string_lossy())
        .unwrap();

    // 2. Save observation
    let obs = backend
        .save_observation(
            "sess_100",
            "architecture",
            "Clean Architecture Rule",
            "Do not import database models into domain logic",
            Some("test_proj"),
            Some("project"),
            Some("arch_rules"),
            Some("agent"),
        )
        .unwrap();

    // 3. Verify outbox captured the observation
    let events = backend.fetch_pending_outbox(50).unwrap();
    assert_eq!(events.len(), 1);
    let event = &events[0];
    assert_eq!(event.entity_type, "observation");
    assert_eq!(event.entity_id, obs.id.to_string());
    assert_eq!(event.operation, "UPSERT");
    assert!(event.payload.contains("Clean Architecture Rule"));

    // Mark processed
    backend.mark_outbox_processed(&[event.id]).unwrap();
    assert!(backend.fetch_pending_outbox(50).unwrap().is_empty());

    // 4. Update observation with soft-delete
    backend
        .soft_delete_observation(obs.id)
        .unwrap();

    // 5. Verify outbox captured the DELETE event via trigger
    let delete_events = backend.fetch_pending_outbox(50).unwrap();
    assert_eq!(delete_events.len(), 1, "Should record 1 DELETE event upon soft deletion");
    let del_event = &delete_events[0];
    assert_eq!(del_event.entity_type, "observation");
    assert_eq!(del_event.entity_id, obs.id.to_string());
    assert_eq!(del_event.operation, "DELETE");
}

#[tokio::test]
async fn test_outbox_purge_processed() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let backend = GraphBackend::open_for_project(root).unwrap();

    backend
        .record_lesson("src/db.rs", None, "Lock contention", "Enable WAL mode")
        .await
        .unwrap();

    let events = backend.fetch_pending_outbox(10).unwrap();
    assert_eq!(events.len(), 1);
    backend.mark_outbox_processed(&[events[0].id]).unwrap();

    // Purging events older than 0 days (i.e. all processed)
    let purged = backend.purge_processed_outbox(0).unwrap();
    assert_eq!(purged, 1, "Should have purged 1 processed event");
}
