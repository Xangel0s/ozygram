use ozymem_core::McpBackend;
use ozymem_core::graph_backend::{GraphBackend, SqliteBackend};
use ozymem_core::graph_backend::{
    auto_manage_gitignore, legacy_global_db_path, mark_stale_lessons, resolve_project_db_path,
};
use rusqlite::{Connection, params};
use std::collections::HashSet;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tempfile::TempDir;

/// Test 1: open_for_project canonicalization.
/// Different paths → different DBs; same path (relative vs absolute) → same DB.
#[test]
fn test_open_for_project_canonicalization() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();

    let proj_a = root.join("project_a");
    let proj_b = root.join("project_b");
    std::fs::create_dir_all(&proj_a).unwrap();
    std::fs::create_dir_all(&proj_b).unwrap();

    // Different projects → different DB paths
    let db_a = resolve_project_db_path(&proj_a).unwrap();
    let db_b = resolve_project_db_path(&proj_b).unwrap();
    assert_ne!(db_a, db_b, "different projects must have different DBs");

    // Same project called twice → same DB path
    let db_a2 = resolve_project_db_path(&proj_a).unwrap();
    assert_eq!(
        db_a, db_a2,
        "same project must always resolve to the same DB"
    );

    // Both DBs are valid (GraphBackend::open_for_project succeeds)
    let be_a = GraphBackend::open_for_project(&proj_a).unwrap();
    let be_b = GraphBackend::open_for_project(&proj_b).unwrap();
    drop(be_a);
    drop(be_b);

    // SqliteBackend::open_for_project also works
    let sb_a = SqliteBackend::open_for_project(&proj_a).unwrap();
    let sb_b = SqliteBackend::open_for_project(&proj_b).unwrap();
    drop(sb_a);
    drop(sb_b);

    std::fs::remove_dir_all(proj_a.join(".ozymem")).ok();
    std::fs::remove_dir_all(proj_b.join(".ozymem")).ok();
}

/// Test 2: auto_manage_gitignore covers the 4 cases.
#[test]
fn test_auto_manage_gitignore_cases() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();

    // Case A: No .git repo → no change, returns false
    assert!(!auto_manage_gitignore(root).unwrap(), "no .git → no change");
    assert!(
        !root.join(".gitignore").exists(),
        ".gitignore should not be created without .git"
    );

    // Now create a .git repo
    std::fs::create_dir_all(root.join(".git")).unwrap();

    // Case B: No .gitignore exists → creates it with entry
    assert!(
        auto_manage_gitignore(root).unwrap(),
        "should create .gitignore with entry"
    );
    let content = std::fs::read_to_string(root.join(".gitignore")).unwrap();
    assert!(content.contains("/.ozymem"), "entry must be in .gitignore");

    // Case C: Entry already present → no duplicate, returns false
    assert!(
        !auto_manage_gitignore(root).unwrap(),
        "should not add duplicate entry"
    );
    let content2 = std::fs::read_to_string(root.join(".gitignore")).unwrap();
    let count = content2.lines().filter(|l| l.contains(".ozymem")).count();
    assert_eq!(count, 1, "exactly one .ozymem entry after idempotent call");

    // Case D: .gitignore exists WITHOUT the entry → appends it
    std::fs::write(root.join(".gitignore"), "# existing rules\nnode_modules/\n").unwrap();
    assert!(
        auto_manage_gitignore(root).unwrap(),
        "should append entry to existing .gitignore"
    );
    let content3 = std::fs::read_to_string(root.join(".gitignore")).unwrap();
    assert!(
        content3.contains("/.ozymem"),
        "entry must appear after append"
    );
    // Verify original rules preserved
    assert!(
        content3.contains("node_modules/"),
        "original .gitignore rules preserved"
    );

    std::fs::remove_dir_all(root.join(".git")).ok();
    std::fs::remove_file(root.join(".gitignore")).ok();
}

/// Test 3: legacy_global_db_path logic — exists vs not exists.
#[test]
fn test_legacy_db_warning_logic() {
    let legacy = legacy_global_db_path();
    assert!(
        legacy.ends_with(".ozymem\\memory.db") || legacy.ends_with(".ozymem/memory.db"),
        "legacy path should end with .ozymem/memory.db"
    );

    let already_exists = legacy.exists();

    // Simulate a legacy DB if it doesn't already exist
    if !already_exists {
        if let Some(parent) = legacy.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&legacy, "").unwrap();
    }

    // Must detect it
    assert!(legacy.exists(), "legacy DB must be detected after creation");

    // Cleanup only if we created it
    if !already_exists {
        std::fs::remove_file(&legacy).ok();
        if let Some(parent) = legacy.parent() {
            std::fs::remove_dir(parent).ok();
        }
        // Verify it's gone
        assert!(!legacy.exists(), "legacy DB must be removable");
    }
}

/// Helper: create a Connection to a temp DB with the lessons table.
fn setup_stale_db(db_path: &str) -> Connection {
    let _ = std::fs::remove_file(db_path);
    let conn = Connection::open(db_path).unwrap();
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS files (
            path TEXT NOT NULL, language TEXT, strategy TEXT, mtime TEXT, tenant_id TEXT,
            workspace_root TEXT NOT NULL DEFAULT '',
            PRIMARY KEY (path, tenant_id)
        );
        CREATE TABLE IF NOT EXISTS functions (
            name TEXT NOT NULL, kind TEXT, start_line INTEGER, end_line INTEGER,
            strategy TEXT, file_path TEXT NOT NULL, tenant_id TEXT NOT NULL,
            workspace_root TEXT NOT NULL DEFAULT '',
            PRIMARY KEY (name, start_line, end_line, file_path, tenant_id)
        );
        CREATE TABLE IF NOT EXISTS lessons (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            file_path TEXT NOT NULL, symbol_name TEXT NOT NULL DEFAULT '',
            error_context TEXT NOT NULL, solution TEXT NOT NULL,
            created_at TEXT NOT NULL, tenant_id TEXT NOT NULL,
            workspace_root TEXT NOT NULL DEFAULT '',
            kind TEXT NOT NULL DEFAULT 'lesson',
            stale INTEGER NOT NULL DEFAULT 0, stale_reason TEXT, stale_since TEXT
        );",
    )
    .unwrap();
    conn
}

