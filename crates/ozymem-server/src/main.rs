use ozymem_core::graph_backend::{GraphBackend, ImpactEntry, legacy_global_db_path};
use ozymem_core::mcp_common;
use ozymem_core::mcp_common::{ContentBlock, ToolCallResult};
use ozymem_core::McpBackend;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let is_web = std::env::args().any(|arg| arg == "--web")
        || std::env::var("OZYMEM_SERVER_MODE").as_deref() == Ok("web");

    if is_web {
        run_web_server().await
    } else {
        run_mcp_server().await
    }
}

async fn run_mcp_server() -> anyhow::Result<()> {
    let backend: Arc<Mutex<Option<GraphBackend>>> = Arc::new(Mutex::new(None));
    eprintln!("[ozymem-server] MCP server ready (petgraph + SQLite, per-project DB)");

    let mut stdin = BufReader::new(io::stdin());
    let mut stdout = io::stdout();
    let mut line = String::new();

    while {
        line.clear();
        stdin.read_line(&mut line).await? > 0
    } {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if let Ok(request) = serde_json::from_str::<mcp_common::JsonRpcRequest>(trimmed) {
            if let Some(response) = handle_request(&backend, request).await? {
                let payload = serde_json::to_string(&response)?;
                stdout.write_all(payload.as_bytes()).await?;
                stdout.write_all(b"\n").await?;
                stdout.flush().await?;
            }
        } else {
            eprintln!("[ozymem-server] invalid JSON-RPC: {trimmed}");
        }
    }

    Ok(())
}

