use ozymem_server::handle_request;
use ozymem_core::graph_backend::GraphBackend;
use ozymem_core::mcp_common;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::RwLock;

fn make_temp_dir(suffix: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("ozymem_test_multi_agent_{}_{}", std::process::id(), suffix));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p.join("src")).unwrap();
    std::fs::write(
        p.join("src").join("auth.rs"),
        "pub fn authenticate(token: &str) -> bool { !token.is_empty() }\n",
    ).unwrap();
    std::fs::write(
        p.join("src").join("billing.rs"),
        "pub fn calculate_total(items: &[f64]) -> f64 { items.iter().sum() }\n",
    ).unwrap();
    p
}

#[tokio::test]
async fn test_antigravity_2_0_multi_agent_concurrency() {
    let dir = make_temp_dir("concurrency_mesh");
    let backend: Arc<RwLock<Option<GraphBackend>>> = Arc::new(RwLock::new(None));

    // 1. Initial agent initializes the workspace
    let init_req = mcp_common::JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(json!("init_coordinator")),
        method: "initialize".to_string(),
        params: Some(json!({
            "rootPath": dir.to_string_lossy().to_string(),
        })),
    };

    let start_init = std::time::Instant::now();
    let init_resp = handle_request(&backend, init_req, None, None).await.unwrap().unwrap();
    assert!(init_resp.result.is_some());
    let init_dur = start_init.elapsed();
    eprintln!("[test] First cold initialize completed in {:?}", init_dur);

    // 2. Simulate 8 concurrent agents and subagents executing simultaneous queries
    let num_agents = 8;
    let mut handles = Vec::new();

    for agent_idx in 0..num_agents {
        let b = backend.clone();
        let project_root_str = dir.to_string_lossy().to_string();

        let handle = tokio::spawn(async move {
            let start = std::time::Instant::now();
            let req = match agent_idx % 4 {
                0 => {
                    // Subagent: Fast-path idempotent re-initialize (should be < 1ms)
                    mcp_common::JsonRpcRequest {
                        jsonrpc: "2.0".to_string(),
                        id: Some(json!(format!("subagent_reinit_{}", agent_idx))),
                        method: "initialize".to_string(),
                        params: Some(json!({
                            "rootPath": project_root_str,
                        })),
                    }
                }
                1 => {
                    // Subagent: AST Contracts exploration
                    mcp_common::JsonRpcRequest {
                        jsonrpc: "2.0".to_string(),
                        id: Some(json!(format!("subagent_contracts_{}", agent_idx))),
                        method: "tools/call".to_string(),
                        params: Some(json!({
                            "name": "ozy_explore",
                            "arguments": {
                                "action": "contracts",
                                "file_path": "src/auth.rs"
                            }
                        })),
                    }
                }
                2 => {
                    // Subagent: File view
                    mcp_common::JsonRpcRequest {
                        jsonrpc: "2.0".to_string(),
                        id: Some(json!(format!("subagent_view_{}", agent_idx))),
                        method: "tools/call".to_string(),
                        params: Some(json!({
                            "name": "ozy_explore",
                            "arguments": {
                                "action": "view",
                                "file_path": "src/billing.rs",
                                "start_line": 1,
                                "end_line": 5
                            }
                        })),
                    }
                }
                _ => {
                    // Subagent: Spec-Driven Development capability map
                    mcp_common::JsonRpcRequest {
                        jsonrpc: "2.0".to_string(),
                        id: Some(json!(format!("subagent_capmap_{}", agent_idx))),
                        method: "tools/call".to_string(),
                        params: Some(json!({
                            "name": "ozy_explore",
                            "arguments": {
                                "action": "capability_map",
                                "initiative": "Payment Service"
                            }
                        })),
                    }
                }
            };

            let resp = handle_request(&b, req, None, None).await;
            let elapsed = start.elapsed();
            (agent_idx, resp, elapsed)
        });

        handles.push(handle);
    }

    let overall_start = std::time::Instant::now();
    for handle in handles {
        let (idx, res, dur) = handle.await.unwrap();
        eprintln!("[test] Agent {} completed in {:?}", idx, dur);
        assert!(res.is_ok(), "Agent {} failed with error: {:?}", idx, res.err());
        let resp = res.unwrap();
        assert!(resp.is_some(), "Agent {} received empty response", idx);
        let val = resp.unwrap();
        assert!(val.error.is_none(), "Agent {} received JSON-RPC error: {:?}", idx, val.error);
        assert!(val.result.is_some(), "Agent {} received missing result", idx);
    }

    let total_elapsed = overall_start.elapsed();
    eprintln!("[test] All {} agents completed in total {:?}", num_agents, total_elapsed);

    // All 8 agents should finish well under 2 seconds total in parallel
    assert!(total_elapsed.as_secs() < 3, "Concurrent agents took too long: {:?}", total_elapsed);

    let _ = std::fs::remove_dir_all(&dir);
}