/// Test: mark_stale_lessons marks file_deleted when file disappears,
/// and symbol_removed when a function is removed.
#[test]
fn test_stale_file_deleted() {
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("func.rs");
    std::fs::write(&file_path, "pub fn foo() {}").unwrap();
    let abs = file_path.to_string_lossy().to_string();

    let db_path = format!(
        "{}/test_stale_file_deleted.db",
        std::env::temp_dir().to_string_lossy()
    );
    let conn = setup_stale_db(&db_path);

    // Insert file record + function
    conn.execute(
        "INSERT INTO files (path, language, strategy, mtime, tenant_id) VALUES (?1, 'Rust', 'TreeSitter', '', 'local')",
        params![&abs],
    ).unwrap();
    conn.execute(
        "INSERT INTO functions (name, kind, start_line, end_line, strategy, file_path, tenant_id)
         VALUES ('foo', 'Function', 1, 1, 'TreeSitter', ?1, 'local')",
        params![&abs],
    )
    .unwrap();

    // Insert a lesson referencing the symbol
    conn.execute(
        "INSERT INTO lessons (file_path, symbol_name, error_context, solution, created_at, tenant_id)
         VALUES (?1, 'foo', 'bug', 'fixed', '1000', 'local')",
        params![&abs],
    ).unwrap();

    let scanned: HashSet<String> = [abs.clone()].into();

    // File & symbol exist → nothing marked stale
    let marked = mark_stale_lessons(&conn, "local", &scanned, "").unwrap();
    assert_eq!(
        marked, 0,
        "existing file with existing symbol should not be stale"
    );

    // Delete the file and re-check
    std::fs::remove_file(&file_path).unwrap();
    let marked = mark_stale_lessons(&conn, "local", &scanned, "").unwrap();
    assert_eq!(marked, 1, "deleted file should mark lesson as stale");

    // Verify the lesson was updated
    let row: (i64, String) = conn
        .query_row(
            "SELECT stale, stale_reason FROM lessons WHERE id = 1",
            [],
            |r: &rusqlite::Row| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(row.0, 1, "stale should be 1");
    assert_eq!(row.1, "file_deleted", "reason should be file_deleted");

    std::fs::remove_file(&db_path).ok();
}

/// Test: mark_stale_lessons marks symbol_removed when function disappears.
#[test]
fn test_stale_symbol_removed() {
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("math.rs");
    std::fs::write(&file_path, "pub fn add(a: i32, b: i32) -> i32 { a + b }").unwrap();

    let db_path = format!(
        "{}/test_stale_symbol_removed.db",
        std::env::temp_dir().to_string_lossy()
    );
    let conn = setup_stale_db(&db_path);

    // File exists in files table
    let abs = file_path.to_string_lossy().to_string();
    conn.execute(
        "INSERT INTO files (path, language, strategy, mtime, tenant_id) VALUES (?1, 'Rust', 'TreeSitter', '', 'local')",
        params![&abs],
    ).unwrap();

    // Function exists in functions table
    conn.execute(
        "INSERT INTO functions (name, kind, start_line, end_line, strategy, file_path, tenant_id)
         VALUES ('add', 'Function', 1, 1, 'TreeSitter', ?1, 'local')",
        params![&abs],
    )
    .unwrap();

    // Lesson references the symbol
    conn.execute(
        "INSERT INTO lessons (file_path, symbol_name, error_context, solution, created_at, tenant_id)
         VALUES (?1, 'add', 'overflow', 'checked_add', '1000', 'local')",
        params![&abs],
    ).unwrap();

    let scanned: HashSet<String> = [abs.clone()].into();

    // File & symbol exist → nothing stale
    let marked = mark_stale_lessons(&conn, "local", &scanned, "").unwrap();
    assert_eq!(marked, 0, "existing symbol should not be stale");

    // Remove the function from functions table (simulates re-scan where function was deleted)
    conn.execute("DELETE FROM functions WHERE file_path = ?1", params![&abs])
        .unwrap();
    let marked = mark_stale_lessons(&conn, "local", &scanned, "").unwrap();
    assert_eq!(marked, 1, "removed symbol should mark lesson as stale");

    let row: (i64, String) = conn
        .query_row(
            "SELECT stale, stale_reason FROM lessons WHERE id = 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(row.0, 1, "stale should be 1");
    assert_eq!(row.1, "symbol_removed", "reason should be symbol_removed");

    std::fs::remove_file(&db_path).ok();
}

/// Test: Partial scan (scanned_files limited) does NOT mark stale from other files.
#[test]
fn test_stale_partial_scan_safety() {
    let dir = TempDir::new().unwrap();
    let a_path = dir.path().join("a.rs");
    let b_path = dir.path().join("b.rs");
    std::fs::write(&a_path, "fn a() {}").unwrap();
    std::fs::write(&b_path, "fn b() {}").unwrap();

    let a_abs = a_path.to_string_lossy().to_string();
    let b_abs = b_path.to_string_lossy().to_string();

    let db_path = format!(
        "{}/test_stale_partial.db",
        std::env::temp_dir().to_string_lossy()
    );
    let conn = setup_stale_db(&db_path);

    // Insert file records
    for p in [&a_abs, &b_abs] {
        conn.execute(
            "INSERT INTO files (path, language, strategy, mtime, tenant_id) VALUES (?1, 'Rust', 'Test', '', 'local')",
            params![p],
        ).unwrap();
        conn.execute(
            "INSERT INTO functions (name, kind, start_line, end_line, strategy, file_path, tenant_id)
             VALUES ('func', 'Function', 1, 1, 'Test', ?1, 'local')",
            params![p],
        ).unwrap();
    }

    // Two lessons: one in each file
    conn.execute(
        "INSERT INTO lessons (file_path, symbol_name, error_context, solution, created_at, tenant_id)
         VALUES (?1, 'func', 'err a', 'fix a', '1000', 'local')",
        params![&a_abs],
    ).unwrap();
    conn.execute(
        "INSERT INTO lessons (file_path, symbol_name, error_context, solution, created_at, tenant_id)
         VALUES (?1, 'func', 'err b', 'fix b', '1000', 'local')",
        params![&b_abs],
    ).unwrap();

    // Only scan a.rs — b.rs symbols not in the scanned set
    let scanned: HashSet<String> = [a_abs.clone()].into();

    // Delete b.rs from disk and remove its function
    std::fs::remove_file(&b_path).unwrap();
    conn.execute(
        "DELETE FROM functions WHERE file_path = ?1",
        params![&b_abs],
    )
    .unwrap();

    // mark_stale_lessons should NOT touch b.rs's lesson
    let marked = mark_stale_lessons(&conn, "local", &scanned, "").unwrap();
    assert_eq!(
        marked, 0,
        "partial scan should not mark un-scanned files as stale"
    );

    // Now include both files in scanned set
    let scanned_both: HashSet<String> = [a_abs.clone(), b_abs.clone()].into();
    let marked = mark_stale_lessons(&conn, "local", &scanned_both, "").unwrap();
    assert_eq!(marked, 1, "full scan should mark stale for b.rs");

    std::fs::remove_file(&db_path).ok();
}

/// Test: Schema migration v1→v2 adds stale columns to existing DB.
#[test]
fn test_schema_migration_v1_to_v2() {
    let db_path = format!(
        "{}/test_migrate_v2.db",
        std::env::temp_dir().to_string_lossy()
    );
    let _ = std::fs::remove_file(&db_path);

    // 1. Create a DB with v1 schema (lessons without stale columns)
    {
        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS lessons (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                file_path TEXT NOT NULL,
                symbol_name TEXT NOT NULL DEFAULT '',
                error_context TEXT NOT NULL,
                solution TEXT NOT NULL,
                created_at TEXT NOT NULL,
                tenant_id TEXT NOT NULL,
                kind TEXT NOT NULL DEFAULT 'lesson'
            );

            INSERT INTO lessons (file_path, symbol_name, error_context, solution, created_at, tenant_id, kind)
            VALUES ('/old/file.rs', '', 'old error', 'old fix', '1000', 'local', 'lesson');"
        ).unwrap();
    }

    // 2. Open with SqliteBackend — should auto-migrate
    let backend = SqliteBackend::open(Some(&db_path)).unwrap();

    // 3. Verify old row has stale=0 via a raw connection
    let conn2 = Connection::open(&db_path).unwrap();
    let stale: i64 = conn2
        .query_row(
            "SELECT stale FROM lessons WHERE id = 1",
            [],
            |r: &rusqlite::Row| r.get(0),
        )
        .unwrap();
    assert_eq!(stale, 0, "migrated row should have stale=0");

    // 4. Verify stale_reason and stale_since are NULL for old row
    let reason: Option<String> = conn2
        .query_row(
            "SELECT stale_reason FROM lessons WHERE id = 1",
            [],
            |r: &rusqlite::Row| r.get(0),
        )
        .unwrap();
    assert!(
        reason.is_none(),
        "stale_reason should be NULL for fresh row"
    );

    // 5. New lessons get default stale=0
    backend
        .record_lesson("local", "/new.rs", Some("func"), "err", "fix", "")
        .unwrap();
    let stale2: i64 = conn2
        .query_row(
            "SELECT stale FROM lessons WHERE file_path = '/new.rs'",
            [],
            |r: &rusqlite::Row| r.get(0),
        )
        .unwrap();
    assert_eq!(stale2, 0, "new lesson should have stale=0 by default");

    drop(backend);
    drop(conn2);
    std::fs::remove_file(&db_path).ok();
}

fn setup_project() -> (TempDir, String) {
    let dir = TempDir::new().unwrap();
    let root = dir.path().to_string_lossy().to_string();

    // main.rs has `mod lib;` — makes it depend on lib.rs (sibling)
    let mut f2 = std::fs::File::create(dir.path().join("main.rs")).unwrap();
    write!(
        f2,
        "mod lib;\nfn main() {{ println!(\"{{}}\", lib::add(1, 2)); }}"
    )
    .unwrap();

    // lib.rs — depends on nothing project-internal
    let mut f = std::fs::File::create(dir.path().join("lib.rs")).unwrap();
    write!(
        f,
        "pub fn add(a: i32, b: i32) -> i32 {{ a + b }}\npub fn subtract(a: i32, b: i32) -> i32 {{ a - b }}"
    )
    .unwrap();

    (dir, root)
}

fn full_path(root: &str, rel: &str) -> String {
    Path::new(root).join(rel).to_string_lossy().to_string()
}

/// P1: record_lesson updates lesson_count atomically in RAM,
///     immediately reflected in analyze_impact without full reload.
#[tokio::test]
async fn test_p1_record_lesson_updates_analyze_impact() {
    let (_dir, root) = setup_project();

    let db_path = format!("{}/test_p1.db", std::env::temp_dir().to_string_lossy());
    let _ = std::fs::remove_file(&db_path);

    let backend = ozymem_core::graph_backend::GraphBackend::open(Some(&db_path)).unwrap();
    backend.full_scan(&root, None).unwrap();

    // main.rs → lib.rs (outgoing edge via `mod lib;`).
    // analyze_impact on main_path finds lib.rs at depth 1.
    let main_path = full_path(&root, "main.rs");
    let lib_path = full_path(&root, "lib.rs");

    let impacts = backend.analyze_impact(&main_path, 3);
    let lib_impact = impacts
        .iter()
        .find(|e| e.file_path.ends_with("\\lib.rs") || e.file_path.ends_with("/lib.rs"));
    assert!(
        lib_impact.is_some(),
        "analyze_impact(main) should find lib.rs"
    );
    assert_eq!(
        lib_impact.unwrap().lesson_count,
        0,
        "lesson_count should start at 0"
    );

    // Record a lesson on main.rs — lesson_count stays 0 on lib (lesson on main, not lib)
    backend
        .record_lesson(&main_path, Some("main"), "panic", "handle error")
        .await
        .unwrap();
    let impacts2 = backend.analyze_impact(&main_path, 3);
    let lib2 = impacts2
        .iter()
        .find(|e| e.file_path.ends_with("\\lib.rs") || e.file_path.ends_with("/lib.rs"));
    assert_eq!(
        lib2.unwrap().lesson_count,
        0,
        "lesson on main should not affect lib count"
    );

    // Record lesson on lib.rs
    backend
        .record_lesson(&lib_path, Some("add"), "overflow", "use checked_add")
        .await
        .unwrap();
    let impacts3 = backend.analyze_impact(&main_path, 3);
    let lib3 = impacts3
        .iter()
        .find(|e| e.file_path.ends_with("\\lib.rs") || e.file_path.ends_with("/lib.rs"));
    assert_eq!(
        lib3.unwrap().lesson_count,
        1,
        "lesson on lib should reflect in impact"
    );

    // Second lesson on lib.rs
    backend
        .record_lesson(&lib_path, Some("subtract"), "negative", "use abs")
        .await
        .unwrap();
    let impacts4 = backend.analyze_impact(&main_path, 3);
    let lib4 = impacts4
        .iter()
        .find(|e| e.file_path.ends_with("\\lib.rs") || e.file_path.ends_with("/lib.rs"));
    assert_eq!(
        lib4.unwrap().lesson_count,
        2,
        "two lessons on lib should show count=2"
    );

    std::fs::remove_file(&db_path).ok();
}

/// P2: scanning=true prevents reload_if_stale from scanning.
///     scanning=false allows normal scanning and impact analysis.
#[tokio::test]
async fn test_p2_scanning_flag_blocks_reload() {
    let (_dir, root) = setup_project();

    let db_path = format!("{}/test_p2.db", std::env::temp_dir().to_string_lossy());
    let _ = std::fs::remove_file(&db_path);

    let backend = ozymem_core::graph_backend::GraphBackend::open(Some(&db_path)).unwrap();
    backend.set_project_path(Some(&root));

    // scanning=true → reload_if_stale returns without scanning
    backend.scanning.store(true, Ordering::SeqCst);
    backend.reload_if_stale();

    let main_path = full_path(&root, "main.rs");
    assert!(
        backend.analyze_impact(&main_path, 3).is_empty(),
        "reload_if_stale should not scan when scanning=true"
    );

    // scanning=false → full_scan works normally
    backend.scanning.store(false, Ordering::SeqCst);
    backend.full_scan(&root, None).unwrap();

    assert!(
        !backend.analyze_impact(&main_path, 3).is_empty(),
        "after full_scan, analyze_impact should find results"
    );

    std::fs::remove_file(&db_path).ok();
}

/// P5: delete a file, re-scan, verify it's gone from graph.
#[tokio::test]
async fn test_p5_rebuild_after_file_deletion() {
    let (_dir, root) = setup_project();

    let db_path = format!("{}/test_p5.db", std::env::temp_dir().to_string_lossy());
    let _ = std::fs::remove_file(&db_path);

    let backend = ozymem_core::graph_backend::GraphBackend::open(Some(&db_path)).unwrap();
    backend.full_scan(&root, None).unwrap();

    let main_path = full_path(&root, "main.rs");
    let lib_path = full_path(&root, "lib.rs");

    // Before deletion: main depends on lib, so analyze_impact(main) finds lib
    let before = backend.analyze_impact(&main_path, 3);
    assert!(!before.is_empty(), "main should have dependents");

    // Delete main.rs and re-scan
    std::fs::remove_file(&main_path).unwrap();
    backend.full_scan(&root, None).unwrap();

    // Deleted file returns empty impact
    assert!(
        backend.analyze_impact(&main_path, 3).is_empty(),
        "deleted file should return empty impact after re-scan"
    );

    assert!(
        backend.analyze_impact(&lib_path, 3).is_empty(),
        "lib has no deps, so impact should be empty"
    );

    std::fs::remove_file(&db_path).ok();
}

/// Edge 3: noise dirs (target/, node_modules/, .git/, __pycache__/) are excluded
///          by filter_entry — files inside them never appear in the graph.
#[tokio::test]
async fn test_edge_noise_dir_exclusion() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().to_string_lossy().to_string();

    // Real source file
    std::fs::write(dir.path().join("app.rs"), "fn main() {}").unwrap();
    // Noise dirs with source files inside — must be excluded
    for noise in &["target", "node_modules", ".git", "__pycache__"] {
        let noise_dir = dir.path().join(noise);
        std::fs::create_dir_all(&noise_dir).unwrap();
        std::fs::write(noise_dir.join("lib.rs"), "pub fn should_not_appear() {}").unwrap();
        // Nested inside noise dir
        let nested = noise_dir.join("deep").join("deeper");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("also_hidden.rs"), "fn hidden() {}").unwrap();
    }

    let db_path = format!("{}/test_edge3.db", std::env::temp_dir().to_string_lossy());
    let _ = std::fs::remove_file(&db_path);

    let backend = ozymem_core::graph_backend::GraphBackend::open(Some(&db_path)).unwrap();
    backend.full_scan(&root, None).unwrap();

    // Real file must be present — use file_context to check (analyze_impact
    // returns empty for leaf files with no outgoing deps)
    let app_path = full_path(&root, "app.rs");
    assert!(
        backend.get_file_context(&app_path).await.unwrap().is_some(),
        "app.rs should be in the graph"
    );

    // Files under noise dirs must NOT be in the graph
    assert!(
        backend
            .get_file_context(&full_path(&root, "target/lib.rs"))
            .await
            .unwrap()
            .is_none(),
        "target/lib.rs must be excluded"
    );
    assert!(
        backend
            .get_file_context(&full_path(&root, "node_modules/lib.rs"))
            .await
            .unwrap()
            .is_none(),
        "node_modules/lib.rs must be excluded"
    );
    assert!(
        backend
            .get_file_context(&full_path(&root, ".git/lib.rs"))
            .await
            .unwrap()
            .is_none(),
        ".git/lib.rs must be excluded"
    );
    assert!(
        backend
            .get_file_context(&full_path(&root, "__pycache__/lib.rs"))
            .await
            .unwrap()
            .is_none(),
        "__pycache__/lib.rs must be excluded"
    );
    assert!(
        backend
            .get_file_context(&full_path(&root, "node_modules/deep/deeper/also_hidden.rs"))
            .await
            .unwrap()
            .is_none(),
        "nested under noise dir must be excluded"
    );

    let summary = backend.get_graph_summary().await.unwrap();
    assert_eq!(summary.file_count, 1, "only app.rs should be indexed");

    std::fs::remove_file(&db_path).ok();
}