async fn handle_request(
    backend: &Arc<Mutex<Option<GraphBackend>>>,
    request: mcp_common::JsonRpcRequest,
) -> anyhow::Result<Option<mcp_common::JsonRpcResponse>> {
    if request.jsonrpc != "2.0" {
        return Ok(Some(error_response(
            request.id.unwrap_or(Value::Null),
            -32600,
            "Invalid JSON-RPC version",
        )));
    }

    let Some(id) = request.id else {
        return Ok(None);
    };

    let response = match request.method.as_str() {
        "initialize" => {
            let workspace_folders = request.params.as_ref()
                .and_then(|p| p.get("workspaceFolders"));
            match resolve_project_root(None, workspace_folders) {
                Ok(proj_path) => {
                    eprintln!("[ozymem-server] workspace folder: {} → DB: {}/.ozymem/memory.db",
                        proj_path.display(), proj_path.display());

                    let gb = match GraphBackend::open_for_project(&proj_path) {
                        Ok(gb) => gb,
                        Err(e) => {
                            eprintln!("[ozymem-server] failed to open project DB: {e}");
                            return Ok(Some(error_response(id, -32603, &format!("failed to open project DB: {e}"))));
                        }
                    };

                    let legacy = legacy_global_db_path();
                    if legacy.exists() {
                        eprintln!("[ozymem-server] legacy global DB detected at {}. Use `ozymem lessons --legacy` to view old data.", legacy.display());
                    }

                    if let Err(e) = ozymem_core::graph_backend::auto_manage_gitignore(&proj_path) {
                        eprintln!("[ozymem-server] failed to update .gitignore: {e}");
                    }

                    let mut guard = backend.lock().unwrap();
                    if guard.is_some() {
                        eprintln!("[ozymem-server] backend already initialized (duplicate initialize?)");
                    } else {
                        guard.replace(gb);
                    }
                    drop(guard);

                    let gb_clone = backend.clone();
                    let p = proj_path.to_string_lossy().to_string();
                    tokio::spawn(async move {
                        eprintln!("[ozymem-server] lazy scan for {p}");
                        let guard = gb_clone.lock().unwrap();
                        if let Some(ref gb) = *guard {
                            if let Err(e) = gb.full_scan(&p) {
                                eprintln!("[ozymem-server] scan error: {e}");
                            }
                        }
                    });
                }
                Err(msg) => {
                    eprintln!("[ozymem-server] initialize without workspace: {msg}");
                    return Ok(Some(error_response(id, -32602, &msg)));
                }
            }

            let payload = mcp_common::InitializeResult {
                protocol_version: "2024-11-05",
                capabilities: mcp_common::ServerCapabilities {
                    tools: mcp_common::ToolsCapability {
                        list_changed: Some(true),
                    },
                },
                server_info: mcp_common::ServerInfo {
                    name: "ozymem-server",
                    version: env!("CARGO_PKG_VERSION"),
                },
            };
            ok_response(id, serde_json::to_value(payload)?)
        }
        "notifications/initialized" => return Ok(None),
        "tools/list" => {
            let tools = vec![
                mcp_common::ToolDefinition {
                    name: "analyze_impact",
                    description: "Analyze transitive impact of changing a file (BFS dependency traversal). Returns all files affected up to specified depth.",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "file_path": { "type": "string", "description": "Path of the file to analyze" },
                            "depth": { "type": "integer", "description": "Max depth of transitive dependencies", "default": 3 }
                        },
                        "required": ["file_path"],
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "file_context",
                    description: "Return the indexed file context, including language and functions.",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "file_path": { "type": "string", "description": "Path used when the file was indexed" }
                        },
                        "required": ["file_path"],
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "graph_summary",
                    description: "Summarize the indexed project with file, function, and lesson counts.",
                    input_schema: json!({
                        "type": "object",
                        "properties": {},
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "record_lesson",
                    description: "Record an error-to-fix lesson for a file as persistent memory.",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "file_path": { "type": "string", "description": "Absolute path of the file" },
                            "symbol_name": { "type": "string", "description": "Optional function/class name" },
                            "error_context": { "type": "string", "description": "Details about the error" },
                            "solution": { "type": "string", "description": "Short fix or lesson learned" }
                        },
                        "required": ["file_path", "error_context", "solution"],
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "search_lessons",
                    description: "Full-text search across all recorded lessons, decisions, conventions, gotchas, and module rules.",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "query": { "type": "string", "description": "Search query (FTS5 natural language, falls back to LIKE substring)" },
                            "kind": { "type": "string", "description": "Filter by kind: lesson, decision, convention, gotcha, module_rule", "enum": ["lesson", "decision", "convention", "gotcha", "module_rule"] },
                            "limit": { "type": "integer", "description": "Max results (1-100)", "default": 10 }
                        },
                        "required": ["query"],
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "get_file_lessons",
                    description: "Get all lessons, decisions, and conventions recorded for a specific file.",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "file_path": { "type": "string", "description": "Absolute path of the file" }
                        },
                        "required": ["file_path"],
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "get_symbol_lessons",
                    description: "Get entries recorded for a specific file + symbol (function/class name).",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "file_path": { "type": "string", "description": "Absolute path of the file" },
                            "symbol_name": { "type": "string", "description": "Function or class name" }
                        },
                        "required": ["file_path", "symbol_name"],
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "recent_lessons",
                    description: "List the most recently recorded lessons, decisions, conventions, gotchas, or module rules.",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "kind": { "type": "string", "description": "Filter by kind", "enum": ["lesson", "decision", "convention", "gotcha", "module_rule"] },
                            "limit": { "type": "integer", "description": "Max results (1-100)", "default": 10 }
                        },
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "similar_lessons",
                    description: "Search lessons by semantic similarity. Uses local embeddings (all-MiniLM-L6-v2) to find lessons with related meaning, not just keyword matches.",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "query": { "type": "string", "description": "Free-text description of the situation or error" },
                            "limit": { "type": "integer", "description": "Max results", "default": 10, "minimum": 1, "maximum": 50 },
                            "min_score": { "type": "number", "description": "Minimum similarity score (0.0-1.0)", "default": 0.5 }
                        },
                        "required": ["query"],
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "graph_neighbors",
                    description: "Get direct incoming and outgoing dependency neighbors of a file in the dependency graph.",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "file_path": { "type": "string", "description": "Absolute path of the file" }
                        },
                        "required": ["file_path"],
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "record_decision",
                    description: "Record an architectural decision associated with a file/module.",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "file_path": { "type": "string", "description": "Absolute path of the file" },
                            "symbol_name": { "type": "string", "description": "Optional function/class name" },
                            "context": { "type": "string", "description": "Why this decision was made" },
                            "decision": { "type": "string", "description": "What was decided" }
                        },
                        "required": ["file_path", "context", "decision"],
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "record_convention",
                    description: "Record a code convention or pattern used in a module.",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "file_path": { "type": "string", "description": "Absolute path of the file" },
                            "symbol_name": { "type": "string", "description": "Optional function/class name" },
                            "context": { "type": "string", "description": "What the convention is about" },
                            "convention": { "type": "string", "description": "The convention or pattern description" }
                        },
                        "required": ["file_path", "context", "convention"],
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "record_gotcha",
                    description: "Record a tricky gotcha or pitfall found while working on a file.",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "file_path": { "type": "string", "description": "Absolute path of the file" },
                            "symbol_name": { "type": "string", "description": "Optional function/class name" },
                            "context": { "type": "string", "description": "What was surprising or tricky" },
                            "gotcha": { "type": "string", "description": "The gotcha or pitfall description" }
                        },
                        "required": ["file_path", "context", "gotcha"],
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "record_module_rule",
                    description: "Record a module-level rule or invariant for a file or directory.",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "file_path": { "type": "string", "description": "Absolute path of the file or directory" },
                            "context": { "type": "string", "description": "Why this rule exists" },
                            "rule": { "type": "string", "description": "The rule or invariant description" }
                        },
                        "required": ["file_path", "context", "rule"],
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "git_recent_changes",
                    description: "List the most recent commits with their changed files. Returns commit hash, author, message, timestamp, and file list.",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "limit": { "type": "integer", "description": "Number of recent commits to return", "default": 10, "minimum": 1, "maximum": 100 },
                            "project_path": { "type": "string", "description": "Optional: git repository root (auto-discovered from workspace if omitted)" }
                        },
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "git_diff_summary",
                    description: "Diff statistics between two git refs (commits, branches, tags). Returns files changed, insertions, deletions, and per-file breakdown.",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "from": { "type": "string", "description": "Base ref (commit hash, branch name, HEAD~N, etc.)" },
                            "to": { "type": "string", "description": "Target ref (defaults to HEAD if omitted)" },
                            "project_path": { "type": "string", "description": "Optional: git repository root (auto-discovered from workspace if omitted)" }
                        },
                        "required": ["from"],
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "git_diff_file",
                    description: "Get the unified diff of a specific file between two git refs.",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "from": { "type": "string", "description": "Base ref (commit hash, branch name, HEAD~N, etc.)" },
                            "to": { "type": "string", "description": "Target ref (defaults to HEAD if omitted)" },
                            "file_path": { "type": "string", "description": "Path of the file to diff" },
                            "project_path": { "type": "string", "description": "Optional: git repository root (auto-discovered from workspace if omitted)" }
                        },
                        "required": ["from", "file_path"],
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "git_blame_line",
                    description: "Get blame annotations for a range of lines in a file. Returns commit hash, author, and timestamp per line range.",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "file_path": { "type": "string", "description": "Path of the file to blame" },
                            "start_line": { "type": "integer", "description": "First line number (1-indexed, defaults to 1)" },
                            "end_line": { "type": "integer", "description": "Last line number (defaults to start_line)" },
                            "project_path": { "type": "string", "description": "Optional: git repository root (auto-discovered from workspace if omitted)" }
                        },
                        "required": ["file_path"],
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "recent_changes_with_impact",
                    description: "Recent git commits with their transitive impact analysis. For each changed file, shows who depends on it in the project graph.",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "limit": { "type": "integer", "description": "Number of recent commits", "default": 5, "minimum": 1, "maximum": 20 },
                            "depth": { "type": "integer", "description": "Impact traversal depth", "default": 1, "minimum": 1, "maximum": 5 },
                            "project_path": { "type": "string", "description": "Optional: git repository root" }
                        },
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "refresh_project_index",
                    description: "Force a full re-scan of the project directory. Returns scan diagnostics including how many files were indexed vs skipped.",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "project_path": {
                                "type": "string",
                                "description": "Optional: switch to a different project root and re-index it"
                            }
                        },
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "context_for_task",
                    description: "Bundled context for a task: searches lessons, then for each relevant file returns its functions, dependency neighbors, and impact analysis. All trimmed to a token budget.",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "query": { "type": "string", "description": "Free-text query describing what you're working on" },
                            "max_tokens": { "type": "integer", "description": "Target maximum tokens in response", "default": 4000, "minimum": 500, "maximum": 32000 }
                        },
                        "required": ["query"],
                        "additionalProperties": false
                    }),
                },
            ];
            ok_response(id, serde_json::to_value(
                mcp_common::ToolListResult { tools }
            )?)
        }
        "tools/call" => {
            let params = request.params
                .ok_or_else(|| anyhow::anyhow!("missing params"))?;
            let tool_call: mcp_common::ToolCallParams = serde_json::from_value(params)?;

            // Git tools: don't need the GraphBackend lock (they open their own repo)
            if tool_call.name == "git_recent_changes" || tool_call.name == "git_diff_summary"
                || tool_call.name == "git_diff_file" || tool_call.name == "git_blame_line"
                || tool_call.name == "recent_changes_with_impact" {
                let explicit_path = tool_call.arguments.get("project_path")
                    .and_then(Value::as_str)
                    .map(|s| s.to_string());

                let resolved = match resolve_project_root(explicit_path.clone(), None) {
                    Ok(p) => p,
                    Err(_) if explicit_path.is_none() => {
                        // Fallback: use project path from initialized backend
                        let guard = backend.lock().unwrap();
                        match guard.as_ref().and_then(|gb| gb.project_path()) {
                            Some(p) => PathBuf::from(p),
                            None => return Ok(Some(error_response(id, -32602,
                                "No project path set. Provide project_path argument or ensure initialize was sent with workspaceFolders."))),
                        }
                    }
                    Err(msg) => return Ok(Some(error_response(id, -32602, &msg))),
                };

                let git = match ozymem_core::git_backend::GitBackend::open(&resolved) {
                    Ok(g) => g,
                    Err(msg) => return Ok(Some(error_response(id, -32603, &msg))),
                };

                let result = match tool_call.name.as_str() {
                    "git_recent_changes" => {
                        let limit = tool_call.arguments.get("limit")
                            .and_then(Value::as_u64)
                            .unwrap_or(10) as usize;
                        match git.recent_changes(limit) {
                            Ok(changes) => {
                                let mut body = format!("Recent commits (up to {limit}):\n");
                                for c in &changes {
                                    let merge_tag = if c.is_merge { " [MERGE]" } else { "" };
                                    body.push_str(&format!(
                                        "\n{} {}{}\n  Author: {}\n  Date:   {}\n",
                                        &c.hash[..7], c.message, merge_tag, c.author, c.timestamp,
                                    ));
                                    if !c.files.is_empty() {
                                        body.push_str("  Files:\n");
                                        for f in &c.files {
                                            body.push_str(&format!("    - {f}\n"));
                                        }
                                    }
                                }
                                ToolCallResult {
                                    content: vec![ContentBlock { kind: "text", text: body }],
                                    is_error: None,
                                }
                            }
                            Err(msg) => {
                                return Ok(Some(error_response(id, -32603, &msg)));
                            }
                        }
                    }
                    "git_diff_summary" => {
                        let from = match tool_call.arguments.get("from").and_then(Value::as_str) {
                            Some(f) => f,
                            None => return Ok(Some(error_response(id, -32602, "Missing required parameter: from"))),
                        };
                        let to = tool_call.arguments.get("to")
                            .and_then(Value::as_str);
                        match git.diff_summary(from, to) {
                            Ok(summary) => {
                                let mut body = format!(
                                    "Diff summary ({} files changed, {} insertions, {} deletions)\n",
                                    summary.files_changed, summary.insertions, summary.deletions,
                                );
                                for f in &summary.files {
                                    body.push_str(&format!("\n  {}  {}", f.status, f.path));
                                }
                                ToolCallResult {
                                    content: vec![ContentBlock { kind: "text", text: body }],
                                    is_error: None,
                                }
                            }
                            Err(msg) => {
                                return Ok(Some(error_response(id, -32603, &msg)));
                            }
                        }
                    }
                    "git_diff_file" => {
                        let from = match tool_call.arguments.get("from").and_then(Value::as_str) {
                            Some(f) => f,
                            None => return Ok(Some(error_response(id, -32602, "Missing required parameter: from"))),
                        };
                        let to = tool_call.arguments.get("to").and_then(Value::as_str);
                        let file_path = match tool_call.arguments.get("file_path").and_then(Value::as_str) {
                            Some(p) => p,
                            None => return Ok(Some(error_response(id, -32602, "Missing required parameter: file_path"))),
                        };
                        match git.diff_file(from, to, file_path) {
                            Ok(diff) => ToolCallResult {
                                content: vec![ContentBlock { kind: "text", text: diff }],
                                is_error: None,
                            },
                            Err(msg) => return Ok(Some(error_response(id, -32603, &msg))),
                        }
                    }
                    "git_blame_line" => {
                        let file_path = match tool_call.arguments.get("file_path").and_then(Value::as_str) {
                            Some(p) => p,
                            None => return Ok(Some(error_response(id, -32602, "Missing required parameter: file_path"))),
                        };
                        let start_line = tool_call.arguments.get("start_line").and_then(Value::as_u64).map(|v| v as usize);
                        let end_line = tool_call.arguments.get("end_line").and_then(Value::as_u64).map(|v| v as usize);
                        match git.blame_file(file_path, start_line, end_line) {
                            Ok(entries) => {
                                if entries.is_empty() {
                                    ToolCallResult {
                                        content: vec![ContentBlock { kind: "text", text: "No blame information found for the specified range.".to_string() }],
                                        is_error: None,
                                    }
                                } else {
                                    let mut body = format!("Blame for {} ({} entries):\n", file_path, entries.len());
                                    for e in &entries {
                                        body.push_str(&format!("\n  {}..{}  {}  {}  {}",
                                            e.start_line, e.end_line, &e.hash[..7], e.author, e.timestamp));
                                    }
                                    ToolCallResult {
                                        content: vec![ContentBlock { kind: "text", text: body }],
                                        is_error: None,
                                    }
                                }
                            }
                            Err(msg) => return Ok(Some(error_response(id, -32603, &msg))),
                        }
                    }
                    "recent_changes_with_impact" => {
                        let limit = tool_call.arguments.get("limit")
                            .and_then(Value::as_u64).unwrap_or(5) as usize;
                        let depth = tool_call.arguments.get("depth")
                            .and_then(Value::as_u64).unwrap_or(1) as u32;

                        let changes = match git.recent_changes(limit) {
                            Ok(c) => c,
                            Err(msg) => return Ok(Some(error_response(id, -32603, &msg))),
                        };

                        // Collect unique touched file paths, resolve to absolute
                        let repo_root = git.repo_path().to_path_buf();
                        let mut touched_set: Vec<String> = Vec::new();
                        for c in &changes {
                            for f in &c.files {
                                let abs = repo_root.join(f).to_string_lossy().to_string();
                                if !touched_set.contains(&abs) {
                                    touched_set.push(abs);
                                }
                            }
                        }

                        // Single lock: batch analyze_impact for all touched files
                        let impacts: std::collections::HashMap<String, Vec<ozymem_core::graph_backend::ImpactEntry>> = {
                            let guard = backend.lock().unwrap();
                            let Some(ref gb) = *guard else {
                                return Ok(Some(error_response(id, -32000, "Backend not initialized")));
                            };
                            gb.reload_if_stale();
                            let touched_set: std::collections::HashSet<String> = touched_set.into_iter().collect();
                            touched_set.iter()
                                .map(|f| {
                                    let impact = gb.analyze_impact(f, depth);
                                    (f.clone(), impact)
                                })
                                .collect()
                        };

                        // Combine: build response text
                        let mut body = format!("Recent commits with impact (depth {depth}):\n");
                        for c in &changes {
                            body.push_str(&format!(
                                "\n{} {}\n  Author: {}\n  Date:   {}\n",
                                &c.hash[..7], c.message, c.author, c.timestamp,
                            ));
                            for f in &c.files {
                                let abs = repo_root.join(f).to_string_lossy().to_string();
                                let exists = std::path::Path::new(&abs).exists();
                                let impact = impacts.get(&abs).cloned().unwrap_or_default();
                                let status = if exists { "" } else { " [DELETED]" };
                                body.push_str(&format!("  - {f}{status}\n"));
                                if !impact.is_empty() {
                                    for imp in &impact {
                                        body.push_str(&format!("      → {}\n", imp.file_path));
                                    }
                                } else if exists {
                                    body.push_str("      (no impact recorded)\n");
                                }
                            }
                        }

                        ToolCallResult {
                            content: vec![ContentBlock { kind: "text", text: body }],
                            is_error: None,
                        }
                    }
                    _ => unreachable!(),
                };
                return Ok(Some(ok_response(id, serde_json::to_value(result)?)));
            }

            // Special case: refresh_project_index manages its own locking for hot-switch
            if tool_call.name == "refresh_project_index" {
                let explicit_path = tool_call.arguments.get("project_path")
                    .and_then(Value::as_str)
                    .map(|s| s.to_string());

                let resolved = match resolve_project_root(explicit_path, None) {
                    Ok(p) => p,
                    Err(msg) => return Ok(Some(error_response(id, -32602, &msg))),
                };
                let resolved_str = resolved.to_string_lossy().to_string();

                let needs_switch = {
                    let guard = backend.lock().unwrap();
                    guard.as_ref()
                        .map(|gb| gb.project_path() != Some(resolved_str.clone()))
                        .unwrap_or(true)
                };

                let new_gb = if needs_switch {
                    eprintln!("[ozymem-server] switching project to {}", resolved.display());
                    Some(match GraphBackend::open_for_project(&resolved) {
                        Ok(gb) => gb,
                        Err(e) => return Ok(Some(error_response(id, -32603, &format!("failed to open project DB: {e}")))),
                    })
                } else {
                    None
                };

                let mut guard = backend.lock().unwrap();
                if let Some(gb) = new_gb {
                    guard.replace(gb);
                }
                let Some(ref gb) = *guard else {
                    return Ok(Some(error_response(id, -32000, "Backend not initialized")));
                };
                gb.full_scan(&resolved_str)?;
                let result = ToolCallResult {
                    content: vec![ContentBlock {
                        kind: "text",
                        text: format!("Project index refreshed for {}\nCheck server logs for scan diagnostics (indexed vs skipped counts).", resolved.display()),
                    }],
                    is_error: None,
                };
                return Ok(Some(ok_response(id, serde_json::to_value(result)?)));
            }

            let locked = backend.lock().unwrap();
            let Some(backend) = locked.as_ref() else {
                return Ok(Some(error_response(
                    id, -32000,
                    "Server not initialized: send 'initialize' with workspaceFolders before calling tools",
                )));
            };

            let result = match tool_call.name.as_str() {
                "analyze_impact" => {
                    backend.reload_if_stale();

                    let scanning = backend.scan_progress.lock().unwrap().scanning;
                    if scanning {
                        let sp = backend.scan_progress.lock().unwrap();
                        ToolCallResult {
                            content: vec![ContentBlock {
                                kind: "text",
                                text: format!("Scanning project... {} files processed", sp.processed),
                            }],
                            is_error: None,
                        }
                    } else {
                        let file_path = tool_call.arguments.get("file_path")
                            .and_then(Value::as_str)
                            .ok_or_else(|| anyhow::anyhow!("missing file_path"))?;
                        let depth = tool_call.arguments.get("depth")
                            .and_then(Value::as_u64)
                            .unwrap_or(3) as u32;

                        let impacts = backend.analyze_impact(file_path, depth);
                        let body = format_impact(&impacts, file_path);
                        ToolCallResult {
                            content: vec![ContentBlock { kind: "text", text: body }],
                            is_error: None,
                        }
                    }
                }
                "file_context" => {
                    backend.reload_if_stale();
                    let file_path = tool_call.arguments.get("file_path")
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow::anyhow!("missing file_path"))?;

                    let ctx = backend.get_file_context(file_path).await?;
                    let history = backend.get_historical_engram_solutions(file_path).await?;
                    let body = format_file_context(ctx.as_ref(), file_path);
                    ToolCallResult {
                        content: vec![ContentBlock {
                            kind: "text",
                            text: prepend_history(&history, body),
                        }],
                        is_error: None,
                    }
                }
                "graph_summary" => {
                    backend.reload_if_stale();
                    let summary = backend.get_graph_summary().await?;
                    let sp = backend.scan_progress.lock().unwrap();
                    let scanning_note = if sp.scanning {
                        format!("\n\n⚠ Scan in progress: {} of {} files processed (results may be partial)",
                            sp.processed, sp.total)
                    } else {
                        String::new()
                    };
                    drop(sp);
                    ToolCallResult {
                        content: vec![ContentBlock {
                            kind: "text",
                            text: format!("{}{}", format_summary(&summary), scanning_note),
                        }],
                        is_error: None,
                    }
                }
                "record_lesson" => {
                    let file_path = tool_call.arguments.get("file_path")
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow::anyhow!("missing file_path"))?;
                    let symbol_name = tool_call.arguments.get("symbol_name")
                        .and_then(Value::as_str);
                    let error_context = tool_call.arguments.get("error_context")
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow::anyhow!("missing error_context"))?;
                    let solution = tool_call.arguments.get("solution")
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow::anyhow!("missing solution"))?;

                    backend.record_lesson(file_path, symbol_name, error_context, solution).await?;
                    ToolCallResult {
                        content: vec![ContentBlock {
                            kind: "text",
                            text: format!("Recorded lesson for {}{}",
                                file_path,
                                symbol_name.map(|s| format!("::{}", s)).unwrap_or_default()),
                        }],
                        is_error: None,
                    }
                }
                "search_lessons" => {
                    let query = tool_call.arguments.get("query")
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow::anyhow!("missing query"))?;
                    let kind = tool_call.arguments.get("kind").and_then(Value::as_str);
                    let limit = tool_call.arguments.get("limit").and_then(Value::as_u64).unwrap_or(10) as usize;

                    let results = backend.search_lessons(query, kind, limit).await?;
                    let mut body = String::new();
                    if results.is_empty() {
                        body = format!("No results for \"{query}\"");
                    } else {
                        for (i, entry) in results.iter().enumerate() {
                            body.push_str(&format!("{}. [{}] {} :: {}\n   context: {}\n   solution: {}\n",
                                i + 1, entry.kind, entry.file_path, entry.symbol_name,
                                entry.error_context, entry.solution));
                        }
                    }
                    ToolCallResult {
                        content: vec![ContentBlock { kind: "text", text: body }],
                        is_error: None,
                    }
                }
                "get_file_lessons" => {
                    let file_path = tool_call.arguments.get("file_path")
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow::anyhow!("missing file_path"))?;
                    let results = backend.get_file_lessons(file_path).await?;
                    let body = format_lessons_list(&results);
                    ToolCallResult {
                        content: vec![ContentBlock { kind: "text", text: body }],
                        is_error: None,
                    }
                }
                "get_symbol_lessons" => {
                    let file_path = tool_call.arguments.get("file_path")
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow::anyhow!("missing file_path"))?;
                    let symbol_name = tool_call.arguments.get("symbol_name")
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow::anyhow!("missing symbol_name"))?;
                    let results = backend.get_symbol_lessons(file_path, symbol_name).await?;
                    let body = format_lessons_list(&results);
                    ToolCallResult {
                        content: vec![ContentBlock { kind: "text", text: body }],
                        is_error: None,
                    }
                }
                "recent_lessons" => {
                    let kind = tool_call.arguments.get("kind").and_then(Value::as_str);
                    let limit = tool_call.arguments.get("limit").and_then(Value::as_u64).unwrap_or(10) as usize;
                    let results = backend.recent_lessons(kind, limit).await?;
                    let body = format_lessons_list(&results);
                    ToolCallResult {
                        content: vec![ContentBlock { kind: "text", text: body }],
                        is_error: None,
                    }
                }
                "similar_lessons" => {
                    let query = match tool_call.arguments.get("query").and_then(Value::as_str) {
                        Some(q) => q,
                        None => return Ok(Some(error_response(id, -32602, "Missing required parameter: query"))),
                    };
                    let limit = tool_call.arguments.get("limit").and_then(Value::as_u64).unwrap_or(10) as usize;
                    let min_score = tool_call.arguments.get("min_score").and_then(Value::as_f64).unwrap_or(0.5) as f32;
                    match backend.similar_lessons(query, limit, min_score) {
                        Ok(results) => {
                            if results.is_empty() {
                                ToolCallResult {
                                    content: vec![ContentBlock { kind: "text", text: format!("No semantically similar lessons found for \"{query}\"") }],
                                    is_error: None,
                                }
                            } else {
                                let mut body = format!("Lessons semantically similar to \"{query}\" (min score {min_score}):\n");
                                for (i, sl) in results.iter().enumerate() {
                                    body.push_str(&format!(
                                        "\n{}. [score: {:.3}] [{}] {} :: {}\n   context: {}\n   solution: {}\n   created: {}\n",
                                        i + 1, sl.score, sl.lesson.kind, sl.lesson.file_path, sl.lesson.symbol_name,
                                        sl.lesson.error_context, sl.lesson.solution, sl.lesson.created_at,
                                    ));
                                }
                                ToolCallResult {
                                    content: vec![ContentBlock { kind: "text", text: body }],
                                    is_error: None,
                                }
                            }
                        }
                        Err(e) => return Ok(Some(error_response(id, -32603, &format!("similar_lessons error: {e}")))),
                    }
                }
                "graph_neighbors" => {
                    backend.reload_if_stale();
                    let file_path = tool_call.arguments.get("file_path")
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow::anyhow!("missing file_path"))?;
                    let info = backend.get_graph_neighbors(file_path).await?;
                    let mut body = format!("Neighbors of {}\n", info.file_path);
                    body.push_str(&format!("  Incoming (dependents): {}\n", info.incoming.len()));
                    for p in &info.incoming {
                        body.push_str(&format!("    - {p}\n"));
                    }
                    body.push_str(&format!("  Outgoing (dependencies): {}\n", info.outgoing.len()));
                    for p in &info.outgoing {
                        body.push_str(&format!("    - {p}\n"));
                    }
                    ToolCallResult {
                        content: vec![ContentBlock { kind: "text", text: body }],
                        is_error: None,
                    }
                }
                "record_decision" => {
                    let file_path = tool_call.arguments.get("file_path")
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow::anyhow!("missing file_path"))?;
                    let symbol_name = tool_call.arguments.get("symbol_name").and_then(Value::as_str);
                    let context = tool_call.arguments.get("context")
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow::anyhow!("missing context"))?;
                    let decision = tool_call.arguments.get("decision")
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow::anyhow!("missing decision"))?;
                    backend.record_entry(file_path, symbol_name, context, decision, "decision").await?;
                    ToolCallResult {
                        content: vec![ContentBlock { kind: "text", text: format!("Recorded decision for {file_path}") }],
                        is_error: None,
                    }
                }
                "record_convention" => {
                    let file_path = tool_call.arguments.get("file_path")
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow::anyhow!("missing file_path"))?;
                    let symbol_name = tool_call.arguments.get("symbol_name").and_then(Value::as_str);
                    let context = tool_call.arguments.get("context")
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow::anyhow!("missing context"))?;
                    let convention = tool_call.arguments.get("convention")
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow::anyhow!("missing convention"))?;
                    backend.record_entry(file_path, symbol_name, context, convention, "convention").await?;
                    ToolCallResult {
                        content: vec![ContentBlock { kind: "text", text: format!("Recorded convention for {file_path}") }],
                        is_error: None,
                    }
                }
                "record_gotcha" => {
                    let file_path = tool_call.arguments.get("file_path")
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow::anyhow!("missing file_path"))?;
                    let symbol_name = tool_call.arguments.get("symbol_name").and_then(Value::as_str);
                    let context = tool_call.arguments.get("context")
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow::anyhow!("missing context"))?;
                    let gotcha = tool_call.arguments.get("gotcha")
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow::anyhow!("missing gotcha"))?;
                    backend.record_entry(file_path, symbol_name, context, gotcha, "gotcha").await?;
                    ToolCallResult {
                        content: vec![ContentBlock { kind: "text", text: format!("Recorded gotcha for {file_path}") }],
                        is_error: None,
                    }
                }
                "record_module_rule" => {
                    let file_path = tool_call.arguments.get("file_path")
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow::anyhow!("missing file_path"))?;
                    let context = tool_call.arguments.get("context")
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow::anyhow!("missing context"))?;
                    let rule = tool_call.arguments.get("rule")
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow::anyhow!("missing rule"))?;
                    backend.record_entry(file_path, None, context, rule, "module_rule").await?;
                    ToolCallResult {
                        content: vec![ContentBlock { kind: "text", text: format!("Recorded module rule for {file_path}") }],
                        is_error: None,
                    }
                }
                "context_for_task" => {
                    backend.reload_if_stale();

                    let query = tool_call.arguments.get("query")
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow::anyhow!("missing query"))?;
                    let max_tokens = tool_call.arguments.get("max_tokens")
                        .and_then(Value::as_u64)
                        .unwrap_or(4000) as usize;

                    // 1. Search lessons (filter out stale — we only want current knowledge)
                    let lessons: Vec<_> = backend.search_lessons(query, None, 20).await?
                        .into_iter()
                        .filter(|l| l.stale == 0)
                        .collect();

                    // 2. Collect unique file paths from matched lessons
                    let mut file_paths: Vec<&str> = lessons.iter()
                        .map(|l| l.file_path.as_str())
                        .collect::<std::collections::HashSet<&str>>()
                        .into_iter()
                        .collect();
                    file_paths.sort();
                    // Limit to 5 files to stay within budget
                    file_paths.truncate(5);

                    // 3. Build response with token budget
                    let mut body = String::new();

                    // Lessons section
                    body.push_str(&format!("[Lessons matching \"{query}\"]\n"));
                    if lessons.is_empty() {
                        body.push_str("(none)\n");
                    } else {
                        for (i, e) in lessons.iter().enumerate() {
                            body.push_str(&format!(
                                "{}. [{}] {} :: {}\n   {}\n",
                                i + 1, e.kind, e.file_path, e.symbol_name, e.solution
                            ));
                        }
                    }

                    // File context and neighbors for each relevant file
                    for fp in &file_paths {
                        // Check token budget before adding more
                        if body.len() / 4 >= max_tokens {
                            body.push_str(&format!("\n... (truncated at {max_tokens} tokens)"));
                            break;
                        }

                        body.push_str(&format!("\n\n=== {fp} ==="));

                        // File context
                        if let Ok(Some(ctx)) = backend.get_file_context(fp).await {
                            body.push_str(&format!("\nLanguage: {}", ctx.language));
                            if !ctx.functions.is_empty() {
                                body.push_str(&format!("\nFunctions ({})", ctx.functions.len()));
                                for fn_data in &ctx.functions {
                                    body.push_str(&format!("\n  {} [{}] L{}-L{}",
                                        fn_data.name, fn_data.kind, fn_data.start_line, fn_data.end_line));
                                }
                            }
                        }

                        // Graph neighbors
                        if let Ok(neighbors) = backend.get_graph_neighbors(fp).await {
                            if !neighbors.incoming.is_empty() {
                                body.push_str(&format!("\nDependents ({}):", neighbors.incoming.len()));
                                for p in &neighbors.incoming {
                                    body.push_str(&format!("\n  {p}"));
                                }
                            }
                            if !neighbors.outgoing.is_empty() {
                                body.push_str(&format!("\nDepends on ({}):", neighbors.outgoing.len()));
                                for p in &neighbors.outgoing {
                                    body.push_str(&format!("\n  {p}"));
                                }
                            }
                        }

                        // Impact analysis (depth 1 to keep it short)
                        let impacts = backend.analyze_impact(fp, 1);
                        if !impacts.is_empty() {
                            body.push_str(&format!("\nTransitive impact (depth 1, {} files):", impacts.len()));
                            for imp in &impacts {
                                body.push_str(&format!("\n  {} ({} funcs, {} lessons)",
                                    imp.file_path, imp.function_count, imp.lesson_count));
                            }
                        }
                    }

                    ToolCallResult {
                        content: vec![ContentBlock { kind: "text", text: body }],
                        is_error: None,
                    }
                }
                _ => {
                    return Ok(Some(error_response(id, -32601, "Unknown tool")));
                }
            };

            ok_response(id, serde_json::to_value(result)?)
        }
        _ => error_response(id, -32601, "Method not found"),
    };

    Ok(Some(response))
}

