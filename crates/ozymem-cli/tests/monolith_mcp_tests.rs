use ozymem_core::graph_backend::GraphBackend;
use ozymem_core::mcp_common;
use ozymem_server::handle_request;
use std::sync::{Arc, Mutex};

#[tokio::test]
async fn test_monolith_cli_has_embedded_mcp_server() {
    let backend: Arc<Mutex<Option<GraphBackend>>> = Arc::new(Mutex::new(None));
    let request = mcp_common::JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(serde_json::json!(1)),
        method: "tools/list".to_string(),
        params: None,
    };

    let response = handle_request(&backend, request, None, None).await.unwrap();
    let resp = response.expect("tools/list should return response");
    let result = resp.result.expect("result must be present");
    let active_tools = result["tools"].as_array().expect("tools array");

    // Must have the unified canonical tools embedded in the single binary
    assert!(active_tools.iter().any(|t| t["name"] == "ozy_patch"));
    assert!(active_tools.iter().any(|t| t["name"] == "ozy_plan"));
    assert!(active_tools.iter().any(|t| t["name"] == "ozy_context"));
    assert!(active_tools.iter().any(|t| t["name"] == "ozy_memory"));
    assert!(active_tools.iter().any(|t| t["name"] == "ozy_doctor"));
    assert_eq!(active_tools.len(), 25);
}