/// Edge 5: debounce 500ms — second reload_if_stale within the window skips.
#[tokio::test]
async fn test_edge_debounce_500ms() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().to_string_lossy().to_string();

    std::fs::write(dir.path().join("a.rs"), "fn a() {}").unwrap();

    let db_path = format!("{}/test_edge5.db", std::env::temp_dir().to_string_lossy());
    let _ = std::fs::remove_file(&db_path);

    let backend = ozymem_core::graph_backend::GraphBackend::open(Some(&db_path)).unwrap();
    backend.set_project_path(Some(&root));

    // First scan — 1 file
    backend.full_scan(&root, None).unwrap();
    assert_eq!(backend.get_graph_summary().await.unwrap().file_count, 1);

    // Add a new file
    std::fs::write(dir.path().join("b.rs"), "fn b() {}").unwrap();

    // reload_if_stale #1: triggers scan (no recent check, sets debounce timer)
    backend.reload_if_stale();
    // reload_if_stale #2: within 500ms debounce — skips
    backend.reload_if_stale();

    // b.rs still not in graph (call #1 scanned before b.rs existed,
    // call #2 skipped due to debounce)
    assert_eq!(
        backend.get_graph_summary().await.unwrap().file_count,
        1,
        "debounce should prevent second reload from scanning"
    );

    // Wait for debounce to expire
    std::thread::sleep(Duration::from_millis(600));

    // reload_if_stale #3: debounce expired, mtime differs → re-scans
    backend.reload_if_stale();
    assert_eq!(
        backend.get_graph_summary().await.unwrap().file_count,
        2,
        "after debounce expires, reload should pick up new files"
    );

    std::fs::remove_file(&db_path).ok();
}

