use ozymem_core::graph_backend::GraphBackend;
use ozymem_core::mcp_common;
use ozymem_core::McpBackend;
use serde_json::Value;
use std::sync::{Arc, Mutex};
use crate::state::{error_response, ok_response};

pub fn handle_completion_complete(
    backend: &Arc<Mutex<Option<GraphBackend>>>,
    id: Value,
    request: &mcp_common::JsonRpcRequest,
) -> anyhow::Result<Option<mcp_common::JsonRpcResponse>> {
    let raw = request
        .params
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("missing params"))?;
            let params: mcp_common::CompletionParams = match serde_json::from_value(raw.clone()) {
                Ok(p) => p,
                Err(e) => {
                    return Ok(Some(error_response(
                        id,
                        -32602,
                        &format!("Invalid params: {e}"),
                    )));
                }
            };
            let completion = match params.ref_param {
                mcp_common::CompletionRef::Prompt { name } => {
                    // Suggest argument values for a prompt
                    match name.as_str() {
                        "analyze-file" => {
                            if params.argument.name == "path" {
                                // Try to find files that match the partial path
                                let guard = backend.lock().unwrap();
                                if let Some(ref gb) = *guard {
                                    match gb.complete_file_path(&params.argument.value, 10) {
                                        Ok(values) => mcp_common::CompletionResult {
                                            completion: "".to_string(),
                                            values: Some(values),
                                            has_more: None,
                                            total: None,
                                        },
                                        Err(_) => mcp_common::CompletionResult {
                                            completion: "".to_string(),
                                            values: None,
                                            has_more: None,
                                            total: None,
                                        },
                                    }
                                } else {
                                    mcp_common::CompletionResult {
                                        completion: "".to_string(),
                                        values: None,
                                        has_more: None,
                                        total: None,
                                    }
                                }
                            } else {
                                mcp_common::CompletionResult {
                                    completion: "".to_string(),
                                    values: None,
                                    has_more: None,
                                    total: None,
                                }
                            }
                        }
                        "review-lessons" => {
                            if params.argument.name == "file_path" {
                                let guard = backend.lock().unwrap();
                                if let Some(ref gb) = *guard {
                                    match gb.complete_file_path(&params.argument.value, 10) {
                                        Ok(values) => mcp_common::CompletionResult {
                                            completion: "".to_string(),
                                            values: Some(values),
                                            has_more: None,
                                            total: None,
                                        },
                                        Err(_) => mcp_common::CompletionResult {
                                            completion: "".to_string(),
                                            values: None,
                                            has_more: None,
                                            total: None,
                                        },
                                    }
                                } else {
                                    mcp_common::CompletionResult {
                                        completion: "".to_string(),
                                        values: None,
                                        has_more: None,
                                        total: None,
                                    }
                                }
                            } else {
                                mcp_common::CompletionResult {
                                    completion: "".to_string(),
                                    values: None,
                                    has_more: None,
                                    total: None,
                                }
                            }
                        }
                        _ => mcp_common::CompletionResult {
                            completion: "".to_string(),
                            values: None,
                            has_more: None,
                            total: None,
                        },
                    }
                }
                mcp_common::CompletionRef::Resource { uri } => {
                    // For resources, try to complete the URI path
                    if uri == "ozymem://file/{path}" && params.argument.name == "path" {
                        let guard = backend.lock().unwrap();
                        if let Some(ref gb) = *guard {
                            match gb.complete_file_path(&params.argument.value, 10) {
                                Ok(values) => mcp_common::CompletionResult {
                                    completion: "".to_string(),
                                    values: Some(values),
                                    has_more: None,
                                    total: None,
                                },
                                Err(_) => mcp_common::CompletionResult {
                                    completion: "".to_string(),
                                    values: None,
                                    has_more: None,
                                    total: None,
                                },
                            }
                        } else {
                            mcp_common::CompletionResult {
                                completion: "".to_string(),
                                values: None,
                                has_more: None,
                                total: None,
                            }
                        }
                    } else {
                        mcp_common::CompletionResult {
                            completion: "".to_string(),
                            values: None,
                            has_more: None,
                            total: None,
                        }
                    }
                }
            };
            Ok(Some(ok_response(id, serde_json::to_value(completion)?)))
}