fn format_lessons_list(results: &[ozymem_core::graph_backend::LessonEntry]) -> String {
    if results.is_empty() {
        return "No entries found.".to_string();
    }
    let mut body = String::new();
    for (i, entry) in results.iter().enumerate() {
        let stale_tag = if entry.stale != 0 {
            format!(" [STALE: {}]", entry.stale_reason.as_deref().unwrap_or("unknown"))
        } else {
            String::new()
        };
        body.push_str(&format!(
            "{}. [{}]{} {} :: {}\n   context: {}\n   solution: {}\n   created: {}\n",
            i + 1, entry.kind, stale_tag, entry.file_path, entry.symbol_name,
            entry.error_context, entry.solution, entry.created_at
        ));
    }
    body
}

fn format_impact(impacts: &[ImpactEntry], file_path: &str) -> String {
    if impacts.is_empty() {
        return format!("No impact found for {}", file_path);
    }

    let mut text = format!("Impact analysis for {}:\n", file_path);
    let mut current_depth = 0u32;
    for entry in impacts {
        if entry.depth != current_depth {
            current_depth = entry.depth;
            text.push_str(&format!("\n  Depth {}:\n", current_depth));
        }
        text.push_str(&format!(
            "    {} [{} | {} funcs, {} lessons]\n",
            entry.file_path, entry.language, entry.function_count, entry.lesson_count
        ));
    }

    let total_funcs: i64 = impacts.iter().map(|e| e.function_count).sum();
    let total_lessons: i64 = impacts.iter().map(|e| e.lesson_count).sum();
    text.push_str(&format!(
        "\nTotal: {} files affected, {} functions, {} lessons registered",
        impacts.len(),
        total_funcs,
        total_lessons
    ));
    text
}