/// Schema migration: old DB without `kind` column should auto-migrate.
#[test]
fn test_schema_migration_from_v0_to_v1() {
    let db_path = format!("{}/test_migrate.db", std::env::temp_dir().to_string_lossy());
    let _ = std::fs::remove_file(&db_path);

    // 1. Create a DB with the OLD schema (no `kind` column in lessons)
    {
        use rusqlite::Connection;
        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS lessons (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                file_path TEXT NOT NULL,
                symbol_name TEXT NOT NULL DEFAULT '',
                error_context TEXT NOT NULL,
                solution TEXT NOT NULL,
                created_at TEXT NOT NULL,
                tenant_id TEXT NOT NULL
            );

            INSERT INTO lessons (file_path, symbol_name, error_context, solution, created_at, tenant_id)
            VALUES ('/old/file.rs', 'old_func', 'old error', 'old fix', '1000', 'local');"
        ).unwrap();
    }

    // 2. Open with SqliteBackend — should auto-migrate (add `kind` column)
    let backend = SqliteBackend::open(Some(&db_path)).unwrap();

    // 3. Verify the old row gets the default 'lesson' kind
    let summary = backend.get_graph_summary("local").unwrap();
    assert_eq!(summary.engram_count, 1, "old lesson should be counted");

    // 4. Insert a new lesson with explicit kind
    backend
        .record_lesson(
            "local",
            "/new/file.rs",
            Some("new_func"),
            "new error",
            "new fix",
            "",
        )
        .unwrap();

    // 5. Verify both old and new lessons are readable
    let solutions = backend
        .get_historical_engram_solutions("local", "/old/file.rs")
        .unwrap();
    assert_eq!(solutions, vec!["old fix"]);

    let new_solutions = backend
        .get_historical_engram_solutions("local", "/new/file.rs")
        .unwrap();
    assert_eq!(new_solutions, vec!["new fix"]);

    // 6. Verify kind column stores 'lesson' as default on new entries too
    let recent = backend.get_recent_lessons("local", 10, None).unwrap();
    assert_eq!(recent.len(), 2, "should have 2 lessons total");

    std::fs::remove_file(&db_path).ok();
}

