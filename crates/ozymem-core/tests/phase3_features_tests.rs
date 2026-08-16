use ozymem_core::bundle::{export_bundle, import_bundle};
use ozymem_core::graph_backend::GraphBackend;
use ozymem_core::registry::ProjectRegistry;
use ozymem_core::McpBackend;
use std::fs;
use tempfile::tempdir;

#[tokio::test]
async fn test_knowledge_bundle_export_and_import_parity() {
    let dir_a = tempdir().unwrap();
    let root_a = dir_a.path();
    let file_a = root_a.join("api.py");
    fs::write(
        &file_a,
        r#"
from fastapi import APIRouter
router = APIRouter()

@router.get("/api/v1/health")
async def health():
    return {"status": "ok"}
"#,
    )
    .unwrap();

    let backend_a = GraphBackend::open_for_project(root_a).unwrap();
    backend_a.index_file_delta(&file_a, root_a).unwrap();

    backend_a
        .record_lesson(
            &file_a.to_string_lossy(),
            Some("health"),
            "health check timeouts",
            "add connection pooling to redis",
        )
        .await
        .unwrap();

    backend_a
        .record_entry(
            &file_a.to_string_lossy(),
            None,
            "architecture standard",
            "never return raw exceptions, always wrap in ErrorDTO",
            "convention",
        )
        .await
        .unwrap();

    // Export bundle
    let bundle_file = root_a.join("proj_a.ozymem");
    let export_summary = export_bundle(&backend_a, "proj_a", &bundle_file).unwrap();
    assert_eq!(export_summary.lessons_exported, 2);
    assert_eq!(export_summary.routes_exported, 1);
    assert!(!export_summary.sha256.is_empty());
    assert!(bundle_file.exists());

    // Import into fresh project B
    let dir_b = tempdir().unwrap();
    let root_b = dir_b.path();
    let backend_b = GraphBackend::open_for_project(root_b).unwrap();

    let import_summary = import_bundle(&backend_b, &bundle_file, true).await.unwrap();
    assert_eq!(import_summary.lessons_imported, 2);
    assert_eq!(import_summary.lessons_skipped_duplicate, 0);

    // Verify lessons exist in B
    let lessons_b = backend_b.recent_lessons(None, 10).await.unwrap();
    assert_eq!(lessons_b.len(), 2);
    assert!(lessons_b.iter().any(|l| l.error_context == "health check timeouts"));
    assert!(lessons_b.iter().any(|l| l.solution.contains("wrap in ErrorDTO")));

    // Re-importing with merge=true should skip duplicates
    let reimport_summary = import_bundle(&backend_b, &bundle_file, true).await.unwrap();
    assert_eq!(reimport_summary.lessons_imported, 0);
    assert_eq!(reimport_summary.lessons_skipped_duplicate, 2);
}

#[tokio::test]
async fn test_project_registry_linking_and_unlinking() {
    let reg = ProjectRegistry::open().unwrap();

    let dir_api = tempdir().unwrap();
    let dir_web = tempdir().unwrap();

    let name_api = format!("test_api_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());
    let name_web = format!("test_web_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());

    reg.register(&name_api, &dir_api.path().to_string_lossy()).unwrap();
    reg.register(&name_web, &dir_web.path().to_string_lossy()).unwrap();

    // Link api -> web
    reg.link_projects(&name_api, &name_web, "api_provider").unwrap();

    let linked = reg.get_linked_projects(&name_api).unwrap();
    assert_eq!(linked.len(), 1);
    assert_eq!(linked[0].0.name, name_web);
    assert_eq!(linked[0].1, "api_provider");

    // Unlink
    let unlinked = reg.unlink_projects(&name_api, &name_web).unwrap();
    assert!(unlinked);

    let linked_after = reg.get_linked_projects(&name_api).unwrap();
    assert_eq!(linked_after.len(), 0);

    // Clean up
    let _ = reg.deregister(&name_api);
    let _ = reg.deregister(&name_web);
}

#[tokio::test]
async fn test_cross_repo_memory_search() {
    let reg = ProjectRegistry::open().unwrap();

    let dir_be = tempdir().unwrap();
    let dir_fe = tempdir().unwrap();

    let be_path = dir_be.path();
    let fe_path = dir_fe.path();

    let be_file = be_path.join("auth.rs");
    let fe_file = fe_path.join("login.ts");
    fs::write(&be_file, "pub fn auth() {}").unwrap();
    fs::write(&fe_file, "function login() {}").unwrap();

    let be_backend = GraphBackend::open_for_project(be_path).unwrap();
    let fe_backend = GraphBackend::open_for_project(fe_path).unwrap();

    be_backend.index_file_delta(&be_file, be_path).unwrap();
    fe_backend.index_file_delta(&fe_file, fe_path).unwrap();

    be_backend
        .record_lesson(
            &be_file.to_string_lossy(),
            Some("auth"),
            "JWT bearer token validation",
            "verify issuer and signature with RS256",
        )
        .await
        .unwrap();

    fe_backend
        .record_lesson(
            &fe_file.to_string_lossy(),
            Some("login"),
            "JWT token persistence error",
            "store token in encrypted session storage",
        )
        .await
        .unwrap();

    let name_be = format!("be_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());
    let name_fe = format!("fe_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());

    reg.register(&name_be, &be_path.to_string_lossy()).unwrap();
    reg.register(&name_fe, &fe_path.to_string_lossy()).unwrap();

    // Cross-repo search for JWT
    let results = reg
        .search_cross_repo_memories("JWT", Some(&[&name_be, &name_fe]), 10)
        .unwrap();

    assert_eq!(results.len(), 2);
    assert!(results.iter().any(|r| r.project_name == name_be && r.lesson.solution.contains("RS256")));
    assert!(results.iter().any(|r| r.project_name == name_fe && r.lesson.solution.contains("session storage")));

    // Clean up
    let _ = reg.deregister(&name_be);
    let _ = reg.deregister(&name_fe);
}