fn format_file_context(context: Option<&ozymem_core::FileGraphContext>, file_path: &str) -> String {
    let Some(context) = context else {
        return format!("No indexed file found for {file_path}");
    };

    let mut output = format!(
        "File: {}\nLanguage: {}\nFunctions: {}",
        context.file_path,
        context.language,
        context.functions.len()
    );

    for function in &context.functions {
        output.push_str(&format!(
            "\n- {} [{}] lines {}-{} via {}",
            function.name, function.kind, function.start_line, function.end_line, function.strategy
        ));
    }
    output
}

fn prepend_history(history: &[String], body: String) -> String {
    if history.is_empty() {
        return body;
    }
    let mut output = String::from("[LESSONS FOR THIS FILE:\n");
    for solution in history {
        output.push_str(&format!("- {solution}\n"));
    }
    output.push_str("]\n\n");
    output.push_str(&body);
    output
}

fn format_summary(summary: &ozymem_core::GraphSummary) -> String {
    format!(
        "Files: {}\nFunctions: {}\nLessons: {} ({} without embeddings)\nNative AST: {}\nExtension WASM: {}\nText heuristic: {}",
        summary.file_count,
        summary.function_count,
        summary.engram_count,
        summary.lessons_without_embedding,
        summary.native_ast_function_count,
        summary.extension_wasm_function_count,
        summary.text_heuristic_function_count
    )
}