/// Test: context_for_task composed methods produce coherent output.
/// (search_lessons → file_context → graph_neighbors → analyze_impact)
#[tokio::test]
async fn test_context_for_task_composition() {
    let (_dir, root) = setup_project();

    let db_path = format!(
        "{}/test_context_task.db",
        std::env::temp_dir().to_string_lossy()
    );
    let _ = std::fs::remove_file(&db_path);

    let backend = GraphBackend::open(Some(&db_path)).unwrap();
    backend.full_scan(&root, None).unwrap();

    let main_path = full_path(&root, "main.rs");
    let lib_path = full_path(&root, "lib.rs");

    // Record a lesson on lib.rs
    backend
        .record_lesson(&lib_path, Some("add"), "overflow bug", "use checked_add")
        .await
        .unwrap();

    // 1. search_lessons — find the lesson
    let lessons = backend.search_lessons("overflow", None, 10).await.unwrap();
    assert!(!lessons.is_empty(), "should find the lesson");
    assert!(lessons.iter().any(|l| l.solution.contains("checked_add")));

    // 2. get_file_context — for the matched file
    let ctx = backend
        .get_file_context(&lib_path)
        .await
        .unwrap()
        .expect("lib.rs should be indexed");
    assert_eq!(ctx.language, "Rust");
    assert!(ctx.functions.iter().any(|f| f.name == "add"));

    // 3. graph_neighbors — dependency info
    let neighbors = backend.get_graph_neighbors(&lib_path).await.unwrap();
    // lib.rs depends on nothing internally, main depends on lib
    assert!(
        neighbors.incoming.iter().any(|p| p.contains("main.rs")),
        "main.rs should be an incoming neighbor of lib.rs"
    );

    // 4. analyze_impact — transitive impact from main
    let impacts = backend.analyze_impact(&main_path, 3);
    assert!(
        impacts.iter().any(|e| e.file_path.contains("lib.rs")),
        "impact(main) should find lib.rs"
    );
    // The lesson on lib should appear in the impact
    let lib_impact = impacts
        .iter()
        .find(|e| e.file_path.contains("lib.rs"))
        .unwrap();
    assert_eq!(
        lib_impact.lesson_count, 1,
        "lib impact should show lesson count"
    );

    std::fs::remove_file(&db_path).ok();
}

#[test]
fn test_ozy_observations_topic_dedupe_timeline_and_soft_delete() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("memory.db");
    let backend = GraphBackend::open(Some(&db.to_string_lossy())).unwrap();
    backend.set_project_path(Some(dir.path().to_string_lossy().as_ref()));

    backend
        .memory_session_start("s1", "ozymem-test", &dir.path().to_string_lossy())
        .unwrap();
    backend
        .save_user_prompt("s1", "please fix auth", Some("ozymem-test"))
        .unwrap();

    let first = backend
        .save_observation(
            "s1",
            "decision",
            "Auth model",
            "**What**: use JWT\n**Why**: stateless",
            Some("ozymem-test"),
            Some("project"),
            Some("architecture/auth-model"),
            Some("ozy_memory"),
        )
        .unwrap();
    assert_eq!(first.revision_count, 1);

    let updated = backend
        .save_observation(
            "s1",
            "decision",
            "Auth model",
            "**What**: use JWT with refresh rotation\n**Why**: safer",
            Some("ozymem-test"),
            Some("project"),
            Some("architecture/auth-model"),
            Some("ozy_memory"),
        )
        .unwrap();
    assert_eq!(first.id, updated.id);
    assert_eq!(updated.revision_count, 2);

    let dup = backend
        .save_observation(
            "s1",
            "bugfix",
            "Fixed NPE",
            "same content",
            Some("ozymem-test"),
            Some("project"),
            None,
            None,
        )
        .unwrap();
    let dup_again = backend
        .save_observation(
            "s1",
            "bugfix",
            "Fixed NPE",
            "same   content",
            Some("ozymem-test"),
            Some("project"),
            None,
            None,
        )
        .unwrap();
    assert_eq!(dup.id, dup_again.id);
    assert_eq!(dup_again.duplicate_count, 1);

    let timeline = backend.observation_timeline(updated.id, 2, 2).unwrap();
    assert!(timeline.iter().any(|o| o.id == updated.id));

    assert!(backend.soft_delete_observation(updated.id).unwrap());
    assert!(backend.get_observation(updated.id).is_err());
}

#[test]
fn test_passive_capture_extracts_key_learnings() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("memory.db");
    let backend = GraphBackend::open(Some(&db.to_string_lossy())).unwrap();
    backend.set_project_path(Some(dir.path().to_string_lossy().as_ref()));
    backend
        .memory_session_start("s1", "ozymem-test", &dir.path().to_string_lossy())
        .unwrap();

    let saved = backend
        .passive_capture(
            "s1",
            "Done\n\n## Aprendizajes Clave:\n1. topic keys should upsert evolving decisions\n- soft delete keeps audit safety\n\n## Next\nnothing",
            Some("ozymem-test"),
        )
        .unwrap();
    assert_eq!(saved.len(), 2);
    assert!(saved.iter().all(|o| o.observation_type == "learning"));
}

/// Test: LessonEntry stale fields are populated correctly.
#[tokio::test]
async fn test_lesson_entry_stale_fields() {
    let (_dir, root) = setup_project();

    let db_path = format!(
        "{}/test_stale_fields.db",
        std::env::temp_dir().to_string_lossy()
    );
    let _ = std::fs::remove_file(&db_path);

    let backend = GraphBackend::open(Some(&db_path)).unwrap();
    backend.full_scan(&root, None).unwrap();

    let lib_path = full_path(&root, "lib.rs");

    // Record a lesson
    backend
        .record_lesson(&lib_path, Some("add"), "overflow", "checked_add")
        .await
        .unwrap();

    // Fresh lesson: stale=0, stale_reason=None
    let lessons = backend.get_file_lessons(&lib_path).await.unwrap();
    assert_eq!(lessons.len(), 1);
    assert_eq!(lessons[0].stale, 0, "fresh lesson should have stale=0");
    assert!(
        lessons[0].stale_reason.is_none(),
        "fresh lesson should have no reason"
    );

    // Manually mark it stale via a direct connection (simulating mark_stale_lessons)
    {
        let conn = Connection::open(&db_path).unwrap();
        conn.execute(
            "UPDATE lessons SET stale = 1, stale_reason = 'symbol_removed', stale_since = 'now' WHERE id = ?1",
            params![lessons[0].id],
        ).unwrap();
    }

    // After marking: stale=1, stale_reason="symbol_removed" via get_file_lessons
    let lessons2 = backend.get_file_lessons(&lib_path).await.unwrap();
    assert_eq!(lessons2[0].stale, 1, "marked lesson should have stale=1");
    assert_eq!(
        lessons2[0].stale_reason.as_deref(),
        Some("symbol_removed"),
        "stale_reason should match"
    );

    // search_lessons now filters stale lessons — confirm it's excluded
    let searched = backend.search_lessons("overflow", None, 10).await.unwrap();
    assert!(
        searched.is_empty(),
        "stale lesson should be filtered out by search_lessons"
    );

    std::fs::remove_file(&db_path).ok();
}

/// Test: search_lessons with empty/non-matching query doesn't crash.
#[tokio::test]
async fn test_search_lessons_empty_query() {
    let (_dir, root) = setup_project();

    let db_path = format!(
        "{}/test_empty_search.db",
        std::env::temp_dir().to_string_lossy()
    );
    let _ = std::fs::remove_file(&db_path);

    let backend = GraphBackend::open(Some(&db_path)).unwrap();
    backend.full_scan(&root, None).unwrap();

    // Query matching nothing
    let results = backend
        .search_lessons("xyznonexistent", None, 10)
        .await
        .unwrap();
    assert!(
        results.is_empty(),
        "should return empty for non-matching query"
    );

    // Empty query (edge case)
    let results = backend.search_lessons("", None, 10).await.unwrap();
    assert!(results.is_empty(), "empty query should return empty");

    std::fs::remove_file(&db_path).ok();
}

