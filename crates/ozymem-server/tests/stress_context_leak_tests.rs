use ozymem_server::handle_request;
use ozymem_core::graph_backend::GraphBackend;
use ozymem_core::mcp_common;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::RwLock;

fn make_temp_dir(suffix: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("ozymem_test_stress_{}_{}", std::process::id(), suffix));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(p.join("src")).unwrap();

    // Small file
    std::fs::write(
        p.join("src").join("small.rs"),
        "pub fn add(a: i32, b: i32) -> i32 { a + b }\n",
    ).unwrap();

    // Massive file > 35,000 chars
    let mut large_content = String::with_capacity(50_000);
    large_content.push_str("// Auto-generated large file for stress testing\n");
    for i in 1..=800 {
        large_content.push_str(&format!(
            "pub fn function_item_{i:04}() -> &'static str {{\n    let msg = \"Stress test payload payload payload {i}\";\n    msg\n}}\n"
        ));
    }
    std::fs::write(p.join("src").join("large.rs"), &large_content).unwrap();

    p
}

#[tokio::test]
async fn test_multi_agent_stress_and_context_leak_prevention() {
    let dir = make_temp_dir("stress_leak_mesh");
    let backend: Arc<RwLock<Option<GraphBackend>>> = Arc::new(RwLock::new(None));

    // 1. Initialize workspace
    let init_req = mcp_common::JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(json!("coordinator_init")),
        method: "initialize".to_string(),
        params: Some(json!({
            "rootPath": dir.to_string_lossy().to_string(),
        })),
    };

    let init_resp = handle_request(&backend, init_req, None, None).await.unwrap().unwrap();
    assert!(init_resp.result.is_some(), "Initialization failed");

    // 2. Launch 20 concurrent subagents with diverse query profiles
    let num_subagents = 20;
    let mut handles = Vec::new();

    for agent_id in 0..num_subagents {
        let b = backend.clone();
        let handle = tokio::spawn(async move {
            let start = std::time::Instant::now();
            let req = match agent_id % 4 {
                0 => {
                    // Profile A: Subagent requesting massive view (>30,000 chars) -> MUST BE COMPACTED
                    mcp_common::JsonRpcRequest {
                        jsonrpc: "2.0".to_string(),
                        id: Some(json!(format!("subagent_massive_view_{agent_id}"))),
                        method: "tools/call".to_string(),
                        params: Some(json!({
                            "name": "ozy_explore",
                            "arguments": {
                                "action": "view",
                                "file_path": "src/large.rs",
                                "start_line": 1,
                                "end_line": 800
                            }
                        })),
                    }
                }
                1 => {
                    // Profile B: Subagent requesting small view -> MUST NOT BE COMPACTED
                    mcp_common::JsonRpcRequest {
                        jsonrpc: "2.0".to_string(),
                        id: Some(json!(format!("subagent_small_view_{agent_id}"))),
                        method: "tools/call".to_string(),
                        params: Some(json!({
                            "name": "ozy_explore",
                            "arguments": {
                                "action": "view",
                                "file_path": "src/small.rs",
                                "start_line": 1,
                                "end_line": 5
                            }
                        })),
                    }
                }
                2 => {
                    // Profile C: Subagent requesting search across large file
                    mcp_common::JsonRpcRequest {
                        jsonrpc: "2.0".to_string(),
                        id: Some(json!(format!("subagent_search_{agent_id}"))),
                        method: "tools/call".to_string(),
                        params: Some(json!({
                            "name": "ozy_explore",
                            "arguments": {
                                "action": "search",
                                "query": "function_item"
                            }
                        })),
                    }
                }
                _ => {
                    // Profile D: Subagent requesting AST contracts of small file
                    mcp_common::JsonRpcRequest {
                        jsonrpc: "2.0".to_string(),
                        id: Some(json!(format!("subagent_contracts_{agent_id}"))),
                        method: "tools/call".to_string(),
                        params: Some(json!({
                            "name": "ozy_explore",
                            "arguments": {
                                "action": "contracts",
                                "file_path": "src/small.rs"
                            }
                        })),
                    }
                }
            };

            let resp = handle_request(&b, req, None, None).await;
            let elapsed = start.elapsed();
            (agent_id, resp, elapsed)
        });
        handles.push(handle);
    }

    // 3. Collect and verify all responses
    let mut compacted_count = 0;
    let mut small_uncompacted_count = 0;

    for handle in handles {
        let (agent_id, res, dur) = handle.await.unwrap();
        eprintln!("[test] Subagent {agent_id} finished in {dur:?}");
        assert!(res.is_ok(), "Subagent {agent_id} execution error: {:?}", res.err());
        let resp = res.unwrap().expect("Missing response");
        assert!(resp.error.is_none(), "Subagent {agent_id} returned error: {:?}", resp.error);
        let result_val = resp.result.expect("Missing result value");

        let content_arr = result_val.get("content").and_then(|c| c.as_array()).expect("Missing content array");
        assert!(!content_arr.is_empty(), "Empty content returned to subagent {agent_id}");

        let total_text_len: usize = content_arr
            .iter()
            .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
            .map(|t| t.len())
            .sum();

        // INVARIANT 1: Zero context leaks! No response ever delivered to the agent exceeds 14,000 chars.
        assert!(
            total_text_len <= 14_000,
            "CONTEXT LEAK DETECTED: Subagent {agent_id} received {total_text_len} chars (exceeded 14,000 threshold)"
        );

        match agent_id % 4 {
            0 => {
                // Massive view: MUST be compacted to a small preview alert
                let text = content_arr[0]["text"].as_str().unwrap();
                assert!(
                    text.contains("[ALERT: OUTPUT_COMPACTED_TO_PRESERVE_CONTEXT]"),
                    "Massive view was not compacted for subagent {agent_id}"
                );
                assert!(text.contains("URI: ozymem://blob/"), "Missing blob URI in compacted output");
                assert!(total_text_len < 4_000, "Compacted output should be concise preview, got {total_text_len}");
                compacted_count += 1;
            }
            1 => {
                // Small view: MUST NOT be compacted
                let text = content_arr[0]["text"].as_str().unwrap();
                assert!(
                    !text.contains("[ALERT: OUTPUT_COMPACTED_TO_PRESERVE_CONTEXT]"),
                    "Small view was unnecessarily compacted for subagent {agent_id}"
                );
                assert!(text.contains("pub fn add"), "Small view text content corrupted");
                small_uncompacted_count += 1;
            }
            _ => {}
        }
    }

    assert_eq!(compacted_count, 5, "Expected all 5 Profile A subagents to trigger compaction");
    assert_eq!(small_uncompacted_count, 5, "Expected all 5 Profile B subagents to remain uncompacted");

    // 4. Verify blob persistence integrity: check .ozymem/blobs/
    let blobs_dir = dir.join(".ozymem").join("blobs");
    assert!(blobs_dir.exists(), "Blobs directory was not created");
    let blob_files: Vec<_> = std::fs::read_dir(&blobs_dir).unwrap().collect();
    assert!(!blob_files.is_empty(), "No blob files written to .ozymem/blobs");
    for entry in blob_files {
        let path = entry.unwrap().path();
        let content = std::fs::read_to_string(&path).unwrap();
        eprintln!("[test] Found blob file {:?}, length = {}", path.file_name(), content.len());
        // All persisted blobs must have captured raw payload > threshold (14,000)
        assert!(content.len() > 14_000, "Persisted blob content length ({}) is below threshold", content.len());
    }

    let _ = std::fs::remove_dir_all(&dir);
}
