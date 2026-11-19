use ozymem_server::handle_request;
use ozymem_core::graph_backend::GraphBackend;
use ozymem_core::mcp_common;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::RwLock;

fn make_temp_dir(suffix: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("ozymem_test_sdd_{}_{}", std::process::id(), suffix));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

#[tokio::test]
async fn test_sdd_capability_map_action() {
    let dir = make_temp_dir("cap_map");
    let backend_inst = GraphBackend::open_for_project(&dir).unwrap();
    let backend: Arc<RwLock<Option<GraphBackend>>> = Arc::new(RwLock::new(Some(backend_inst)));

    let req = mcp_common::JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(json!(1)),
        method: "tools/call".to_string(),
        params: Some(json!({
            "name": "ozy_explore",
            "arguments": {
                "action": "capability_map",
                "initiative": "E-Commerce Core"
            }
        })),
    };

    let resp = handle_request(&backend, req, None, None).await.unwrap().unwrap();
    let res = resp.result.unwrap();
    let content = res["content"].as_array().unwrap();
    let text = content[0]["text"].as_str().unwrap();

    assert!(text.contains("# Capability Map: E-Commerce Core"));
    assert!(text.contains("| Module id | Responsibility | Depends on |"));
    assert!(text.contains("**Build order:**"));

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn test_sdd_spec_action() {
    let dir = make_temp_dir("spec");
    let backend_inst = GraphBackend::open_for_project(&dir).unwrap();
    let backend: Arc<RwLock<Option<GraphBackend>>> = Arc::new(RwLock::new(Some(backend_inst)));

    let req = mcp_common::JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(json!(2)),
        method: "tools/call".to_string(),
        params: Some(json!({
            "name": "ozy_explore",
            "arguments": {
                "action": "spec",
                "module": "billing",
                "objective": "Integrate Stripe payment intent workflow"
            }
        })),
    };

    let resp = handle_request(&backend, req, None, None).await.unwrap().unwrap();
    let res = resp.result.unwrap();
    let content = res["content"].as_array().unwrap();
    let text = content[0]["text"].as_str().unwrap();

    assert!(text.contains("# Spec: billing"));
    assert!(text.contains("Integrate Stripe payment intent workflow"));
    assert!(text.contains("## Tech Stack"));
    assert!(text.contains("## Commands"));
    assert!(text.contains("## Project Structure"));
    assert!(text.contains("## Code Style & AST Contracts"));
    assert!(text.contains("## Testing Strategy"));
    assert!(text.contains("## Boundaries"));
    assert!(text.contains("## Success Criteria"));

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn test_sdd_boundary_audit_in_refactor_simulation() {
    let dir = make_temp_dir("refactor");
    let file_path = dir.join("main.py");
    std::fs::write(&file_path, "def greet():\n    return 'hello'\n").unwrap();

    let backend_inst = GraphBackend::open_for_project(&dir).unwrap();
    let backend: Arc<RwLock<Option<GraphBackend>>> = Arc::new(RwLock::new(Some(backend_inst)));

    let req = mcp_common::JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(json!(3)),
        method: "tools/call".to_string(),
        params: Some(json!({
            "name": "ozy_refactor",
            "arguments": {
                "file_path": file_path.to_str().unwrap(),
                "action": "simulate",
                "target_code": "return 'hello'",
                "replacement": "return 'world'"
            }
        })),
    };

    let resp = handle_request(&backend, req, None, None).await.unwrap().unwrap();
    let res = resp.result.unwrap();
    let content = res["content"].as_array().unwrap();
    let text = content[0]["text"].as_str().unwrap();

    assert!(text.contains("--- 2. SPEC-DRIVEN DEVELOPMENT (SDD) BOUNDARY AUDIT ---"));
    assert!(text.contains("Status: [VERIFIED]") || text.contains("Status: [SPEC_BOUNDARY_WARN]"));
    assert!(text.contains("--- 3. AST PATCH SIMULATION (IN-RAM) ---"));
    assert!(text.contains("Status: [SUCCESS]"));

    let _ = std::fs::remove_dir_all(&dir);
}