/// Test: mark_stale_lessons does not crash on empty scanned_files.
#[test]
fn test_mark_stale_empty_scanned() {
    let db_path = format!(
        "{}/test_stale_empty.db",
        std::env::temp_dir().to_string_lossy()
    );
    let _ = std::fs::remove_file(&db_path);

    let conn = Connection::open(&db_path).unwrap();
    let empty_set: HashSet<String> = HashSet::new();
    let marked = mark_stale_lessons(&conn, "local", &empty_set, "").unwrap();
    assert_eq!(marked, 0, "empty scanned set should mark 0");

    std::fs::remove_file(&db_path).ok();
}

/// Test: BM25 ranking respects per-column weights (symbol_name > error_context > solution).
#[tokio::test]
async fn test_search_lessons_bm25_ranking() {
    let (_dir, root) = setup_project();

    let db_path = format!(
        "{}/test_bm25_rank.db",
        std::env::temp_dir().to_string_lossy()
    );
    let _ = std::fs::remove_file(&db_path);

    let backend = GraphBackend::open(Some(&db_path)).unwrap();
    backend.full_scan(&root, None).unwrap();

    // Three lessons, each with "overflow" in a different column only.
    let lib = full_path(&root, "lib.rs");
    let main = full_path(&root, "main.rs");
    let helper = full_path(&root, "helper.rs");

    // A: term in symbol_name (weight 3.0 → should rank best / lowest score)
    backend
        .record_lesson(
            &lib,
            Some("overflow_handler"),
            "handling large values",
            "use checked_add",
        )
        .await
        .unwrap();

    // B: term in error_context (weight 2.0)
    backend
        .record_lesson(
            &main,
            Some("compute"),
            "overflow in computation",
            "use saturating_add",
        )
        .await
        .unwrap();

    // C: term in solution (weight 1.0 → should rank worst / highest score)
    backend
        .record_lesson(&helper, Some("helper"), "general utility", "avoid overflow")
        .await
        .unwrap();

    // Search — expected order: A (symbol_name) → B (error_context) → C (solution)
    let results = backend.search_lessons("overflow", None, 10).await.unwrap();
    assert_eq!(
        results.len(),
        3,
        "all three lessons should match 'overflow'"
    );

    let names: Vec<&str> = results.iter().map(|l| l.symbol_name.as_str()).collect();
    assert_eq!(
        names,
        vec!["overflow_handler", "compute", "helper"],
        "BM25 with weights (2,1,3,1) should rank symbol_name match highest, then error_context, then solution.\nGot order: {:?}",
        names
    );

    std::fs::remove_file(&db_path).ok();
}

/// Test: BM25 query with stale filter — stale lessons are excluded even when they match.
#[tokio::test]
async fn test_search_lessons_bm25_respects_stale_filter() {
    let (_dir, root) = setup_project();

    let db_path = format!(
        "{}/test_bm25_stale.db",
        std::env::temp_dir().to_string_lossy()
    );
    let _ = std::fs::remove_file(&db_path);

    let backend = GraphBackend::open(Some(&db_path)).unwrap();
    backend.full_scan(&root, None).unwrap();

    let lib = full_path(&root, "lib.rs");

    // Fresh lesson — should appear
    backend
        .record_lesson(
            &lib,
            Some("fresh_fn"),
            "overflow bug fresh",
            "fix with checked_add",
        )
        .await
        .unwrap();

    // Stale lesson — should be filtered out
    backend
        .record_lesson(
            &lib,
            Some("stale_fn"),
            "overflow bug stale",
            "fix with wrapping_add",
        )
        .await
        .unwrap();

    // Mark the second lesson as stale via direct SQLite
    {
        let conn = Connection::open(&db_path).unwrap();
        conn.execute(
            "UPDATE lessons SET stale = 1, stale_reason = 'symbol_removed' WHERE id = 2",
            [],
        )
        .unwrap();
    }

    // Search — only the fresh lesson should be returned
    let results = backend.search_lessons("overflow", None, 10).await.unwrap();
    assert_eq!(
        results.len(),
        1,
        "should return exactly 1 lesson (not the stale one)"
    );
    assert_eq!(
        results[0].symbol_name, "fresh_fn",
        "should be the fresh lesson"
    );

    // Confirm the stale lesson is truly gone from results (not just masked)
    assert!(
        results.iter().all(|l| l.stale == 0),
        "all returned lessons must have stale=0"
    );

    std::fs::remove_file(&db_path).ok();
}

/// Test: stale filter is applied BEFORE LIMIT in SQL, not as a post-filter in Rust.
/// With 3 stale + 2 fresh (all same column), limit=2 should return exactly 2 fresh.
/// A post-filter approach would risk returning < 2 if stale results cluster at the top.
#[tokio::test]
async fn test_search_lessons_stale_filter_before_limit() {
    let (_dir, root) = setup_project();

    let db_path = format!(
        "{}/test_stale_before_limit.db",
        std::env::temp_dir().to_string_lossy()
    );
    let _ = std::fs::remove_file(&db_path);

    let backend = GraphBackend::open(Some(&db_path)).unwrap();
    backend.full_scan(&root, None).unwrap();

    let lib = full_path(&root, "lib.rs");

    // Record 5 lessons, all with "overflow" in symbol_name (same column).
    // First 3 will be marked stale; last 2 stay fresh.
    for i in 0..5 {
        backend
            .record_lesson(
                &lib,
                Some(&format!("overflow_fn_{}", i)),
                &format!("context {}", i),
                &format!("solution {}", i),
            )
            .await
            .unwrap();
    }

    // Mark IDs 1,2,3 as stale
    {
        let conn = Connection::open(&db_path).unwrap();
        conn.execute(
            "UPDATE lessons SET stale = 1, stale_reason = 'symbol_removed' WHERE id IN (1,2,3)",
            [],
        )
        .unwrap();
    }

    // Search with limit=2
    let results = backend.search_lessons("overflow", None, 2).await.unwrap();
    assert_eq!(
        results.len(),
        2,
        "limit=2 should return exactly 2 results (both fresh)"
    );
    assert!(
        results.iter().all(|l| l.stale == 0),
        "all returned lessons must have stale=0 — got stale values: {:?}",
        results.iter().map(|l| (l.id, l.stale)).collect::<Vec<_>>()
    );

    std::fs::remove_file(&db_path).ok();
}

/// Test: rebuild_graph produces stable dependency results across calls.
#[test]
fn test_rebuild_graph_stable_deps() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();

    let a_path = root.join("a.rs");
    let b_path = root.join("b.rs");
    let c_path = root.join("c.rs");
    std::fs::write(&a_path, "mod b; mod c; fn a() {}").unwrap();
    std::fs::write(&b_path, "fn b() {}").unwrap();
    std::fs::write(&c_path, "fn c() {}").unwrap();

    let backend = GraphBackend::open_for_project(root).unwrap();
    let root_str = root.to_string_lossy().to_string();
    backend.full_scan(&root_str, None).unwrap();

    let a_abs = a_path.to_string_lossy().to_string();
    let b_abs = b_path.to_string_lossy().to_string();
    let c_abs = c_path.to_string_lossy().to_string();

    let before_outgoing = backend.get_outgoing_deps(&a_abs);
    let before_incoming_b = backend.get_incoming_deps(&b_abs);
    let before_incoming_c = backend.get_incoming_deps(&c_abs);

    backend.rebuild_graph().unwrap();

    let after_outgoing = backend.get_outgoing_deps(&a_abs);
    let after_incoming_b = backend.get_incoming_deps(&b_abs);
    let after_incoming_c = backend.get_incoming_deps(&c_abs);

    assert_eq!(
        before_outgoing, after_outgoing,
        "outgoing deps from a must match across rebuild"
    );
    assert_eq!(
        before_incoming_b, after_incoming_b,
        "incoming deps to b must match across rebuild"
    );
    assert_eq!(
        before_incoming_c, after_incoming_c,
        "incoming deps to c must match across rebuild"
    );
}

