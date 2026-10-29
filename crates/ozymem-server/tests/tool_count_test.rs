use ozymem_server::handle_request;
use ozymem_core::graph_backend::GraphBackend;
use ozymem_core::mcp_common;
use std::sync::{Arc, Mutex};

#[tokio::test]
async fn test_mcp_tool_count_matches_docs_and_schemas() {
    // 1. By default, only active unified tools starting with 'ozy_' are visible (25 tools)
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

    assert_eq!(
        active_tools.len(),
        25,
        "Default active tools must be exactly 25 as documented in README.md"
    );

    for tool in active_tools {
        let name = tool["name"].as_str().expect("tool name");
        assert!(
            name.starts_with("ozy_"),
            "Active unified tool '{}' should start with 'ozy_'",
            name
        );
    }

    // 2. Read README.md to ensure documentation integrity (T1.4 requirement)
    let readme_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("README.md");

    if readme_path.exists() {
        let readme_content = std::fs::read_to_string(&readme_path)
            .expect("should read workspace README.md");
        assert!(
            readme_content.contains("25 unified native MCP tools")
                || readme_content.contains("25 active tools"),
            "README.md must explicitly mention 23 active/unified tools"
        );
        assert!(
            readme_content.contains("57 optional legacy aliases")
                || readme_content.contains("57 legacy aliases"),
            "README.md must explicitly mention 57 legacy aliases"
        );
    }
}