pub fn handle_prompts_list(
    id: Value,
) -> anyhow::Result<Option<mcp_common::JsonRpcResponse>> {
    let prompts = vec![
        mcp_common::PromptDefinition {
            name: "analyze-file".to_string(),
                    description: "Analyze a file: its context, dependency neighbors, transitive impact, and associated lessons.".to_string(),
                    arguments: Some(vec![
                        mcp_common::PromptArgument {
                            name: "path".to_string(),
                            description: "Absolute path of the file to analyze".to_string(),
                            required: Some(true),
                        },
                        mcp_common::PromptArgument {
                            name: "depth".to_string(),
                            description: "How deep to traverse transitive dependencies (default: 3)".to_string(),
                            required: Some(false),
                        },
                    ]),
                },
                mcp_common::PromptDefinition {
                    name: "review-lessons".to_string(),
                    description: "Review all lessons recorded for a file".to_string(),
                    arguments: Some(vec![
                        mcp_common::PromptArgument {
                            name: "file_path".to_string(),
                            description: "File path to review lessons for".to_string(),
                            required: Some(true),
                        },
                    ]),
                },
                mcp_common::PromptDefinition {
                    name: "project-status".to_string(),
                    description: "Project overview: summary, file count, and recent lessons".to_string(),
                    arguments: Some(vec![
                        mcp_common::PromptArgument {
                            name: "project_path".to_string(),
                            description: "Project root path (optional, defaults to active)".to_string(),
                            required: Some(false),
                        },
                    ]),
                },
            ];
            let result = mcp_common::ListPromptsResult {
                prompts,
                next_cursor: None,
            };
            Ok(Some(ok_response(id, serde_json::to_value(result)?)))
}