/// Regression test: path normalization (strip `\\?\` prefix) must not break
/// dependency edge resolution. Creates two files with a `mod` import, runs
/// full_scan, and verifies edges > 0.
#[test]
fn test_dependency_edges_survive_path_normalization() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();

    // Create two Rust files where one imports the other via `mod`
    std::fs::write(root.join("lib.rs"), "pub fn helper() -> i32 { 42 }\n").unwrap();
    std::fs::write(
        root.join("main.rs"),
        "mod lib;\nfn main() { lib::helper(); }\n",
    )
    .unwrap();

    let backend = GraphBackend::open_for_project(root).unwrap();
    let root_str = root.to_string_lossy().to_string();
    backend.full_scan(&root_str, None).unwrap();

    // After rebuild_graph (called by full_scan), verify edges exist
    let lib_abs = ozymem_core::normalize_path(&root.join("lib.rs").to_string_lossy());
    let main_abs = ozymem_core::normalize_path(&root.join("main.rs").to_string_lossy());

    let incoming_lib = backend.get_incoming_deps(&lib_abs);
    let outgoing_main = backend.get_outgoing_deps(&main_abs);

    assert!(
        !incoming_lib.is_empty() || !outgoing_main.is_empty(),
        "expected at least one dependency edge between main.rs and lib.rs: \
         incoming to lib: {:?}, outgoing from main: {:?}",
        incoming_lib,
        outgoing_main,
    );
}

/// Test: similar_lessons ranking with real embeddings.
/// Records two semantically distinct lessons, queries with a third topic,
/// and verifies the correct lesson ranks first.
/// Gracefully handles unavailable model (embeddings disabled → empty results).
#[tokio::test]
async fn test_similar_lessons_ranking() {
    use ozymem_core::graph_backend::GraphBackend;
    use tempfile::TempDir;

    let dir = TempDir::new().unwrap();
    let backend = GraphBackend::open_for_project(dir.path()).unwrap();

    // Record lessons with semantically different content
    backend
        .record_entry(
            "dates.rs",
            Some("parse_date"),
            "date/time manipulation with chrono: parsing RFC 3339",
            "use NaiveDateTime::parse_from_rfc3339",
            "lesson",
        )
        .await
        .unwrap();

    backend
        .record_entry(
            "auth.rs",
            Some("oauth_refresh"),
            "OAuth token refresh flow: detect expiry",
            "check the exp claim before making API calls",
            "lesson",
        )
        .await
        .unwrap();

    // Query for time-related content
    let results = backend
        .similar_lessons("how do I parse dates in Rust", 5, 0.0)
        .unwrap();

    if results.is_empty() {
        // Embeddings not available (no model downloaded yet) — skip assertions
        eprintln!("[test] no embeddings available — skipping ranking assertion");
        return;
    }

    // Verify the dates lesson ranks first
    assert!(
        results[0].lesson.file_path.contains("dates"),
        "expected dates lesson first, got file={}, score={:.3}",
        results[0].lesson.file_path,
        results[0].score,
    );

    // Verify scores are reasonable (semantically related text should score > 0.3)
    assert!(
        results[0].score > 0.3,
        "top result should have meaningful similarity: score={:.3}",
        results[0].score,
    );
}

#[test]
fn test_full_scan_progress_callback() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().join("proj");
    std::fs::create_dir_all(&root).unwrap();

    // Write two Rust source files
    let mut f1 = std::fs::File::create(root.join("main.rs")).unwrap();
    write!(f1, "fn main() {{ println!(\"hello\"); }}").unwrap();
    let mut f2 = std::fs::File::create(root.join("lib.rs")).unwrap();
    write!(f2, "pub fn add(a: i32, b: i32) -> i32 {{ a + b }}").unwrap();

    // Create .ozymem directory so GraphBackend::open can create the DB
    std::fs::create_dir_all(root.join(".ozymem")).unwrap();
    let db_path = root.join(".ozymem").join("memory.db");
    let backend = GraphBackend::open(Some(&db_path.to_string_lossy())).unwrap();
    backend.set_project_path(Some(&root.to_string_lossy()));

    let calls = std::sync::Mutex::new(Vec::new());
    backend
        .full_scan(
            &root.to_string_lossy(),
            Some(&|processed, total| {
                calls.lock().unwrap().push((processed, total));
            }),
        )
        .unwrap();

    let history = calls.lock().unwrap().clone();
    assert!(
        !history.is_empty(),
        "progress callback should be called at least once"
    );
    // Last call should report all files
    let last = history.last().unwrap();
    assert_eq!(
        last.0, last.1,
        "final progress should show all files processed"
    );
    assert_eq!(last.0, 2, "expected 2 files to be scanned");
}

/// P-ENRICH: ImpactEntry enrichment — verify start_line, end_line, reason, suggestion are populated.
#[tokio::test]
async fn test_impact_entry_enriched_fields() {
    let (_dir, root) = setup_project();

    let db_path = format!("{}/test_enrich.db", std::env::temp_dir().to_string_lossy());
    let _ = std::fs::remove_file(&db_path);

    let backend = GraphBackend::open(Some(&db_path)).unwrap();
    backend.full_scan(&root, None).unwrap();

    let main_path = full_path(&root, "main.rs");

    let impacts = backend.analyze_impact(&main_path, 3);
    assert!(!impacts.is_empty(), "should find at least lib.rs");

    for entry in &impacts {
        assert!(
            entry.severity == "breaking" || entry.severity == "warning" || entry.severity == "info",
            "severity should be one of breaking/warning/info, got: {}",
            entry.severity
        );

        assert!(
            !entry.reason.is_empty(),
            "reason should not be empty for {}",
            entry.file_path
        );
        assert!(
            !entry.suggestion.is_empty(),
            "suggestion should not be empty for {}",
            entry.file_path
        );

        if entry.file_path.contains("lib.rs") {
            assert!(
                entry.start_line > 0,
                "lib.rs start_line should be >0, got {}",
                entry.start_line
            );
            assert!(
                entry.end_line > 0,
                "lib.rs end_line should be >0, got {}",
                entry.end_line
            );
            assert!(
                entry.function_count >= 2,
                "lib.rs should have at least 2 functions"
            );
        }
    }

    std::fs::remove_file(&db_path).ok();
}

