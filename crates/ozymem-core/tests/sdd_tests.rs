use ozymem_core::graph_backend::{
    extract_module_id, infer_module_responsibility, GraphBackend,
};
use tempfile::TempDir;

#[test]
fn test_extract_module_id_various_layouts() {
    let ws = "C:/my_project";

    // 1. Cargo workspaces / monorepo
    assert_eq!(extract_module_id("C:/my_project/crates/ozymem-core/src/lib.rs", ws), "ozymem-core");
    assert_eq!(extract_module_id("crates/ozymem-server/src/main.rs", ws), "ozymem-server");
    assert_eq!(extract_module_id("packages/auth-provider/index.ts", ws), "auth-provider");

    // 2. Python app layouts
    assert_eq!(extract_module_id("app/routers/invoices.py", ws), "routers");
    assert_eq!(extract_module_id("app/models/user.py", ws), "models");
    assert_eq!(extract_module_id("api/routers/auth.py", ws), "routers");

    // 3. Frontend React / TypeScript layouts
    assert_eq!(extract_module_id("src/components/Header.tsx", ws), "components");
    assert_eq!(extract_module_id("src/hooks/useAuth.ts", ws), "hooks");
    assert_eq!(extract_module_id("src/pages/Dashboard.tsx", ws), "pages");

    // 4. Flat / Root files
    assert_eq!(extract_module_id("Cargo.toml", ws), "root");
    assert_eq!(extract_module_id("main.py", ws), "root");
    assert_eq!(extract_module_id("README.md", ws), "root");

    // 5. Migrations and services
    assert_eq!(extract_module_id("migrations/001_init.sql", ws), "migrations");
    assert_eq!(extract_module_id("services/billing/worker.py", ws), "billing");
}

#[test]
fn test_infer_module_responsibility() {
    assert!(infer_module_responsibility("auth").to_lowercase().contains("authentication"));
    assert!(infer_module_responsibility("models").to_lowercase().contains("persistence") || infer_module_responsibility("models").to_lowercase().contains("schema"));
    assert!(infer_module_responsibility("routers").to_lowercase().contains("route") || infer_module_responsibility("routers").to_lowercase().contains("endpoint"));
    assert!(infer_module_responsibility("migrations").to_lowercase().contains("migration"));
    assert!(infer_module_responsibility("components").to_lowercase().contains("component"));
    assert!(infer_module_responsibility("custom_engine").contains("custom_engine"));
}

#[test]
fn test_generate_capability_map_empty_project() {
    let dir = TempDir::new().unwrap();
    let backend = GraphBackend::open_for_project(dir.path()).unwrap();

    let cap_map = backend.generate_capability_map(Some("SaaS Initiative"));
    assert_eq!(cap_map.initiative, "SaaS Initiative");
    assert!(cap_map.markdown_table.contains("# Capability Map: SaaS Initiative"));
    assert!(cap_map.markdown_table.contains("| Module id | Responsibility | Depends on | Files | Functions |"));
    assert!(cap_map.markdown_table.contains("**Build order:**"));
}
