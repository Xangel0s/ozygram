use ozymem_core::graph_backend::GraphBackend;
use ozymem_core::McpBackend;
use std::fs;
use tempfile::tempdir;

#[tokio::test]
async fn test_path_resolution_cascade_and_relative() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    let sub_dir = root.join("src").join("controllers");
    fs::create_dir_all(&sub_dir).unwrap();
    let file1 = sub_dir.join("user_controller.rs");
    fs::write(
        &file1,
        r#"
pub fn get_user_profile(id: u64) -> String {
    format!("User {}", id)
}
"#,
    )
    .unwrap();

    let backend = GraphBackend::open_for_project(root).unwrap();
    backend.full_scan(&root.to_string_lossy(), None).unwrap();

    // 1. Exact match
    let resolved_exact = backend.resolve_target_path(&file1.to_string_lossy());
    assert!(resolved_exact.is_some());

    // 2. Relative match
    let resolved_rel = backend.resolve_target_path("src/controllers/user_controller.rs");
    assert!(resolved_rel.is_some());

    // 3. Suffix match
    let resolved_suffix = backend.resolve_target_path("user_controller.rs");
    assert!(resolved_suffix.is_some());

    // 4. get_file_context with relative path
    let ctx = backend.get_file_context("src/controllers/user_controller.rs").await.unwrap();
    assert!(ctx.is_some());
    let unwrapped = ctx.unwrap();
    assert!(unwrapped.language.eq_ignore_ascii_case("rust"));
    assert_eq!(unwrapped.functions.len(), 1);
    assert_eq!(unwrapped.functions[0].name, "get_user_profile");

    // 5. analyze_impact with suffix path
    let impact = backend.analyze_impact("user_controller.rs", 2);
    // user_controller is the only file, so 0 dependent neighbors, but doesn't panic and resolves cleanly
    assert_eq!(impact.len(), 0);
}

#[tokio::test]
async fn test_ast_diagnostics_detection_in_scan() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    let broken_py = root.join("broken.py");
    fs::write(
        &broken_py,
        r#"
def incomplete_function(
    print("missing closing paren")
"#,
    )
    .unwrap();

    let valid_py = root.join("valid.py");
    fs::write(
        &valid_py,
        r#"
def working_function():
    return 42
"#,
    )
    .unwrap();

    let backend = GraphBackend::open_for_project(root).unwrap();
    backend.full_scan(&root.to_string_lossy(), None).unwrap();

    let count = backend.count_ast_diagnostics().unwrap();
    assert!(count > 0, "Should have detected syntax diagnostics in broken.py");

    let diags = backend.get_ast_diagnostics(10).unwrap();
    assert!(!diags.is_empty());
    assert!(diags.iter().any(|d| d.file_path.contains("broken.py")));
}

#[tokio::test]
async fn test_search_ast_symbols_fallback() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    let auth_py = root.join("auth.py");
    fs::write(
        &auth_py,
        r#"
class AuthService:
    def authenticate_jwt_token(self, token: str):
        return True

def hash_password(plain: str):
    return "hashed"
"#,
    )
    .unwrap();

    let backend = GraphBackend::open_for_project(root).unwrap();
    backend.full_scan(&root.to_string_lossy(), None).unwrap();

    let symbols = backend.search_ast_symbols("authenticate_jwt", 5).unwrap();
    assert_eq!(symbols.len(), 1);
    assert_eq!(symbols[0].name, "authenticate_jwt_token");

    let auth_symbols = backend.search_ast_symbols("Auth", 5).unwrap();
    assert!(auth_symbols.len() >= 1);
}

#[tokio::test]
async fn test_subpath_graph_summary_monorepo() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    let backend_dir = root.join("crm-backend").join("app");
    fs::create_dir_all(&backend_dir).unwrap();
    let be_file = backend_dir.join("main.py");
    fs::write(
        &be_file,
        r#"
def start_crm_server():
    pass
"#,
    )
    .unwrap();

    let frontend_dir = root.join("crm-frontend").join("src");
    fs::create_dir_all(&frontend_dir).unwrap();
    let fe_file = frontend_dir.join("index.ts");
    fs::write(
        &fe_file,
        r#"
function renderDashboard() {
    return null;
}
"#,
    )
    .unwrap();

    let backend = GraphBackend::open_for_project(root).unwrap();
    backend.full_scan(&root.to_string_lossy(), None).unwrap();

    let overall_summary = backend.get_graph_summary().await.unwrap();
    assert_eq!(overall_summary.file_count, 2);

    let backend_sub = backend.get_subpath_graph_summary("crm-backend").unwrap();
    assert_eq!(backend_sub.file_count, 1);
    assert_eq!(backend_sub.function_count, 1);

    let frontend_sub = backend.get_subpath_graph_summary("crm-frontend").unwrap();
    assert_eq!(frontend_sub.file_count, 1);
    assert_eq!(frontend_sub.function_count, 1);
}