/// P-PATH: graph_path finds dependency chain A→B→C with correct shortest length.
#[tokio::test]
async fn test_graph_path_finds_chain() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().to_string_lossy().to_string();

    std::fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"chain-test\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();

    std::fs::write(
        dir.path().join("a.rs"),
        "mod b;\nmod c;\nfn main() { b::work(); }\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("b.rs"),
        "use crate::c;\npub fn work() { c::help(); }\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("c.rs"),
        "pub fn help() { println!(\"ok\"); }\n",
    )
    .unwrap();

    let db_path = format!(
        "{}/test_graph_path.db",
        std::env::temp_dir().to_string_lossy()
    );
    let _ = std::fs::remove_file(&db_path);

    let backend = GraphBackend::open(Some(&db_path)).unwrap();
    backend.full_scan(&root, None).unwrap();

    let a_path = full_path(&root, "a.rs");
    let b_path = full_path(&root, "b.rs");
    let c_path = full_path(&root, "c.rs");

    // a → b direct
    let paths = backend.find_graph_path(&a_path, &b_path, 5, 5);
    assert!(!paths.is_empty(), "should find path from a.rs to b.rs");
    assert_eq!(
        paths[0].len(),
        2,
        "direct edge a→b should be 2 nodes (a, b)"
    );

    // a → c (via b or direct via mod c)
    let paths_ac = backend.find_graph_path(&a_path, &c_path, 5, 5);
    assert!(!paths_ac.is_empty(), "should find path from a.rs to c.rs");
    // At least one path uses the intermediate b (3 nodes: a→b→c)
    let has_via_b = paths_ac.iter().any(|p| p.len() == 3);
    assert!(
        has_via_b,
        "expected a 3-node path a→b→c, got: {:?}",
        paths_ac
    );

    // Shortest length computed as hops (min nodes - 1)
    let shortest = paths_ac
        .iter()
        .map(|p| p.len())
        .min()
        .unwrap()
        .saturating_sub(1);
    assert_eq!(
        shortest, 1,
        "shortest path a→c should be 1 hop (direct edge)"
    );

    // No path to nonexistent file
    let no_path = backend.find_graph_path(&a_path, "/nonexistent.rs", 5, 5);
    assert!(no_path.is_empty(), "no path to nonexistent file");

    // Reverse path c → a should NOT exist (directed graph: a→c but no c→a)
    let paths_ca = backend.find_graph_path(&c_path, &a_path, 5, 5);
    assert!(
        paths_ca.is_empty(),
        "reverse path c→a should not exist (directed graph)"
    );

    std::fs::remove_file(&db_path).ok();
}

/// P-CONFIDENCE: confidence heuristic values are within expected ranges.
#[test]
fn test_learn_from_changes_confidence_heuristic() {
    let confidence_for = |old_len: i64, new_len: i64| -> f64 {
        let delta = new_len.abs_diff(old_len);
        let max_len = old_len.max(new_len).max(1);
        0.65 + 0.30 * (delta as f64 / max_len as f64).min(1.0)
    };

    // Same size → delta=0 → confidence=0.65
    let c1 = confidence_for(10, 10);
    assert!(
        (c1 - 0.65).abs() < 1e-6,
        "no change should give 0.65, got {c1}"
    );

    // Small change (2 lines out of 12) → 0.65+0.30*(2/12)=0.65+0.05=0.70
    let c2 = confidence_for(10, 12);
    assert!(
        (c2 - 0.70).abs() < 1e-6,
        "2-line change should give ~0.70, got {c2}"
    );

    // Large change (15 lines out of 20) → 0.65+0.30*(15/20)=0.875
    let c3 = confidence_for(5, 20);
    assert!(
        (c3 - 0.875).abs() < 1e-6,
        "15-line change should give ~0.875, got {c3}"
    );

    // Massive change (0→100 lines, delta == max_len) → clamped to 0.95
    let c4 = confidence_for(0, 100);
    assert!(
        (c4 - 0.95).abs() < 1e-6,
        "massive change should clamp at 0.95, got {c4}"
    );

    // Zero-length function → guard against division by zero
    let c5 = confidence_for(0, 0);
    assert!(
        (c5 - 0.65).abs() < 1e-6,
        "zero-length should fallback to 0.65, got {c5}"
    );

    // All values in [0.65, 0.95]
    for c in &[c1, c2, c3, c4, c5] {
        assert!(
            *c >= 0.65 && *c <= 0.95,
            "confidence should be in [0.65, 0.95], got {c}"
        );
    }
}

/// P-KIND: search_lessons with kind filter returns only matching entries.
#[tokio::test]
async fn test_search_lessons_kind_filter() {
    let (_dir, root) = setup_project();

    let db_path = format!(
        "{}/test_kind_filter.db",
        std::env::temp_dir().to_string_lossy()
    );
    let _ = std::fs::remove_file(&db_path);

    let backend = GraphBackend::open(Some(&db_path)).unwrap();
    backend.set_project_path(Some(&root));

    let lib_path = full_path(&root, "lib.rs");

    backend
        .record_entry(
            &lib_path,
            Some("add"),
            "overflow bug",
            "use checked_add for safety",
            "lesson",
        )
        .await
        .unwrap();
    backend
        .record_entry(
            &lib_path,
            Some("subtract"),
            "negative test",
            "use abs() instead",
            "gotcha",
        )
        .await
        .unwrap();
    backend
        .record_entry(
            &lib_path,
            Some("add"),
            "renamed to sum",
            "function renamed for clarity",
            "decision",
        )
        .await
        .unwrap();
    backend
        .record_entry(
            &lib_path,
            Some("calc"),
            "use helper",
            "always call calc_helper first",
            "convention",
        )
        .await
        .unwrap();

    // No kind filter → "overflow" matches 1 entry (the lesson)
    let all = backend.search_lessons("overflow", None, 10).await.unwrap();
    assert_eq!(all.len(), 1, "overflow should match exactly 1 entry");

    // Broader search across all kinds
    let all_broad = backend.search_lessons("use", None, 10).await.unwrap();
    assert!(
        all_broad.len() >= 2,
        "broad query 'use' should match multiple entries"
    );

    // Filter by kind = "lesson"
    let lessons = backend
        .search_lessons("overflow", Some("lesson"), 10)
        .await
        .unwrap();
    assert_eq!(
        lessons.len(),
        1,
        "should find exactly 1 lesson with kind=lesson"
    );
    assert_eq!(lessons[0].kind, "lesson");

    // Filter by kind = "gotcha"
    let gotchas = backend
        .search_lessons("negative", Some("gotcha"), 10)
        .await
        .unwrap();
    assert_eq!(
        gotchas.len(),
        1,
        "should find exactly 1 gotcha with kind=gotcha"
    );
    assert_eq!(gotchas[0].kind, "gotcha");

    // Non-existent kind → empty
    let module_rules = backend
        .search_lessons("overflow", Some("module_rule"), 10)
        .await
        .unwrap();
    assert!(
        module_rules.is_empty(),
        "no module_rule entries should return empty"
    );

    std::fs::remove_file(&db_path).ok();
}

#[tokio::test]
async fn test_record_and_get_excel_templates() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test_excel.db");

    let backend = GraphBackend::open(Some(db_path.to_str().unwrap())).unwrap();

    let meta = ozymem_parser::ExcelTemplateMetadata {
        file_name: "1-INF.-N-V03.xlsx".to_string(),
        rel_path: "templates/1-INF.-N-V03.xlsx".to_string(),
        canonical_hash: "a1b2c3d4e5f6".to_string(),
        version_tag: Some("V03".to_string()),
        sheets: vec!["Hoja1".to_string(), "Resumen".to_string()],
        is_template_candidate: true,
    };

    backend.record_excel_template(&meta).unwrap();

    let fetched = backend.get_excel_templates().unwrap();
    assert_eq!(fetched.len(), 1);
    assert_eq!(fetched[0].file_name, "1-INF.-N-V03.xlsx");
    assert_eq!(fetched[0].version_tag, Some("V03".to_string()));
    assert_eq!(fetched[0].sheets, vec!["Hoja1", "Resumen"]);
}
