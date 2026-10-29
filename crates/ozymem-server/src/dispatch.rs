use ozymem_core::graph_backend::{legacy_global_db_path, GraphBackend};
use ozymem_core::mcp_common::{self, ContentBlock, ToolCallResult};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::brain::handle_ozy_brain;
use crate::doctor::handle_ozy_doctor;
use crate::packages::handle_package_tool;
use crate::projects::handle_project_tool;
use crate::skills::handle_ozy_skills;
use crate::state::{error_response, log_spawn, ok_response, Notifier};
use crate::tools::handle_lookup_engram;
use crate::verifier::handle_verify_diff;
use crate::exploration::handle_exploration;

use crate::git::handle_git_tool;
use crate::graph::handle_graph_tool;
use crate::memory::handle_memory_tool;
use crate::prompts::{handle_completion_complete, handle_prompts_get, handle_prompts_list};
use crate::resources::{
    handle_resource_subscribe, handle_resource_templates, handle_resource_unsubscribe,
    handle_resources_list, handle_resources_read,
};
use crate::schemas::handle_tools_list;
use crate::unified::handle_unified_tool;

pub async fn handle_request(
    backend: &Arc<Mutex<Option<GraphBackend>>>,
    request: mcp_common::JsonRpcRequest,
    notifier: Option<&Notifier>,
    subscribed: Option<&Arc<Mutex<HashSet<String>>>>,
) -> anyhow::Result<Option<mcp_common::JsonRpcResponse>> {
    crate::idle_watcher::touch_activity();

    let log = |level: &str, msg: String| {
        if let Some(n) = notifier {
            n.log(level, msg);
        }
    };

    if request.jsonrpc != "2.0" {
        return Ok(Some(error_response(
            request.id.unwrap_or(Value::Null),
            -32600,
            "Invalid JSON-RPC version",
        )));
    }

    let Some(ref id_ref) = request.id else {
        return Ok(None);
    };
    let id = id_ref.clone();

    let response = match request.method.as_str() {
        "initialize" => {
            let project_path = crate::state::resolve_workspace_from_params(request.params.as_ref())
                .or_else(|| {
                    if let Ok(cwd) = std::env::current_dir() {
                        if crate::state::is_ide_install_dir(&cwd) {
                            None
                        } else {
                            crate::state::find_project_root_from(&cwd).or(Some(cwd))
                        }
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| PathBuf::from("."));

            let mut guard = backend.lock().unwrap();
            let gb = match GraphBackend::open_for_project(&project_path) {
                Ok(b) => b,
                Err(e) => {
                    log("error", format!("[ozymem] Failed to open DB at {}: {e}", project_path.display()));
                    return Ok(Some(error_response(
                        id,
                        -32603,
                        &format!("Failed to open DB: {e}"),
                    )));
                }
            };
            *guard = Some(gb);
            drop(guard);

            let legacy_db = legacy_global_db_path();
            if legacy_db.exists() && legacy_db.is_file() {
                let project_db = project_path.join(".ozymem").join("memory.db");
                if project_db.exists() {
                    eprintln!(
                        "[ozymem] Legacy global DB detected at {}. OzyMem now uses per-project DB at {}. Run 'ozymem migrate' to copy lessons, or delete the old file.",
                        legacy_db.display(),
                        project_db.display()
                    );
                }
            }

            let result = mcp_common::InitializeResult {
                protocol_version: "2024-11-05",
                capabilities: mcp_common::ServerCapabilities {
                    tools: mcp_common::ToolsCapability {
                        list_changed: Some(true),
                    },
                    resources: Some(mcp_common::ResourcesCapability {
                        subscribe: Some(true),
                        list_changed: Some(true),
                    }),
                    prompts: Some(mcp_common::PromptsCapability {
                        list_changed: Some(true),
                    }),
                    logging: Some(mcp_common::LoggingCapability {}),
                    sampling: Some(mcp_common::SamplingCapability {}),
                    completions: Some(mcp_common::CompletionsCapability {}),
                },
                server_info: mcp_common::ServerInfo {
                    name: "ozymem-server",
                    version: env!("CARGO_PKG_VERSION"),
                },
            };

            let init_msg = format!(
                "ozymem-server v{} initialized for project: {}",
                env!("CARGO_PKG_VERSION"),
                project_path.display()
            );
            log("info", init_msg);
            if let Some(n) = notifier {
                n.log("info", format!("[ozymem] Ready (project DB at {})", project_path.display()));
            }

            let gb_warm = backend.clone();
            tokio::task::spawn_blocking(move || {
                let guard = gb_warm.lock().unwrap();
                if let Some(ref gb) = *guard {
                    let status = gb.get_embedding_status();
                    if status != ozymem_core::graph_backend::EmbeddingModelStatus::Ready {
                        gb.start_background_embedder_download();
                    }
                }
            });

            let backend_clone = backend.clone();
            let notifier_clone = notifier.cloned();
            let project_path_scan = project_path.to_string_lossy().to_string();
            tokio::task::spawn_blocking(move || {
                let guard = backend_clone.lock().unwrap();
                if let Some(ref gb) = *guard {
                    let progress_cb = notifier_clone.as_ref().map(|n| {
                        let n = n.clone();
                        Box::new(move |cur: u64, tot: u64| {
                            n.progress(&json!("init"), cur, Some(tot));
                        }) as Box<dyn Fn(u64, u64) + Send + Sync>
                    });
                    let cb_ref = progress_cb.as_ref().map(|cb| cb.as_ref() as &dyn Fn(u64, u64));
                    if let Err(e) = gb.full_scan(&project_path_scan, cb_ref) {
                        eprintln!("[ozymem] background scan error: {e}");
                    }
                }
            });

            // Live filesystem watcher: auto-reindex file modifications from external editors/scripts
            if std::env::var("OZYMEM_DISABLE_WATCHER").is_err() {
                let backend_weak = Arc::downgrade(backend);
                let watcher_root = project_path.clone();
                tokio::task::spawn_blocking(move || {
                    match ozymem_core::LiveWatcher::watch(&watcher_root, Some(std::time::Duration::from_millis(300))) {
                        Ok(watcher) => {
                            eprintln!("[ozymem-watcher] Live filesystem watcher started for {}", watcher_root.display());
                            loop {
                                let Some(backend_arc) = backend_weak.upgrade() else {
                                    // Backend was dropped, exit watcher loop immediately
                                    break;
                                };
                                if let Some(event) = watcher.recv_timeout(std::time::Duration::from_millis(200)) {
                                    match event {
                                        ozymem_core::DeltaFileEvent::Modified(p) => {
                                            if !ozymem_core::graph_backend::helpers::is_noise_path(&p) {
                                                let guard = backend_arc.lock().unwrap();
                                                if let Some(ref gb) = *guard {
                                                    let _ = gb.index_file_delta(&p, &watcher_root);
                                                }
                                            }
                                        }
                                        ozymem_core::DeltaFileEvent::Removed(p) => {
                                            if !ozymem_core::graph_backend::helpers::is_noise_path(&p) {
                                                let guard = backend_arc.lock().unwrap();
                                                if let Some(ref gb) = *guard {
                                                    let _ = gb.remove_file_delta(&p, &watcher_root);
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("[ozymem-watcher] Info: watcher skipped or unavailable for {}: {e}", watcher_root.display());
                        }
                    }
                });
            }

            Some(ok_response(id, serde_json::to_value(result)?))
        }
        "notifications/initialized" => return Ok(None),
        "tools/list" => return handle_tools_list(id, &request),
        "tools/call" => {
            let params: mcp_common::ToolCallParams = match request.params {
                Some(p) => serde_json::from_value(p)?,
                None => {
                    return Ok(Some(error_response(
                        id,
                        -32602,
                        "Missing params for tools/call",
                    )));
                }
            };
            let tool_call = params;
            log_spawn("info", format!("[ozymem] tool call: {}", tool_call.name), &notifier.cloned());

            // Project lifecycle tools
            if matches!(
                tool_call.name.as_str(),
                "list_projects" | "get_project_memories" | "delete_project" | "suggest_stale_projects"
            ) {
                return handle_project_tool(id, &tool_call, notifier).await.map(Some);
            }

            // Package management tools
            if matches!(
                tool_call.name.as_str(),
                "create_project"
                    | "add_package"
                    | "remove_package"
                    | "get_dependencies"
                    | "run_script"
                    | "analyze_package"
                    | "verify_dependencies"
            ) {
                return handle_package_tool(id, &tool_call, notifier).await.map(Some);
            }

            if tool_call.name == "ozy_skills" {
                let res = handle_ozy_skills(&tool_call).await?;
                return Ok(Some(ok_response(id, serde_json::to_value(res)?)));
            }

            let guard = backend.lock().unwrap();
            let Some(ref backend_guard) = *guard else {
                return Ok(Some(error_response(
                    id,
                    -32000,
                    "Server not initialized: send 'initialize' with workspaceFolders first",
                )));
            };
            let backend = backend_guard;

            let result: ToolCallResult = match tool_call.name.as_str() {
                "lookup_engram" | "ozy_lookup_engram" | "ozymem_lookup_engram" => {
                    handle_lookup_engram(backend, &tool_call)?
                }
                "ozy_verify_diff" | "verify_diff" => {
                    handle_verify_diff(backend, &tool_call)?
                }
                "ozy_doctor" => {
                    let action = tool_call
                        .arguments
                        .get("action")
                        .and_then(Value::as_str)
                        .unwrap_or("status");
                    if action == "audit_migrations" {
                        let migrations_path = tool_call
                            .arguments
                            .get("migrations_path")
                            .and_then(Value::as_str);
                        let report = crate::sql_linter::handle_audit_migrations(
                            Some(backend),
                            migrations_path,
                        )?;
                        ToolCallResult {
                            content: vec![ContentBlock { kind: "text", text: report }],
                            is_error: None,
                        }
                    } else if action == "code" || action == "code_doctor" || action == "code_health" {
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
                            crate::doctor::build_code_doctor_report(backend, scope, min_lines, max_findings).await?;
                        ToolCallResult {
                            content: vec![ContentBlock {
                                kind: "text",
                                text: report,
                            }],
                            is_error: None,
                        }
                    } else {
                        handle_ozy_doctor(Some(backend), &tool_call).await?
                    }
                }
                "ozy_brain" => {
                    let has_action = tool_call.arguments.get("action").is_some();
                    let has_goal = tool_call.arguments.get("goal").is_some() || tool_call.arguments.get("query").is_some();
                    if !has_action && !has_goal {
                        ToolCallResult {
                            content: vec![ContentBlock {
                                kind: "text",
                                text: "[OZY_BRAIN] Invocación sin parámetros. Especifica una acción y un objetivo.\n\
                                       Acciones disponibles: plan, reflect, risk_review, deep_semantic_search, dream_rsi.\n\
                                       Ejemplo: ozy_brain(action=\"plan\", goal=\"Tu objetivo aquí\")".to_string(),
                            }],
                            is_error: Some(true),
                        }
                    } else {
                        let action = tool_call
                            .arguments
                            .get("action")
                            .and_then(Value::as_str)
                            .unwrap_or("plan");
                        let report = handle_ozy_brain(backend, &tool_call, action).await?;
                        let is_error = if report.starts_with("[ERROR:") { Some(true) } else { None };
                        ToolCallResult {
                            content: vec![ContentBlock {
                                kind: "text",
                                text: report,
                            }],
                            is_error,
                        }
                    }
                }
                "deep_semantic_search" | "ozy_deep_search" => {
                    let report = handle_ozy_brain(backend, &tool_call, "deep_semantic_search").await?;
                    ToolCallResult {
                        content: vec![ContentBlock {
                            kind: "text",
                            text: report,
                        }],
                        is_error: None,
                    }
                }
                "ozy_exploration" | "exploration" | "record_exploration_step" | "get_exploration_tree" => {
                    match handle_exploration(backend, &tool_call) {
                        Ok(res) => res,
                        Err(e) => ToolCallResult {
                            content: vec![ContentBlock {
                                kind: "text",
                                text: serde_json::to_string_pretty(&json!({
                                    "status": "error",
                                    "error": e.to_string()
                                })).unwrap_or_else(|_| format!("Error: {e}")),
                            }],
                            is_error: Some(true),
                        },
                    }
                }
                "ozy_get_symbol" | "get_symbol" | "ozymem_get_symbol" => {
                    match crate::symbols::handle_get_symbol(backend, &tool_call) {
                        Ok(res) => res,
                        Err(e) => ToolCallResult {
                            content: vec![ContentBlock {
                                kind: "text",
                                text: format!("[ALERT: ERROR] {e}"),
                            }],
                            is_error: Some(true),
                        },
                    }
                }
                "ozy_file_tree" | "file_tree" | "ast_tree" => {
                    match crate::symbols::handle_file_tree(backend, &tool_call) {
                        Ok(res) => res,
                        Err(e) => ToolCallResult {
                            content: vec![ContentBlock {
                                kind: "text",
                                text: format!("[ALERT: ERROR] {e}"),
                            }],
                            is_error: Some(true),
                        },
                    }
                }
                "ozy_parse" | "parse_file" | "ozymem_parse" => {
                    match crate::symbols::handle_parse(backend, &tool_call) {
                        Ok(res) => res,
                        Err(e) => ToolCallResult {
                            content: vec![ContentBlock {
                                kind: "text",
                                text: format!("[ALERT: ERROR] {e}"),
                            }],
                            is_error: Some(true),
                        },
                    }
                }
                                "ozy_patch" | "patch" => {
                    let action = tool_call.arguments.get("action").and_then(Value::as_str).unwrap_or("apply");
                    match action {
                        "simulate" => {
                            match crate::symbols::handle_simulate_patch(backend, &tool_call) {
                                Ok(res) => res,
                                Err(e) => ToolCallResult {
                                    content: vec![ContentBlock {
                                        kind: "text",
                                        text: format!("[ALERT: ERROR] {e}"),
                                    }],
                                    is_error: Some(true),
                                },
                            }
                        }
                        "replace_symbol" => {
                            match crate::symbols::handle_replace_symbol(backend, &tool_call) {
                                Ok(res) => res,
                                Err(e) => ToolCallResult {
                                    content: vec![ContentBlock {
                                        kind: "text",
                                        text: format!("[ALERT: ERROR] {e}"),
                                    }],
                                    is_error: Some(true),
                                },
                            }
                        }
                        _ => {
                            match crate::symbols::handle_ast_patch(backend, &tool_call) {
                                Ok(res) => res,
                                Err(e) => ToolCallResult {
                                    content: vec![ContentBlock {
                                        kind: "text",
                                        text: format!("[ALERT: ERROR] {e}"),
                                    }],
                                    is_error: Some(true),
                                },
                            }
                        }
                    }
                }
                "ozy_plan" | "plan" => {
                    let goal_opt = tool_call.arguments.get("goal").and_then(Value::as_str);
                    if goal_opt.is_none() {
                        ToolCallResult {
                            content: vec![ContentBlock {
                                kind: "text",
                                text: "[ALERT: BAD_REQUEST] 'goal' es requerido para ozy_plan.".to_string(),
                            }],
                            is_error: Some(true),
                        }
                    } else {
                        match handle_ozy_brain(backend, &tool_call, "plan").await {
                            Ok(report_str) => ToolCallResult {
                                content: vec![ContentBlock {
                                    kind: "text",
                                    text: report_str,
                                }],
                                is_error: None,
                            },
                            Err(e) => ToolCallResult {
                                content: vec![ContentBlock {
                                    kind: "text",
                                    text: format!("[ALERT: ERROR] {e}"),
                                }],
                                is_error: Some(true),
                            },
                        }
                    }
                }
                "ozy_ast_patch" | "ast_patch" => {
                    match crate::symbols::handle_ast_patch(backend, &tool_call) {
                        Ok(res) => res,
                        Err(e) => ToolCallResult {
                            content: vec![ContentBlock {
                                kind: "text",
                                text: format!("[ALERT: ERROR] {e}"),
                            }],
                            is_error: Some(true),
                        },
                    }
                }
                "ozy_replace_symbol" | "replace_symbol" | "ozymem_replace_symbol" => {
                    match crate::symbols::handle_replace_symbol(backend, &tool_call) {
                        Ok(res) => res,
                        Err(e) => ToolCallResult {
                            content: vec![ContentBlock {
                                kind: "text",
                                text: format!("[ALERT: ERROR] {e}"),
                            }],
                            is_error: Some(true),
                        },
                    }
                }
                "ozy_diagnostics_quick" | "diagnostics_quick" | "check_syntax" => {
                    match crate::symbols::handle_diagnostics_quick(backend, &tool_call) {
                        Ok(res) => res,
                        Err(e) => ToolCallResult {
                            content: vec![ContentBlock {
                                kind: "text",
                                text: format!("[ALERT: ERROR] {e}"),
                            }],
                            is_error: Some(true),
                        },
                    }
                }
                "ozy_search_literal" | "search_literal" => {
                    match crate::symbols::handle_search_literal(backend, &tool_call) {
                        Ok(res) => res,
                        Err(e) => ToolCallResult {
                            content: vec![ContentBlock {
                                kind: "text",
                                text: format!("[ALERT: ERROR] {e}"),
                            }],
                            is_error: Some(true),
                        },
                    }
                }
                "ozy_find_references" | "find_references" => {
                    match crate::symbols::handle_find_references(backend, &tool_call) {
                        Ok(res) => res,
                        Err(e) => ToolCallResult {
                            content: vec![ContentBlock {
                                kind: "text",
                                text: format!("[ALERT: ERROR] {e}"),
                            }],
                            is_error: Some(true),
                        },
                    }
                }
                "ozy_view_skeleton" | "view_skeleton" | "skeleton" => {
                    match crate::symbols::handle_view_skeleton(backend, &tool_call) {
                        Ok(res) => res,
                        Err(e) => ToolCallResult {
                            content: vec![ContentBlock {
                                kind: "text",
                                text: format!("[ALERT: ERROR] {e}"),
                            }],
                            is_error: Some(true),
                        },
                    }
                }
                "ozy_simulate_patch" | "simulate_patch" => {
                    match crate::symbols::handle_simulate_patch(backend, &tool_call) {
                        Ok(res) => res,
                        Err(e) => ToolCallResult {
                            content: vec![ContentBlock {
                                kind: "text",
                                text: format!("[ALERT: ERROR] {e}"),
                            }],
                            is_error: Some(true),
                        },
                    }
                }
                _ => {
                    if let Some(res) = handle_memory_tool(id.clone(), backend, &tool_call, notifier, subscribed).await? {
                        return Ok(Some(res));
                    } else if let Some(res) = handle_git_tool(id.clone(), backend, &tool_call, notifier, subscribed).await? {
                        return Ok(Some(res));
                    } else if let Some(res) = handle_unified_tool(id.clone(), backend, &tool_call, notifier, subscribed).await? {
                        return Ok(Some(res));
                    } else if let Some(res) = handle_graph_tool(id.clone(), backend, &tool_call, notifier, subscribed).await? {
                        return Ok(Some(res));
                    } else {
                        return Ok(Some(error_response(id, -32601, "Unknown tool")));
                    }
                }
            };

            Some(ok_response(id, serde_json::to_value(result)?))
        }
        "resources/list" => return handle_resources_list(backend, id, &request).await,
        "resources/read" => return handle_resources_read(backend, id, &request).await,
        "resources/templates/list" => return handle_resource_templates(id),
        "resources/subscribe" => return handle_resource_subscribe(id, &request, subscribed),
        "resources/unsubscribe" => return handle_resource_unsubscribe(id, &request, subscribed),
        "completion/complete" | "completions/complete" => return handle_completion_complete(backend, id, &request),
        "prompts/list" => return handle_prompts_list(id),
        "prompts/get" => return handle_prompts_get(backend, id, &request).await,
        _ => Some(error_response(id, -32601, "Method not found")),
    };

    Ok(response)
}