pub async fn handle_prompts_get(
    backend: &Arc<Mutex<Option<GraphBackend>>>,
    id: Value,
    request: &mcp_common::JsonRpcRequest,
) -> anyhow::Result<Option<mcp_common::JsonRpcResponse>> {
    let guard = backend.lock().unwrap();
    let Some(ref gb) = *guard else {
        return Ok(Some(error_response(id, -32000, "Server not initialized")));
    };
    let params: mcp_common::GetPromptParams = match request
        .params
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("missing params"))
        .and_then(|p| {
            serde_json::from_value(p.clone()).map_err(|e| anyhow::anyhow!("invalid params: {e}"))
        }) {
        Ok(p) => p,
        Err(e) => return Ok(Some(error_response(id, -32602, &e.to_string()))),
    };
    let response = match params.name.as_str() {
                "analyze-file" => {
                    let file_path = match params.arguments.get("path").and_then(Value::as_str) {
                        Some(p) => p,
                        None => {
                            return Ok(Some(error_response(
                                id,
                                -32602,
                                "Missing required argument: path",
                            )));
                        }
                    };
                    let depth = params
                        .arguments
                        .get("depth")
                        .and_then(Value::as_u64)
                        .unwrap_or(3) as u32;
                    gb.reload_if_stale();

                    let impacts = gb.analyze_impact(file_path, depth);
                    let ctx = gb.get_file_context(file_path).await.unwrap_or(None);
                    let lessons = gb.get_file_lessons(file_path).await.unwrap_or_default();
                    let history = gb
                        .get_historical_engram_solutions(file_path)
                        .await
                        .unwrap_or_default();

                    let mut text = format!("# Analysis of {}\n\n", file_path);
                    // Context section
                    text.push_str("## File Context\n\n");
                    if let Some(ref c) = ctx {
                        text.push_str(&format!("- Language: {}\n", c.language));
                        text.push_str(&format!("- Functions: {}\n\n", c.functions.len()));
                        for f in &c.functions {
                            text.push_str(&format!(
                                "  - `{}` [{}] L{}-L{}\n",
                                f.name, f.kind, f.start_line, f.end_line
                            ));
                        }
                    } else {
                        text.push_str("Not indexed.\n");
                    }
                    // Historical engrams
                    if !history.is_empty() {
                        text.push_str("\n## Historical Lessons\n\n");
                        for h in &history {
                            text.push_str(&format!("- {h}\n"));
                        }
                    }
                    // Impact section
                    text.push_str(&format!("\n## Impact Analysis (depth {depth})\n\n"));
                    if impacts.is_empty() {
                        text.push_str("No transitive impact found.\n");
                    } else {
                        for entry in &impacts {
                            text.push_str(&format!(
                                "- {} ({} funcs, {} lessons)\n",
                                entry.file_path, entry.function_count, entry.lesson_count
                            ));
                        }
                    }
                    // Lessons section
                    if !lessons.is_empty() {
                        text.push_str("\n## Lessons\n\n");
                        for entry in &lessons {
                            text.push_str(&format!(
                                "- [{}] {} :: {}\n  Context: {}\n  Solution: {}\n",
                                entry.kind,
                                entry.file_path,
                                entry.symbol_name,
                                entry.error_context,
                                entry.solution
                            ));
                        }
                    }

                    let result = mcp_common::GetPromptResult {
                        messages: vec![mcp_common::PromptMessage {
                            role: "assistant",
                            content: mcp_common::PromptContent::Text {
                                kind: "text".to_string(),
                                text,
                            },
                        }],
                        description: Some(format!("Analysis of {}", file_path)),
                    };
                    Ok(Some(ok_response(id, serde_json::to_value(result)?)))
                }
                "review-lessons" => {
                    let file_path = match params.arguments.get("file_path").and_then(Value::as_str)
                    {
                        Some(p) => p,
                        None => {
                            return Ok(Some(error_response(
                                id,
                                -32602,
                                "Missing required argument: file_path",
                            )));
                        }
                    };
                    let lessons = gb.get_file_lessons(file_path).await.unwrap_or_default();
                    let mut text = format!("# Lessons for {}\n\n", file_path);
                    if lessons.is_empty() {
                        text.push_str("No lessons recorded for this file.\n");
                    } else {
                        for (i, entry) in lessons.iter().enumerate() {
                            text.push_str(&format!("{}. [{}] {} :: {}\n   Context: {}\n   Solution: {}\n   Created: {}\n\n",
                                i + 1, entry.kind, entry.file_path, entry.symbol_name,
                                entry.error_context, entry.solution, entry.created_at));
                        }
                    }
                    let result = mcp_common::GetPromptResult {
                        messages: vec![mcp_common::PromptMessage {
                            role: "assistant",
                            content: mcp_common::PromptContent::Text {
                                kind: "text".to_string(),
                                text,
                            },
                        }],
                        description: Some(format!("Lesson review for {}", file_path)),
                    };
                    Ok(Some(ok_response(id, serde_json::to_value(result)?)))
                }
                "project-status" => {
                    gb.reload_if_stale();
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
                    let lessons = gb.recent_lessons(None, 10).await.unwrap_or_default();
                    let mut text = format!("# Project Status\n\n");
                    text.push_str(&format!("## Summary\n\n- Files: {}\n- Functions: {}\n- Lessons: {}\n- AST functions: {}\n- Text heuristic: {}\n\n",
                        summary.file_count, summary.function_count, summary.engram_count,
                        summary.native_ast_function_count, summary.text_heuristic_function_count));
                    if !lessons.is_empty() {
                        text.push_str("## Recent Lessons\n\n");
                        for (i, entry) in lessons.iter().enumerate() {
                            text.push_str(&format!(
                                "{}. [{}] {} :: {}\n   {}\n\n",
                                i + 1,
                                entry.kind,
                                entry.file_path,
                                entry.symbol_name,
                                entry.solution
                            ));
                        }
                    }
                    let result = mcp_common::GetPromptResult {
                        messages: vec![mcp_common::PromptMessage {
                            role: "assistant",
                            content: mcp_common::PromptContent::Text {
                                kind: "text".to_string(),
                                text,
                            },
                        }],
                        description: Some("Project status overview".to_string()),
                    };
                    Ok(Some(ok_response(id, serde_json::to_value(result)?)))
                }
                _ => Ok(Some(error_response(id, -32602, &format!("Unknown prompt: {}", params.name)))),
            };

    response
}
