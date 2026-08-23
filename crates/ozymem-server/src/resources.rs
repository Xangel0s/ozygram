use ozymem_core::graph_backend::GraphBackend;
use ozymem_core::mcp_common;
use ozymem_core::McpBackend;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use crate::state::{error_response, ok_response};

pub async fn handle_resources_list(
    backend: &Arc<Mutex<Option<GraphBackend>>>,
    id: Value,
    request: &mcp_common::JsonRpcRequest,
) -> anyhow::Result<Option<mcp_common::JsonRpcResponse>> {
    let guard = backend.lock().unwrap();
            let Some(ref gb) = *guard else {
                return Ok(Some(error_response(
                    id,
                    -32000,
                    "Server not initialized: send 'initialize' with workspaceFolders before listing resources",
                )));
            };
            let project_path = gb.project_path().unwrap_or_default();
            let resources = vec![
                mcp_common::ResourceDescription {
                    uri: "ozymem://summary".to_string(),
                    name: "Project Summary".to_string(),
                    description: "Indexed project overview with file, function, and lesson counts.".to_string(),
                    mime_type: Some("application/json".to_string()),
                    size: None,
                },
                mcp_common::ResourceDescription {
                    uri: format!("ozymem://file/{project_path}"),
                    name: "File Context".to_string(),
                    description: "Context for a specific indexed file (replace {path} with absolute file path). Use resource templates to read individual files.".to_string(),
                    mime_type: Some("application/json".to_string()),
                    size: None,
                },
                mcp_common::ResourceDescription {
                    uri: "ozymem://recent-lessons".to_string(),
                    name: "Recent Lessons".to_string(),
                    description: "Most recently recorded lessons, decisions, conventions, gotchas, and module rules.".to_string(),
                    mime_type: Some("application/json".to_string()),
                    size: None,
                },
                mcp_common::ResourceDescription {
                    uri: "ozymem://full-context".to_string(),
                    name: "Full Project Context".to_string(),
                    description: "Bundled context: project summary, all file paths, and recent lessons.".to_string(),
                    mime_type: Some("application/json".to_string()),
                    size: None,
                },
            ];
            // Pagination: only 3 resources, no cursor needed but support the protocol
            let _cursor = request
                .params
                .as_ref()
                .and_then(|p| p.get("cursor"))
                .and_then(Value::as_str);
            let result = mcp_common::ListResourcesResult {
                resources,
                next_cursor: None,
            };
            Ok(Some(ok_response(id, serde_json::to_value(result)?)))
}

