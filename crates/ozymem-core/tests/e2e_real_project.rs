//! End-to-end test: GraphBackend against the actual ozymem-partner project.
//!
//! Scans the project, then tests all 4 MCP tools:
//!   - graph_summary
//!   - analyze_impact
//!   - file_context
//!   - record_lesson
//!
//! Run with: cargo test -p ozymem-core --test e2e_real_project -- --nocapture

use ozymem_core::McpBackend;
use std::path::Path;

/// CARGO_MANIFEST_DIR for ozymem-core is crates/ozymem-core/ — go up 2 levels to workspace root
fn workspace_root() -> String {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    here.parent().and_then(|p| p.parent()).map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| here.to_string_lossy().to_string())
}

fn path_in_root(root: &str, segments: &[&str]) -> String {
    let mut p = Path::new(root).to_path_buf();
    for s in segments {
        p = p.join(s);
    }
    p.to_string_lossy().to_string()
}

#[tokio::test]
async fn test_e2e_graph_summary() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("test_e2e_summary.db").to_string_lossy().to_string();

    let backend = ozymem_core::graph_backend::GraphBackend::open(Some(&db_path)).unwrap();
    let root = workspace_root();
    backend.full_scan(&root, None).unwrap();

    let summary = backend.get_graph_summary().await.unwrap();

    eprintln!("=== graph_summary ===");
    eprintln!("  files:       {}", summary.file_count);
    eprintln!("  functions:   {}", summary.function_count);
    eprintln!("  lessons:     {}", summary.engram_count);
    eprintln!("  native_ast:  {}", summary.native_ast_function_count);
    eprintln!("  wasm:        {}", summary.extension_wasm_function_count);
    eprintln!("  heuristic:   {}", summary.text_heuristic_function_count);
    eprintln!("  vertices:    {}", summary.vertex_count);
    eprintln!("  edges:       {}", summary.edge_count);

    assert!(summary.file_count > 0, "should find at least one file in the project");
    assert!(summary.function_count > 0, "should find at least one function");
}

#[tokio::test]
async fn test_e2e_analyze_impact_on_core() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("test_e2e_impact.db").to_string_lossy().to_string();

    let backend = ozymem_core::graph_backend::GraphBackend::open(Some(&db_path)).unwrap();
    let root = workspace_root();
    backend.full_scan(&root, None).unwrap();

    let core_src = path_in_root(&root, &["crates", "ozymem-core", "src", "graph_backend", "schema.rs"]);
    let impacts = backend.analyze_impact(&core_src, 3);

    eprintln!("=== analyze_impact on graph_backend.rs (depth=3) ===");
    for entry in &impacts {
        eprintln!("  depth={} lessons={} funcs={} lang={} path={}",
            entry.depth, entry.lesson_count, entry.function_count, entry.language, entry.file_path);
    }
    eprintln!("  -> {} transitive dependents found", impacts.len());

    // Record a lesson
    backend.record_lesson(&core_src, Some("GraphBackend"), "test lesson", "E2E test").await.unwrap();
    let impacts2 = backend.analyze_impact(&core_src, 3);
    let total_lessons: i64 = impacts2.iter().map(|e| e.lesson_count).sum();
    eprintln!("  -> total lesson count across dependents: {total_lessons}");
}

#[tokio::test]
async fn test_e2e_file_context() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("test_e2e_context.db").to_string_lossy().to_string();

    let backend = ozymem_core::graph_backend::GraphBackend::open(Some(&db_path)).unwrap();
    let root = workspace_root();
    backend.full_scan(&root, None).unwrap();

    let core_src = path_in_root(&root, &["crates", "ozymem-core", "src", "graph_backend", "schema.rs"]);
    let ctx = backend.get_file_context(&core_src).await.unwrap();

    assert!(ctx.is_some(), "file_context should return Some for {core_src}");
    let ctx = ctx.unwrap();

    eprintln!("=== file_context for graph_backend.rs ===");
    eprintln!("  language: {}", ctx.language);
    eprintln!("  functions: {}", ctx.functions.len());
    for f in &ctx.functions {
        eprintln!("    {} ({} lines {}-{}) [{}]", f.name, f.kind, f.start_line, f.end_line, f.strategy);
    }

    assert_eq!(ctx.language, "Rust");
    assert!(ctx.functions.len() >= 5, "graph_backend.rs should have many functions (found {})", ctx.functions.len());
}

#[tokio::test]
async fn test_e2e_record_lesson_and_history() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("test_e2e_lesson.db").to_string_lossy().to_string();

    let backend = ozymem_core::graph_backend::GraphBackend::open(Some(&db_path)).unwrap();
    let root = workspace_root();
    backend.full_scan(&root, None).unwrap();

    let lib_rs = path_in_root(&root, &["crates", "ozymem-core", "src", "lib.rs"]);

    backend.record_lesson(&lib_rs, Some("full_scan"), "missing dep extraction", "add extract_dependency_hints call").await.unwrap();
    backend.record_lesson(&lib_rs, Some("analyze_impact"), "BFS depth panic", "fix visited depth tracking").await.unwrap();

    let history = backend.get_historical_engram_solutions(&lib_rs).await.unwrap();
    eprintln!("=== lessons recorded ===");
    for h in &history {
        eprintln!("  solution: {h}");
    }

    assert_eq!(history.len(), 2, "should have 2 lessons");
}

#[tokio::test]
async fn test_regression_instances_do_not_share_engrams_in_independent_tempdirs() {
    let dir1 = tempfile::tempdir().unwrap();
    let dir2 = tempfile::tempdir().unwrap();

    let db_path1 = dir1.path().join("db1.db").to_string_lossy().to_string();
    let db_path2 = dir2.path().join("db2.db").to_string_lossy().to_string();

    let b1 = ozymem_core::graph_backend::GraphBackend::open(Some(&db_path1)).unwrap();
    let b2 = ozymem_core::graph_backend::GraphBackend::open(Some(&db_path2)).unwrap();

    b1.engram_store.update_file_incremental("test1.rs", vec![]);
    b2.engram_store.update_file_incremental("test2.rs", vec![]);

    b1.engram_store.compact_and_persist().unwrap();
    b2.engram_store.compact_and_persist().unwrap();

    let rkyv1 = dir1.path().join(ozymem_core::ENGRAM_FILE);
    let rkyv2 = dir2.path().join(ozymem_core::ENGRAM_FILE);

    assert!(rkyv1.exists(), "rkyv1 must exist in dir1");
    assert!(rkyv2.exists(), "rkyv2 must exist in dir2");
    assert_ne!(rkyv1, rkyv2, "instances in separate tempdirs must never share storage paths");
}
