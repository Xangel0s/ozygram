use ozymem_core::graph_backend::GraphBackend;
use ozymem_core::mcp_common::{self, ContentBlock, ToolCallResult};
use ozymem_core::McpBackend;
use serde_json::Value;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use crate::doctor::build_code_doctor_report;
use crate::formatters::{build_architecture_report, get_last_commit, build_ozy_project_context, build_ozy_task_context, format_file_context_enriched, format_impact, format_lessons_list, format_summary};
use crate::projects::handle_ozy_project_readonly;
use crate::state::{error_response, ok_response, resolve_project_root, Notifier};

pub async fn handle_unified_tool(
    id: Value,
    backend: &GraphBackend,
    tool_call: &mcp_common::ToolCallParams,
    notifier: Option<&Notifier>,
    _subscribed: Option<&Arc<Mutex<HashSet<String>>>>,
) -> anyhow::Result<Option<mcp_common::JsonRpcResponse>> {
    let res = match tool_call.name.as_str() {
                    "search" => {
                        
                        let gb = backend;
                        gb.reload_if_stale();
                        let query = tool_call
                            .arguments
                            .get("query")
                            .and_then(Value::as_str)
                            .ok_or_else(|| anyhow::anyhow!("missing query"))?;
                        let scope = tool_call.arguments.get("scope").and_then(Value::as_str);
                        let mode = tool_call.arguments.get("mode").and_then(Value::as_str);
                        let limit = tool_call
                            .arguments
                            .get("limit")
                            .and_then(Value::as_u64)
                            .unwrap_or(20) as usize;

                        let res: Vec<ozymem_core::graph_backend::UnifiedSearchResult> = gb.unified_search(query, scope, mode, limit).await?;
                        let body = serde_json::to_string_pretty(&res)?;
                        ToolCallResult {
                            content: vec![ContentBlock {
                                kind: "text",
                                text: body,
                            }],
                            is_error: None,
                        }
                    }
                    "verify_contracts" => {
                        
                        let gb = backend;
                        gb.reload_if_stale();
                        let report = gb.verify_export_contracts()?;
                        let body = serde_json::to_string_pretty(&report)?;
                        ToolCallResult {
                            content: vec![ContentBlock {
                                kind: "text",
                                text: body,
                            }],
                            is_error: None,
                        }
                    }
                    "context" => {
                        
                        let gb = backend;
                        gb.reload_if_stale();
                        let file_path = tool_call
                            .arguments
                            .get("file_path")
                            .and_then(Value::as_str)
                            .ok_or_else(|| anyhow::anyhow!("missing file_path"))?;
                        let ctx = gb.get_file_context(file_path).await?;
                        let history: Vec<String> = gb.get_historical_engram_solutions(file_path).await?;
                        let neighbor_info = gb.get_graph_neighbors(file_path).await.ok();
                        let last_git = get_last_commit(file_path).await;

                        let body = format_file_context_enriched(
                            ctx.as_ref(),
                            file_path,
                            &history,
                            neighbor_info.as_ref(),
                            last_git.as_deref(),
                        );
                        ToolCallResult {
                            content: vec![ContentBlock {
                                kind: "text",
                                text: body,
                            }],
                            is_error: None,
                        }
                    }
                    "impact" => {
                        
                        let gb = backend;
                        gb.reload_if_stale();
                        let file_path = tool_call
                            .arguments
                            .get("file_path")
                            .and_then(Value::as_str)
                            .ok_or_else(|| anyhow::anyhow!("missing file_path"))?;
                        let depth = tool_call
                            .arguments
                            .get("depth")
                            .and_then(Value::as_u64)
                            .unwrap_or(3) as u32;

                        let impacts = gb.analyze_impact(file_path, depth);
                        let body = format_impact(&impacts, file_path);
                        ToolCallResult {
                            content: vec![ContentBlock {
                                kind: "text",
                                text: body,
                            }],
                            is_error: None,
                        }
                    }
                "ozy_context" => {
                    backend.reload_if_stale();
                    let action = tool_call
                        .arguments
                        .get("action")
                        .and_then(Value::as_str)
                        .unwrap_or("task");
                    match action {
                        "summary" => {
                            let subpath = tool_call
                                .arguments
                                .get("subpath")
                                .or_else(|| tool_call.arguments.get("scope"))
                                .and_then(Value::as_str);
                            let summary = if let Some(sub) = subpath {
                                backend.get_subpath_graph_summary(sub)?
                            } else {
                                backend.get_graph_summary().await?
                            };
                            ToolCallResult {
                                content: vec![ContentBlock {
                                    kind: "text",
                                    text: format_summary(&summary),
                                }],
                                is_error: None,
                            }
                        }
                        "files" => {
                            let files = backend.list_all_files()?;
                            ToolCallResult {
                                content: vec![ContentBlock {
                                    kind: "text",
                                    text: format!(
                                        "Indexed files ({}):\n\n{}",
                                        files.len(),
                                        files.join("\n")
                                    ),
                                }],
                                is_error: None,
                            }
                        }
                        "recent" => {
                            let limit = tool_call
                                .arguments
                                .get("limit")
                                .and_then(Value::as_u64)
                                .unwrap_or(20) as usize;
                            let results = backend.recent_lessons(None, limit).await?;
                            ToolCallResult {
                                content: vec![ContentBlock {
                                    kind: "text",
                                    text: format_lessons_list(&results),
                                }],
                                is_error: None,
                            }
                        }
                        "file" => {
                            let file_path = tool_call
                                .arguments
                                .get("file_path")
                                .and_then(Value::as_str)
                                .ok_or_else(|| anyhow::anyhow!("missing file_path"))?;
                            let ctx = backend.get_file_context(file_path).await?;
                            let history =
                                backend.get_historical_engram_solutions(file_path).await?;
                            let neighbors = backend.get_graph_neighbors(file_path).await.ok();
                            let last_git = get_last_commit(file_path).await;
                            ToolCallResult {
                                content: vec![ContentBlock {
                                    kind: "text",
                                    text: format_file_context_enriched(
                                        ctx.as_ref(),
                                        file_path,
                                        &history,
                                        neighbors.as_ref(),
                                        last_git.as_deref(),
                                    ),
                                }],
                                is_error: None,
                            }
                        }
                        "project" => {
                            let project_name = tool_call
                                .arguments
                                .get("project_name")
                                .and_then(Value::as_str);
                            let query = tool_call.arguments.get("query").and_then(Value::as_str);
                            let limit = tool_call
                                .arguments
                                .get("limit")
                                .and_then(Value::as_u64)
                                .unwrap_or(10) as usize;
                            let max_tokens = tool_call
                                .arguments
                                .get("max_tokens")
                                .and_then(Value::as_u64)
                                .unwrap_or(4000)
                                as usize;
                            let body =
                                build_ozy_project_context(project_name, query, limit, max_tokens)
                                    .await?;
                            ToolCallResult {
                                content: vec![ContentBlock {
                                    kind: "text",
                                    text: body,
                                }],
                                is_error: None,
                            }
                        }
                        _ => {
                            let query = tool_call
                                .arguments
                                .get("query")
                                .and_then(Value::as_str)
                                .unwrap_or("project context");
                            let max_tokens = tool_call
                                .arguments
                                .get("max_tokens")
                                .and_then(Value::as_u64)
                                .unwrap_or(4000)
                                as usize;
                            let body = build_ozy_task_context(backend, query, max_tokens).await?;
                            ToolCallResult {
                                content: vec![ContentBlock {
                                    kind: "text",
                                    text: body,
                                }],
                                is_error: None,
                            }
                        }
                    }
                }
                "ozy_graph" => {
                    backend.reload_if_stale();
                    let action = tool_call
                        .arguments
                        .get("action")
                        .and_then(Value::as_str)
                        .unwrap_or("summary");
                    let body = match action {
                        "neighbors" => {
                            let file_path = tool_call
                                .arguments
                                .get("file_path")
                                .and_then(Value::as_str)
                                .ok_or_else(|| anyhow::anyhow!("missing file_path"))?;
                            let info = backend.get_graph_neighbors(file_path).await?;
                            serde_json::to_string_pretty(&info)?
                        }
                        "impact" => {
                            let file_path = tool_call
                                .arguments
                                .get("file_path")
                                .and_then(Value::as_str)
                                .ok_or_else(|| anyhow::anyhow!("missing file_path"))?;
                            let depth = tool_call
                                .arguments
                                .get("depth")
                                .and_then(Value::as_u64)
                                .unwrap_or(3) as u32;
                            format_impact(&backend.analyze_impact(file_path, depth), file_path)
                        }
                        "path" => {
                            let from = tool_call
                                .arguments
                                .get("from")
                                .and_then(Value::as_str)
                                .ok_or_else(|| anyhow::anyhow!("missing from"))?;
                            let to = tool_call
                                .arguments
                                .get("to")
                                .and_then(Value::as_str)
                                .ok_or_else(|| anyhow::anyhow!("missing to"))?;
                            serde_json::to_string_pretty(
                                &backend.find_graph_path(
                                    from,
                                    to,
                                    tool_call
                                        .arguments
                                        .get("max_paths")
                                        .and_then(Value::as_u64)
                                        .unwrap_or(1) as usize,
                                    tool_call
                                        .arguments
                                        .get("max_hops")
                                        .and_then(Value::as_u64)
                                        .unwrap_or(10) as usize,
                                ),
                            )?
                        }
                        "architecture_report" => build_architecture_report(backend).await?,
                        "api_routes" => {
                            let file_path = tool_call.arguments.get("file_path").and_then(Value::as_str);
                            let routes = backend.map_api_routes(file_path)?;
                            serde_json::to_string_pretty(&routes)?
                        }
                        _ => {
                            let subpath = tool_call
                                .arguments
                                .get("subpath")
                                .or_else(|| tool_call.arguments.get("scope"))
                                .and_then(Value::as_str);
                            let summary = if let Some(sub) = subpath {
                                backend.get_subpath_graph_summary(sub)?
                            } else {
                                backend.get_graph_summary().await?
                            };
                            format_summary(&summary)
                        }
                    };
                    ToolCallResult {
                        content: vec![ContentBlock {
                            kind: "text",
                            text: body,
                        }],
                        is_error: None,
                    }
                }
                "ozy_code_doctor" => {
                    backend.reload_if_stale();
                    let min_lines = tool_call
                        .arguments
                        .get("min_duplicate_lines")
                        .and_then(Value::as_u64)
                        .unwrap_or(6) as usize;
                    let max_findings = tool_call
                        .arguments
                        .get("max_findings")
                        .and_then(Value::as_u64)
                        .unwrap_or(20) as usize;
                    let scope = tool_call.arguments.get("scope").and_then(Value::as_str);
                    let report =
                        build_code_doctor_report(backend, scope, min_lines, max_findings).await?;
                    ToolCallResult {
                        content: vec![ContentBlock {
                            kind: "text",
                            text: report,
                        }],
                        is_error: None,
                    }
                }
                "ozy_project" => {
                    let action = tool_call
                        .arguments
                        .get("action")
                        .and_then(Value::as_str)
                        .unwrap_or("list");
                    match action {
                        "refresh" => {
                            let project_path = tool_call
                                .arguments
                                .get("project_path")
                                .and_then(Value::as_str)
                                .map(|s| s.to_string())
                                .or_else(|| backend.project_path());
                            let resolved = resolve_project_root(project_path, None)
                                .map_err(|m| anyhow::anyhow!(m))?;
                            let resolved_str = resolved.to_string_lossy().to_string();
                            backend.full_scan(&resolved_str, None)?;
                            ToolCallResult {
                                content: vec![ContentBlock {
                                    kind: "text",
                                    text: format!(
                                        "Project index refreshed for {}",
                                        resolved.display()
                                    ),
                                }],
                                is_error: None,
                            }
                        }
                        "create_ignore" => {
                            let patterns = match tool_call
                                .arguments
                                .get("patterns")
                                .and_then(Value::as_array)
                            {
                                Some(arr) => arr
                                    .iter()
                                    .filter_map(|v| v.as_str().map(String::from))
                                    .collect::<Vec<_>>(),
                                None => {
                                    return Ok(Some(error_response(
                                        id,
                                        -32602,
                                        "Missing required parameter: patterns",
                                    )));
                                }
                            };
                            if patterns.is_empty() {
                                return Ok(Some(error_response(
                                    id,
                                    -32602,
                                    "patterns must not be empty",
                                )));
                            }
                            let explicit_path = tool_call
                                .arguments
                                .get("project_path")
                                .and_then(Value::as_str)
                                .map(|s| s.to_string());
                            let resolved = match resolve_project_root(explicit_path.clone(), None) {
                                Ok(p) => p,
                                Err(_) if explicit_path.is_none() => match backend.project_path() {
                                    Some(p) => PathBuf::from(p),
                                    None => {
                                        return Ok(Some(error_response(
                                            id,
                                            -32602,
                                            "No project path set. Provide project_path or ensure initialize was sent with workspaceFolders.",
                                        )));
                                    }
                                },
                                Err(msg) => return Ok(Some(error_response(id, -32602, &msg))),
                            };
                            let ozymemignore_path = resolved.join(".ozymignore");
                            let mut existing = Vec::new();
                            if ozymemignore_path.exists() {
                                if let Ok(content) = std::fs::read_to_string(&ozymemignore_path) {
                                    for line in content.lines() {
                                        let t = line.trim();
                                        if !t.is_empty() && !t.starts_with('#') {
                                            existing.push(t.to_string());
                                        }
                                    }
                                }
                            }
                            let mut added = Vec::new();
                            for p in &patterns {
                                if !existing.contains(p) {
                                    existing.push(p.clone());
                                    added.push(p.clone());
                                }
                            }
                            let mut content = String::from(
                                "# OzyMem ignore patterns\n# Lines starting with # are comments\n\n",
                            );
                            for p in &existing {
                                content.push_str(p);
                                content.push('\n');
                            }
                            std::fs::write(&ozymemignore_path, content)?;
                            let resolved_str = resolved.to_string_lossy().to_string();
                            let progress = tool_call.arguments.get("_meta").and_then(|m| m.get("progressToken"));
                            backend.full_scan(
                                &resolved_str,
                                Some(&|processed, total| {
                                    if let Some(ref n) = notifier {
                                        if let Some(ref pt) = progress {
                                            n.progress(pt, processed, Some(total));
                                        }
                                    }
                                }),
                            )?;
                            ToolCallResult {
                                content: vec![ContentBlock {
                                    kind: "text",
                                    text: format!(
                                        "Created/updated .ozymignore at {}.\n  Total patterns: {}\n  Newly added: {}\n  The project has been re-scanned ignoring these patterns.",
                                        ozymemignore_path.display(),
                                        existing.len(),
                                        if added.is_empty() {
                                            "none (all already present)".to_string()
                                        } else {
                                            added.join(", ")
                                        },
                                    ),
                                }],
                                is_error: None,
                            }
                        }
                        _ => handle_ozy_project_readonly(&tool_call).await?,
                    }
                }
        _ => return Ok(None),
    };
    Ok(Some(ok_response(id, serde_json::to_value(res)?)))
}