pub async fn handle_resources_read(
    backend: &Arc<Mutex<Option<GraphBackend>>>,
    id: Value,
    request: &mcp_common::JsonRpcRequest,
) -> anyhow::Result<Option<mcp_common::JsonRpcResponse>> {
    let guard = backend.lock().unwrap();
            let Some(ref gb) = *guard else {
                return Ok(Some(error_response(id, -32000, "Server not initialized")));
            };
            let params: mcp_common::ReadResourceParams = match request
                .params.as_ref()
                .ok_or_else(|| anyhow::anyhow!("missing params"))
                .and_then(|p| {
                    serde_json::from_value(p.clone()).map_err(|e| anyhow::anyhow!("invalid params: {e}"))
                }) {
                Ok(p) => p,
                Err(e) => return Ok(Some(error_response(id, -32602, &e.to_string()))),
            };
            let uri = params.uri.clone();
            let contents = match uri.as_str() {
                "ozymem://summary" => match gb.get_graph_summary().await {
                    Ok(summary) => vec![mcp_common::ResourceContents::Text {
                        uri: params.uri,
                        mime_type: "application/json".to_string(),
                        text: serde_json::to_string_pretty(&summary)?,
                    }],
                    Err(e) => {
                        return Ok(Some(error_response(
                            id,
                            -32603,
                            &format!("failed to read summary: {e}"),
                        )));
                    }
                },
                "ozymem://recent-lessons" => match gb.recent_lessons(None, 20).await {
                    Ok(lessons) => vec![mcp_common::ResourceContents::Text {
                        uri: params.uri,
                        mime_type: "application/json".to_string(),
                        text: serde_json::to_string_pretty(&lessons)?,
                    }],
                    Err(e) => {
                        return Ok(Some(error_response(
                            id,
                            -32603,
                            &format!("failed to read lessons: {e}"),
                        )));
                    }
                },
                "ozymem://full-context" => {
                    let summary =
                        gb.get_graph_summary()
                            .await
                            .unwrap_or(ozymem_core::GraphSummary {
                                file_count: 0,
                                function_count: 0,
                                engram_count: 0,
                                native_ast_function_count: 0,
                                extension_wasm_function_count: 0,
                                text_heuristic_function_count: 0,
                                vertex_count: 0,
                                edge_count: 0,
                                memory_usage: String::new(),
                                lessons_without_embedding: 0,
                            });
                    let files = gb.list_all_files().unwrap_or_default();
                    let lessons = gb.recent_lessons(None, 50).await.unwrap_or_default();
                    let context = serde_json::json!({
                        "summary": summary,
                        "file_count": files.len(),
                        "files": files,
                        "recent_lessons": lessons,
                    });
                    vec![mcp_common::ResourceContents::Text {
                        uri: params.uri,
                        mime_type: "application/json".to_string(),
                        text: serde_json::to_string_pretty(&context)?,
                    }]
                }
                _ if uri.starts_with("ozymem://file/") => {
                    let file_path = &uri["ozymem://file/".len()..];
                    gb.reload_if_stale();
                    match gb.get_file_context(file_path).await {
                        Ok(Some(ctx)) => vec![mcp_common::ResourceContents::Text {
                            uri: params.uri,
                            mime_type: "application/json".to_string(),
                            text: serde_json::to_string_pretty(&ctx)?,
                        }],
                        Ok(None) => vec![mcp_common::ResourceContents::Text {
                            uri: params.uri,
                            mime_type: "text/plain".to_string(),
                            text: format!("No indexed file found for {file_path}"),
                        }],
                        Err(e) => {
                            return Ok(Some(error_response(
                                id,
                                -32603,
                                &format!("failed to read file context: {e}"),
                            )));
                        }
                    }
                }
                _ => {
                    return Ok(Some(error_response(
                        id,
                        -32602,
                        &format!("Unknown resource URI: {}", params.uri),
                    )));
                }
            };
            let result = mcp_common::ReadResourceResult { contents };
            Ok(Some(ok_response(id, serde_json::to_value(result)?)))
}

pub fn handle_resource_templates(
    id: Value,
) -> anyhow::Result<Option<mcp_common::JsonRpcResponse>> {
    let templates = vec![
        mcp_common::ResourceTemplate {
            uri_template: "ozymem://file/{path}".to_string(),
            name: "File Context by Path".to_string(),
                    description: "Context for a specific indexed file. Replace {path} with the absolute file path.".to_string(),
                    mime_type: Some("application/json".to_string()),
                },
                mcp_common::ResourceTemplate {
                    uri_template: "ozymem://file/{path}/neighbors".to_string(),
                    name: "File Graph Neighbors".to_string(),
                    description: "Dependency neighbors (incoming and outgoing) for a specific indexed file.".to_string(),
                    mime_type: Some("application/json".to_string()),
                },
            ];
            let result = mcp_common::ListResourceTemplatesResult {
                resource_templates: templates,
            };
            Ok(Some(ok_response(id, serde_json::to_value(result)?)))
}

pub fn handle_resource_subscribe(
    id: Value,
    request: &mcp_common::JsonRpcRequest,
    subscribed: Option<&Arc<Mutex<HashSet<String>>>>,
) -> anyhow::Result<Option<mcp_common::JsonRpcResponse>> {
    let res = if let Some(sub) = subscribed {
        let uri = request
            .params
            .as_ref()
            .and_then(|p| p.get("uri"))
            .and_then(Value::as_str)
            .map(|s| s.to_string());
        if let Some(uri) = uri {
            sub.lock().unwrap().insert(uri);
        }
        ok_response(id, json!({}))
    } else {
        error_response(id, -32000, "Subscriptions not available")
    };
    Ok(Some(res))
}

pub fn handle_resource_unsubscribe(
    id: Value,
    request: &mcp_common::JsonRpcRequest,
    subscribed: Option<&Arc<Mutex<HashSet<String>>>>,
) -> anyhow::Result<Option<mcp_common::JsonRpcResponse>> {
    let res = if let Some(sub) = subscribed {
        let uri = request
            .params
            .as_ref()
            .and_then(|p| p.get("uri"))
            .and_then(Value::as_str)
            .map(|s| s.to_string());
        if let Some(uri) = uri {
            sub.lock().unwrap().remove(&uri);
        }
        ok_response(id, json!({}))
    } else {
        error_response(id, -32000, "Subscriptions not available")
    };
    Ok(Some(res))
}
