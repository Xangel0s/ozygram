use ozymem_core::graph_backend::GraphBackend;
use ozymem_core::McpBackend;
use ozymem_core::mcp_common::{self, ContentBlock, ToolCallResult};
use ozymem_core::registry::ProjectRegistry;
use serde_json::Value;
use std::path::Path;
use crate::formatters::days_since_str;
use crate::packages::handle_package_tool;
use crate::state::{Notifier, error_response, ok_response};

pub(crate) async fn handle_ozy_project_readonly(
    tool_call: &mcp_common::ToolCallParams,
) -> anyhow::Result<ToolCallResult> {
    let action = tool_call
        .arguments
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or("list");
    let delegated_tool = match action {
        "memories" => Some("get_project_memories"),
        "delete" => Some("delete_project"),
        "dependencies" => Some("get_dependencies"),
        "verify_dependencies" => Some("verify_dependencies"),
        _ => None,
    };
    if let Some(name) = delegated_tool {
        let delegated = mcp_common::ToolCallParams {
            name: name.to_string(),
            arguments: tool_call.arguments.clone(),
        };
        let response = if matches!(action, "dependencies" | "verify_dependencies") {
            handle_package_tool(Value::Null, &delegated, None).await?
        } else {
            handle_project_tool(Value::Null, &delegated, None).await?
        };
        return tool_result_from_response(response);
    }
    let reg = ozymem_core::registry::ProjectRegistry::open()?;
    let payload = match action {
        "stale" => {
            let days = tool_call
                .arguments
                .get("days")
                .and_then(Value::as_u64)
                .unwrap_or(90) as i64;
            let stale = reg.get_stale_projects(days)?;
            serde_json::json!({"action":"stale","days":days,"projects":stale})
        }
        _ => serde_json::json!({"action":"list","projects":reg.list_projects()?}),
    };
    Ok(ToolCallResult {
        content: vec![ContentBlock {
            kind: "text",
            text: serde_json::to_string_pretty(&payload)?,
        }],
        is_error: None,
    })
}

pub(crate) fn tool_result_from_response(
    response: mcp_common::JsonRpcResponse,
) -> anyhow::Result<ToolCallResult> {
    if let Some(error) = response.error {
        return Ok(ToolCallResult {
            content: vec![ContentBlock {
                kind: "text",
                text: error.message,
            }],
            is_error: Some(true),
        });
    }
    let Some(result) = response.result else {
        return Ok(ToolCallResult {
            content: vec![ContentBlock {
                kind: "text",
                text: "Tool returned no result".to_string(),
            }],
            is_error: Some(true),
        });
    };
    let is_error = result.get("is_error").and_then(Value::as_bool);
    let content = result
        .get("content")
        .and_then(Value::as_array)
        .map(|blocks| {
            blocks
                .iter()
                .filter_map(|block| {
                    block
                        .get("text")
                        .and_then(Value::as_str)
                        .map(|text| ContentBlock {
                            kind: "text",
                            text: text.to_string(),
                        })
                })
                .collect::<Vec<_>>()
        })
        .filter(|blocks| !blocks.is_empty())
        .unwrap_or_else(|| {
            vec![ContentBlock {
                kind: "text",
                text: result.to_string(),
            }]
        });
    Ok(ToolCallResult { content, is_error })
}


