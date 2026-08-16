use ozymem_core::graph_backend::GraphBackend;
use ozymem_core::sync::{check_noise_or_huge_file, DeltaFileEvent, DeltaIndexResult, LiveWatcher, NoiseFilterDecision, DEFAULT_MAX_DELTA_FILE_BYTES};
use ozymem_core::McpBackend;
use std::fs;
use std::time::Duration;
use tempfile::tempdir;

#[test]
fn test_delta_hasher_skips_unchanged_content() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let file_path = root.join("calculator.py");

    fs::write(&file_path, "def calculate(a, b):\n    return a + b\n").unwrap();

    let backend = GraphBackend::open_for_project(root).unwrap();

    // 1. Initial indexing: must index 1 function
    let res1 = backend.index_file_delta(&file_path, root).unwrap();
    match res1 {
        DeltaIndexResult::Indexed { symbols, .. } => assert_eq!(symbols, 1),
        other => panic!("Expected DeltaIndexResult::Indexed, got {:?}", other),
    }

    // 2. Unchanged indexing: must return Unchanged without reparsing
    let res2 = backend.index_file_delta(&file_path, root).unwrap();
    assert_eq!(res2, DeltaIndexResult::Unchanged);

    // 3. Modified indexing: add another function
    fs::write(
        &file_path,
        "def calculate(a, b):\n    return a + b\n\ndef multiply(a, b):\n    return a * b\n",
    )
    .unwrap();

    let res3 = backend.index_file_delta(&file_path, root).unwrap();
    match res3 {
        DeltaIndexResult::Indexed { symbols, .. } => assert_eq!(symbols, 2),
        other => panic!("Expected DeltaIndexResult::Indexed with 2 symbols, got {:?}", other),
    }
}

#[test]
fn test_delta_filters_noise_lockfiles_and_minified() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    let minified = root.join("bundle.min.js");
    fs::write(&minified, "function a(){};").unwrap();

    let lockfile = root.join("package-lock.json");
    fs::write(&lockfile, "{\"name\": \"test\"}").unwrap();

    let logfile = root.join("app.log");
    fs::write(&logfile, "2026-08-16 log entry").unwrap();

    assert_eq!(
        check_noise_or_huge_file(&minified, DEFAULT_MAX_DELTA_FILE_BYTES),
        NoiseFilterDecision::MinifiedOrBundle
    );
    assert_eq!(
        check_noise_or_huge_file(&lockfile, DEFAULT_MAX_DELTA_FILE_BYTES),
        NoiseFilterDecision::LockFile
    );
    assert_eq!(
        check_noise_or_huge_file(&logfile, DEFAULT_MAX_DELTA_FILE_BYTES),
        NoiseFilterDecision::LargeOrBinaryData
    );

    let backend = GraphBackend::open_for_project(root).unwrap();
    assert_eq!(
        backend.index_file_delta(&minified, root).unwrap(),
        DeltaIndexResult::SkippedNoise
    );
    assert_eq!(
        backend.index_file_delta(&lockfile, root).unwrap(),
        DeltaIndexResult::SkippedNoise
    );
}

#[test]
fn test_delta_filters_too_large_files() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let large_file = root.join("huge.py");

    // Write a file exceeding 256 KB
    let dummy_content = "def big(): pass\n".repeat(20_000);
    fs::write(&large_file, dummy_content).unwrap();

    let backend = GraphBackend::open_for_project(root).unwrap();
    let res = backend.index_file_delta(&large_file, root).unwrap();

    match res {
        DeltaIndexResult::SkippedTooLarge { size_bytes } => {
            assert!(size_bytes > DEFAULT_MAX_DELTA_FILE_BYTES);
        }
        other => panic!("Expected SkippedTooLarge, got {:?}", other),
    }
}

#[tokio::test]
async fn test_incremental_petgraph_update_and_removal() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let file_a = root.join("service.py");
    let file_b = root.join("models.py");

    fs::write(&file_b, "class User:\n    pass\n").unwrap();
    fs::write(&file_a, "from models import User\n\ndef get_user():\n    return User()\n").unwrap();

    let backend = GraphBackend::open_for_project(root).unwrap();

    let res_b = backend.index_file_delta(&file_b, root).unwrap();
    assert!(matches!(res_b, DeltaIndexResult::Indexed { symbols: 1, .. }));

    let res_a = backend.index_file_delta(&file_a, root).unwrap();
    assert!(matches!(res_a, DeltaIndexResult::Indexed { symbols: 1, .. }));

    // Verify summary reflects both files
    let summary = backend.get_graph_summary().await.unwrap();
    assert_eq!(summary.file_count, 2);
    assert_eq!(summary.function_count, 2);

    // Test removal of file_b
    let removed = backend.remove_file_delta(&file_b, root).unwrap();
    assert!(removed);

    let summary_after = backend.get_graph_summary().await.unwrap();
    assert_eq!(summary_after.file_count, 1);
    assert_eq!(summary_after.function_count, 1);
}

#[test]
fn test_live_watcher_captures_file_events() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    let watcher = LiveWatcher::watch(root, Some(Duration::from_millis(50))).unwrap();
    let test_file = root.join("watcher_test.py");

    fs::write(&test_file, "print('hello watcher')\n").unwrap();

    let event = watcher.recv_timeout(Duration::from_secs(2));
    assert!(event.is_some(), "Expected a live file event within 2s");

    let event = event.unwrap();
    match event {
        DeltaFileEvent::Modified(p) => {
            assert!(p.to_string_lossy().contains("watcher_test.py"));
        }
        DeltaFileEvent::Removed(_) => panic!("Expected DeltaFileEvent::Modified"),
    }
}
