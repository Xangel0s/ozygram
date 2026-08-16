use ozymem_core::graph_backend::GraphBackend;
use ozymem_core::McpBackend;
use std::fs;
use tempfile::tempdir;

#[tokio::test]
async fn test_map_api_routes_fastapi_and_express() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    let py_file = root.join("routes.py");
    fs::write(
        &py_file,
        r#"
from fastapi import APIRouter
router = APIRouter()

@router.get("/api/v1/items")
async fn list_items():
    return []

@router.post("/api/v1/items/create")
async fn create_item(payload: ItemCreateDTO):
    return payload
"#,
    )
    .unwrap();

    let js_file = root.join("app.js");
    fs::write(
        &js_file,
        r#"
const express = require('express');
const app = express();

app.get('/health', healthHandler);
app.delete('/api/v1/items/:id', deleteItem);
"#,
    )
    .unwrap();

    let backend = GraphBackend::open_for_project(root).unwrap();

    // Index both files
    backend.index_file_delta(&py_file, root).unwrap();
    backend.index_file_delta(&js_file, root).unwrap();

    let routes = backend.map_api_routes(None).unwrap();
    assert_eq!(routes.len(), 4);

    let get_items = routes.iter().find(|r| r.path == "/api/v1/items").unwrap();
    assert_eq!(get_items.method, "GET");
    assert_eq!(get_items.framework, "FastAPI");

    let post_items = routes.iter().find(|r| r.path == "/api/v1/items/create").unwrap();
    assert_eq!(post_items.method, "POST");
    assert_eq!(post_items.dto_model, Some("ItemCreateDTO".into()));

    let health = routes.iter().find(|r| r.path == "/health").unwrap();
    assert_eq!(health.method, "GET");
    assert_eq!(health.framework, "Express");

    let delete_item = routes.iter().find(|r| r.path == "/api/v1/items/:id").unwrap();
    assert_eq!(delete_item.method, "DELETE");
}

#[tokio::test]
async fn test_rank_and_prune_lessons_decay_and_scoring() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let file_path = root.join("calc.py");

    fs::write(&file_path, "def add(a, b):\n    return a + b\n").unwrap();

    let backend = GraphBackend::open_for_project(root).unwrap();
    backend.index_file_delta(&file_path, root).unwrap();

    // Record a lesson
    backend
        .record_lesson(
            &file_path.to_string_lossy(),
            Some("add"),
            "addition overflow error",
            "cast inputs to i64 before adding",
        )
        .await
        .unwrap();

    // 1. Initial ranking: file and symbol exist -> healthy confidence ~1.0
    let report1 = backend.rank_and_prune_lessons(0.5).unwrap();
    assert_eq!(report1.active_count, 1);
    assert_eq!(report1.pruned_count, 0);

    // 2. Delete file from disk and re-rank -> confidence decays heavily
    fs::remove_file(&file_path).unwrap();

    let report2 = backend.rank_and_prune_lessons(0.5).unwrap();
    assert_eq!(report2.pruned_count, 1);
    assert!(report2.pruned_lessons[0].reason.contains("file_not_found"));
    assert!(report2.pruned_lessons[0].confidence_score < 0.5);
}

#[tokio::test]
async fn test_code_drift_detection_against_conventions() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let file_path = root.join("order_service.rs");

    fs::write(&file_path, "pub fn process_order() {}\n").unwrap();

    let backend = GraphBackend::open_for_project(root).unwrap();
    backend.index_file_delta(&file_path, root).unwrap();

    // Record convention with 'never' / 'always' rules
    backend
        .record_entry(
            &file_path.to_string_lossy(),
            Some("process_order"),
            "pricing precision standard",
            "never use floating point f64 for currency prices, always use integer cents i64",
            "convention",
        )
        .await
        .unwrap();

    // Diff introducing floating point
    let violating_diff = "+    let price: f64 = 19.99;\n+    println!(\"price: {}\", price);";
    let alerts = backend
        .detect_code_drift(&[file_path.to_string_lossy().to_string()], violating_diff)
        .unwrap();

    assert_eq!(alerts.len(), 1);
    assert_eq!(alerts[0].severity, "warning");
    assert!(alerts[0].diff_snippet.contains("price: f64"));
    assert!(alerts[0].rule_snippet.contains("never use floating point"));

    // Compliant diff -> no alerts
    let compliant_diff = "+    let price_cents: i64 = 1999;";
    let no_alerts = backend
        .detect_code_drift(&[file_path.to_string_lossy().to_string()], compliant_diff)
        .unwrap();
    assert_eq!(no_alerts.len(), 0);
}

#[tokio::test]
async fn test_touch_lesson_boosts_confidence() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let file_path = root.join("math.py");

    fs::write(&file_path, "def sub(a, b): return a - b\n").unwrap();

    let backend = GraphBackend::open_for_project(root).unwrap();
    backend.index_file_delta(&file_path, root).unwrap();

    backend
        .record_lesson(
            &file_path.to_string_lossy(),
            Some("sub"),
            "subtraction context",
            "solution note",
        )
        .await
        .unwrap();

    let lessons = backend.get_file_lessons(&file_path.to_string_lossy()).await.unwrap();
    assert_eq!(lessons.len(), 1);
    let lesson_id = lessons[0].id;

    // Call touch_lesson
    backend.touch_lesson(lesson_id).unwrap();

    let lessons_after = backend.get_file_lessons(&file_path.to_string_lossy()).await.unwrap();
    assert_eq!(lessons_after[0].touch_count, 1);
    assert!(!lessons_after[0].last_verified_at.is_empty());
}