pub(crate) async fn handle_project_tool(
    id: Value,
    tool_call: &mcp_common::ToolCallParams,
    notifier: Option<&Notifier>,
) -> anyhow::Result<mcp_common::JsonRpcResponse> {
    let log = |level: &str, msg: String| {
        if let Some(ref n) = notifier {
            n.log(level, msg);
        }
    };

    match tool_call.name.as_str() {
        "list_projects" => {
            let reg = ProjectRegistry::open()?;
            let projects = reg.list_projects()?;
            let mut body = format!("Registered projects ({}):\n\n", projects.len());
            for p in &projects {
                let db_path = Path::new(&p.path).join(".ozymem").join("memory.db");
                let lesson_count = if db_path.exists() {
                    match GraphBackend::open(Some(&db_path.to_string_lossy())) {
                        Ok(gb) => gb.lesson_count().unwrap_or(0),
                        Err(_) => 0,
                    }
                } else {
                    0
                };
                let status = p.status.as_str();
                let last = p.last_opened.as_deref().unwrap_or("never");
                body.push_str(&format!(
                    "  {} [{}]\n    Path: {}\n    Files: {}, Lessons: {}\n    Last activity: {}\n\n",
                    p.name, status, p.path, p.file_count, lesson_count, last,
                ));
            }
            Ok(ok_response(
                id,
                serde_json::to_value(ToolCallResult {
                    content: vec![ContentBlock {
                        kind: "text",
                        text: body,
                    }],
                    is_error: None,
                })?,
            ))
        }

        "get_project_memories" => {
            let project_name = match tool_call
                .arguments
                .get("project_name")
                .and_then(Value::as_str)
            {
                Some(n) => n,
                None => {
                    return Ok(error_response(
                        id,
                        -32602,
                        "Missing required parameter: project_name",
                    ));
                }
            };
            let query = tool_call.arguments.get("query").and_then(Value::as_str);
            let kind = tool_call.arguments.get("kind").and_then(Value::as_str);
            let limit = tool_call
                .arguments
                .get("limit")
                .and_then(Value::as_u64)
                .unwrap_or(20) as usize;

            let reg = ProjectRegistry::open()?;
            let project = match reg.get_project_by_name(project_name)? {
                Some(p) => p,
                None => {
                    return Ok(error_response(
                        id,
                        -32602,
                        &format!("Project '{}' not found in registry", project_name),
                    ));
                }
            };
            let db_path = Path::new(&project.path).join(".ozymem").join("memory.db");
            if !db_path.exists() {
                return Ok(error_response(
                    id,
                    -32602,
                    &format!(
                        "No memory.db found for project '{}' at {}",
                        project_name,
                        db_path.display()
                    ),
                ));
            }

            let gb = GraphBackend::open(Some(&db_path.to_string_lossy()))?;
            gb.set_project_path(Some(&project.path));

            let lessons = if let Some(q) = query {
                gb.search_lessons(q, kind, limit).await?
            } else {
                gb.recent_lessons(kind, limit).await?
            };

            let mut body = format!(
                "Memories for '{}' ({} entries):\n\n",
                project_name,
                lessons.len()
            );
            if lessons.is_empty() {
                body.push_str("No lessons, decisions, conventions, gotchas, or module rules recorded for this project.\n");
            } else {
                for (i, entry) in lessons.iter().enumerate() {
                    let stale_tag = if entry.stale != 0 {
                        format!(
                            " [STALE: {}]",
                            entry.stale_reason.as_deref().unwrap_or("unknown")
                        )
                    } else {
                        String::new()
                    };
                    body.push_str(&format!(
                        "{}. [{}]{} {} :: {}\n   context: {}\n   solution: {}\n   created: {}\n\n",
                        i + 1,
                        entry.kind,
                        stale_tag,
                        entry.file_path,
                        entry.symbol_name,
                        entry.error_context,
                        entry.solution,
                        entry.created_at,
                    ));
                }
            }

            Ok(ok_response(
                id,
                serde_json::to_value(ToolCallResult {
                    content: vec![ContentBlock {
                        kind: "text",
                        text: body,
                    }],
                    is_error: None,
                })?,
            ))
        }

        "delete_project" => {
            let project_name = match tool_call
                .arguments
                .get("project_name")
                .and_then(Value::as_str)
            {
                Some(n) => n,
                None => {
                    return Ok(error_response(
                        id,
                        -32602,
                        "Missing required parameter: project_name",
                    ));
                }
            };
            let force = tool_call
                .arguments
                .get("force")
                .and_then(Value::as_bool)
                .unwrap_or(false);

            let reg = ProjectRegistry::open()?;
            let project = match reg.get_project_by_name(project_name)? {
                Some(p) => p,
                None => {
                    return Ok(error_response(
                        id,
                        -32602,
                        &format!("Project '{}' not found in registry", project_name),
                    ));
                }
            };

            let db_path = Path::new(&project.path).join(".ozymem").join("memory.db");
            let gb = GraphBackend::open(Some(&db_path.to_string_lossy()))?;
            gb.set_project_path(Some(&project.path));
            let lesson_count = gb.lesson_count().unwrap_or(0);

            if !force {
                // Preview mode
                let summary = gb
                    .get_graph_summary()
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
                let days_since = days_since_str(project.last_opened.as_deref());

                let body = format!(
                    "Project '{}' [{}]\n  Path: {}\n  Files: {}, Functions: {}, Lessons: {}\n  Last activity: {} ({} days ago)\n  The {} lessons will be KEPT in memory.db.\n\nUse force=true to delete project data but preserve learnings.",
                    project.name,
                    project.status.as_str(),
                    project.path,
                    summary.file_count,
                    summary.function_count,
                    lesson_count,
                    project.last_opened.as_deref().unwrap_or("never"),
                    days_since,
                    lesson_count,
                );
                return Ok(ok_response(
                    id,
                    serde_json::to_value(ToolCallResult {
                        content: vec![ContentBlock {
                            kind: "text",
                            text: body,
                        }],
                        is_error: None,
                    })?,
                ));
            }

            // Execute deletion
            log(
                "info",
                format!(
                    "[ozymem-server] deleting project '{}' (keeping lessons)",
                    project_name
                ),
            );

            // 1. Clear indexed data (files, functions, dependencies) – lessons remain
            gb.clear_data()?;

            // 2. Deregister from ProjectRegistry
            let removed = reg.deregister(project_name)?;
            if removed {
                let body = format!(
                    "Project '{}' deregistered. {lesson_count} lessons preserved in memory.db at {}.",
                    project_name,
                    db_path.display(),
                );
                log("info", format!("[ozymem-server] {}", &body));
                Ok(ok_response(
                    id,
                    serde_json::to_value(ToolCallResult {
                        content: vec![ContentBlock {
                            kind: "text",
                            text: body,
                        }],
                        is_error: None,
                    })?,
                ))
            } else {
                Ok(error_response(
                    id,
                    -32603,
                    &format!("Failed to deregister project '{}'", project_name),
                ))
            }
        }

        "suggest_stale_projects" => {
            let days = tool_call
                .arguments
                .get("days")
                .and_then(Value::as_u64)
                .unwrap_or(90) as i64;

            let reg = ProjectRegistry::open()?;
            let stale = reg.get_stale_projects(days)?;

            if stale.is_empty() {
                let body = format!("No projects have been SLEEPING for more than {days} days.");
                return Ok(ok_response(
                    id,
                    serde_json::to_value(ToolCallResult {
                        content: vec![ContentBlock {
                            kind: "text",
                            text: body,
                        }],
                        is_error: None,
                    })?,
                ));
            }

            let mut body = format!(
                "Stale projects (SLEEPING > {} days, {} found):\n\n",
                days,
                stale.len()
            );
            for p in &stale {
                let db_path = Path::new(&p.path).join(".ozymem").join("memory.db");
                let lesson_count = if db_path.exists() {
                    match GraphBackend::open(Some(&db_path.to_string_lossy())) {
                        Ok(gb) => gb.lesson_count().unwrap_or(0),
                        Err(_) => 0,
                    }
                } else {
                    0
                };
                let days_since = days_since_str(p.last_opened.as_deref());

                body.push_str(&format!(
                    "  {} [{}]\n    Path: {}\n    Files: {}, Lessons: {}\n    Last activity: {} ({} days ago)\n    Use `delete_project` with force=true to deregister\n\n",
                    p.name,
                    p.status.as_str(),
                    p.path,
                    p.file_count,
                    lesson_count,
                    p.last_opened.as_deref().unwrap_or("never"),
                    days_since,
                ));
            }

            Ok(ok_response(
                id,
                serde_json::to_value(ToolCallResult {
                    content: vec![ContentBlock {
                        kind: "text",
                        text: body,
                    }],
                    is_error: None,
                })?,
            ))
        }

        _ => Ok(error_response(
            id,
            -32601,
            "Unknown project management tool",
        )),
    }
}

// ---------------------------------------------------------------------------
// Package management tools handler
// ---------------------------------------------------------------------------

