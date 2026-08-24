use ozymem_server::brain::*;
use ozymem_server::state::*;
use ozymem_server::handle_request;
use ozymem_core::graph_backend::GraphBackend;
use ozymem_core::mcp_common;
use ozymem_core::McpBackend;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};


    #[tokio::test]
    async fn test_tool_call_before_initialize_returns_error() {
        let backend: Arc<Mutex<Option<GraphBackend>>> = Arc::new(Mutex::new(None));

        let request = mcp_common::JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(1)),
            method: "tools/call".to_string(),
            params: Some(serde_json::json!({
                "name": "analyze_impact",
                "arguments": {
                    "file_path": "/test.rs",
                    "depth": 3
                }
            })),
        };

        let response = handle_request(&backend, request, None, None).await.unwrap();
        assert!(response.is_some(), "should return some response, not crash");

        let resp = response.unwrap();
        assert!(
            resp.error.is_some(),
            "should return a JSON-RPC error, not success"
        );
        let err = resp.error.as_ref().unwrap();
        assert_eq!(
            err.code, -32000,
            "error code should be -32000 for server-not-initialized"
        );
        assert!(
            err.message.contains("workspaceFolders"),
            "error message should tell client what to send"
        );
    }

    #[tokio::test]
    async fn test_initialize_sets_backend_and_returns_ok() {
        let backend: Arc<Mutex<Option<GraphBackend>>> = Arc::new(Mutex::new(None));

        // Create a temp dir manually (avoid tempfile dev-dep)
        let tmp_root =
            std::env::temp_dir().join(format!("ozymem_test_init_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp_root);
        std::fs::create_dir_all(&tmp_root).unwrap();

        let proj_uri = format!("file:///{}", tmp_root.to_string_lossy().replace('\\', "/"));

        let request = mcp_common::JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(1)),
            method: "initialize".to_string(),
            params: Some(serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "test", "version": "1" },
                "workspaceFolders": [{
                    "uri": proj_uri,
                    "name": "test-project"
                }]
            })),
        };

        let response = handle_request(&backend, request, None, None).await.unwrap();
        assert!(response.is_some(), "initialize should return a response");

        let resp = response.unwrap();
        assert!(
            resp.error.is_none(),
            "initialize should not error: {:?}",
            resp.error
        );

        // Verify backend was set
        let guard = backend.lock().unwrap();
        assert!(guard.is_some(), "backend should be set after initialize");

        // Cleanup
        std::fs::remove_dir_all(&tmp_root).ok();
    }

    #[tokio::test]
    async fn test_context_for_task_empty_results() {
        let backend: Arc<Mutex<Option<GraphBackend>>> = Arc::new(Mutex::new(None));

        let tmp_root = std::env::temp_dir().join(format!("ozymem_test_cft_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp_root);
        std::fs::create_dir_all(&tmp_root).unwrap();
        // Create a dummy source file so full_scan has something to index
        std::fs::write(tmp_root.join("main.rs"), "fn main() {}").unwrap();

        let proj_uri = format!("file:///{}", tmp_root.to_string_lossy().replace('\\', "/"));

        // Initialize
        let init_req = mcp_common::JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(1)),
            method: "initialize".to_string(),
            params: Some(serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "test", "version": "1" },
                "workspaceFolders": [{
                    "uri": proj_uri,
                    "name": "test-project"
                }]
            })),
        };
        handle_request(&backend, init_req, None, None)
            .await
            .unwrap();

        // Run a quick scan before calling context_for_task
        {
            let guard = backend.lock().unwrap();
            if let Some(ref gb) = *guard {
                gb.full_scan(&tmp_root.to_string_lossy(), None).ok();
            }
        }

        // context_for_task with query that matches nothing
        let cft_req = mcp_common::JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(2)),
            method: "tools/call".to_string(),
            params: Some(serde_json::json!({
                "name": "context_for_task",
                "arguments": {
                    "query": "xyznonexistent_12345",
                    "max_tokens": 500
                }
            })),
        };
        let response = handle_request(&backend, cft_req, None, None).await.unwrap();
        assert!(
            response.is_some(),
            "context_for_task should return a response"
        );

        let resp = response.unwrap();
        assert!(
            resp.error.is_none(),
            "context_for_task should not error: {:?}",
            resp.error
        );

        let result = resp.result.unwrap();
        let text = result["content"][0]["text"].as_str().unwrap_or("");
        assert!(!text.is_empty(), "response text should not be empty");
        assert!(text.contains("(none)"), "should indicate no lessons found");

        // Verify truncation test: with max_tokens=500, output should be within budget
        assert!(
            text.len() / 4 <= 500,
            "output should be within token budget"
        );

        std::fs::remove_dir_all(&tmp_root).ok();
    }

    #[tokio::test]
    async fn test_context_for_task_stale_lessons_excluded() {
        let backend: Arc<Mutex<Option<GraphBackend>>> = Arc::new(Mutex::new(None));

        let tmp_root =
            std::env::temp_dir().join(format!("ozymem_test_cft_stale_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp_root);
        std::fs::create_dir_all(&tmp_root).unwrap();
        std::fs::write(tmp_root.join("main.rs"), "fn main() {}").unwrap();

        let proj_uri = format!("file:///{}", tmp_root.to_string_lossy().replace('\\', "/"));

        // Initialize
        let init_req = mcp_common::JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(1)),
            method: "initialize".to_string(),
            params: Some(serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "test", "version": "1" },
                "workspaceFolders": [{
                    "uri": proj_uri,
                    "name": "test-project"
                }]
            })),
        };
        handle_request(&backend, init_req, None, None)
            .await
            .unwrap();

        let main_abs = tmp_root.join("main.rs").to_string_lossy().to_string();

        // Run scan, then record two lessons on the same file with overlapping query keywords
        {
            let guard = backend.lock().unwrap();
            let gb = guard.as_ref().unwrap();
            gb.full_scan(&tmp_root.to_string_lossy(), None).ok();

            // Fresh lesson — should appear in output
            gb.record_lesson(
                &main_abs,
                Some("main"),
                "overflow bug in main",
                "Use checked_add for overflow safety",
            )
            .await
            .unwrap();

            // Stale lesson (will be marked stale right after) — should be filtered out
            gb.record_lesson(
                &main_abs,
                Some("main"),
                "old overflow approach",
                "Use wrapping_add for overflow (deprecated)",
            )
            .await
            .unwrap();
        }

        // Manually mark the second lesson as stale via direct SQLite
        {
            let db_path = tmp_root.join(".ozymem").join("memory.db");
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            // lessons are ordered by created_at ascending, so id=2 is the second one
            conn.execute(
                "UPDATE lessons SET stale = 1, stale_reason = 'symbol_removed' WHERE id = 2",
                [],
            )
            .unwrap();
        }

        // context_for_task with "overflow" — both lessons match, but stale should be filtered
        let cft_req = mcp_common::JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(2)),
            method: "tools/call".to_string(),
            params: Some(serde_json::json!({
                "name": "context_for_task",
                "arguments": {
                    "query": "overflow",
                    "max_tokens": 5000
                }
            })),
        };
        let response = handle_request(&backend, cft_req, None, None).await.unwrap();
        assert!(response.is_some());

        let resp = response.unwrap();
        assert!(resp.error.is_none(), "error: {:?}", resp.error);

        let result = resp.result.unwrap();
        let text = result["content"][0]["text"].as_str().unwrap_or("");

        // The fresh lesson's solution should appear
        assert!(
            text.contains("checked_add"),
            "output should contain the fresh lesson's solution, got:\n{}",
            text
        );

        // The stale lesson's solution should NOT appear anywhere in the output
        assert!(
            !text.contains("wrapping_add"),
            "output should NOT contain the stale lesson's content:\n{}",
            text
        );

        std::fs::remove_dir_all(&tmp_root).ok();
    }

    #[tokio::test]
    async fn test_context_for_task_truncation_boundary() {
        let backend: Arc<Mutex<Option<GraphBackend>>> = Arc::new(Mutex::new(None));

        let tmp_root =
            std::env::temp_dir().join(format!("ozymem_test_cft_trunc_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp_root);
        std::fs::create_dir_all(&tmp_root).unwrap();
        std::fs::write(tmp_root.join("main.rs"), "fn main() {}").unwrap();

        let proj_uri = format!("file:///{}", tmp_root.to_string_lossy().replace('\\', "/"));

        let init_req = mcp_common::JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(1)),
            method: "initialize".to_string(),
            params: Some(serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "test", "version": "1" },
                "workspaceFolders": [{
                    "uri": proj_uri,
                    "name": "test-project"
                }]
            })),
        };
        handle_request(&backend, init_req, None, None)
            .await
            .unwrap();

        let main_abs = tmp_root.join("main.rs").to_string_lossy().to_string();

        // Run scan, then record lessons with modestly long content.
        // The handler writes all lessons inline, then chops per-file context
        // sections at the token boundary.  We keep lesson content short enough
        // to fit in the budget so the File context section triggers the cut.
        {
            let guard = backend.lock().unwrap();
            let gb = guard.as_ref().unwrap();
            gb.full_scan(&tmp_root.to_string_lossy(), None).ok();

            for i in 0..3 {
                gb.record_lesson(
                    &main_abs,
                    Some("main"),
                    &format!("overflow pattern {}", i),
                    &format!("short explanation for pattern {}", i),
                )
                .await
                .unwrap();
            }
        }

        // Also write a second source file to generate more file-context output
        std::fs::write(tmp_root.join("utils.rs"), "fn helper() {} fn compute() {}").unwrap();
        {
            let guard = backend.lock().unwrap();
            let gb = guard.as_ref().unwrap();
            gb.full_scan(&tmp_root.to_string_lossy(), None).ok();
        }

        // context_for_task with a tight budget — the lessons section fits,
        // but the per-file sections should be cut short.
        let cft_req = mcp_common::JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(2)),
            method: "tools/call".to_string(),
            params: Some(serde_json::json!({
                "name": "context_for_task",
                "arguments": {
                    "query": "overflow",
                    "max_tokens": 200
                }
            })),
        };
        let response = handle_request(&backend, cft_req, None, None).await.unwrap();
        assert!(response.is_some());

        let resp = response.unwrap();
        assert!(resp.error.is_none(), "error: {:?}", resp.error);

        let result = resp.result.unwrap();
        let text = result["content"][0]["text"].as_str().unwrap_or("");

        // Output should be within the token budget (len/4 is approximate)
        // Allow a small overshoot because lessons-section content is written
        // before the per-file truncation check.
        let max_chars = (200 * 4) + 100;
        assert!(
            text.len() <= max_chars,
            "output {} chars (~{} tokens) exceeds budget, max ~{} chars:\n{}",
            text.len(),
            text.len() / 4,
            max_chars,
            &text[..text.len().min(300)]
        );

        // Should show the truncation message (unless by coincidence the
        // lessons + one file section fit exactly within budget)
        if text.len() >= 700 {
            assert!(
                text.contains("truncated at"),
                "output longer than expected should contain truncation message"
            );
        }

        std::fs::remove_dir_all(&tmp_root).ok();
    }

    #[tokio::test]
    async fn test_initialize_includes_resources_and_prompts_capabilities() {
        let backend: Arc<Mutex<Option<GraphBackend>>> = Arc::new(Mutex::new(None));
        let tmp_root =
            std::env::temp_dir().join(format!("ozymem_test_init_caps_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp_root);
        std::fs::create_dir_all(&tmp_root).unwrap();
        let proj_uri = format!("file:///{}", tmp_root.to_string_lossy().replace('\\', "/"));

        let request = mcp_common::JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(1)),
            method: "initialize".to_string(),
            params: Some(serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "test", "version": "1" },
                "workspaceFolders": [{ "uri": proj_uri, "name": "test" }]
            })),
        };
        let response = handle_request(&backend, request, None, None).await.unwrap();
        let resp = response.unwrap();
        assert!(resp.error.is_none());

        let result = resp.result.unwrap();
        assert!(
            result
                .get("capabilities")
                .and_then(|c| c.get("resources"))
                .is_some(),
            "initialize should declare resources capability"
        );
        assert!(
            result
                .get("capabilities")
                .and_then(|c| c.get("prompts"))
                .is_some(),
            "initialize should declare prompts capability"
        );
        assert!(
            result
                .get("capabilities")
                .and_then(|c| c.get("logging"))
                .is_some(),
            "initialize should declare logging capability"
        );
        assert!(
            result
                .get("capabilities")
                .and_then(|c| c.get("tools"))
                .is_some(),
            "initialize should declare tools capability"
        );

        std::fs::remove_dir_all(&tmp_root).ok();
    }

    #[tokio::test]
    async fn test_resources_list_before_initialize_returns_error() {
        let backend: Arc<Mutex<Option<GraphBackend>>> = Arc::new(Mutex::new(None));
        let request = mcp_common::JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(1)),
            method: "resources/list".to_string(),
            params: None,
        };
        let response = handle_request(&backend, request, None, None).await.unwrap();
        let resp = response.unwrap();
        assert!(resp.error.is_some());
        assert_eq!(resp.error.as_ref().unwrap().code, -32000);
    }

    #[tokio::test]
    async fn test_resources_list_returns_resources() {
        let backend: Arc<Mutex<Option<GraphBackend>>> = Arc::new(Mutex::new(None));
        let tmp_root =
            std::env::temp_dir().join(format!("ozymem_test_res_list_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp_root);
        std::fs::create_dir_all(&tmp_root).unwrap();
        let proj_uri = format!("file:///{}", tmp_root.to_string_lossy().replace('\\', "/"));

        let init_req = mcp_common::JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(1)),
            method: "initialize".to_string(),
            params: Some(serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "test", "version": "1" },
                "workspaceFolders": [{ "uri": proj_uri, "name": "test" }]
            })),
        };
        handle_request(&backend, init_req, None, None)
            .await
            .unwrap();

        let list_req = mcp_common::JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(2)),
            method: "resources/list".to_string(),
            params: None,
        };
        let response = handle_request(&backend, list_req, None, None)
            .await
            .unwrap();
        let resp = response.unwrap();
        assert!(resp.error.is_none(), "resources/list should not error");

        let result = resp.result.unwrap();
        let resources = result["resources"].as_array().unwrap();
        assert!(!resources.is_empty(), "should list at least one resource");

        let uris: Vec<&str> = resources.iter().filter_map(|r| r["uri"].as_str()).collect();
        assert!(uris.contains(&"ozymem://summary"), "should include summary");
        assert!(
            uris.contains(&"ozymem://recent-lessons"),
            "should include recent-lessons"
        );

        std::fs::remove_dir_all(&tmp_root).ok();
    }

    #[tokio::test]
    async fn test_resources_read_summary() {
        let backend: Arc<Mutex<Option<GraphBackend>>> = Arc::new(Mutex::new(None));
        let tmp_root =
            std::env::temp_dir().join(format!("ozymem_test_res_sum_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp_root);
        std::fs::create_dir_all(&tmp_root).unwrap();
        let proj_uri = format!("file:///{}", tmp_root.to_string_lossy().replace('\\', "/"));

        let init_req = mcp_common::JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(1)),
            method: "initialize".to_string(),
            params: Some(serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "test", "version": "1" },
                "workspaceFolders": [{ "uri": proj_uri, "name": "test" }]
            })),
        };
        handle_request(&backend, init_req, None, None)
            .await
            .unwrap();

        let read_req = mcp_common::JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(2)),
            method: "resources/read".to_string(),
            params: Some(serde_json::json!({ "uri": "ozymem://summary" })),
        };
        let response = handle_request(&backend, read_req, None, None)
            .await
            .unwrap();
        let resp = response.unwrap();
        assert!(
            resp.error.is_none(),
            "resources/read summary should not error: {:?}",
            resp.error
        );

        let result = resp.result.unwrap();
        let contents = result["contents"].as_array().unwrap();
        assert_eq!(contents.len(), 1);
        assert_eq!(contents[0]["uri"].as_str().unwrap(), "ozymem://summary");
        assert!(contents[0]["text"].as_str().unwrap().contains("file_count"));

        std::fs::remove_dir_all(&tmp_root).ok();
    }

    #[tokio::test]
    async fn test_resources_read_file_context() {
        let backend: Arc<Mutex<Option<GraphBackend>>> = Arc::new(Mutex::new(None));
        let tmp_root =
            std::env::temp_dir().join(format!("ozymem_test_res_ctx_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp_root);
        std::fs::create_dir_all(&tmp_root).unwrap();
        std::fs::write(tmp_root.join("main.rs"), "fn hello() {}").unwrap();
        let proj_uri = format!("file:///{}", tmp_root.to_string_lossy().replace('\\', "/"));

        let init_req = mcp_common::JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(1)),
            method: "initialize".to_string(),
            params: Some(serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "test", "version": "1" },
                "workspaceFolders": [{ "uri": proj_uri, "name": "test" }]
            })),
        };
        handle_request(&backend, init_req, None, None)
            .await
            .unwrap();

        // Run scan
        {
            let guard = backend.lock().unwrap();
            let gb = guard.as_ref().unwrap();
            gb.full_scan(&tmp_root.to_string_lossy(), None).ok();
        }

        let main_abs = tmp_root.join("main.rs").to_string_lossy().to_string();
        let uri = format!("ozymem://file/{}", main_abs);
        let read_req = mcp_common::JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(2)),
            method: "resources/read".to_string(),
            params: Some(serde_json::json!({ "uri": uri })),
        };
        let response = handle_request(&backend, read_req, None, None)
            .await
            .unwrap();
        let resp = response.unwrap();
        assert!(
            resp.error.is_none(),
            "resources/read file should not error: {:?}",
            resp.error
        );

        let result = resp.result.unwrap();
        let contents = result["contents"].as_array().unwrap();
        assert_eq!(contents.len(), 1);
        let text = contents[0]["text"].as_str().unwrap();
        assert!(
            text.contains("hello"),
            "file context should contain function name"
        );

        std::fs::remove_dir_all(&tmp_root).ok();
    }

    #[tokio::test]
    async fn test_resources_read_unknown_uri_returns_error() {
        let backend: Arc<Mutex<Option<GraphBackend>>> = Arc::new(Mutex::new(None));
        let tmp_root =
            std::env::temp_dir().join(format!("ozymem_test_res_bad_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp_root);
        std::fs::create_dir_all(&tmp_root).unwrap();
        let proj_uri = format!("file:///{}", tmp_root.to_string_lossy().replace('\\', "/"));

        let init_req = mcp_common::JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(1)),
            method: "initialize".to_string(),
            params: Some(serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "test", "version": "1" },
                "workspaceFolders": [{ "uri": proj_uri, "name": "test" }]
            })),
        };
        handle_request(&backend, init_req, None, None)
            .await
            .unwrap();

        let read_req = mcp_common::JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(2)),
            method: "resources/read".to_string(),
            params: Some(serde_json::json!({ "uri": "ozymem://nonexistent" })),
        };
        let response = handle_request(&backend, read_req, None, None)
            .await
            .unwrap();
        let resp = response.unwrap();
        assert!(resp.error.is_some(), "unknown URI should return error");
        assert_eq!(resp.error.as_ref().unwrap().code, -32602);

        std::fs::remove_dir_all(&tmp_root).ok();
    }

    #[tokio::test]
    async fn test_resource_templates_list() {
        let backend: Arc<Mutex<Option<GraphBackend>>> = Arc::new(Mutex::new(None));
        let request = mcp_common::JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(1)),
            method: "resources/templates/list".to_string(),
            params: None,
        };
        let response = handle_request(&backend, request, None, None).await.unwrap();
        let resp = response.unwrap();
        assert!(resp.error.is_none());

        let result = resp.result.unwrap();
        let templates = result["resourceTemplates"].as_array().unwrap();
        assert!(!templates.is_empty(), "should list at least one template");

        let uri_templates: Vec<&str> = templates
            .iter()
            .filter_map(|t| t["uriTemplate"].as_str())
            .collect();
        assert!(
            uri_templates.contains(&"ozymem://file/{path}"),
            "should include file template"
        );
    }

    #[tokio::test]
    async fn test_prompts_list_returns_prompts() {
        let backend: Arc<Mutex<Option<GraphBackend>>> = Arc::new(Mutex::new(None));
        let request = mcp_common::JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(1)),
            method: "prompts/list".to_string(),
            params: None,
        };
        let response = handle_request(&backend, request, None, None).await.unwrap();
        let resp = response.unwrap();
        assert!(resp.error.is_none());

        let result = resp.result.unwrap();
        let prompts = result["prompts"].as_array().unwrap();
        assert!(!prompts.is_empty(), "should list at least one prompt");

        let names: Vec<&str> = prompts.iter().filter_map(|p| p["name"].as_str()).collect();
        assert!(
            names.contains(&"analyze-file"),
            "should include analyze-file"
        );
        assert!(
            names.contains(&"review-lessons"),
            "should include review-lessons"
        );
    }

    #[tokio::test]
    async fn test_prompts_get_before_initialize_returns_error() {
        let backend: Arc<Mutex<Option<GraphBackend>>> = Arc::new(Mutex::new(None));
        let request = mcp_common::JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(1)),
            method: "prompts/get".to_string(),
            params: Some(
                serde_json::json!({ "name": "review-lessons", "arguments": { "file_path": "/test.rs" } }),
            ),
        };
        let response = handle_request(&backend, request, None, None).await.unwrap();
        let resp = response.unwrap();
        assert!(resp.error.is_some());
        assert_eq!(resp.error.as_ref().unwrap().code, -32000);
    }

    #[tokio::test]
    async fn test_prompts_get_analyze_file() {
        let backend: Arc<Mutex<Option<GraphBackend>>> = Arc::new(Mutex::new(None));
        let tmp_root =
            std::env::temp_dir().join(format!("ozymem_test_prompt_af_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp_root);
        std::fs::create_dir_all(&tmp_root).unwrap();
        std::fs::write(tmp_root.join("main.rs"), "fn analyze_me() {}").unwrap();
        let proj_uri = format!("file:///{}", tmp_root.to_string_lossy().replace('\\', "/"));

        let init_req = mcp_common::JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(1)),
            method: "initialize".to_string(),
            params: Some(serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "test", "version": "1" },
                "workspaceFolders": [{ "uri": proj_uri, "name": "test" }]
            })),
        };
        handle_request(&backend, init_req, None, None)
            .await
            .unwrap();
        {
            let guard = backend.lock().unwrap();
            let gb = guard.as_ref().unwrap();
            gb.full_scan(&tmp_root.to_string_lossy(), None).ok();
        }

        let main_abs = tmp_root.join("main.rs").to_string_lossy().to_string();
        let prompt_req = mcp_common::JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(2)),
            method: "prompts/get".to_string(),
            params: Some(
                serde_json::json!({ "name": "analyze-file", "arguments": { "path": main_abs, "depth": 1 } }),
            ),
        };
        let response = handle_request(&backend, prompt_req, None, None)
            .await
            .unwrap();
        let resp = response.unwrap();
        assert!(
            resp.error.is_none(),
            "prompts/get analyze-file should not error: {:?}",
            resp.error
        );

        let result = resp.result.unwrap();
        let messages = result["messages"].as_array().unwrap();
        assert!(!messages.is_empty(), "should return at least one message");
        let text = messages[0]["content"]["text"].as_str().unwrap_or("");
        assert!(
            text.contains("Analysis of"),
            "should contain analysis header"
        );
        assert!(text.contains("analyze_me"), "should mention function name");

        std::fs::remove_dir_all(&tmp_root).ok();
    }

    #[tokio::test]
    async fn test_prompts_get_review_lessons() {
        let backend: Arc<Mutex<Option<GraphBackend>>> = Arc::new(Mutex::new(None));
        let tmp_root =
            std::env::temp_dir().join(format!("ozymem_test_prompt_rl_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp_root);
        std::fs::create_dir_all(&tmp_root).unwrap();
        std::fs::write(tmp_root.join("main.rs"), "fn main() {}").unwrap();
        let proj_uri = format!("file:///{}", tmp_root.to_string_lossy().replace('\\', "/"));

        let init_req = mcp_common::JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(1)),
            method: "initialize".to_string(),
            params: Some(serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "test", "version": "1" },
                "workspaceFolders": [{ "uri": proj_uri, "name": "test" }]
            })),
        };
        handle_request(&backend, init_req, None, None)
            .await
            .unwrap();
        let main_abs = tmp_root.join("main.rs").to_string_lossy().to_string();

        {
            let guard = backend.lock().unwrap();
            let gb = guard.as_ref().unwrap();
            gb.full_scan(&tmp_root.to_string_lossy(), None).ok();
            gb.record_lesson(&main_abs, Some("main"), "test error", "test solution")
                .await
                .unwrap();
        }

        let prompt_req = mcp_common::JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(2)),
            method: "prompts/get".to_string(),
            params: Some(
                serde_json::json!({ "name": "review-lessons", "arguments": { "file_path": main_abs } }),
            ),
        };
        let response = handle_request(&backend, prompt_req, None, None)
            .await
            .unwrap();
        let resp = response.unwrap();
        assert!(
            resp.error.is_none(),
            "prompts/get review-lessons should not error: {:?}",
            resp.error
        );

        let result = resp.result.unwrap();
        let messages = result["messages"].as_array().unwrap();
        let text = messages[0]["content"]["text"].as_str().unwrap_or("");
        assert!(
            text.contains("test solution"),
            "should contain lesson solution"
        );
        assert!(
            text.contains("test error"),
            "should contain lesson error context"
        );

        std::fs::remove_dir_all(&tmp_root).ok();
    }

    #[tokio::test]
    async fn test_prompts_get_unknown_prompt_returns_error() {
        let backend: Arc<Mutex<Option<GraphBackend>>> = Arc::new(Mutex::new(None));
        let tmp_root =
            std::env::temp_dir().join(format!("ozymem_test_prompt_unk_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp_root);
        std::fs::create_dir_all(&tmp_root).unwrap();
        let proj_uri = format!("file:///{}", tmp_root.to_string_lossy().replace('\\', "/"));

        let init_req = mcp_common::JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(1)),
            method: "initialize".to_string(),
            params: Some(serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "test", "version": "1" },
                "workspaceFolders": [{ "uri": proj_uri, "name": "test" }]
            })),
        };
        handle_request(&backend, init_req, None, None)
            .await
            .unwrap();

        let request = mcp_common::JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(2)),
            method: "prompts/get".to_string(),
            params: Some(serde_json::json!({ "name": "nonexistent-prompt", "arguments": {} })),
        };
        let response = handle_request(&backend, request, None, None).await.unwrap();
        let resp = response.unwrap();
        assert!(resp.error.is_some());
        assert_eq!(resp.error.as_ref().unwrap().code, -32602);
        std::fs::remove_dir_all(&tmp_root).ok();
    }

    #[test]
    fn test_notifier_log_sends_valid_json() {
        let (n, mut rx) = Notifier::new();
        n.log("info", "hello world".into());

        let payload = rx.try_recv().unwrap();
        let v: Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(v["jsonrpc"], "2.0");
        assert_eq!(v["method"], "notifications/message");
        assert_eq!(v["params"]["level"], "info");
        assert_eq!(v["params"]["data"], "hello world");
    }

    #[test]
    fn test_notifier_progress_sends_valid_json() {
        let (n, mut rx) = Notifier::new();
        n.progress(&json!("token-42"), 5, Some(10));

        let payload = rx.try_recv().unwrap();
        let v: Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(v["method"], "notifications/progress");
        assert_eq!(v["params"]["progressToken"], "token-42");
        assert_eq!(v["params"]["progress"], 5);
        assert_eq!(v["params"]["total"], 10);
    }

    #[test]
    fn test_notifier_progress_without_total() {
        let (n, mut rx) = Notifier::new();
        n.progress(&json!(42), 1, None);

        let payload = rx.try_recv().unwrap();
        let v: Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(v["params"]["progress"], 1);
        assert!(v["params"].get("total").is_none());
    }

    #[tokio::test]
    async fn test_initialize_includes_sampling_and_completions_capabilities() {
        let backend: Arc<Mutex<Option<GraphBackend>>> = Arc::new(Mutex::new(None));
        let tmp_root = std::env::temp_dir().join("ozymem_test_sampling_caps");
        std::fs::create_dir_all(&tmp_root).ok();
        let proj_uri = format!("file://{}", tmp_root.to_string_lossy().replace('\\', "/"));

        let init_req = mcp_common::JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(1)),
            method: "initialize".to_string(),
            params: Some(json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "test", "version": "1" },
                "workspaceFolders": [{ "uri": proj_uri, "name": "test" }]
            })),
        };
        let response = handle_request(&backend, init_req, None, None)
            .await
            .unwrap();
        let resp = response.unwrap();
        let caps = resp.result.unwrap()["capabilities"].clone();
        assert_eq!(
            caps["sampling"],
            json!({}),
            "sampling capability should be present"
        );
        assert_eq!(
            caps["completions"],
            json!({}),
            "completions capability should be present"
        );
        std::fs::remove_dir_all(&tmp_root).ok();
    }

    #[tokio::test]
    async fn test_tools_list_pagination_no_cursor() {
        let backend: Arc<Mutex<Option<GraphBackend>>> = Arc::new(Mutex::new(None));
        let request = mcp_common::JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(1)),
            method: "tools/list".to_string(),
            params: None,
        };
        let response = handle_request(&backend, request, None, None).await.unwrap();
        let resp = response.unwrap();
        let result = resp.result.unwrap();
        let tools = result["tools"].as_array().unwrap();
        assert!(
            tools.len() >= 7,
            "default tools/list should expose unified tools"
        );
        assert!(
            tools.iter().all(|tool| tool
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .starts_with("ozy_")),
            "legacy tools should be hidden by default"
        );
        assert!(
            result.get("nextCursor").is_none(),
            "no cursor for first page without params"
        );
    }

    #[tokio::test]
    async fn test_tools_list_pagination_with_cursor() {
        let backend: Arc<Mutex<Option<GraphBackend>>> = Arc::new(Mutex::new(None));
        let request = mcp_common::JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(1)),
            method: "tools/list".to_string(),
            params: Some(json!({
                "cursor": "0"
            })),
        };
        let response = handle_request(&backend, request, None, None).await.unwrap();
        let resp = response.unwrap();
        let result = resp.result.unwrap();
        assert!(
            result.get("nextCursor").is_some() || result["tools"].as_array().unwrap().len() > 0
        );
    }

    #[tokio::test]
    async fn test_completions_unknown_prompt_returns_empty() {
        let backend: Arc<Mutex<Option<GraphBackend>>> = Arc::new(Mutex::new(None));
        let request = mcp_common::JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(1)),
            method: "completions/complete".to_string(),
            params: Some(json!({
                "argument": { "name": "path", "value": "test" },
                "ref": { "type": "ref/prompt", "name": "nonexistent-prompt" }
            })),
        };
        let response = handle_request(&backend, request, None, None).await.unwrap();
        let resp = response.unwrap();
        let result = resp.result.unwrap();
        assert_eq!(result["completion"], "");
        assert!(result.get("values").is_none());
    }

    #[tokio::test]
    async fn test_completions_invalid_params_returns_error() {
        let backend: Arc<Mutex<Option<GraphBackend>>> = Arc::new(Mutex::new(None));
        let request = mcp_common::JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(1)),
            method: "completions/complete".to_string(),
            params: Some(json!({
                "argument": { "name": "path", "value": "test" },
                "ref": { "type": "invalid_type" }
            })),
        };
        let response = handle_request(&backend, request, None, None).await.unwrap();
        let resp = response.unwrap();
        assert!(resp.error.is_some(), "invalid ref type should return error");
    }

    #[test]
    fn test_complete_file_path() {
        let dir = std::env::temp_dir().join("ozymem_test_complete_path");
        std::fs::create_dir_all(&dir).ok();
        let root = dir.join("proj");
        std::fs::create_dir_all(root.join(".ozymem")).unwrap();
        let root_str = root.to_string_lossy().to_string();
        let db_path = root.join(".ozymem").join("memory.db");

        let backend = GraphBackend::open(Some(&db_path.to_string_lossy())).unwrap();
        backend.set_project_path(Some(&root_str));

        let mut f = std::fs::File::create(root.join("test_main.rs")).unwrap();
        use std::io::Write;
        write!(f, "fn main() {{}}").unwrap();
        backend.full_scan(&root_str, None).unwrap();

        let results = backend.complete_file_path("test", 10).unwrap();
        assert!(
            !results.is_empty(),
            "should find at least one matching file"
        );
        assert!(
            results[0].contains("test_main"),
            "should match test_main.rs"
        );

        let results = backend.complete_file_path("nonexistent_xyz", 5).unwrap();
        assert!(results.is_empty(), "should return empty for no matches");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn test_tools_list_exposes_unified_tools_and_hides_legacy() {
        let backend: Arc<Mutex<Option<GraphBackend>>> = Arc::new(Mutex::new(None));
        let request = mcp_common::JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(1)),
            method: "tools/list".to_string(),
            params: None,
        };
        let response = handle_request(&backend, request, None, None)
            .await
            .unwrap()
            .unwrap();
        assert!(response.error.is_none());
        let tools = response
            .result
            .unwrap()
            .get("tools")
            .and_then(Value::as_array)
            .unwrap()
            .clone();
        for name in [
            "ozy_context",
            "ozy_memory",
            "ozy_graph",
            "ozy_code_doctor",
            "ozy_doctor",
            "ozy_skills",
            "ozy_brain",
            "ozy_project",
        ] {
            assert!(
                tools
                    .iter()
                    .any(|t| t.get("name").and_then(Value::as_str) == Some(name)),
                "missing unified tool {name}"
            );
        }
        assert_eq!(
            tools
                .iter()
                .find(|t| t.get("name").and_then(Value::as_str) == Some("analyze_impact")),
            None,
            "legacy tools should remain callable internally but hidden from tools/list"
        );
    }

    #[tokio::test]
    async fn test_ozy_skills_lists_official_metadata_without_backend() {
        let backend: Arc<Mutex<Option<GraphBackend>>> = Arc::new(Mutex::new(None));
        let request = mcp_common::JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(1)),
            method: "tools/call".to_string(),
            params: Some(
                json!({"name":"ozy_skills","arguments":{"action":"search","query":"react","limit":5}}),
            ),
        };
        let response = handle_request(&backend, request, None, None)
            .await
            .unwrap()
            .unwrap();
        assert!(
            response.error.is_none(),
            "ozy_skills should not require initialized backend"
        );
        let text = response.result.unwrap()["content"][0]["text"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(text.contains("skills.sh/official"));
        assert!(text.contains("facebook/react"));
        assert!(text.contains("executed_external_content") || text.contains("official_only"));
    }

    #[test]
    fn test_ozy_brain_persistent_server_fallback() {
        let payload = json!({
            "project": "ozymem-test",
            "goal": "test persistent fallback"
        });
        let res = try_call_ozy_brain_persistent("plan", &payload, 50);
        assert!(res.is_err(), "connecting to closed port must return error for fallback");
    }

    #[test]
    fn test_ozy_brain_schema_rejection() {
        let invalid_json = json!({
            "action": "plan",
            "summary": "invalid payload missing required fields"
        });
        let result = validate_ozy_brain_response_schema(&invalid_json);
        assert!(result.is_err(), "malformed json response must fail schema validation");

        let valid_json = json!({
            "action": "plan",
            "summary": "valid summary",
            "plan": ["step 1"],
            "risks": ["risk 1"],
            "recommendations": ["rec 1"],
            "memory_updates": [],
            "confidence": 0.9,
            "brain_version": "0.2.0",
            "brain_schema_version": "v1"
        });
        let valid_res = validate_ozy_brain_response_schema(&valid_json);
        assert!(valid_res.is_ok(), "valid json response must pass schema validation");
    }

    #[test]
    fn test_ozy_brain_worker_plan_returns_safe_json() {
        if resolve_ozy_brain_dir().is_none() {
            eprintln!("Skipping ozy brain worker test: python/ozy-brain not found");
            return;
        }
        let payload = json!({
            "project": "ozymem-test",
            "goal": "build a safer autonomous brain",
            "files": ["src/main.rs"],
            "git_context": {"dirty": true, "status_files": [{"status":"M", "path":"src/main.rs"}]},
            "memories": [{"title":"validate before reporting"}]
        });
        let result = call_ozy_brain_worker("plan", &payload, 10_000).unwrap();
        assert_eq!(result["action"], "plan");
        assert_eq!(result["safe_mode"], true);
        assert!(
            result["plan"].as_array().unwrap().len() >= 3,
            "brain should return actionable plan steps"
        );
        assert!(
            result["suggested_mcp_calls"]
                .as_array()
                .unwrap()
                .iter()
                .any(|call| call.get("tool").and_then(Value::as_str) == Some("ozy_context")),
            "brain should suggest safe follow-up MCP calls"
        );
        assert_eq!(result["structured_plan"]["autonomy_level"], "advisory");
        assert!(
            result["structured_plan"]["phases"]
                .as_array()
                .unwrap()
                .len()
                >= 5,
            "brain should return structured execution phases"
        );
        assert_eq!(result["execution_policy"]["rust_authority"], true);
        assert_eq!(result["brain_context_pack"]["dirty"], true);
        assert!(
            result["brain_context_pack"]["candidate_file_scores"]
                .as_array()
                .unwrap()
                .iter()
                .any(|file| file.get("path").and_then(Value::as_str) == Some("src/main.rs")),
            "brain should score dirty/indexed candidate files"
        );
    }

    #[test]
    fn test_ozy_brain_worker_reflect_and_risk_review() {
        if resolve_ozy_brain_dir().is_none() {
            return;
        }
        let reflect_payload = json!({
            "project": "ozymem-test",
            "goal": "fix auth bug",
            "failures": ["Permission denied"],
            "changes": ["src/main.rs", "src/extra.rs"],
            "files": ["src/main.rs"]
        });
        let reflect_res = call_ozy_brain_worker("reflect", &reflect_payload, 10_000).unwrap();
        assert_eq!(reflect_res["action"], "reflect");
        assert_eq!(reflect_res["reflection_report"]["total_failures"], 1);
        assert_eq!(reflect_res["reflection_report"]["scope_creep_detected"], true);

        let risk_payload = json!({
            "project": "ozymem-test",
            "goal": "drop table auth_tokens and migration",
            "files": ["src/auth.rs"]
        });
        let risk_res = call_ozy_brain_worker("risk_review", &risk_payload, 10_000).unwrap();
        assert_eq!(risk_res["action"], "risk_review");
        assert_eq!(risk_res["risk_assessment"]["risk_level"], "critical");
        assert_eq!(risk_res["risk_assessment"]["requires_user_confirmation"], true);

        let mental_payload = json!({
            "project": "ozymem-test",
            "files": ["crates/ozymem-core/src/lib.rs"]
        });
        let mental_res = call_ozy_brain_worker("build_mental_model", &mental_payload, 10_000).unwrap();
        assert_eq!(mental_res["action"], "build_mental_model");
        assert_eq!(mental_res["mental_model"]["project"], "ozymem-test");

        // Test Adversarial Critic Worker
        let critic_payload = json!({
            "project": "ozymem-test",
            "files": ["src/payments.rs"],
            "diff": "ALTER TABLE payments DROP COLUMN stripe_id;",
            "plan": ["Remove legacy payment column"]
        });
        let critic_res = call_ozy_brain_worker("audit_changes_with_critic", &critic_payload, 30_000).unwrap();
        assert_eq!(critic_res["action"], "audit_changes_with_critic");
        assert!(critic_res["risk_assessment"]["risk_level"].is_string());
        assert!(critic_res["risk_assessment"]["summary"].is_string());

        // Test Repository Hotspots Worker
        let hotspots_payload = json!({
            "project": "ozymem-test",
            "limit": 5
        });
        let hotspots_res = call_ozy_brain_worker("get_repository_hotspots", &hotspots_payload, 25_000).unwrap();
        assert_eq!(hotspots_res["action"], "get_repository_hotspots");
        assert!(hotspots_res["structured_plan"].get("hotspots").is_some());
    }

    #[tokio::test]
    async fn test_ozy_code_doctor_detects_duplicate_blocks() {
        let backend_ref: Arc<Mutex<Option<GraphBackend>>> = Arc::new(Mutex::new(None));
        let tmp_root =
            std::env::temp_dir().join(format!("ozymem_test_code_doctor_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp_root);
        std::fs::create_dir_all(tmp_root.join("src")).unwrap();
        let duplicate = "fn duplicated() {\n let a = 1;\n let b = 2;\n let c = a + b;\n println!(\"{}\", c);\n}\n";
        std::fs::write(tmp_root.join("src").join("a.rs"), duplicate).unwrap();
        std::fs::write(tmp_root.join("src").join("b.rs"), duplicate).unwrap();
        let uri = format!("file:///{}", tmp_root.to_string_lossy().replace('\\', "/"));
        let init = mcp_common::JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(1)),
            method: "initialize".to_string(),
            params: Some(
                json!({"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"1"},"workspaceFolders":[{"uri":uri,"name":"test"}]}),
            ),
        };
        handle_request(&backend_ref, init, None, None)
            .await
            .unwrap();
        {
            let guard = backend_ref.lock().unwrap();
            guard
                .as_ref()
                .unwrap()
                .full_scan(&tmp_root.to_string_lossy(), None)
                .unwrap();
        }
        let req = mcp_common::JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(2)),
            method: "tools/call".to_string(),
            params: Some(
                json!({"name":"ozy_code_doctor","arguments":{"min_duplicate_lines":4,"max_findings":5}}),
            ),
        };
        let response = handle_request(&backend_ref, req, None, None)
            .await
            .unwrap()
            .unwrap();
        assert!(response.error.is_none());
        let text = response.result.unwrap()["content"][0]["text"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(text.contains("Duplicate block") || text.contains("Duplicate findings"));
        std::fs::remove_dir_all(&tmp_root).ok();
    }

    #[tokio::test]
    async fn test_tool_lookup_engram() {
        let backend_ref: Arc<Mutex<Option<GraphBackend>>> = Arc::new(Mutex::new(None));
        let tmp_root = std::env::temp_dir().join(format!("ozymem_test_engram_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp_root);
        std::fs::create_dir_all(&tmp_root).unwrap();
        let code = r#"
/// Inicia la sesión del usuario
pub fn authenticate(user: &str, token: &str) -> bool {
    !user.is_empty() && !token.is_empty()
}
"#;
        std::fs::write(tmp_root.join("auth.rs"), code).unwrap();
        let uri = format!("file:///{}", tmp_root.to_string_lossy().replace('\\', "/"));
        let init = mcp_common::JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(1)),
            method: "initialize".to_string(),
            params: Some(
                json!({"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"1"},"workspaceFolders":[{"uri":uri,"name":"test"}]}),
            ),
        };
        handle_request(&backend_ref, init, None, None)
            .await
            .unwrap();
        {
            let guard = backend_ref.lock().unwrap();
            guard
                .as_ref()
                .unwrap()
                .full_scan(&tmp_root.to_string_lossy(), None)
                .unwrap();
        }
        let req = mcp_common::JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(2)),
            method: "tools/call".to_string(),
            params: Some(
                json!({"name":"lookup_engram","arguments":{"symbol_path":"authenticate"}}),
            ),
        };
        let response = handle_request(&backend_ref, req, None, None)
            .await
            .unwrap()
            .unwrap();
        assert!(response.error.is_none());
        let text = response.result.unwrap()["content"][0]["text"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(text.contains("[ENGRAM_CONTRACT:"));
        assert!(text.contains("authenticate"));
        assert!(text.contains("pub fn authenticate"));
        assert!(text.contains("Inicia la sesión del usuario"));
        std::fs::remove_dir_all(&tmp_root).ok();
    }

    #[tokio::test]
    async fn test_task_context_injects_engram_prefill() {
        let backend_ref: Arc<Mutex<Option<GraphBackend>>> = Arc::new(Mutex::new(None));
        let tmp_root = std::env::temp_dir().join(format!("ozymem_test_task_engram_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp_root);
        std::fs::create_dir_all(&tmp_root).unwrap();
        let code = r#"
/// Procesa el pago con tarjeta
pub fn process_payment(amount: f64) -> Result<(), String> {
    Ok(())
}
"#;
        std::fs::write(tmp_root.join("payment.rs"), code).unwrap();
        let uri = format!("file:///{}", tmp_root.to_string_lossy().replace('\\', "/"));
        let init = mcp_common::JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(1)),
            method: "initialize".to_string(),
            params: Some(
                json!({"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"1"},"workspaceFolders":[{"uri":uri,"name":"test"}]}),
            ),
        };
        handle_request(&backend_ref, init, None, None)
            .await
            .unwrap();
        {
            let guard = backend_ref.lock().unwrap();
            let gb = guard.as_ref().unwrap();
            gb.full_scan(&tmp_root.to_string_lossy(), None).unwrap();
            gb.record_lesson("payment.rs", Some("process_payment"), "error amount negative", "validate amount > 0").await.unwrap();
        }

        let req = mcp_common::JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(2)),
            method: "tools/call".to_string(),
            params: Some(
                json!({"name":"ozy_context","arguments":{"action":"task","query":"payment"}}),
            ),
        };
        let response = handle_request(&backend_ref, req, None, None)
            .await
            .unwrap()
            .unwrap();
        assert!(response.error.is_none());
        let text = response.result.unwrap()["content"][0]["text"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(text.contains("[ENGRAM_CACHE: Deterministic Symbol Contracts]"));
        assert!(text.contains("process_payment"));
        assert!(text.contains("Procesa el pago con tarjeta"));
        std::fs::remove_dir_all(&tmp_root).ok();
    }

    #[tokio::test]
    async fn test_speculative_engrams_prefetching() {
        let backend_ref: Arc<Mutex<Option<GraphBackend>>> = Arc::new(Mutex::new(None));
        let tmp_root = std::env::temp_dir().join(format!("ozymem_test_speculative_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp_root);
        std::fs::create_dir_all(&tmp_root).unwrap();

        let code_order = r#"
use crate::user::get_user_by_id;

/// Crea una nueva orden de compra
pub fn create_order(user_id: u64, total: f64) -> Result<(), String> {
    let _ = get_user_by_id(user_id);
    Ok(())
}
"#;
        let code_user = r#"
/// Obtiene los datos del usuario por ID
pub fn get_user_by_id(id: u64) -> Option<String> {
    Some(format!("User #{}", id))
}
"#;
        std::fs::write(tmp_root.join("order.rs"), code_order).unwrap();
        std::fs::write(tmp_root.join("user.rs"), code_user).unwrap();

        let uri = format!("file:///{}", tmp_root.to_string_lossy().replace('\\', "/"));
        let init = mcp_common::JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(1)),
            method: "initialize".to_string(),
            params: Some(
                json!({"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"1"},"workspaceFolders":[{"uri":uri,"name":"test"}]}),
            ),
        };
        handle_request(&backend_ref, init, None, None)
            .await
            .unwrap();

        {
            let guard = backend_ref.lock().unwrap();
            let gb = guard.as_ref().unwrap();
            gb.full_scan(&tmp_root.to_string_lossy(), None).unwrap();
            gb.record_lesson("order.rs", Some("create_order"), "invalid total", "validate total > 0").await.unwrap();

            // Direct core method check
            let speculative = gb.get_speculative_engrams("order.rs", 5);
            assert!(!speculative.is_empty(), "Should speculate user.rs symbols");
            assert!(speculative.iter().any(|s| s.symbol_path.contains("get_user_by_id")));
        }

        let req = mcp_common::JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(2)),
            method: "tools/call".to_string(),
            params: Some(
                json!({"name":"ozy_context","arguments":{"action":"task","query":"order"}}),
            ),
        };
        let response = handle_request(&backend_ref, req, None, None)
            .await
            .unwrap()
            .unwrap();
        assert!(response.error.is_none());
        let text = response.result.unwrap()["content"][0]["text"]
            .as_str()
            .unwrap()
            .to_string();

        assert!(text.contains("[ENGRAM_CACHE: Deterministic Symbol Contracts]"));
        assert!(text.contains("[SPECULATIVE_PREFETCH: Next Likely Contracts from Dependency Neighbors]"));
        assert!(text.contains("get_user_by_id"));
        assert!(text.contains("Obtiene los datos del usuario por ID"));

        std::fs::remove_dir_all(&tmp_root).ok();
    }

    #[tokio::test]
    async fn test_tool_verify_diff_sandboxed_and_guidance() {
        let backend_ref: Arc<Mutex<Option<GraphBackend>>> = Arc::new(Mutex::new(None));
        let tmp_root = std::env::temp_dir().join(format!("ozymem_test_verify_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp_root);
        std::fs::create_dir_all(&tmp_root).unwrap();

        let cargo_toml = r#"
[package]
name = "test_subproject"
version = "0.1.0"
edition = "2021"

[dependencies]
"#;
        let main_broken = r#"
pub struct UserState {
    pub id: u64,
    pub name: String,
}

fn main() {
    let _ = UserState {
        id: 1,
    };
}
"#;
        std::fs::create_dir_all(tmp_root.join("src")).unwrap();
        std::fs::write(tmp_root.join("Cargo.toml"), cargo_toml).unwrap();
        std::fs::write(tmp_root.join("src/main.rs"), main_broken).unwrap();

        let uri = format!("file:///{}", tmp_root.to_string_lossy().replace('\\', "/"));
        let init = mcp_common::JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(1)),
            method: "initialize".to_string(),
            params: Some(
                json!({"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"1"},"workspaceFolders":[{"uri":uri,"name":"test"}]}),
            ),
        };
        handle_request(&backend_ref, init, None, None)
            .await
            .unwrap();

        let req = mcp_common::JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(2)),
            method: "tools/call".to_string(),
            params: Some(
                json!({"name":"ozy_verify_diff","arguments":{"file_path":"src/main.rs","project_path":tmp_root.to_string_lossy()}}),
            ),
        };
        let response = handle_request(&backend_ref, req, None, None)
            .await
            .unwrap()
            .unwrap();

        let text = response.result.unwrap()["content"][0]["text"]
            .as_str()
            .unwrap()
            .to_string();

        assert!(text.contains("[VERIFICATION_FAILED: Closed-Loop Sandbox Detected"));
        assert!(text.contains("E0063") || text.contains("missing field `name`"));
        assert!(text.contains("Auto-Correction Procedural Rules"));
        assert!(text.contains("TRIGGER:") && text.contains("ACTION:"));

        std::fs::remove_dir_all(&tmp_root).ok();
    }

    #[tokio::test]
    async fn test_git_notes_export_and_import_roundtrip() {
        let backend_ref: Arc<Mutex<Option<GraphBackend>>> = Arc::new(Mutex::new(None));
        let tmp_root = std::env::temp_dir().join(format!("ozymem_test_git_notes_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp_root);
        std::fs::create_dir_all(&tmp_root).unwrap();

        // 1. Initialize git repo with git config
        let _ = std::process::Command::new("git").args(["init", "--initial-branch=main"]).arg(&tmp_root).output();
        let _ = std::process::Command::new("git").args(["config", "user.email", "agent@test.com"]).current_dir(&tmp_root).output();
        let _ = std::process::Command::new("git").args(["config", "user.name", "Agent Test"]).current_dir(&tmp_root).output();

        let code = r#"
/// Servicio de auditoría distribuida
pub fn audit_log(event: &str) -> bool {
    !event.is_empty()
}
"#;
        std::fs::write(tmp_root.join("audit.rs"), code).unwrap();
        let _ = std::process::Command::new("git").args(["add", "."]).current_dir(&tmp_root).output();
        let _ = std::process::Command::new("git").args(["commit", "-m", "Initial audit commit"]).current_dir(&tmp_root).output();

        let uri = format!("file:///{}", tmp_root.to_string_lossy().replace('\\', "/"));
        let init = mcp_common::JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(1)),
            method: "initialize".to_string(),
            params: Some(
                json!({"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"1"},"workspaceFolders":[{"uri":uri,"name":"test"}]}),
            ),
        };
        handle_request(&backend_ref, init, None, None).await.unwrap();

        {
            let guard = backend_ref.lock().unwrap();
            let gb = guard.as_ref().unwrap();
            gb.full_scan(&tmp_root.to_string_lossy(), None).unwrap();
            gb.record_lesson("audit.rs", Some("audit_log"), "event empty error", "validate event len > 0").await.unwrap();
        }

        // 2. Export to git notes via MCP
        let export_req = mcp_common::JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(2)),
            method: "tools/call".to_string(),
            params: Some(
                json!({
                    "name":"ozy_export_memory_notes",
                    "arguments": {
                        "procedural_rules": [
                            {"trigger": "empty_event", "action": "check len", "engram_block": "[TRIGGER: empty] -> [ACTION: check len]"}
                        ]
                    }
                }),
            ),
        };
        let export_resp = handle_request(&backend_ref, export_req, None, None).await.unwrap().unwrap();
        assert!(export_resp.error.is_none());
        let export_text = export_resp.result.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
        assert!(export_text.contains("[GIT_NOTES_EXPORTED]"));
        assert!(export_text.contains("Lessons: 1"));

        // 3. Clear local SQLite memory by removing .ozymem and reinitializing
        {
            let mut guard = backend_ref.lock().unwrap();
            *guard = None;
        }
        let _ = std::fs::remove_dir_all(tmp_root.join(".ozymem"));

        let reinit = mcp_common::JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(3)),
            method: "initialize".to_string(),
            params: Some(
                json!({"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"1"},"workspaceFolders":[{"uri":uri,"name":"test"}]}),
            ),
        };
        handle_request(&backend_ref, reinit, None, None).await.unwrap();

        {
            let guard = backend_ref.lock().unwrap();
            let gb = guard.as_ref().unwrap();
            let lessons = gb.recent_lessons(None, 10).await.unwrap();
            assert_eq!(lessons.len(), 0, "Lessons should be empty on fresh DB");
        }

        // 4. Import from git notes via MCP
        let import_req = mcp_common::JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(3)),
            method: "tools/call".to_string(),
            params: Some(
                json!({
                    "name":"ozy_import_memory_notes",
                    "arguments": {}
                }),
            ),
        };
        let import_resp = handle_request(&backend_ref, import_req, None, None).await.unwrap().unwrap();
        assert!(import_resp.error.is_none());
        let import_text = import_resp.result.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
        assert!(import_text.contains("[GIT_NOTES_IMPORTED]"));
        assert!(import_text.contains("Merged Lessons: 1"));
        assert!(import_text.contains("Restored Procedural Rules: 1"));

        // 5. Verify restored lessons
        {
            let guard = backend_ref.lock().unwrap();
            let gb = guard.as_ref().unwrap();
            let lessons = gb.recent_lessons(None, 10).await.unwrap();
            assert_eq!(lessons.len(), 1, "Lesson must be restored from git note");
            assert_eq!(lessons[0].symbol_name, "audit_log");
        }

        std::fs::remove_dir_all(&tmp_root).ok();
    }