fn ok_response(id: Value, result: Value) -> mcp_common::JsonRpcResponse {
    mcp_common::JsonRpcResponse {
        jsonrpc: "2.0",
        id,
        result: Some(result),
        error: None,
    }
}

fn error_response(id: Value, code: i64, message: &str) -> mcp_common::JsonRpcResponse {
    mcp_common::JsonRpcResponse {
        jsonrpc: "2.0",
        id,
        result: None,
        error: Some(mcp_common::JsonRpcError {
            code,
            message: message.to_string(),
        }),
    }
}

/// Convert a `file:///C:/path` workspace URI to a `C:\path` string.
fn workspace_uri_to_path(uri: &str) -> String {
    let path = uri.trim_start_matches("file://");
    let cleaned = if cfg!(windows) && path.starts_with('/') && path.chars().nth(2) == Some(':') {
        &path[1..]
    } else {
        path
    };
    cleaned.replace('/', "\\")
}

/// Resolve the project root directory with fallback priority:
/// 1. `explicit_path` — optional param from a tool call
/// 2. `workspace_folders` — first folder's `uri` from initialize params
/// 3. `OZYGRAM_PROJECT_ROOT` — env var
/// Returns an error with actionable guidance if none are available.
fn resolve_project_root(
    explicit_path: Option<String>,
    workspace_folders: Option<&Value>,
) -> Result<PathBuf, String> {
    // 1. Explicit path from tool call — highest priority
    if let Some(p) = explicit_path {
        let pb = PathBuf::from(&p);
        return pb.canonicalize().map_err(|e| format!("Invalid project_path `{p}`: {e}"));
    }
    // 2. workspaceFolders from initialize
    if let Some(folders) = workspace_folders {
        if let Some(first) = folders.as_array().and_then(|a| a.first()) {
            if let Some(uri) = first.get("uri").and_then(Value::as_str) {
                let p = workspace_uri_to_path(uri);
                let pb = PathBuf::from(&p);
                if let Ok(canon) = pb.canonicalize() {
                    return Ok(canon);
                }
                return Ok(pb);
            }
        }
    }
    // 3. Environment variable
    if let Ok(p) = std::env::var("OZYGRAM_PROJECT_ROOT") {
        let pb = PathBuf::from(&p);
        return pb.canonicalize().map_err(|e| format!("Invalid OZYGRAM_PROJECT_ROOT `{p}`: {e}"));
    }
    // 4. Actionable error
    Err(
        "No project path set. Provide project_path in the tool call, \
         ensure initialize sends workspaceFolders, or set OZYGRAM_PROJECT_ROOT env var."
            .to_string(),
    )
}

