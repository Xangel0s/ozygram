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

fn resolve_project_root_str(uri: &str) -> String {
    if let Some(path) = uri.strip_prefix("file:///") {
        if cfg!(windows) {
            path.replace('/', "\\")
        } else {
            format!("/{}", path)
        }
    } else if let Some(path) = uri.strip_prefix("file://") {
        if cfg!(windows) {
            path.replace('/', "\\")
        } else {
            path.to_string()
        }
    } else {
        uri.to_string()
    }
}

pub async fn handle_request(
    backend: &Arc<Mutex<Option<GraphBackend>>>,
    request: mcp_common::JsonRpcRequest,
    notifier: Option<&Notifier>,
    subscribed: Option<&Arc<Mutex<HashSet<String>>>>,
) -> anyhow::Result<Option<mcp_common::JsonRpcResponse>> {
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
            let workspace_folders = request
                .params
                .as_ref()
                .and_then(|p| p.get("workspaceFolders"))
                .and_then(Value::as_array);

            let project_path_str = workspace_folders
                .and_then(|folders| folders.first())
                .and_then(|f| f.get("uri"))
                .and_then(Value::as_str)
                .map(resolve_project_root_str)
                .unwrap_or_else(|| ".".to_string());
            let project_path = PathBuf::from(&project_path_str);

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
                    log(
                        "warn",
                        format!(
                            "[ozymem] Legacy global DB detected at {}. OzyMem now uses per-project DB at {}. Run 'ozymem migrate' to copy lessons, or delete the old file.",
                            legacy_db.display(),
                            project_db.display()
                        ),
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
                "ozy_doctor" => handle_ozy_doctor(Some(backend), &tool_call).await?,
                "ozy_brain" => {
                    let action = tool_call
                        .arguments
                        .get("action")
                        .and_then(Value::as_str)
                        .unwrap_or("plan");
                    let report = handle_ozy_brain(backend, &tool_call, action).await?;
                    ToolCallResult {
                        content: vec![ContentBlock {
                            kind: "text",
                            text: report,
                        }],
                        is_error: None,
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
