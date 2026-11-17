use ozymem_core::graph_backend::GraphBackend;
use ozymem_core::mcp_common;
use ozymem_server::handle_request;
use std::sync::Arc;
use tokio::sync::RwLock;

#[tokio::test]
async fn test_monolith_cli_has_embedded_mcp_server() {
    let backend: Arc<RwLock<Option<GraphBackend>>> = Arc::new(RwLock::new(None));
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

    // Must have the unified canonical 6-tool suite embedded in the single binary by default
    assert!(active_tools.iter().any(|t| t["name"] == "ozy_refactor"));
    assert!(active_tools.iter().any(|t| t["name"] == "ozy_explore"));
    assert!(active_tools.iter().any(|t| t["name"] == "ozy_simulate"));
    assert!(active_tools.iter().any(|t| t["name"] == "ozy_apply"));
    assert!(active_tools.iter().any(|t| t["name"] == "ozy_memory"));
    assert!(active_tools.iter().any(|t| t["name"] == "ozy_read_blob"));
    assert_eq!(active_tools.len(), 6);

    // When all=true is requested, returns the full catalog of 31 tools (4 new canonical + 27 legacy)
    let req_all = mcp_common::JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(serde_json::json!(2)),
        method: "tools/list".to_string(),
        params: Some(serde_json::json!({ "all": true })),
    };
    let resp_all = handle_request(&backend, req_all, None, None).await.unwrap().unwrap();
    let tools_all = resp_all.result.unwrap()["tools"].as_array().unwrap().clone();
    assert_eq!(tools_all.len(), 31);
}