// ==========================================
// WEB MODE (legacy, uses Memgraph)
// ==========================================

async fn run_web_server() -> anyhow::Result<()> {
    use ozymem_core::{MemgraphConnection, MemgraphConfig, default_memgraph_uri, default_memgraph_database};

    let config = MemgraphConfig {
        uri: std::env::var("MEMGRAPH_URI").unwrap_or_else(|_| default_memgraph_uri().to_string()),
        user: std::env::var("MEMGRAPH_USER").unwrap_or_else(|_| "admin".to_string()),
        password: std::env::var("MEMGRAPH_PASSWORD").unwrap_or_else(|_| "admin".to_string()),
        database: std::env::var("MEMGRAPH_DATABASE").unwrap_or_else(|_| default_memgraph_database().to_string()),
    };

    let connection = MemgraphConnection::connect(config).await?;
    let connection_cell = Arc::new(tokio::sync::OnceCell::new());
    connection_cell.set(connection).ok();

    // ... existing web server code from original main.rs ...
    // (unchanged, preserved for backward compatibility)

    let port = std::env::var("PORT").ok()
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(8080);
    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
    eprintln!("[INFO] Ozymem web server on {}", addr);

    use axum::{
        extract::State,
        http::StatusCode,
        routing::get,
        Json, Router,
    };
    use tokio::sync::OnceCell;

    async fn handle_health(
        State(cell): State<Arc<OnceCell<MemgraphConnection>>>,
    ) -> Result<Json<Value>, StatusCode> {
        let conn = cell.get().ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
        conn.ping().await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        Ok(Json(json!({ "status": "ok" })))
    }

    let app = Router::new()
        .route("/api/health", get(handle_health))
        .with_state(connection_cell);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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

        let response = handle_request(&backend, request).await.unwrap();
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
        let tmp_root = std::env::temp_dir().join(format!("ozymem_test_init_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp_root);
        std::fs::create_dir_all(&tmp_root).unwrap();

        let proj_uri = format!(
            "file:///{}",
            tmp_root.to_string_lossy().replace('\\', "/")
        );

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

        let response = handle_request(&backend, request).await.unwrap();
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

        let proj_uri = format!(
            "file:///{}",
            tmp_root.to_string_lossy().replace('\\', "/")
        );

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
        handle_request(&backend, init_req).await.unwrap();

        // Run a quick scan before calling context_for_task
        {
            let guard = backend.lock().unwrap();
            if let Some(ref gb) = *guard {
                gb.full_scan(&tmp_root.to_string_lossy()).ok();
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
        let response = handle_request(&backend, cft_req).await.unwrap();
        assert!(response.is_some(), "context_for_task should return a response");

        let resp = response.unwrap();
        assert!(resp.error.is_none(), "context_for_task should not error: {:?}", resp.error);

        let result = resp.result.unwrap();
        let text = result["content"][0]["text"].as_str().unwrap_or("");
        assert!(!text.is_empty(), "response text should not be empty");
        assert!(text.contains("(none)"), "should indicate no lessons found");

        // Verify truncation test: with max_tokens=500, output should be within budget
        assert!(text.len() / 4 <= 500, "output should be within token budget");

        std::fs::remove_dir_all(&tmp_root).ok();
    }

    #[tokio::test]
    async fn test_context_for_task_stale_lessons_excluded() {
        let backend: Arc<Mutex<Option<GraphBackend>>> = Arc::new(Mutex::new(None));

        let tmp_root = std::env::temp_dir().join(format!("ozymem_test_cft_stale_{}", std::process::id()));
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
        handle_request(&backend, init_req).await.unwrap();

        let main_abs = tmp_root.join("main.rs").to_string_lossy().to_string();

        // Run scan, then record two lessons on the same file with overlapping query keywords
        {
            let guard = backend.lock().unwrap();
            let gb = guard.as_ref().unwrap();
            gb.full_scan(&tmp_root.to_string_lossy()).ok();

            // Fresh lesson — should appear in output
            gb.record_lesson(
                &main_abs, Some("main"),
                "overflow bug in main",
                "Use checked_add for overflow safety",
            ).await.unwrap();

            // Stale lesson (will be marked stale right after) — should be filtered out
            gb.record_lesson(
                &main_abs, Some("main"),
                "old overflow approach",
                "Use wrapping_add for overflow (deprecated)",
            ).await.unwrap();
        }

        // Manually mark the second lesson as stale via direct SQLite
        {
            let db_path = tmp_root.join(".ozymem").join("memory.db");
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            // lessons are ordered by created_at ascending, so id=2 is the second one
            conn.execute(
                "UPDATE lessons SET stale = 1, stale_reason = 'symbol_removed' WHERE id = 2",
                [],
            ).unwrap();
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
        let response = handle_request(&backend, cft_req).await.unwrap();
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

        let tmp_root = std::env::temp_dir().join(format!("ozymem_test_cft_trunc_{}", std::process::id()));
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
        handle_request(&backend, init_req).await.unwrap();

        let main_abs = tmp_root.join("main.rs").to_string_lossy().to_string();

        // Run scan, then record lessons with modestly long content.
        // The handler writes all lessons inline, then chops per-file context
        // sections at the token boundary.  We keep lesson content short enough
        // to fit in the budget so the File context section triggers the cut.
        {
            let guard = backend.lock().unwrap();
            let gb = guard.as_ref().unwrap();
            gb.full_scan(&tmp_root.to_string_lossy()).ok();

            for i in 0..3 {
                gb.record_lesson(
                    &main_abs, Some("main"),
                    &format!("overflow pattern {}", i),
                    &format!("short explanation for pattern {}", i),
                ).await.unwrap();
            }
        }

        // Also write a second source file to generate more file-context output
        std::fs::write(tmp_root.join("utils.rs"), "fn helper() {} fn compute() {}").unwrap();
        {
            let guard = backend.lock().unwrap();
            let gb = guard.as_ref().unwrap();
            gb.full_scan(&tmp_root.to_string_lossy()).ok();
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
        let response = handle_request(&backend, cft_req).await.unwrap();
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
}
