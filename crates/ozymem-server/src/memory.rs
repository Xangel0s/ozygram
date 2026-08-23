use ozymem_core::graph_backend::GraphBackend;
use ozymem_core::mcp_common::{self, ContentBlock, ToolCallResult};
use ozymem_core::McpBackend;
use serde_json::Value;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use crate::formatters::{format_lessons_list, format_observations_list};
use crate::state::{error_response, notify_subscribed, ok_response, Notifier};

pub async fn handle_memory_tool(
    id: Value,
    backend: &GraphBackend,
    tool_call: &mcp_common::ToolCallParams,
    notifier: Option<&Notifier>,
    subscribed: Option<&Arc<Mutex<HashSet<String>>>>,
) -> anyhow::Result<Option<mcp_common::JsonRpcResponse>> {
    let res = match tool_call.name.as_str() {
                    "memory" => {
                        
                        let gb = backend;
                        gb.reload_if_stale();
                        let action = tool_call
                            .arguments
                            .get("action")
                            .and_then(Value::as_str)
                            .unwrap_or("search");
                        let kind = tool_call
                            .arguments
                            .get("kind")
                            .and_then(Value::as_str)
                            .unwrap_or("lesson");
                        if action == "record" {
                            let file_path = tool_call
                                .arguments
                                .get("file_path")
                                .and_then(Value::as_str)
                                .unwrap_or("");
                            let error_context = tool_call
                                .arguments
                                .get("error_context")
                                .and_then(Value::as_str)
                                .unwrap_or("");
                            let solution = tool_call
                                .arguments
                                .get("solution")
                                .and_then(Value::as_str)
                                .unwrap_or("");
                            gb.record_entry(file_path, None, error_context, solution, kind)
                                .await?;
                            ToolCallResult {
                                content: vec![ContentBlock {
                                    kind: "text",
                                    text: format!(
                                        "Record [{kind}] saved successfully for {file_path}"
                                    ),
                                }],
                                is_error: None,
                            }
                        } else {
                            let query = tool_call
                                .arguments
                                .get("query")
                                .and_then(Value::as_str)
                                .unwrap_or("");
                            let res: Vec<ozymem_core::graph_backend::LessonEntry> = gb.search_lessons(query, Some(kind), 20).await?;
                            let body = serde_json::to_string_pretty(&res)?;
                            ToolCallResult {
                                content: vec![ContentBlock {
                                    kind: "text",
                                    text: body,
                                }],
                                is_error: None,
                            }
                        }
                    }
                "ozy_memory" => {
                    let action = tool_call
                        .arguments
                        .get("action")
                        .and_then(Value::as_str)
                        .unwrap_or("search");
                    let kind = tool_call
                        .arguments
                        .get("kind")
                        .and_then(Value::as_str)
                        .unwrap_or("lesson");
                    match action {
                        "session_start" => {
                            let session_id = tool_call
                                .arguments
                                .get("session_id")
                                .and_then(Value::as_str)
                                .ok_or_else(|| anyhow::anyhow!("missing session_id"))?;
                            let project = tool_call
                                .arguments
                                .get("project")
                                .and_then(Value::as_str)
                                .unwrap_or("default");
                            let directory = tool_call
                                .arguments
                                .get("directory")
                                .and_then(Value::as_str)
                                .unwrap_or("");
                            backend.memory_session_start(session_id, project, directory)?;
                            ToolCallResult {
                                content: vec![ContentBlock {
                                    kind: "text",
                                    text: format!("Session started: {session_id} ({project})"),
                                }],
                                is_error: None,
                            }
                        }
                        "session_end" => {
                            let session_id = tool_call
                                .arguments
                                .get("session_id")
                                .and_then(Value::as_str)
                                .ok_or_else(|| anyhow::anyhow!("missing session_id"))?;
                            let summary =
                                tool_call.arguments.get("content").and_then(Value::as_str);
                            backend.memory_session_end(session_id, summary)?;
                            ToolCallResult {
                                content: vec![ContentBlock {
                                    kind: "text",
                                    text: format!("Session completed: {session_id}"),
                                }],
                                is_error: None,
                            }
                        }
                        "save" => {
                            let session_id = tool_call
                                .arguments
                                .get("session_id")
                                .and_then(Value::as_str)
                                .unwrap_or("manual");
                            let title = tool_call
                                .arguments
                                .get("title")
                                .and_then(Value::as_str)
                                .ok_or_else(|| anyhow::anyhow!("missing title"))?;
                            let content = tool_call
                                .arguments
                                .get("content")
                                .and_then(Value::as_str)
                                .ok_or_else(|| anyhow::anyhow!("missing content"))?;
                            let project =
                                tool_call.arguments.get("project").and_then(Value::as_str);
                            let scope = tool_call.arguments.get("scope").and_then(Value::as_str);
                            let topic_key =
                                tool_call.arguments.get("topic_key").and_then(Value::as_str);
                            backend.memory_session_start(
                                session_id,
                                project.unwrap_or("default"),
                                "",
                            )?;
                            let obs = backend.save_observation(
                                session_id,
                                kind,
                                title,
                                content,
                                project,
                                scope,
                                topic_key,
                                Some("ozy_memory"),
                            )?;
                            ToolCallResult {
                                content: vec![ContentBlock {
                                    kind: "text",
                                    text: format_observations_list(&[obs]),
                                }],
                                is_error: None,
                            }
                        }
                        "get" => {
                            let id = tool_call
                                .arguments
                                .get("id")
                                .and_then(Value::as_i64)
                                .ok_or_else(|| anyhow::anyhow!("missing id"))?;
                            ToolCallResult {
                                content: vec![ContentBlock {
                                    kind: "text",
                                    text: format_observations_list(&[backend.get_observation(id)?]),
                                }],
                                is_error: None,
                            }
                        }
                        "timeline" => {
                            let id = tool_call
                                .arguments
                                .get("id")
                                .and_then(Value::as_i64)
                                .ok_or_else(|| anyhow::anyhow!("missing id"))?;
                            let before = tool_call
                                .arguments
                                .get("before")
                                .and_then(Value::as_u64)
                                .unwrap_or(5) as usize;
                            let after = tool_call
                                .arguments
                                .get("after")
                                .and_then(Value::as_u64)
                                .unwrap_or(5) as usize;
                            ToolCallResult {
                                content: vec![ContentBlock {
                                    kind: "text",
                                    text: format_observations_list(
                                        &backend.observation_timeline(id, before, after)?,
                                    ),
                                }],
                                is_error: None,
                            }
                        }
                        "delete" => {
                            let id = tool_call
                                .arguments
                                .get("id")
                                .and_then(Value::as_i64)
                                .ok_or_else(|| anyhow::anyhow!("missing id"))?;
                            let deleted = backend.soft_delete_observation(id)?;
                            ToolCallResult {
                                content: vec![ContentBlock {
                                    kind: "text",
                                    text: format!("Observation {id} soft-deleted: {deleted}"),
                                }],
                                is_error: None,
                            }
                        }
                        "prompt" => {
                            let session_id = tool_call
                                .arguments
                                .get("session_id")
                                .and_then(Value::as_str)
                                .unwrap_or("manual");
                            let content = tool_call
                                .arguments
                                .get("content")
                                .and_then(Value::as_str)
                                .ok_or_else(|| anyhow::anyhow!("missing content"))?;
                            let project =
                                tool_call.arguments.get("project").and_then(Value::as_str);
                            backend.memory_session_start(
                                session_id,
                                project.unwrap_or("default"),
                                "",
                            )?;
                            let id = backend.save_user_prompt(session_id, content, project)?;
                            ToolCallResult {
                                content: vec![ContentBlock {
                                    kind: "text",
                                    text: format!("Prompt saved: {id}"),
                                }],
                                is_error: None,
                            }
                        }
                        "passive" => {
                            let session_id = tool_call
                                .arguments
                                .get("session_id")
                                .and_then(Value::as_str)
                                .unwrap_or("manual");
                            let content = tool_call
                                .arguments
                                .get("content")
                                .and_then(Value::as_str)
                                .ok_or_else(|| anyhow::anyhow!("missing content"))?;
                            let project =
                                tool_call.arguments.get("project").and_then(Value::as_str);
                            backend.memory_session_start(
                                session_id,
                                project.unwrap_or("default"),
                                "",
                            )?;
                            let saved = backend.passive_capture(session_id, content, project)?;
                            ToolCallResult {
                                content: vec![ContentBlock {
                                    kind: "text",
                                    text: format_observations_list(&saved),
                                }],
                                is_error: None,
                            }
                        }
                        "record" => {
                            let file_path = tool_call
                                .arguments
                                .get("file_path")
                                .and_then(Value::as_str)
                                .ok_or_else(|| anyhow::anyhow!("missing file_path"))?;
                            let symbol_name = tool_call
                                .arguments
                                .get("symbol_name")
                                .and_then(Value::as_str);
                            let context = tool_call
                                .arguments
                                .get("context")
                                .and_then(Value::as_str)
                                .unwrap_or("");
                            let content = tool_call
                                .arguments
                                .get("content")
                                .and_then(Value::as_str)
                                .unwrap_or("");
                            backend
                                .record_entry(file_path, symbol_name, context, content, kind)
                                .await?;
                            ToolCallResult {
                                content: vec![ContentBlock {
                                    kind: "text",
                                    text: format!("Recorded {kind} for {file_path}"),
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
                            ToolCallResult {
                                content: vec![ContentBlock {
                                    kind: "text",
                                    text: format_lessons_list(
                                        &backend.get_file_lessons(file_path).await?,
                                    ),
                                }],
                                is_error: None,
                            }
                        }
                        "symbol" => {
                            let file_path = tool_call
                                .arguments
                                .get("file_path")
                                .and_then(Value::as_str)
                                .ok_or_else(|| anyhow::anyhow!("missing file_path"))?;
                            let symbol_name = tool_call
                                .arguments
                                .get("symbol_name")
                                .and_then(Value::as_str)
                                .ok_or_else(|| anyhow::anyhow!("missing symbol_name"))?;
                            ToolCallResult {
                                content: vec![ContentBlock {
                                    kind: "text",
                                    text: format_lessons_list(
                                        &backend.get_symbol_lessons(file_path, symbol_name).await?,
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
                                .unwrap_or(10) as usize;
                            ToolCallResult {
                                content: vec![ContentBlock {
                                    kind: "text",
                                    text: format_lessons_list(
                                        &backend.recent_lessons(Some(kind), limit).await?,
                                    ),
                                }],
                                is_error: None,
                            }
                        }
                        "similar" => {
                            let query = tool_call
                                .arguments
                                .get("query")
                                .and_then(Value::as_str)
                                .ok_or_else(|| anyhow::anyhow!("missing query"))?;
                            let limit = tool_call
                                .arguments
                                .get("limit")
                                .and_then(Value::as_u64)
                                .unwrap_or(10) as usize;
                            let min_score = tool_call
                                .arguments
                                .get("min_score")
                                .and_then(Value::as_f64)
                                .unwrap_or(0.5) as f32;
                            let results = backend.similar_lessons(query, limit, min_score)?;
                            let mut text = format!("Similar memories for {query}:\n");
                            for (i, sl) in results.iter().enumerate() {
                                text.push_str(&format!(
                                    "{}. [score {:.3}] [{}] {} :: {}\n   {}\n",
                                    i + 1,
                                    sl.score,
                                    sl.lesson.kind,
                                    sl.lesson.file_path,
                                    sl.lesson.symbol_name,
                                    sl.lesson.solution
                                ));
                            }
                            ToolCallResult {
                                content: vec![ContentBlock { kind: "text", text }],
                                is_error: None,
                            }
                        }
                        _ => {
                            let query = tool_call
                                .arguments
                                .get("query")
                                .and_then(Value::as_str)
                                .unwrap_or("");
                            let limit = tool_call
                                .arguments
                                .get("limit")
                                .and_then(Value::as_u64)
                                .unwrap_or(10) as usize;
                            let wants_observations = tool_call.arguments.contains_key("project")
                                || tool_call.arguments.contains_key("scope")
                                || tool_call.arguments.contains_key("topic_key")
                                || !matches!(
                                    kind,
                                    "lesson" | "decision" | "convention" | "gotcha" | "module_rule"
                                );
                            if wants_observations {
                                let project =
                                    tool_call.arguments.get("project").and_then(Value::as_str);
                                let scope =
                                    tool_call.arguments.get("scope").and_then(Value::as_str);
                                let observation_type = if tool_call.arguments.contains_key("kind") {
                                    Some(kind)
                                } else {
                                    None
                                };
                                ToolCallResult {
                                    content: vec![ContentBlock {
                                        kind: "text",
                                        text: format_observations_list(
                                            &backend.search_observations(
                                                query,
                                                project,
                                                scope,
                                                observation_type,
                                                limit,
                                            )?,
                                        ),
                                    }],
                                    is_error: None,
                                }
                            } else {
                                ToolCallResult {
                                    content: vec![ContentBlock {
                                        kind: "text",
                                        text: format_lessons_list(
                                            &backend
                                                .search_lessons(query, Some(kind), limit)
                                                .await?,
                                        ),
                                    }],
                                    is_error: None,
                                }
                            }
                        }
                    }
                }
                "record_lesson" => {
                    let file_path = tool_call
                        .arguments
                        .get("file_path")
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow::anyhow!("missing file_path"))?;
                    let symbol_name = tool_call
                        .arguments
                        .get("symbol_name")
                        .and_then(Value::as_str);
                    let error_context = tool_call
                        .arguments
                        .get("error_context")
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow::anyhow!("missing error_context"))?;
                    let solution = tool_call
                        .arguments
                        .get("solution")
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow::anyhow!("missing solution"))?;

                    backend
                        .record_lesson(file_path, symbol_name, error_context, solution)
                        .await?;
                    notify_subscribed(
                        subscribed,
                        notifier,
                        &["ozymem://summary", "ozymem://recent-lessons"],
                    );
                    ToolCallResult {
                        content: vec![ContentBlock {
                            kind: "text",
                            text: format!(
                                "Recorded lesson for {}{}",
                                file_path,
                                symbol_name.map(|s| format!("::{}", s)).unwrap_or_default()
                            ),
                        }],
                        is_error: None,
                    }
                }
                "search_lessons" => {
                    let query = tool_call
                        .arguments
                        .get("query")
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow::anyhow!("missing query"))?;
                    let kind = tool_call.arguments.get("kind").and_then(Value::as_str);
                    let limit = tool_call
                        .arguments
                        .get("limit")
                        .and_then(Value::as_u64)
                        .unwrap_or(10) as usize;

                    let results = backend.search_lessons(query, kind, limit).await?;
                    let mut body = String::new();
                    if results.is_empty() {
                        body = format!("No results for \"{query}\"");
                    } else {
                        for (i, entry) in results.iter().enumerate() {
                            body.push_str(&format!(
                                "{}. [{}] {} :: {}\n   context: {}\n   solution: {}\n",
                                i + 1,
                                entry.kind,
                                entry.file_path,
                                entry.symbol_name,
                                entry.error_context,
                                entry.solution
                            ));
                        }
                    }
                    ToolCallResult {
                        content: vec![ContentBlock {
                            kind: "text",
                            text: body,
                        }],
                        is_error: None,
                    }
                }
                "get_file_lessons" => {
                    let file_path = tool_call
                        .arguments
                        .get("file_path")
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow::anyhow!("missing file_path"))?;
                    let results = backend.get_file_lessons(file_path).await?;
                    let body = format_lessons_list(&results);
                    ToolCallResult {
                        content: vec![ContentBlock {
                            kind: "text",
                            text: body,
                        }],
                        is_error: None,
                    }
                }
                "get_symbol_lessons" => {
                    let file_path = tool_call
                        .arguments
                        .get("file_path")
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow::anyhow!("missing file_path"))?;
                    let symbol_name = tool_call
                        .arguments
                        .get("symbol_name")
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow::anyhow!("missing symbol_name"))?;
                    let results = backend.get_symbol_lessons(file_path, symbol_name).await?;
                    let body = format_lessons_list(&results);
                    ToolCallResult {
                        content: vec![ContentBlock {
                            kind: "text",
                            text: body,
                        }],
                        is_error: None,
                    }
                }
                "recent_lessons" => {
                    let kind = tool_call.arguments.get("kind").and_then(Value::as_str);
                    let limit = tool_call
                        .arguments
                        .get("limit")
                        .and_then(Value::as_u64)
                        .unwrap_or(10) as usize;
                    let results = backend.recent_lessons(kind, limit).await?;
                    let body = format_lessons_list(&results);
                    ToolCallResult {
                        content: vec![ContentBlock {
                            kind: "text",
                            text: body,
                        }],
                        is_error: None,
                    }
                }
                "similar_lessons" => {
                    let query = match tool_call.arguments.get("query").and_then(Value::as_str) {
                        Some(q) => q,
                        None => {
                            return Ok(Some(error_response(
                                id,
                                -32602,
                                "Missing required parameter: query",
                            )));
                        }
                    };
                    let limit = tool_call
                        .arguments
                        .get("limit")
                        .and_then(Value::as_u64)
                        .unwrap_or(10) as usize;
                    let min_score = tool_call
                        .arguments
                        .get("min_score")
                        .and_then(Value::as_f64)
                        .unwrap_or(0.5) as f32;
                    let kind_filter = tool_call.arguments.get("kind").and_then(Value::as_str);
                    match backend.similar_lessons(query, limit, min_score) {
                        Ok(results) => {
                            let filtered: Vec<&ozymem_core::graph_backend::SimilarLesson> =
                                if let Some(kind) = kind_filter {
                                    results.iter().filter(|r| r.lesson.kind == kind).collect()
                                } else {
                                    results.iter().collect()
                                };
                            if filtered.is_empty() {
                                ToolCallResult {
                                    content: vec![ContentBlock {
                                        kind: "text",
                                        text: format!(
                                            "No semantically similar lessons found for \"{query}\"{}",
                                            kind_filter
                                                .map_or(String::new(), |k| format!(" (kind: {k})"))
                                        ),
                                    }],
                                    is_error: None,
                                }
                            } else {
                                let mut body = format!(
                                    "Lessons semantically similar to \"{query}\" (min score {min_score}):\n"
                                );
                                for (i, sl) in filtered.iter().enumerate() {
                                    body.push_str(&format!(
                                        "\n{}. [score: {:.3}] [{}] {} :: {}\n   context: {}\n   solution: {}\n   created: {}\n",
                                        i + 1, sl.score, sl.lesson.kind, sl.lesson.file_path, sl.lesson.symbol_name,
                                        sl.lesson.error_context, sl.lesson.solution, sl.lesson.created_at,
                                    ));
                                }
                                ToolCallResult {
                                    content: vec![ContentBlock {
                                        kind: "text",
                                        text: body,
                                    }],
                                    is_error: None,
                                }
                            }
                        }
                        Err(e) => {
                            return Ok(Some(error_response(
                                id,
                                -32603,
                                &format!("similar_lessons error: {e}"),
                            )));
                        }
                    }
                }
                "record_decision" => {
                    let file_path = tool_call
                        .arguments
                        .get("file_path")
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow::anyhow!("missing file_path"))?;
                    let symbol_name = tool_call
                        .arguments
                        .get("symbol_name")
                        .and_then(Value::as_str);
                    let context = tool_call
                        .arguments
                        .get("context")
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow::anyhow!("missing context"))?;
                    let decision = tool_call
                        .arguments
                        .get("decision")
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow::anyhow!("missing decision"))?;
                    backend
                        .record_entry(file_path, symbol_name, context, decision, "decision")
                        .await?;
                    notify_subscribed(
                        subscribed,
                        notifier,
                        &["ozymem://summary", "ozymem://recent-lessons"],
                    );
                    ToolCallResult {
                        content: vec![ContentBlock {
                            kind: "text",
                            text: format!("Recorded decision for {file_path}"),
                        }],
                        is_error: None,
                    }
                }
                "record_convention" => {
                    let file_path = tool_call
                        .arguments
                        .get("file_path")
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow::anyhow!("missing file_path"))?;
                    let symbol_name = tool_call
                        .arguments
                        .get("symbol_name")
                        .and_then(Value::as_str);
                    let context = tool_call
                        .arguments
                        .get("context")
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow::anyhow!("missing context"))?;
                    let convention = tool_call
                        .arguments
                        .get("convention")
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow::anyhow!("missing convention"))?;
                    backend
                        .record_entry(file_path, symbol_name, context, convention, "convention")
                        .await?;
                    notify_subscribed(
                        subscribed,
                        notifier,
                        &["ozymem://summary", "ozymem://recent-lessons"],
                    );
                    ToolCallResult {
                        content: vec![ContentBlock {
                            kind: "text",
                            text: format!("Recorded convention for {file_path}"),
                        }],
                        is_error: None,
                    }
                }
                "record_gotcha" => {
                    let file_path = tool_call
                        .arguments
                        .get("file_path")
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow::anyhow!("missing file_path"))?;
                    let symbol_name = tool_call
                        .arguments
                        .get("symbol_name")
                        .and_then(Value::as_str);
                    let context = tool_call
                        .arguments
                        .get("context")
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow::anyhow!("missing context"))?;
                    let gotcha = tool_call
                        .arguments
                        .get("gotcha")
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow::anyhow!("missing gotcha"))?;
                    backend
                        .record_entry(file_path, symbol_name, context, gotcha, "gotcha")
                        .await?;
                    notify_subscribed(
                        subscribed,
                        notifier,
                        &["ozymem://summary", "ozymem://recent-lessons"],
                    );
                    ToolCallResult {
                        content: vec![ContentBlock {
                            kind: "text",
                            text: format!("Recorded gotcha for {file_path}"),
                        }],
                        is_error: None,
                    }
                }
                "record_module_rule" => {
                    let file_path = tool_call
                        .arguments
                        .get("file_path")
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow::anyhow!("missing file_path"))?;
                    let context = tool_call
                        .arguments
                        .get("context")
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow::anyhow!("missing context"))?;
                    let rule = tool_call
                        .arguments
                        .get("rule")
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow::anyhow!("missing rule"))?;
                    backend
                        .record_entry(file_path, None, context, rule, "module_rule")
                        .await?;
                    notify_subscribed(
                        subscribed,
                        notifier,
                        &["ozymem://summary", "ozymem://recent-lessons"],
                    );
                    ToolCallResult {
                        content: vec![ContentBlock {
                            kind: "text",
                            text: format!("Recorded module rule for {file_path}"),
                        }],
                        is_error: None,
                    }
                }
        _ => return Ok(None),
    };
    Ok(Some(ok_response(id, serde_json::to_value(res)?)))
}
