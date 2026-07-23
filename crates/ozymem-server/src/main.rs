use ozymem_core::graph_backend::{GraphBackend, ImpactEntry, legacy_global_db_path};
use ozymem_core::mcp_common;
use ozymem_core::mcp_common::{ContentBlock, ToolCallResult};
use ozymem_core::registry::ProjectRegistry;
use ozymem_core::McpBackend;
use ozymem_parser::parse_source;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

// ---------------------------------------------------------------------------
// Notification infrastructure — sends JSON-RPC notifications to stdout
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct Notifier {
    tx: mpsc::UnboundedSender<String>,
}

impl Notifier {
    fn new() -> (Self, mpsc::UnboundedReceiver<String>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (Notifier { tx }, rx)
    }

    fn log(&self, level: &str, message: String) {
        let notification = serde_json::to_string(&json!({
            "jsonrpc": "2.0",
            "method": "notifications/message",
            "params": {
                "level": level,
                "logger": "ozymem-server",
                "data": message
            }
        }))
        .unwrap_or_default();
        let _ = self.tx.send(notification);
    }

    fn progress(&self, progress_token: &Value, progress: u64, total: Option<u64>) {
        let mut params = json!({
            "progressToken": progress_token,
            "progress": progress,
        });
        if let Some(t) = total {
            params["total"] = json!(t);
        }
        let notification = serde_json::to_string(&json!({
            "jsonrpc": "2.0",
            "method": "notifications/progress",
            "params": params
        }))
        .unwrap_or_default();
        let _ = self.tx.send(notification);
    }

    /// Send a raw JSON-RPC notification (for resource updates, etc.)
    fn raw(&self, method: &str, params: Value) {
        let notification = serde_json::to_string(&json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }))
        .unwrap_or_default();
        let _ = self.tx.send(notification);
    }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    run_mcp_server().await
}

async fn run_mcp_server() -> anyhow::Result<()> {
    let backend: Arc<Mutex<Option<GraphBackend>>> = Arc::new(Mutex::new(None));
    let (notifier, rx) = Notifier::new();
    let subscribed: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));
    notifier.log("info", "[ozymem-server] MCP server ready (petgraph + SQLite, per-project DB)".into());

    let mut stdin = BufReader::new(io::stdin());
    let mut stdout = io::stdout();

    // Spawn a background task that drains the notification channel and writes to stdout.
    // This lets any async task send notifications without owning stdout.
    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_flag_clone = stop_flag.clone();
    let writer_handle = tokio::spawn(async move {
        let mut rx = rx;
        let mut stdout = io::stdout();
        while !stop_flag_clone.load(Ordering::Relaxed) {
            match tokio::time::timeout(std::time::Duration::from_millis(200), rx.recv()).await {
                Ok(Some(payload)) => {
                    let _ = stdout.write_all(payload.as_bytes()).await;
                    let _ = stdout.write_all(b"\n").await;
                    let _ = stdout.flush().await;
                }
                Ok(None) => break,
                Err(_) => { /* timeout, loop and check flag */ }
            }
        }
    });

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
            if let Some(response) = handle_request(&backend, request, Some(&notifier), Some(&subscribed)).await? {
                let payload = serde_json::to_string(&response)?;
                stdout.write_all(payload.as_bytes()).await?;
                stdout.write_all(b"\n").await?;
                stdout.flush().await?;
            }
        } else {
            notifier.log("error", format!("[ozymem-server] invalid JSON-RPC: {trimmed}"));
        }
    }

    stop_flag.store(true, Ordering::Relaxed);
    let _ = writer_handle.await;

    Ok(())
}

// Helper for logging from inside spawned tasks (no notifier = silently dropped)
fn log_spawn(level: &str, msg: String, notifier: &Option<Notifier>) {
    if let Some(n) = notifier {
        n.log(level, msg);
    }
}

/// Send `notifications/methods/resources/updated` for subscribed URIs.
fn notify_subscribed(subscribed: Option<&Arc<Mutex<HashSet<String>>>>, notifier: Option<&Notifier>, uris: &[&str]) {
    if let Some(sub) = subscribed {
        let subs = sub.lock().unwrap();
        for uri in uris {
            if subs.contains(*uri) {
                if let Some(ref n) = notifier {
                    n.raw("notifications/methods/resources/updated", json!({"uri": uri}));
                }
            }
        }
    }
}

async fn handle_request(
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

    let Some(id) = request.id else {
        return Ok(None);
    };

    let response = match request.method.as_str() {
        "initialize" => {
            let workspace_folders = request.params.as_ref()
                .and_then(|p| p.get("workspaceFolders"));
            match resolve_project_root(None, workspace_folders) {
                Ok(proj_path) => {
                    log("info", format!("[ozymem-server] workspace folder: {} → DB: {}/.ozymem/memory.db",
                        proj_path.display(), proj_path.display()));

                    let gb = match GraphBackend::open_for_project(&proj_path) {
                        Ok(gb) => gb,
                        Err(e) => {
                            log("error", format!("[ozymem-server] failed to open project DB: {e}"));
                            return Ok(Some(error_response(id, -32603, &format!("failed to open project DB: {e}"))));
                        }
                    };

                    let legacy = legacy_global_db_path();
                    if legacy.exists() {
                        log("warn", format!("[ozymem-server] legacy global DB detected at {}. Use `ozymem lessons --legacy` to view old data.", legacy.display()));
                    }

                    if let Err(e) = ozymem_core::graph_backend::auto_manage_gitignore(&proj_path) {
                        log("warn", format!("[ozymem-server] failed to update .gitignore: {e}"));
                    }

                    let mut guard = backend.lock().unwrap();
                    if guard.is_some() {
                        log("warn", "[ozymem-server] backend already initialized (duplicate initialize?)".into());
                    } else {
                        guard.replace(gb);
                    }
                    drop(guard);

                    let gb_clone = backend.clone();
                    let p = proj_path.to_string_lossy().to_string();
                    let n = notifier.cloned();
                    tokio::spawn(async move {
                        log_spawn("info", format!("[ozymem-server] lazy scan for {p}"), &n);
                        let guard = gb_clone.lock().unwrap();
                        if let Some(ref gb) = *guard {
                            if let Err(e) = gb.full_scan(&p, None) {
                                log_spawn("error", format!("[ozymem-server] scan error: {e}"), &n);
                            }
                        }
                    });
                }
                Err(msg) => {
                    log("error", format!("[ozymem-server] initialize without workspace: {msg}"));
                    return Ok(Some(error_response(id, -32602, &msg)));
                }
            }

            let payload = mcp_common::InitializeResult {
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
            ok_response(id, serde_json::to_value(payload)?)
        }
        "notifications/initialized" => return Ok(None),
        "tools/list" => {
            let tools = vec![
                mcp_common::ToolDefinition {
                    name: "analyze_impact",
                    description: "Analyze transitive impact of changing a file (BFS with severity: [BREAKING]/[WARN]/[INFO] — shows affected functions per file with line ranges, reasons, and suggestions)",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "file_path": { "type": "string", "description": "File path" },
                            "depth": { "type": "integer", "description": "Max traversal depth", "default": 3 }
                        },
                        "required": ["file_path"],
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "file_context",
                    description: "Indexed file context (language + functions + graph neighbors + lessons + last git commit)",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "file_path": { "type": "string", "description": "File path" }
                        },
                        "required": ["file_path"],
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "graph_summary",
                    description: "Project summary with file/function/lesson counts",
                    input_schema: json!({
                        "type": "object",
                        "properties": {},
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "record_lesson",
                    description: "Record a lesson for a file",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "file_path": { "type": "string", "description": "File path" },
                            "symbol_name": { "type": "string", "description": "Function/class name (optional)" },
                            "error_context": { "type": "string", "description": "Error description" },
                            "solution": { "type": "string", "description": "Fix description" }
                        },
                        "required": ["file_path", "error_context", "solution"],
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "search_lessons",
                    description: "FTS5 search across all memory entries",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "query": { "type": "string", "description": "FTS5 search query" },
                            "kind": { "type": "string", "description": "Filter by kind", "enum": ["lesson", "decision", "convention", "gotcha", "module_rule"] },
                            "limit": { "type": "integer", "description": "Max results", "default": 10 }
                        },
                        "required": ["query"],
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "get_file_lessons",
                    description: "Get all memory entries for a file",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "file_path": { "type": "string", "description": "File path" }
                        },
                        "required": ["file_path"],
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "get_symbol_lessons",
                    description: "Get entries for a file + symbol",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "file_path": { "type": "string", "description": "File path" },
                            "symbol_name": { "type": "string", "description": "Function/class name" }
                        },
                        "required": ["file_path", "symbol_name"],
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "recent_lessons",
                    description: "Most recent memory entries",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "kind": { "type": "string", "description": "Filter by kind", "enum": ["lesson", "decision", "convention", "gotcha", "module_rule"] },
                            "limit": { "type": "integer", "description": "Max results", "default": 10 }
                        },
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "similar_lessons",
                    description: "Semantic similarity search across lessons",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "query": { "type": "string", "description": "Free-text description of the situation" },
                            "kind": { "type": "string", "description": "Filter by kind (optional)", "enum": ["lesson", "decision", "convention", "gotcha", "module_rule"] },
                            "limit": { "type": "integer", "description": "Max results", "default": 10, "minimum": 1, "maximum": 50 },
                            "min_score": { "type": "number", "description": "Minimum similarity score (0.0-1.0)", "default": 0.5 }
                        },
                        "required": ["query"],
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "graph_neighbors",
                    description: "Dependency neighbors (incoming + outgoing)",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "file_path": { "type": "string", "description": "File path" }
                        },
                        "required": ["file_path"],
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "record_decision",
                    description: "Record a decision for a file/module",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "file_path": { "type": "string", "description": "File path" },
                            "symbol_name": { "type": "string", "description": "Function/class name (optional)" },
                            "context": { "type": "string", "description": "Why this decision was made" },
                            "decision": { "type": "string", "description": "What was decided" }
                        },
                        "required": ["file_path", "context", "decision"],
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "record_convention",
                    description: "Record a code convention",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "file_path": { "type": "string", "description": "File path" },
                            "symbol_name": { "type": "string", "description": "Function/class name (optional)" },
                            "context": { "type": "string", "description": "What it's about" },
                            "convention": { "type": "string", "description": "Convention description" }
                        },
                        "required": ["file_path", "context", "convention"],
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "record_gotcha",
                    description: "Record a gotcha/pitfall",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "file_path": { "type": "string", "description": "File path" },
                            "symbol_name": { "type": "string", "description": "Function/class name (optional)" },
                            "context": { "type": "string", "description": "What was surprising" },
                            "gotcha": { "type": "string", "description": "Gotcha description" }
                        },
                        "required": ["file_path", "context", "gotcha"],
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "record_module_rule",
                    description: "Record a module-level rule",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "file_path": { "type": "string", "description": "File or directory path" },
                            "context": { "type": "string", "description": "Why this rule exists" },
                            "rule": { "type": "string", "description": "Rule description" }
                        },
                        "required": ["file_path", "context", "rule"],
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "git_recent_changes",
                    description: "Recent git commits with file list",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "limit": { "type": "integer", "description": "Number of commits", "default": 10, "minimum": 1, "maximum": 100 },
                            "project_path": { "type": "string", "description": "Git repo root (auto-discovered)" }
                        },
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "git_diff_summary",
                    description: "Git diff stats between two refs",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "from": { "type": "string", "description": "Base ref (commit, branch, HEAD~N)" },
                            "to": { "type": "string", "description": "Target ref (defaults to HEAD)" },
                            "project_path": { "type": "string", "description": "Git repo root (auto-discovered)" }
                        },
                        "required": ["from"],
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "git_diff_file",
                    description: "Unified diff of a file between refs",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "from": { "type": "string", "description": "Base ref (commit, branch, HEAD~N)" },
                            "to": { "type": "string", "description": "Target ref (defaults to HEAD)" },
                            "file_path": { "type": "string", "description": "File path to diff" },
                            "project_path": { "type": "string", "description": "Git repo root (auto-discovered)" }
                        },
                        "required": ["from", "file_path"],
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "git_blame_line",
                    description: "Git blame for a file or line range",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "file_path": { "type": "string", "description": "File path to blame" },
                            "start_line": { "type": "integer", "description": "First line (1-indexed, default: 1)" },
                            "end_line": { "type": "integer", "description": "Last line (default: start_line)" },
                            "project_path": { "type": "string", "description": "Git repo root (auto-discovered)" }
                        },
                        "required": ["file_path"],
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "recent_changes_with_impact",
                    description: "Recent commits with transitive impact analysis",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "limit": { "type": "integer", "description": "Number of commits", "default": 5, "minimum": 1, "maximum": 20 },
                            "depth": { "type": "integer", "description": "Impact depth", "default": 1, "minimum": 1, "maximum": 5 },
                            "project_path": { "type": "string", "description": "Git repo root" }
                        },
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "refresh_project_index",
                    description: "Force full re-scan of the project",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "project_path": { "type": "string", "description": "Different project root to switch and scan" }
                        },
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "context_for_task",
                    description: "Bundled context: lessons + files + neighbors + impact",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "query": { "type": "string", "description": "Free-text task description" },
                            "max_tokens": { "type": "integer", "description": "Token budget", "default": 4000, "minimum": 500, "maximum": 32000 }
                        },
                        "required": ["query"],
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "list_projects",
                    description: "List registered projects",
                    input_schema: json!({
                        "type": "object",
                        "properties": {},
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "get_project_memories",
                    description: "Get memory entries for a specific project",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "project_name": { "type": "string", "description": "Registered project name" },
                            "query": { "type": "string", "description": "FTS5 search query (optional)" },
                            "kind": { "type": "string", "description": "Filter by kind", "enum": ["lesson", "decision", "convention", "gotcha", "module_rule"] },
                            "limit": { "type": "integer", "description": "Max results", "default": 20 }
                        },
                        "required": ["project_name"],
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "delete_project",
                    description: "Delete project (keeps lessons, requires force=true)",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "project_name": { "type": "string", "description": "Registered project name" },
                            "force": { "type": "boolean", "description": "Preview (false) or execute (true)", "default": false }
                        },
                        "required": ["project_name"],
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "suggest_stale_projects",
                    description: "Suggest stale/dormant projects for cleanup",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "days": { "type": "integer", "description": "Min days since last activity", "default": 90 }
                        },
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "list_files",
                    description: "List all indexed file paths in the project",
                    input_schema: json!({
                        "type": "object",
                        "properties": {},
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "create_ozymignore",
                    description: "Create or update .ozymignore with ignore patterns and re-scan",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "patterns": { "type": "array", "items": { "type": "string" }, "description": "Ignore patterns to add (e.g. coverage/, *.log)" },
                            "project_path": { "type": "string", "description": "Project root (optional, defaults to active)" }
                        },
                        "required": ["patterns"],
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "create_project",
                    description: "Create a new project directory with package manager init and optional dependency install",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "name": { "type": "string", "description": "Project name (directory name)" },
                            "path": { "type": "string", "description": "Parent directory (defaults to current dir)" },
                            "type": { "type": "string", "description": "Project type", "enum": ["node", "rust"], "default": "node" },
                            "packages": { "type": "array", "items": { "type": "string" }, "description": "Packages to install on init" }
                        },
                        "required": ["name"],
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "add_package",
                    description: "Install npm/pnpm packages in a registered project",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "project_name": { "type": "string", "description": "Registered project name" },
                            "packages": { "type": "array", "items": { "type": "string" }, "description": "Packages to install" },
                            "dev": { "type": "boolean", "description": "Install as dev dependency", "default": false }
                        },
                        "required": ["project_name", "packages"],
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "remove_package",
                    description: "Uninstall npm/pnpm packages from a registered project",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "project_name": { "type": "string", "description": "Registered project name" },
                            "packages": { "type": "array", "items": { "type": "string" }, "description": "Packages to remove" }
                        },
                        "required": ["project_name", "packages"],
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "get_dependencies",
                    description: "Read package.json dependencies without opening the file",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "project_name": { "type": "string", "description": "Registered project name" }
                        },
                        "required": ["project_name"],
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "run_script",
                    description: "Run a script from package.json in a registered project",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "project_name": { "type": "string", "description": "Registered project name" },
                            "script": { "type": "string", "description": "Script name from package.json" },
                            "args": { "type": "array", "items": { "type": "string" }, "description": "Extra args for the script" }
                        },
                        "required": ["project_name", "script"],
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "analyze_package",
                    description: "Read a package's metadata from node_modules (no index overhead)",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "project_name": { "type": "string", "description": "Registered project name" },
                            "package_name": { "type": "string", "description": "Package name to inspect" }
                        },
                        "required": ["project_name", "package_name"],
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "verify_dependencies",
                    description: "Scan source imports against package.json for missing/unused deps",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "project_name": { "type": "string", "description": "Registered project name" }
                        },
                        "required": ["project_name"],
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "find_symbol",
                    description: "Find symbol definitions (functions, classes) indexed by tree-sitter — no grep noise",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "symbol_name": { "type": "string", "description": "Symbol name to search for (supports LIKE patterns: % for wildcard)" },
                            "kind": { "type": "string", "description": "Filter by symbol kind", "enum": ["Function", "Class"] },
                            "max_results": { "type": "integer", "description": "Max results", "default": 20, "minimum": 1, "maximum": 100 }
                        },
                        "required": ["symbol_name"],
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "learn_from_changes",
                    description: "Auto-generate lessons from git diff (tree-sitter detects new/removed/modified functions, embeddings auto-computed, includes graph impact analysis)",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "message": { "type": "string", "description": "Context about the change (used as error_context for generated entries)" },
                            "from": { "type": "string", "description": "Base git ref", "default": "HEAD~1" },
                            "to": { "type": "string", "description": "Target git ref", "default": "HEAD" },
                            "project_path": { "type": "string", "description": "Git repo root (auto-discovered from active project if omitted)" },
                            "preview": { "type": "boolean", "description": "Preview only — no entries recorded", "default": false },
                            "max_impact": { "type": "integer", "description": "Max dependents to show per file in impact section", "default": 5 }
                        },
                        "required": ["message"],
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "graph_path",
                    description: "Find dependency paths between two files using petgraph (shortest connection in the project graph)",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "from": { "type": "string", "description": "Start file path" },
                            "to": { "type": "string", "description": "End file path" },
                            "max_paths": { "type": "integer", "description": "Max paths to return", "default": 1, "minimum": 1, "maximum": 10 },
                            "max_hops": { "type": "integer", "description": "Max intermediate hops", "default": 10, "minimum": 1, "maximum": 50 }
                        },
                        "required": ["from", "to"],
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "smart_search",
                    description: "Unified search across symbols (tree-sitter), lessons (FTS5), and embeddings (semantic) — best match across all indexes",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "query": { "type": "string", "description": "Search query (used for symbol LIKE, FTS5 text, and embedding similarity)" },
                            "max_results": { "type": "integer", "description": "Max results", "default": 10, "minimum": 1, "maximum": 30 },
                            "min_score": { "type": "number", "description": "Minimum similarity score (0-1)", "default": 0.3 }
                        },
                        "required": ["query"],
                        "additionalProperties": false
                    }),
                },
            ];
            // Pagination support for tools/list
            let page_size = 50;
            let params_cursor = request.params.as_ref()
                .and_then(|p| p.get("cursor"))
                .and_then(Value::as_str)
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(0);
            let total = tools.len();
            let tools_slice: Vec<_> = tools.into_iter().skip(params_cursor * page_size).take(page_size).collect();
            let has_more = params_cursor * page_size + page_size < total;
            ok_response(id, serde_json::to_value(
                mcp_common::ToolListResult {
                    tools: tools_slice,
                    next_cursor: if has_more { Some((params_cursor + 1).to_string()) } else { None },
                }
            )?)
        }
        "tools/call" => {
            let raw_params = request.params
                .ok_or_else(|| anyhow::anyhow!("missing params"))?;

            // Extract progressToken from _meta before consuming raw_params
            let progress_token = raw_params.get("_meta")
                .and_then(|m| m.get("progressToken"))
                .cloned();

            let tool_call: mcp_common::ToolCallParams = serde_json::from_value(raw_params)?;

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
                    log("info", format!("[ozymem-server] switching project to {}", resolved.display()));
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
                let progress = &progress_token;
                gb.full_scan(&resolved_str, Some(&|processed, total| {
                    if let Some(ref n) = notifier {
                        if let Some(ref pt) = progress {
                            n.progress(pt, processed, Some(total));
                        }
                    }
                }))?;
                let result = ToolCallResult {
                    content: vec![ContentBlock {
                        kind: "text",
                        text: format!("Project index refreshed for {}\nCheck server logs for scan diagnostics (indexed vs skipped counts).", resolved.display()),
                    }],
                    is_error: None,
                };
                return Ok(Some(ok_response(id, serde_json::to_value(result)?)));
            }

            // Project management tools: open their own ProjectRegistry + memory.db
            if tool_call.name == "list_projects" || tool_call.name == "get_project_memories"
                || tool_call.name == "delete_project" || tool_call.name == "suggest_stale_projects" {
                return Ok(Some(handle_project_tool(id, &tool_call, notifier).await?));
            }

            // Package management tools: create projects, install packages, run scripts
            if tool_call.name == "create_project" || tool_call.name == "add_package"
                || tool_call.name == "remove_package" || tool_call.name == "get_dependencies"
                || tool_call.name == "run_script"
                || tool_call.name == "analyze_package"
                || tool_call.name == "verify_dependencies" {
                return Ok(Some(handle_package_tool(id, &tool_call, notifier).await?));
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

                    // Enriched: graph neighbors (dependent count)
                    let neighbor_info = backend.get_graph_neighbors(file_path).await.ok();

                    // Enriched: last git commit
                    let last_git = get_last_commit(file_path).await;

                    let body = format_file_context_enriched(
                        ctx.as_ref(),
                        file_path,
                        &history,
                        neighbor_info.as_ref(),
                        last_git.as_deref(),
                    );
                    ToolCallResult {
                        content: vec![ContentBlock { kind: "text", text: body }],
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
                "list_files" => {
                    backend.reload_if_stale();
                    let files = backend.list_all_files()?;
                    if files.is_empty() {
                        ToolCallResult {
                            content: vec![ContentBlock { kind: "text", text: "No files indexed. Run refresh_project_index first.".to_string() }],
                            is_error: None,
                        }
                    } else {
                        let body = format!("Indexed files ({}):\n\n{}", files.len(), files.join("\n"));
                        ToolCallResult {
                            content: vec![ContentBlock { kind: "text", text: body }],
                            is_error: None,
                        }
                    }
                }
                // Creates or updates .ozymignore, appends patterns, re-scans the project.
                // Falls back to the active backend's project_path if no explicit path given.
                "create_ozymignore" => {
                    let patterns = match tool_call.arguments.get("patterns").and_then(Value::as_array) {
                        Some(arr) => arr.iter().filter_map(|v| v.as_str().map(String::from)).collect::<Vec<_>>(),
                        None => return Ok(Some(error_response(id, -32602, "Missing required parameter: patterns"))),
                    };
                    if patterns.is_empty() {
                        return Ok(Some(error_response(id, -32602, "patterns must not be empty")));
                    }
                    let explicit_path = tool_call.arguments.get("project_path")
                        .and_then(Value::as_str).map(|s| s.to_string());
                    let resolved = match resolve_project_root(explicit_path.clone(), None) {
                        Ok(p) => p,
                        Err(_) if explicit_path.is_none() => {
                            match backend.project_path() {
                                Some(p) => PathBuf::from(p),
                                None => return Ok(Some(error_response(id, -32602,
                                    "No project path set. Provide project_path or ensure initialize was sent with workspaceFolders."))),
                            }
                        }
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
                    let mut content = String::from("# OzyMem ignore patterns\n# Lines starting with # are comments\n\n");
                    for p in &existing {
                        content.push_str(p);
                        content.push('\n');
                    }
                    std::fs::write(&ozymemignore_path, content)?;
                    log("info", format!("[ozymem-server] .ozymignore updated at {} ({} patterns)", ozymemignore_path.display(), existing.len()));
                    let resolved_str = resolved.to_string_lossy().to_string();
                    let progress = &progress_token;
                    backend.full_scan(&resolved_str, Some(&|processed, total| {
                        if let Some(ref n) = notifier {
                            if let Some(ref pt) = progress {
                                n.progress(pt, processed, Some(total));
                            }
                        }
                    }))?;
                    let body = format!(
                        "Created/updated .ozymignore at {}.\n  Total patterns: {}\n  Newly added: {}\n  The project has been re-scanned ignoring these patterns.",
                        ozymemignore_path.display(),
                        existing.len(),
                        if added.is_empty() { "none (all already present)".to_string() } else { added.join(", ") },
                    );
                    ToolCallResult {
                        content: vec![ContentBlock { kind: "text", text: body }],
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
                    notify_subscribed(subscribed, notifier, &["ozymem://summary", "ozymem://recent-lessons"]);
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
                    let kind_filter = tool_call.arguments.get("kind").and_then(Value::as_str);
                    match backend.similar_lessons(query, limit, min_score) {
                        Ok(results) => {
                            let filtered: Vec<&ozymem_core::graph_backend::SimilarLesson> = if let Some(kind) = kind_filter {
                                results.iter().filter(|r| r.lesson.kind == kind).collect()
                            } else {
                                results.iter().collect()
                            };
                            if filtered.is_empty() {
                                ToolCallResult {
                                    content: vec![ContentBlock { kind: "text", text: format!("No semantically similar lessons found for \"{query}\"{}",
                                        kind_filter.map_or(String::new(), |k| format!(" (kind: {k})"))) }],
                                    is_error: None,
                                }
                            } else {
                                let mut body = format!("Lessons semantically similar to \"{query}\" (min score {min_score}):\n");
                                for (i, sl) in filtered.iter().enumerate() {
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
                    notify_subscribed(subscribed, notifier, &["ozymem://summary", "ozymem://recent-lessons"]);
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
                    notify_subscribed(subscribed, notifier, &["ozymem://summary", "ozymem://recent-lessons"]);
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
                    notify_subscribed(subscribed, notifier, &["ozymem://summary", "ozymem://recent-lessons"]);
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
                    notify_subscribed(subscribed, notifier, &["ozymem://summary", "ozymem://recent-lessons"]);
                    ToolCallResult {
                        content: vec![ContentBlock { kind: "text", text: format!("Recorded module rule for {file_path}") }],
                        is_error: None,
                    }
                }
                "find_symbol" => {
                    backend.reload_if_stale();

                    let symbol_name = tool_call.arguments.get("symbol_name")
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow::anyhow!("missing symbol_name"))?;
                    let kind_filter = tool_call.arguments.get("kind").and_then(Value::as_str);
                    let max_results = tool_call.arguments.get("max_results")
                        .and_then(Value::as_u64)
                        .unwrap_or(20) as usize;

                    let project_path = backend.project_path()
                        .ok_or_else(|| anyhow::anyhow!("no project path set"))?;

                    let results = backend.find_symbol(symbol_name, &project_path).await?;

                    // Post-filter by kind (parsed from "[Kind]" in result string)
                    let filtered: Vec<&String> = if let Some(kind) = kind_filter {
                        results.iter().filter(|r| {
                            r.split('[').nth(1)
                                .and_then(|s| s.split(']').next())
                                .map(|k| k == kind)
                                .unwrap_or(false)
                        }).collect()
                    } else {
                        results.iter().collect()
                    };

                    let truncated: Vec<&&String> = filtered.iter().take(max_results).collect();

                    let body = if truncated.is_empty() {
                        format!("No definitions found for '{}'{}",
                            symbol_name,
                            kind_filter.map_or(String::new(), |k| format!(" (kind: {k})")))
                    } else {
                        let mut text = format!("Definitions matching '{}' ({} total, filtered to {}, showing {}):\n",
                            symbol_name, results.len(), filtered.len(), truncated.len());
                        for (i, entry) in truncated.iter().enumerate() {
                            text.push_str(&format!("{}. {}\n", i + 1, entry));
                        }
                        text
                    };

                    ToolCallResult {
                        content: vec![ContentBlock { kind: "text", text: body }],
                        is_error: None,
                    }
                }
                "learn_from_changes" => {
                    let result = handle_learn_from_changes(backend, &tool_call, notifier, subscribed).await?;
                    result
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
                "graph_path" => {
                    backend.reload_if_stale();

                    let from = tool_call.arguments.get("from")
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow::anyhow!("missing from"))?;
                    let to = tool_call.arguments.get("to")
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow::anyhow!("missing to"))?;
                    let max_paths = tool_call.arguments.get("max_paths")
                        .and_then(Value::as_u64).unwrap_or(1) as usize;
                    let max_hops = tool_call.arguments.get("max_hops")
                        .and_then(Value::as_u64).unwrap_or(10) as usize;

                    let paths = backend.find_graph_path(from, to, max_paths, max_hops);

                    let body = if paths.is_empty() {
                        format!("No path found between '{}' and '{}' (they may not be connected in the dependency graph)", from, to)
                    } else {
                        let shortest = paths.iter().map(|p| p.len()).min().unwrap_or(0).saturating_sub(1);
                        let mut text = format!("Found {} path(s) between '{}' and '{}' (shortest: {} hops, max allowed: {}):\n", paths.len(), from, to, shortest, max_hops);
                        for (i, path) in paths.iter().enumerate() {
                            let hops = path.len() - 1;
                            let marker = if hops == shortest { " ◀ SHORTEST" } else { "" };
                            text.push_str(&format!("\nPath {} ({} hop(s)){}:\n", i + 1, hops, marker));
                            for (j, file) in path.iter().enumerate() {
                                let arrow = if j < path.len() - 1 { " → " } else { "" };
                                text.push_str(&format!("  {}{}", file, arrow));
                                if j < path.len() - 1 { text.push('\n'); }
                            }
                            text.push('\n');
                        }
                        text
                    };

                    ToolCallResult {
                        content: vec![ContentBlock { kind: "text", text: body }],
                        is_error: None,
                    }
                }
                "smart_search" => {
                    backend.reload_if_stale();

                    let query = tool_call.arguments.get("query")
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow::anyhow!("missing query"))?;
                    let max_results = tool_call.arguments.get("max_results")
                        .and_then(Value::as_u64).unwrap_or(10) as usize;
                    let min_score = tool_call.arguments.get("min_score")
                        .and_then(Value::as_f64).unwrap_or(0.3) as f32;

                    let project_path = backend.project_path()
                        .ok_or_else(|| anyhow::anyhow!("no project path set"))?;

                    // Run searches in parallel
                    let (symbols, lessons, semantic) = tokio::join!(
                        backend.find_symbol(query, &project_path),
                        backend.search_lessons(query, None, max_results),
                        async {
                            backend.similar_lessons(&format!("search:{}", query), max_results, min_score)
                                .unwrap_or_default()
                        }
                    );

                    let symbol_results = symbols.unwrap_or_default();
                    let lesson_results = lessons.unwrap_or_default();

                    let mut body = format!("Smart search for '{}':\n", query);

                    // Symbols section
                    body.push_str(&format!("\n── Symbol definitions ({}):\n", symbol_results.len()));
                    for (i, s) in symbol_results.iter().enumerate().take(max_results) {
                        body.push_str(&format!("  {}. {}\n", i + 1, s));
                    }

                    // Lesson section (FTS5)
                    body.push_str(&format!("\n── Lessons (FTS5, {}):\n", lesson_results.len()));
                    for (i, l) in lesson_results.iter().enumerate().take(max_results) {
                        body.push_str(&format!("  {}. [{}] {} :: {}\n     {}\n", i + 1, l.kind, l.symbol_name, l.error_context, l.solution));
                    }

                    // Semantic section
                    body.push_str(&format!("\n── Semantic (embeddings, {}):\n", semantic.len()));
                    for (i, s) in semantic.iter().enumerate().take(max_results) {
                        body.push_str(&format!("  {}. [score: {:.2}] [{}] {} :: {}\n     {}\n", i + 1, s.score, s.lesson.kind, s.lesson.symbol_name, s.lesson.error_context, s.lesson.solution));
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
        "resources/list" => {
            let guard = backend.lock().unwrap();
            let Some(ref gb) = *guard else {
                return Ok(Some(error_response(id, -32000, "Server not initialized: send 'initialize' with workspaceFolders before listing resources")));
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
            let _cursor = request.params.as_ref()
                .and_then(|p| p.get("cursor"))
                .and_then(Value::as_str);
            let result = mcp_common::ListResourcesResult {
                resources,
                next_cursor: None,
            };
            ok_response(id, serde_json::to_value(result)?)
        }
        "resources/read" => {
            let guard = backend.lock().unwrap();
            let Some(ref gb) = *guard else {
                return Ok(Some(error_response(id, -32000, "Server not initialized")));
            };
            let params: mcp_common::ReadResourceParams = match request.params
                .ok_or_else(|| anyhow::anyhow!("missing params"))
                .and_then(|p| serde_json::from_value(p).map_err(|e| anyhow::anyhow!("invalid params: {e}")))
            {
                Ok(p) => p,
                Err(e) => return Ok(Some(error_response(id, -32602, &e.to_string()))),
            };
            let uri = params.uri.clone();
            let contents = match uri.as_str() {
                "ozymem://summary" => {
                    match gb.get_graph_summary().await {
                        Ok(summary) => vec![mcp_common::ResourceContents::Text {
                            uri: params.uri,
                            mime_type: "application/json".to_string(),
                            text: serde_json::to_string_pretty(&summary)?,
                        }],
                        Err(e) => return Ok(Some(error_response(id, -32603, &format!("failed to read summary: {e}")))),
                    }
                }
                "ozymem://recent-lessons" => {
                    match gb.recent_lessons(None, 20).await {
                        Ok(lessons) => vec![mcp_common::ResourceContents::Text {
                            uri: params.uri,
                            mime_type: "application/json".to_string(),
                            text: serde_json::to_string_pretty(&lessons)?,
                        }],
                        Err(e) => return Ok(Some(error_response(id, -32603, &format!("failed to read lessons: {e}")))),
                    }
                }
                "ozymem://full-context" => {
                    let summary = gb.get_graph_summary().await.unwrap_or(ozymem_core::GraphSummary {
                        file_count: 0, function_count: 0, engram_count: 0,
                        native_ast_function_count: 0, extension_wasm_function_count: 0,
                        text_heuristic_function_count: 0, vertex_count: 0, edge_count: 0,
                        memory_usage: String::new(), lessons_without_embedding: 0,
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
                        Err(e) => return Ok(Some(error_response(id, -32603, &format!("failed to read file context: {e}")))),
                    }
                }
                _ => return Ok(Some(error_response(id, -32602, &format!("Unknown resource URI: {}", params.uri)))),
            };
            let result = mcp_common::ReadResourceResult { contents };
            ok_response(id, serde_json::to_value(result)?)
        }
        "resources/templates/list" => {
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
            let result = mcp_common::ListResourceTemplatesResult { resource_templates: templates };
            ok_response(id, serde_json::to_value(result)?)
        }
        "resources/subscribe" => {
            if let Some(sub) = subscribed {
                let uri = request.params.as_ref()
                    .and_then(|p| p.get("uri"))
                    .and_then(Value::as_str)
                    .map(|s| s.to_string());
                if let Some(uri) = uri {
                    sub.lock().unwrap().insert(uri);
                }
                ok_response(id, json!({}))
            } else {
                error_response(id, -32000, "Subscriptions not available")
            }
        }
        "resources/unsubscribe" => {
            if let Some(sub) = subscribed {
                let uri = request.params.as_ref()
                    .and_then(|p| p.get("uri"))
                    .and_then(Value::as_str)
                    .map(|s| s.to_string());
                if let Some(uri) = uri {
                    sub.lock().unwrap().remove(&uri);
                }
                ok_response(id, json!({}))
            } else {
                error_response(id, -32000, "Subscriptions not available")
            }
        }
        "completions/complete" => {
            let raw = request.params.as_ref()
                .ok_or_else(|| anyhow::anyhow!("missing params"))?;
            let params: mcp_common::CompletionParams = match serde_json::from_value(raw.clone()) {
                Ok(p) => p,
                Err(e) => return Ok(Some(error_response(id, -32602, &format!("Invalid params: {e}")))),
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
            ok_response(id, serde_json::to_value(completion)?)
        }
        "prompts/list" => {
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
            ok_response(id, serde_json::to_value(result)?)
        }
        "prompts/get" => {
            let guard = backend.lock().unwrap();
            let Some(ref gb) = *guard else {
                return Ok(Some(error_response(id, -32000, "Server not initialized")));
            };
            let params: mcp_common::GetPromptParams = match request.params
                .ok_or_else(|| anyhow::anyhow!("missing params"))
                .and_then(|p| serde_json::from_value(p).map_err(|e| anyhow::anyhow!("invalid params: {e}")))
            {
                Ok(p) => p,
                Err(e) => return Ok(Some(error_response(id, -32602, &e.to_string()))),
            };
            match params.name.as_str() {
                "analyze-file" => {
                    let file_path = match params.arguments.get("path").and_then(Value::as_str) {
                        Some(p) => p,
                        None => return Ok(Some(error_response(id, -32602, "Missing required argument: path"))),
                    };
                    let depth = params.arguments.get("depth").and_then(Value::as_u64).unwrap_or(3) as u32;
                    gb.reload_if_stale();

                    let impacts = gb.analyze_impact(file_path, depth);
                    let ctx = gb.get_file_context(file_path).await.unwrap_or(None);
                    let lessons = gb.get_file_lessons(file_path).await.unwrap_or_default();
                    let history = gb.get_historical_engram_solutions(file_path).await.unwrap_or_default();

                    let mut text = format!("# Analysis of {}\n\n", file_path);
                    // Context section
                    text.push_str("## File Context\n\n");
                    if let Some(ref c) = ctx {
                        text.push_str(&format!("- Language: {}\n", c.language));
                        text.push_str(&format!("- Functions: {}\n\n", c.functions.len()));
                        for f in &c.functions {
                            text.push_str(&format!("  - `{}` [{}] L{}-L{}\n", f.name, f.kind, f.start_line, f.end_line));
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
                            text.push_str(&format!("- {} ({} funcs, {} lessons)\n", entry.file_path, entry.function_count, entry.lesson_count));
                        }
                    }
                    // Lessons section
                    if !lessons.is_empty() {
                        text.push_str("\n## Lessons\n\n");
                        for entry in &lessons {
                            text.push_str(&format!("- [{}] {} :: {}\n  Context: {}\n  Solution: {}\n",
                                entry.kind, entry.file_path, entry.symbol_name, entry.error_context, entry.solution));
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
                    ok_response(id, serde_json::to_value(result)?)
                }
                "review-lessons" => {
                    let file_path = match params.arguments.get("file_path").and_then(Value::as_str) {
                        Some(p) => p,
                        None => return Ok(Some(error_response(id, -32602, "Missing required argument: file_path"))),
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
                    ok_response(id, serde_json::to_value(result)?)
                }
                "project-status" => {
                    gb.reload_if_stale();
                    let summary = gb.get_graph_summary().await.unwrap_or(ozymem_core::GraphSummary {
                        file_count: 0, function_count: 0, engram_count: 0,
                        native_ast_function_count: 0, extension_wasm_function_count: 0,
                        text_heuristic_function_count: 0, vertex_count: 0, edge_count: 0,
                        memory_usage: String::new(), lessons_without_embedding: 0,
                    });
                    let lessons = gb.recent_lessons(None, 10).await.unwrap_or_default();
                    let mut text = format!("# Project Status\n\n");
                    text.push_str(&format!("## Summary\n\n- Files: {}\n- Functions: {}\n- Lessons: {}\n- AST functions: {}\n- Text heuristic: {}\n\n",
                        summary.file_count, summary.function_count, summary.engram_count,
                        summary.native_ast_function_count, summary.text_heuristic_function_count));
                    if !lessons.is_empty() {
                        text.push_str("## Recent Lessons\n\n");
                        for (i, entry) in lessons.iter().enumerate() {
                            text.push_str(&format!("{}. [{}] {} :: {}\n   {}\n\n",
                                i + 1, entry.kind, entry.file_path, entry.symbol_name, entry.solution));
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
                    ok_response(id, serde_json::to_value(result)?)
                }
                _ => error_response(id, -32602, &format!("Unknown prompt: {}", params.name)),
            }
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

    // Count by severity
    let mut breaking = 0usize;
    let mut warnings = 0usize;
    let mut infos = 0usize;

    for entry in impacts {
        if entry.depth != current_depth {
            current_depth = entry.depth;
            text.push_str(&format!("\n  Depth {}:\n", current_depth));
        }
        let sev_tag = match entry.severity.as_str() {
            "breaking" => { breaking += 1; "[BREAKING]" }
            "warning" => { warnings += 1; "[WARN]" }
            _ => { infos += 1; "[INFO]" }
        };
        text.push_str(&format!(
            "    {:>10} {} (L{}-L{}) [{} | {} funcs, {} lessons]\n",
            sev_tag, entry.file_path, entry.start_line, entry.end_line,
            entry.language, entry.function_count, entry.lesson_count
        ));

        // Reason and suggestion
        text.push_str(&format!("           ├── reason: {}\n", entry.reason));
        text.push_str(&format!("           └── suggestion: {}\n", entry.suggestion));

        // Show key functions if available
        if !entry.functions.is_empty() {
            for f in &entry.functions {
                text.push_str(&format!("                ├── {}\n", f));
            }
        }
    }

    let total_funcs: i64 = impacts.iter().map(|e| e.function_count).sum();
    let total_lessons: i64 = impacts.iter().map(|e| e.lesson_count).sum();
    text.push_str(&format!(
        "\nTotal: {} files affected, {} functions, {} lessons registered\n",
        impacts.len(), total_funcs, total_lessons
    ));

    // Summary bar
    text.push_str(&format!(
        "Severity: {} [BREAKING] | {} [WARN] | {} [INFO]",
        breaking, warnings, infos
    ));

    text
}

fn format_file_context_enriched(
    context: Option<&ozymem_core::FileGraphContext>,
    file_path: &str,
    history: &[String],
    neighbors: Option<&ozymem_core::graph_backend::NeighborInfo>,
    last_commit: Option<&str>,
) -> String {
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

    // Graph neighbors section
    if let Some(n) = neighbors {
        output.push_str(&format!("\n\nDependents (files that import this): {}", n.incoming.len()));
        for dep in n.incoming.iter().take(5) {
            output.push_str(&format!("\n  ← {}", dep));
        }
        if n.incoming.len() > 5 {
            output.push_str(&format!("\n  ... and {} more", n.incoming.len() - 5));
        }
        output.push_str(&format!("\nDepends on (files this imports): {}", n.outgoing.len()));
        for dep in n.outgoing.iter().take(5) {
            output.push_str(&format!("\n  → {}", dep));
        }
        if n.outgoing.len() > 5 {
            output.push_str(&format!("\n  ... and {} more", n.outgoing.len() - 5));
        }
    }

    // Last git commit
    if let Some(commit) = last_commit {
        output.push_str(&format!("\n\nLast commit touching this file: {}", commit));
    }

    // Lessons for this file
    if !history.is_empty() {
        output.push_str(&format!("\n\nLessons recorded for this file ({}):", history.len()));
        for solution in history {
            output.push_str(&format!("\n- {}", solution));
        }
    }

    output
}

async fn get_last_commit(file_path: &str) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["log", "-1", "--format=%h %s (%ar)", "--", file_path])
        .output()
        .ok()?;
    if output.status.success() {
        let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !s.is_empty() { Some(s) } else { None }
    } else {
        None
    }
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
            data: None,
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

// ---------------------------------------------------------------------------
// Project management tools handler
// ---------------------------------------------------------------------------

async fn handle_project_tool(
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
                    content: vec![ContentBlock { kind: "text", text: body }],
                    is_error: None,
                })?,
            ))
        }

        "get_project_memories" => {
            let project_name = match tool_call.arguments.get("project_name").and_then(Value::as_str) {
                Some(n) => n,
                None => return Ok(error_response(id, -32602, "Missing required parameter: project_name")),
            };
            let query = tool_call.arguments.get("query").and_then(Value::as_str);
            let kind = tool_call.arguments.get("kind").and_then(Value::as_str);
            let limit = tool_call.arguments.get("limit").and_then(Value::as_u64).unwrap_or(20) as usize;

            let reg = ProjectRegistry::open()?;
            let project = match reg.get_project_by_name(project_name)? {
                Some(p) => p,
                None => return Ok(error_response(id, -32602, &format!("Project '{}' not found in registry", project_name))),
            };
            let db_path = Path::new(&project.path).join(".ozymem").join("memory.db");
            if !db_path.exists() {
                return Ok(error_response(id, -32602, &format!("No memory.db found for project '{}' at {}", project_name, db_path.display())));
            }

            let gb = GraphBackend::open(Some(&db_path.to_string_lossy()))?;
            gb.set_project_path(Some(&project.path));

            let lessons = if let Some(q) = query {
                gb.search_lessons(q, kind, limit).await?
            } else {
                gb.recent_lessons(kind, limit).await?
            };

            let mut body = format!("Memories for '{}' ({} entries):\n\n", project_name, lessons.len());
            if lessons.is_empty() {
                body.push_str("No lessons, decisions, conventions, gotchas, or module rules recorded for this project.\n");
            } else {
                for (i, entry) in lessons.iter().enumerate() {
                    let stale_tag = if entry.stale != 0 {
                        format!(" [STALE: {}]", entry.stale_reason.as_deref().unwrap_or("unknown"))
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
                    content: vec![ContentBlock { kind: "text", text: body }],
                    is_error: None,
                })?,
            ))
        }

        "delete_project" => {
            let project_name = match tool_call.arguments.get("project_name").and_then(Value::as_str) {
                Some(n) => n,
                None => return Ok(error_response(id, -32602, "Missing required parameter: project_name")),
            };
            let force = tool_call.arguments.get("force").and_then(Value::as_bool).unwrap_or(false);

            let reg = ProjectRegistry::open()?;
            let project = match reg.get_project_by_name(project_name)? {
                Some(p) => p,
                None => return Ok(error_response(id, -32602, &format!("Project '{}' not found in registry", project_name))),
            };

            let db_path = Path::new(&project.path).join(".ozymem").join("memory.db");
            let gb = GraphBackend::open(Some(&db_path.to_string_lossy()))?;
            gb.set_project_path(Some(&project.path));
            let lesson_count = gb.lesson_count().unwrap_or(0);

            if !force {
                // Preview mode
                let summary = gb.get_graph_summary().await.unwrap_or(ozymem_core::GraphSummary {
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
                        content: vec![ContentBlock { kind: "text", text: body }],
                        is_error: None,
                    })?,
                ));
            }

            // Execute deletion
            log("info", format!("[ozymem-server] deleting project '{}' (keeping lessons)", project_name));

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
                        content: vec![ContentBlock { kind: "text", text: body }],
                        is_error: None,
                    })?,
                ))
            } else {
                Ok(error_response(id, -32603, &format!("Failed to deregister project '{}'", project_name)))
            }
        }

        "suggest_stale_projects" => {
            let days = tool_call.arguments.get("days").and_then(Value::as_u64).unwrap_or(90) as i64;

            let reg = ProjectRegistry::open()?;
            let stale = reg.get_stale_projects(days)?;

            if stale.is_empty() {
                let body = format!("No projects have been SLEEPING for more than {days} days.");
                return Ok(ok_response(
                    id,
                    serde_json::to_value(ToolCallResult {
                        content: vec![ContentBlock { kind: "text", text: body }],
                        is_error: None,
                    })?,
                ));
            }

            let mut body = format!("Stale projects (SLEEPING > {} days, {} found):\n\n", days, stale.len());
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
                    content: vec![ContentBlock { kind: "text", text: body }],
                    is_error: None,
                })?,
            ))
        }

        _ => Ok(error_response(id, -32601, "Unknown project management tool")),
    }
}

// ---------------------------------------------------------------------------
// Package management tools handler
// ---------------------------------------------------------------------------

async fn handle_package_tool(
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
        "create_project" => {
            let name = match tool_call.arguments.get("name").and_then(Value::as_str) {
                Some(n) => n,
                None => return Ok(error_response(id, -32602, "Missing required parameter: name")),
            };
            let parent = tool_call.arguments.get("path").and_then(Value::as_str)
                .map(|s| PathBuf::from(s))
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
            let proj_type = tool_call.arguments.get("type").and_then(Value::as_str).unwrap_or("node");
            let packages = tool_call.arguments.get("packages").and_then(Value::as_array)
                .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect::<Vec<_>>())
                .unwrap_or_default();

            let project_dir = parent.join(name);
            if project_dir.exists() {
                return Ok(error_response(id, -32602, &format!("Directory already exists: {}", project_dir.display())));
            }

            std::fs::create_dir_all(&project_dir)?;
            log("info", format!("[ozymem-server] created directory {}", project_dir.display()));

            // Run package manager init
            match proj_type {
                "node" => {
                    let init_cmd = if cfg!(target_os = "windows") {
                        std::process::Command::new("cmd")
                            .args(["/C", "pnpm", "init", "-y"])
                            .current_dir(&project_dir)
                            .output()
                            .map(|o| o.status.success())
                            .unwrap_or(false)
                    } else {
                        std::process::Command::new("pnpm")
                            .args(["init", "-y"])
                            .current_dir(&project_dir)
                            .output()
                            .map(|o| o.status.success())
                            .unwrap_or(false)
                    };
                    if !init_cmd {
                        let fallback = if cfg!(target_os = "windows") {
                            std::process::Command::new("cmd")
                                .args(["/C", "npm", "init", "-y"])
                                .current_dir(&project_dir)
                                .output()
                        } else {
                            std::process::Command::new("npm")
                                .args(["init", "-y"])
                                .current_dir(&project_dir)
                                .output()
                        };
                        fallback?;
                    }
                }
                "rust" => {
                    std::process::Command::new("cargo")
                        .args(["init"])
                        .current_dir(&project_dir)
                        .output()?;
                }
                _ => {}
            }

            // Install packages
            let mut installed = Vec::new();
            if !packages.is_empty() {
                let cmd = if cfg!(target_os = "windows") { "cmd" } else { "sh" };
                let args: Vec<&str> = if cfg!(target_os = "windows") {
                    let mut a = vec!["/C", "pnpm", "add"];
                    a.extend(packages.iter().map(|s| s.as_str()));
                    a
                } else {
                    let mut a = vec!["pnpm", "add"];
                    a.extend(packages.iter().map(|s| s.as_str()));
                    a
                };
                let output = std::process::Command::new(cmd)
                    .args(&args)
                    .current_dir(&project_dir)
                    .output()?;
                if output.status.success() {
                    installed = packages.clone();
                    log("info", format!("[ozymem-server] installed {} packages in {}", installed.len(), project_dir.display()));
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    log("warn", format!("[ozymem-server] pnpm install warning: {stderr}"));
                }
            }

            // Register in ProjectRegistry
            let reg = ProjectRegistry::open()?;
            let path_str = ozymem_core::normalize_path(&project_dir.to_string_lossy());
            let _project = reg.register(name, &path_str)?;
            log("info", format!("[ozymem-server] registered project '{}'", name));

            // Open backend and scan
            let gb = GraphBackend::open_for_project(&project_dir)?;
            let resolved_str = project_dir.to_string_lossy().to_string();
            gb.full_scan(&resolved_str, None)?;

            let body = format!(
                "Project '{}' created at {}.\n  Type: {}\n  Packages installed: {}\n  Project registered and scanned.",
                name, path_str, proj_type,
                if installed.is_empty() { "none".to_string() } else { installed.join(", ") },
            );
            Ok(ok_response(id, serde_json::to_value(ToolCallResult {
                content: vec![ContentBlock { kind: "text", text: body }],
                is_error: None,
            })?))
        }

        "add_package" => {
            let project_name = match tool_call.arguments.get("project_name").and_then(Value::as_str) {
                Some(n) => n,
                None => return Ok(error_response(id, -32602, "Missing required parameter: project_name")),
            };
            let packages = match tool_call.arguments.get("packages").and_then(Value::as_array) {
                Some(arr) => arr.iter().filter_map(|v| v.as_str().map(String::from)).collect::<Vec<_>>(),
                None => return Ok(error_response(id, -32602, "Missing required parameter: packages")),
            };
            let dev = tool_call.arguments.get("dev").and_then(Value::as_bool).unwrap_or(false);

            let reg = ProjectRegistry::open()?;
            let project = match reg.get_project_by_name(project_name)? {
                Some(p) => p,
                None => return Ok(error_response(id, -32602, &format!("Project '{}' not found", project_name))),
            };

            let project_dir = PathBuf::from(&project.path);
            let cmd = if cfg!(target_os = "windows") { "cmd" } else { "sh" };
            let mut shell_args = vec!["/C", "pnpm", "add"];
            if dev {
                shell_args.push("-D");
            }
            for p in &packages {
                shell_args.push(p.as_str());
            }
            let output = std::process::Command::new(cmd)
                .args(&shell_args)
                .current_dir(&project_dir)
                .output()?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Ok(error_response(id, -32603, &format!("pnpm install failed: {stderr}")));
            }

            // Re-scan project to update package.json in the graph
            let gb = GraphBackend::open_for_project(&project_dir)?;
            let resolved = project_dir.to_string_lossy().to_string();
            gb.full_scan(&resolved, None)?;

            let body = format!(
                "Installed {} into '{}'{}.\nProject re-scanned.",
                packages.join(", "),
                project_name,
                if dev { " (dev dependency)" } else { "" },
            );
            log("info", format!("[ozymem-server] {body}"));
            Ok(ok_response(id, serde_json::to_value(ToolCallResult {
                content: vec![ContentBlock { kind: "text", text: body }],
                is_error: None,
            })?))
        }

        "remove_package" => {
            let project_name = match tool_call.arguments.get("project_name").and_then(Value::as_str) {
                Some(n) => n,
                None => return Ok(error_response(id, -32602, "Missing required parameter: project_name")),
            };
            let packages = match tool_call.arguments.get("packages").and_then(Value::as_array) {
                Some(arr) => arr.iter().filter_map(|v| v.as_str().map(String::from)).collect::<Vec<_>>(),
                None => return Ok(error_response(id, -32602, "Missing required parameter: packages")),
            };

            let reg = ProjectRegistry::open()?;
            let project = match reg.get_project_by_name(project_name)? {
                Some(p) => p,
                None => return Ok(error_response(id, -32602, &format!("Project '{}' not found", project_name))),
            };

            let project_dir = PathBuf::from(&project.path);
            let cmd = if cfg!(target_os = "windows") { "cmd" } else { "sh" };
            let mut shell_args = vec!["/C", "pnpm", "remove"];
            for p in &packages {
                shell_args.push(p.as_str());
            }
            let output = std::process::Command::new(cmd)
                .args(&shell_args)
                .current_dir(&project_dir)
                .output()?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Ok(error_response(id, -32603, &format!("pnpm remove failed: {stderr}")));
            }

            // Re-scan
            let gb = GraphBackend::open_for_project(&project_dir)?;
            let resolved = project_dir.to_string_lossy().to_string();
            gb.full_scan(&resolved, None)?;

            let body = format!(
                "Removed {} from '{}'.\nProject re-scanned.",
                packages.join(", "), project_name,
            );
            log("info", format!("[ozymem-server] {body}"));
            Ok(ok_response(id, serde_json::to_value(ToolCallResult {
                content: vec![ContentBlock { kind: "text", text: body }],
                is_error: None,
            })?))
        }

        "get_dependencies" => {
            let project_name = match tool_call.arguments.get("project_name").and_then(Value::as_str) {
                Some(n) => n,
                None => return Ok(error_response(id, -32602, "Missing required parameter: project_name")),
            };

            let reg = ProjectRegistry::open()?;
            let project = match reg.get_project_by_name(project_name)? {
                Some(p) => p,
                None => return Ok(error_response(id, -32602, &format!("Project '{}' not found", project_name))),
            };

            let pkg_path = Path::new(&project.path).join("package.json");
            if !pkg_path.exists() {
                return Ok(error_response(id, -32602, "No package.json found in project"));
            }

            let content = std::fs::read_to_string(&pkg_path)?;
            let pkg: serde_json::Value = serde_json::from_str(&content)?;

            let deps = pkg.get("dependencies").cloned().unwrap_or(json!({}));
            let dev_deps = pkg.get("devDependencies").cloned().unwrap_or(json!({}));
            let scripts = pkg.get("scripts").cloned().unwrap_or(json!({}));

            let body = format!(
                "Project: {}\n\nDependencies ({})",
                project_name,
                deps.as_object().map(|o| o.len()).unwrap_or(0),
            );
            let deps_text = if let Some(obj) = deps.as_object() {
                let mut lines: Vec<String> = obj.iter().map(|(k, v)| format!("  {}: {}", k, v.as_str().unwrap_or("?"))).collect();
                lines.sort();
                lines.join("\n")
            } else { String::new() };
            let dev_text = if let Some(obj) = dev_deps.as_object() {
                let mut lines: Vec<String> = obj.iter().map(|(k, v)| format!("  {}: {}", k, v.as_str().unwrap_or("?"))).collect();
                lines.sort();
                if lines.is_empty() { String::new() } else { format!("\n\nDev Dependencies ({}):\n{}", obj.len(), lines.join("\n")) }
            } else { String::new() };
            let scripts_text = if let Some(obj) = scripts.as_object() {
                let mut lines: Vec<String> = obj.iter().map(|(k, v)| format!("  {}: {}", k, v.as_str().unwrap_or("?"))).collect();
                lines.sort();
                if lines.is_empty() { String::new() } else { format!("\n\nScripts:\n{}", lines.join("\n")) }
            } else { String::new() };

            Ok(ok_response(id, serde_json::to_value(ToolCallResult {
                content: vec![ContentBlock { kind: "text", text: format!("{}\n{}\n{}{}", body, deps_text, dev_text, scripts_text) }],
                is_error: None,
            })?))
        }

        "run_script" => {
            let project_name = match tool_call.arguments.get("project_name").and_then(Value::as_str) {
                Some(n) => n,
                None => return Ok(error_response(id, -32602, "Missing required parameter: project_name")),
            };
            let script = match tool_call.arguments.get("script").and_then(Value::as_str) {
                Some(s) => s,
                None => return Ok(error_response(id, -32602, "Missing required parameter: script")),
            };
            let extra_args: Vec<&str> = tool_call.arguments.get("args").and_then(Value::as_array)
                .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
                .unwrap_or_default();

            let reg = ProjectRegistry::open()?;
            let project = match reg.get_project_by_name(project_name)? {
                Some(p) => p,
                None => return Ok(error_response(id, -32602, &format!("Project '{}' not found", project_name))),
            };

            let project_dir = PathBuf::from(&project.path);
            let cmd = if cfg!(target_os = "windows") { "cmd" } else { "sh" };
            let mut shell_args = vec!["/C", "pnpm", "run", script];
            shell_args.extend(extra_args);

            let output = match std::process::Command::new(cmd)
                .args(&shell_args)
                .current_dir(&project_dir)
                .output()
            {
                Ok(o) => o,
                Err(e) => return Ok(error_response(id, -32603, &format!("Failed to run script: {e}"))),
            };

            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let exit_code = output.status.code().unwrap_or(-1);

            let mut body = format!("Script '{}' in '{}' (exit code: {})", script, project_name, exit_code);
            if !stdout.is_empty() {
                body.push_str("\n\n--- stdout ---\n");
                body.push_str(&stdout);
            }
            if !stderr.is_empty() {
                body.push_str("\n\n--- stderr ---\n");
                body.push_str(&stderr);
            }

            Ok(ok_response(id, serde_json::to_value(ToolCallResult {
                content: vec![ContentBlock { kind: "text", text: body }],
                is_error: if output.status.success() { None } else { Some(true) },
            })?))
        }

        "analyze_package" => {
            let project_name = match tool_call.arguments.get("project_name").and_then(Value::as_str) {
                Some(n) => n,
                None => return Ok(error_response(id, -32602, "Missing required: project_name")),
            };
            let package_name = match tool_call.arguments.get("package_name").and_then(Value::as_str) {
                Some(n) => n,
                None => return Ok(error_response(id, -32602, "Missing required: package_name")),
            };

            let reg = ProjectRegistry::open()?;
            let project = match reg.get_project_by_name(project_name)? {
                Some(p) => p,
                None => return Ok(error_response(id, -32602, &format!("Project '{}' not found", project_name))),
            };

            let pkg_path = Path::new(&project.path).join("node_modules").join(package_name).join("package.json");
            if !pkg_path.exists() {
                return Ok(error_response(id, -32602, &format!("Package '{}' not found in node_modules", package_name)));
            }

            let content = std::fs::read_to_string(&pkg_path)?;
            let pkg: serde_json::Value = serde_json::from_str(&content)?;

            let name = pkg.get("name").and_then(Value::as_str).unwrap_or(package_name);
            let version = pkg.get("version").and_then(Value::as_str).unwrap_or("unknown");
            let description = pkg.get("description").and_then(Value::as_str).unwrap_or("");
            let license = pkg.get("license").and_then(Value::as_str).unwrap_or("unknown");

            let deps = pkg.get("dependencies").and_then(|d| d.as_object())
                .map(|o| {
                    let mut v: Vec<String> = o.iter().map(|(k, v)| format!("    {}: {}", k, v.as_str().unwrap_or("?"))).collect();
                    v.sort();
                    v.join("\n")
                }).unwrap_or_default();
            let dev_deps = pkg.get("devDependencies").and_then(|d| d.as_object())
                .map(|o| {
                    let mut v: Vec<String> = o.iter().map(|(k, v)| format!("    {}: {}", k, v.as_str().unwrap_or("?"))).collect();
                    v.sort();
                    v.join("\n")
                }).unwrap_or_default();

            let mut body = format!(
                "Package: {}\nVersion: {}\nLicense: {}\n",
                name, version, license,
            );
            if !description.is_empty() {
                body.push_str(&format!("Description: {}\n", description));
            }
            if !deps.is_empty() {
                let count = deps.lines().count();
                body.push_str(&format!("\nDependencies ({}):\n{}", count, deps));
            }
            if !dev_deps.is_empty() {
                let count = dev_deps.lines().count();
                body.push_str(&format!("\n\nDev Dependencies ({}):\n{}", count, dev_deps));
            }

            Ok(ok_response(id, serde_json::to_value(ToolCallResult {
                content: vec![ContentBlock { kind: "text", text: body }],
                is_error: None,
            })?))
        }

        "verify_dependencies" => {
            let project_name = match tool_call.arguments.get("project_name").and_then(Value::as_str) {
                Some(n) => n,
                None => return Ok(error_response(id, -32602, "Missing required: project_name")),
            };

            let reg = ProjectRegistry::open()?;
            let project = match reg.get_project_by_name(project_name)? {
                Some(p) => p,
                None => return Ok(error_response(id, -32602, &format!("Project '{}' not found", project_name))),
            };

            let project_dir = Path::new(&project.path);

            // Read package.json
            let pkg_path = project_dir.join("package.json");
            if !pkg_path.exists() {
                return Ok(error_response(id, -32602, "No package.json found"));
            }
            let pkg_content = std::fs::read_to_string(&pkg_path)?;
            let pkg: serde_json::Value = serde_json::from_str(&pkg_content)?;

            let mut declared: std::collections::HashSet<String> = std::collections::HashSet::new();
            for key in &["dependencies", "devDependencies", "peerDependencies"] {
                if let Some(obj) = pkg.get(key).and_then(|v| v.as_object()) {
                    for name in obj.keys() {
                        declared.insert(name.clone());
                    }
                }
            }

            // Scan source files for imports
            let mut used: std::collections::HashSet<String> = std::collections::HashSet::new();
            // Regex to extract module names from import statements
            let import_re = regex::Regex::new(r#"(?:from\s+['"]|require\s*\(\s*['"]|import\s+['"])([^'"]+)"#)
                .map_err(|e| anyhow::anyhow!("regex error: {e}"))?;

            // Walk source files, ignoring noise and .ozymignore/.gitignore patterns
            let ignore_patterns = ozymem_core::graph_backend::load_ignore_patterns(project_dir);
            let has_ignores = !ignore_patterns.is_empty();

            for entry in walkdir::WalkDir::new(project_dir)
                .into_iter()
                .filter_entry(|e: &walkdir::DirEntry| {
                    if ozymem_core::graph_backend::is_noise_dir(e.path()) { return false; }
                    if has_ignores && ozymem_core::graph_backend::path_matches_ignore(e.path(), &ignore_patterns, project_dir) { return false; }
                    true
                })
                .filter_map(|e: Result<walkdir::DirEntry, walkdir::Error>| e.ok())
            {
                let path = entry.path();
                if !path.is_file() { continue; }
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                if !matches!(ext, "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs") { continue; }

                let source = match std::fs::read_to_string(path) {
                    Ok(s) => s,
                    Err(_) => continue,
                };

                for cap in import_re.captures_iter(&source) {
                    let module = cap.get(1).map(|m| m.as_str()).unwrap_or("");
                    // Skip relative and bare-specifier imports
                    if module.starts_with('.') || module.starts_with('/') { continue; }
                    // Extract package name (handle @scoped/packages)
                    let pkg_name = if module.starts_with('@') {
                        let parts: Vec<&str> = module.splitn(3, '/').collect();
                        if parts.len() >= 2 { format!("{}/{}", parts[0], parts[1]) } else { module.to_string() }
                    } else {
                        module.split('/').next().unwrap_or(module).to_string()
                    };
                    used.insert(pkg_name);
                }
            }

            // Compare
            let mut missing = Vec::new();
            let mut unused = Vec::new();
            for p in &used {
                if !declared.contains(p) {
                    missing.push(p.clone());
                }
            }
            for p in &declared {
                if !used.contains(p) {
                    unused.push(p.clone());
                }
            }
            missing.sort();
            unused.sort();

            let mut body = format!("Dependency verification for '{}':\n", project_name);
            body.push_str(&format!("  Declared: {}\n", declared.len()));
            body.push_str(&format!("  Used in imports: {}\n\n", used.len()));

            if missing.is_empty() {
                body.push_str("✅ All used dependencies are declared in package.json\n");
            } else {
                body.push_str(&format!("❌ Missing from package.json (used but not declared, {})", missing.len()));
                for p in &missing { body.push_str(&format!("\n    - {}", p)); }
                body.push('\n');
            }
            if unused.is_empty() {
                body.push_str("✅ All declared dependencies are used\n");
            } else {
                body.push_str(&format!("⚠ Unused in source (declared but not imported, {})", unused.len()));
                for p in &unused { body.push_str(&format!("\n    - {}", p)); }
                body.push('\n');
            }

            Ok(ok_response(id, serde_json::to_value(ToolCallResult {
                content: vec![ContentBlock { kind: "text", text: body }],
                is_error: None,
            })?))
        }

        _ => Ok(error_response(id, -32601, "Unknown package management tool")),
    }
}

/// Compute days since a date string like "2026-02-15T10:30:00" or "2026-02-15 10:30:00".
fn days_since_str(date_str: Option<&str>) -> i64 {
    let s = match date_str {
        Some(s) => s,
        None => return 0,
    };
    let date_part = s.replace('T', " ").split(' ').next().unwrap_or("").to_string();
    let parts: Vec<i64> = date_part.split('-').filter_map(|p| p.parse().ok()).collect();
    if parts.len() != 3 {
        return 0;
    }
    let (y, m, d) = (parts[0], parts[1], parts[2]);

    // Days since epoch (1970-01-01) for the given date
    let mut total = 0i64;
    for year in 1970..y {
        let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
        total += if leap { 366 } else { 365 };
    }
    for month in 1..m {
        let dim = match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 => {
                let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
                if leap { 29 } else { 28 }
            }
            _ => 0,
        };
        total += dim;
    }
    total += d - 1;

    // Days since epoch for now
    let now_days = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|dur| dur.as_secs() as i64 / 86400)
        .unwrap_or(0);

    (now_days - total).max(0)
}

/// Detect tree-sitter language from file path.
fn detect_lang(path: &str) -> ozymem_parser::SupportedLanguage {
    match Path::new(path).extension().and_then(|e| e.to_str()) {
        Some("py") => ozymem_parser::SupportedLanguage::Python,
        Some("go") => ozymem_parser::SupportedLanguage::Go,
        Some("rs") => ozymem_parser::SupportedLanguage::Rust,
        Some("js") | Some("jsx") => ozymem_parser::SupportedLanguage::JavaScript,
        Some("ts") | Some("tsx") => ozymem_parser::SupportedLanguage::TypeScriptReact,
        Some("sql") => ozymem_parser::SupportedLanguage::SQL,
        _ => ozymem_parser::SupportedLanguage::Unknown,
    }
}

async fn handle_learn_from_changes(
    backend: &GraphBackend,
    tool_call: &mcp_common::ToolCallParams,
    notifier: Option<&Notifier>,
    subscribed: Option<&Arc<Mutex<HashSet<String>>>>,
) -> anyhow::Result<ToolCallResult> {
    let message = tool_call.arguments.get("message")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("missing message"))?;
    let from = tool_call.arguments.get("from")
        .and_then(Value::as_str)
        .unwrap_or("HEAD~1");
    let to = tool_call.arguments.get("to")
        .and_then(Value::as_str)
        .unwrap_or("HEAD");
    let preview = tool_call.arguments.get("preview")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let max_impact = tool_call.arguments.get("max_impact")
        .and_then(Value::as_u64)
        .unwrap_or(5) as usize;

    let project_path = backend.project_path()
        .ok_or_else(|| anyhow::anyhow!("no project path set"))?;

    if !Path::new(&project_path).join(".git").exists() {
        return Ok(ToolCallResult {
            content: vec![ContentBlock { kind: "text", text: "Not a git repository — learn_from_changes requires git".to_string() }],
            is_error: Some(true),
        });
    }

    let output = std::process::Command::new("git")
        .args(["diff", "--name-only", "--diff-filter=ACDMR", from, to, "--"])
        .current_dir(&project_path)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Ok(ToolCallResult {
            content: vec![ContentBlock { kind: "text", text: format!("Git diff failed: {stderr}") }],
            is_error: Some(true),
        });
    }

    let files: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();

    if files.is_empty() {
        return Ok(ToolCallResult {
            content: vec![ContentBlock { kind: "text", text: "No changes detected between the specified refs".to_string() }],
            is_error: None,
        });
    }

    let mut entries: Vec<String> = Vec::new();
    let mut parsed_files: Vec<String> = Vec::new();

    for file in &files {
        let lang = detect_lang(file);
        if lang == ozymem_parser::SupportedLanguage::Unknown { continue; }

        let old_output = std::process::Command::new("git")
            .args(["show", &format!("{from}:{file}")])
            .current_dir(&project_path)
            .output();

        let old_source = match old_output {
            Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).to_string(),
            _ => continue,
        };

        let new_output = std::process::Command::new("git")
            .args(["show", &format!("{to}:{file}")])
            .current_dir(&project_path)
            .output();

        let new_source = match new_output {
            Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).to_string(),
            _ => String::new(),
        };

        let old_parsed = parse_source(file, lang, &old_source).ok();
        let new_parsed = parse_source(file, lang, &new_source).ok();

        let old_funcs: Vec<&ozymem_parser::ExtractedFunction> = old_parsed.iter()
            .flat_map(|p| p.functions.iter())
            .collect();
        let new_funcs: Vec<&ozymem_parser::ExtractedFunction> = new_parsed.iter()
            .flat_map(|p| p.functions.iter())
            .collect();

        let old_names: HashSet<&str> = old_funcs.iter().map(|f| f.name.as_str()).collect();
        let new_names: HashSet<&str> = new_funcs.iter().map(|f| f.name.as_str()).collect();

        let mut file_had_changes = false;

        // New functions → lesson (confidence: 0.92 — tree-sitter confirms new AST nodes)
        for f in &new_funcs {
            if !old_names.contains(f.name.as_str()) {
                file_had_changes = true;
                let kind_str = match f.kind {
                    ozymem_parser::SymbolKind::Function => "Function",
                    ozymem_parser::SymbolKind::Class => "Class",
                };
                let solution = format!("New {} `{}` at line {} in {}", kind_str, f.name, f.start_line, file);
                if !preview {
                    backend.record_entry(file, Some(&f.name), message, &solution, "lesson").await?;
                }
                entries.push(format!("📘 {}: {} `{}` (line {}) [confidence: 0.92]", file, kind_str, f.name, f.start_line));
            }
        }

        // Removed functions → decision (confidence: 0.88 — tree-sitter confirms absence)
        for f in &old_funcs {
            if !new_names.contains(f.name.as_str()) {
                file_had_changes = true;
                let kind_str = match f.kind {
                    ozymem_parser::SymbolKind::Function => "Function",
                    ozymem_parser::SymbolKind::Class => "Class",
                };
                let solution = format!("{} `{}` was removed from {} (was at line {})", kind_str, f.name, file, f.start_line);
                if !preview {
                    backend.record_entry(file, Some(&f.name), message, &solution, "decision").await?;
                }
                entries.push(format!("📗 {}: removed {} `{}` [confidence: 0.88]", file, kind_str, f.name));
            }
        }

        // Modified functions (same name, different line range) → convention
        // Confidence proportional to line-range delta: more change = higher confidence
        for f in &new_funcs {
            if let Some(old_f) = old_funcs.iter().find(|o| o.name == f.name && (o.start_line != f.start_line || o.end_line != f.end_line)) {
                file_had_changes = true;
                let kind_str = match f.kind {
                    ozymem_parser::SymbolKind::Function => "Function",
                    ozymem_parser::SymbolKind::Class => "Class",
                };
                let old_len = old_f.end_line - old_f.start_line;
                let new_len = f.end_line - f.start_line;
                let delta = new_len.abs_diff(old_len);
                let max_len = old_len.max(new_len).max(1);
                let confidence = 0.65 + 0.30 * (delta as f64 / max_len as f64).min(1.0);
                let solution = format!("{} `{}` in {} changed (was L{}-L{}, now L{}-L{})",
                    kind_str, f.name, file, old_f.start_line, old_f.end_line, f.start_line, f.end_line);
                if !preview {
                    backend.record_entry(file, Some(&f.name), message, &solution, "convention").await?;
                }
                entries.push(format!("📙 {}: {} `{}` modified (L{}-L{} → L{}-L{}) [confidence: {:.2}]",
                    file, kind_str, f.name, old_f.start_line, old_f.end_line, f.start_line, f.end_line, confidence));
            }
        }

        if file_had_changes {
            parsed_files.push(file.clone());
        }
    }

    // Impact analysis via graph neighbors
    let mut impact_lines: Vec<String> = Vec::new();
    for file in &parsed_files {
        if let Ok(neighbors) = backend.get_graph_neighbors(file).await {
            let dependent_count = neighbors.incoming.len();
            let dep_on_count = neighbors.outgoing.len();
            if dependent_count > 0 || dep_on_count > 0 {
                let mut line = format!("  {file}:");
                if dependent_count > 0 {
                    let show: Vec<&String> = neighbors.incoming.iter().take(max_impact).collect();
                    for p in &show {
                        line.push_str(&format!("\n    ← {}", p));
                    }
                    if dependent_count > max_impact {
                        line.push_str(&format!("\n    ... and {} more dependents", dependent_count - max_impact));
                    }
                }
                if dep_on_count > 0 {
                    let show: Vec<&String> = neighbors.outgoing.iter().take(max_impact).collect();
                    for p in &show {
                        line.push_str(&format!("\n    → {}", p));
                    }
                    if dep_on_count > max_impact {
                        line.push_str(&format!("\n    ... and {} more dependencies", dep_on_count - max_impact));
                    }
                }
                impact_lines.push(line);
            }
        }
    }

    if !preview && !entries.is_empty() {
        notify_subscribed(subscribed, notifier, &["ozymem://summary", "ozymem://recent-lessons"]);
    }

    let body = if entries.is_empty() {
        format!("No function-level changes detected in {} files (diff {from}..{to})", files.len())
    } else {
        let header = if preview {
            format!("🔍 PREVIEW — {} entries from {} files (diff {from}..{to}):\n", entries.len(), parsed_files.len())
        } else {
            format!("Recorded {} entries from {} files (diff {from}..{to}):\n", entries.len(), parsed_files.len())
        };
        let mut body = header;
        for e in &entries {
            body.push_str(e);
            body.push('\n');
        }
        if !impact_lines.is_empty() {
            body.push_str("\n═══ Impact (graph neighbors) ═══\n");
            for l in &impact_lines {
                body.push_str(l);
                body.push('\n');
            }
            body.push_str("⚠ Run analyze_impact for full transitive depth.\n");
        }
        body
    };

    Ok(ToolCallResult {
        content: vec![ContentBlock { kind: "text", text: body }],
        is_error: None,
    })
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
        handle_request(&backend, init_req, None, None).await.unwrap();

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
        handle_request(&backend, init_req, None, None).await.unwrap();

        let main_abs = tmp_root.join("main.rs").to_string_lossy().to_string();

        // Run scan, then record two lessons on the same file with overlapping query keywords
        {
            let guard = backend.lock().unwrap();
            let gb = guard.as_ref().unwrap();
            gb.full_scan(&tmp_root.to_string_lossy(), None).ok();

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
        handle_request(&backend, init_req, None, None).await.unwrap();

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
        let tmp_root = std::env::temp_dir().join(format!("ozymem_test_init_caps_{}", std::process::id()));
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
        assert!(result.get("capabilities").and_then(|c| c.get("resources")).is_some(),
            "initialize should declare resources capability");
        assert!(result.get("capabilities").and_then(|c| c.get("prompts")).is_some(),
            "initialize should declare prompts capability");
        assert!(result.get("capabilities").and_then(|c| c.get("logging")).is_some(),
            "initialize should declare logging capability");
        assert!(result.get("capabilities").and_then(|c| c.get("tools")).is_some(),
            "initialize should declare tools capability");

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
        let tmp_root = std::env::temp_dir().join(format!("ozymem_test_res_list_{}", std::process::id()));
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
        handle_request(&backend, init_req, None, None).await.unwrap();

        let list_req = mcp_common::JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(2)),
            method: "resources/list".to_string(),
            params: None,
        };
        let response = handle_request(&backend, list_req, None, None).await.unwrap();
        let resp = response.unwrap();
        assert!(resp.error.is_none(), "resources/list should not error");

        let result = resp.result.unwrap();
        let resources = result["resources"].as_array().unwrap();
        assert!(!resources.is_empty(), "should list at least one resource");

        let uris: Vec<&str> = resources.iter().filter_map(|r| r["uri"].as_str()).collect();
        assert!(uris.contains(&"ozymem://summary"), "should include summary");
        assert!(uris.contains(&"ozymem://recent-lessons"), "should include recent-lessons");

        std::fs::remove_dir_all(&tmp_root).ok();
    }

    #[tokio::test]
    async fn test_resources_read_summary() {
        let backend: Arc<Mutex<Option<GraphBackend>>> = Arc::new(Mutex::new(None));
        let tmp_root = std::env::temp_dir().join(format!("ozymem_test_res_sum_{}", std::process::id()));
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
        handle_request(&backend, init_req, None, None).await.unwrap();

        let read_req = mcp_common::JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(2)),
            method: "resources/read".to_string(),
            params: Some(serde_json::json!({ "uri": "ozymem://summary" })),
        };
        let response = handle_request(&backend, read_req, None, None).await.unwrap();
        let resp = response.unwrap();
        assert!(resp.error.is_none(), "resources/read summary should not error: {:?}", resp.error);

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
        let tmp_root = std::env::temp_dir().join(format!("ozymem_test_res_ctx_{}", std::process::id()));
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
        handle_request(&backend, init_req, None, None).await.unwrap();

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
        let response = handle_request(&backend, read_req, None, None).await.unwrap();
        let resp = response.unwrap();
        assert!(resp.error.is_none(), "resources/read file should not error: {:?}", resp.error);

        let result = resp.result.unwrap();
        let contents = result["contents"].as_array().unwrap();
        assert_eq!(contents.len(), 1);
        let text = contents[0]["text"].as_str().unwrap();
        assert!(text.contains("hello"), "file context should contain function name");

        std::fs::remove_dir_all(&tmp_root).ok();
    }

    #[tokio::test]
    async fn test_resources_read_unknown_uri_returns_error() {
        let backend: Arc<Mutex<Option<GraphBackend>>> = Arc::new(Mutex::new(None));
        let tmp_root = std::env::temp_dir().join(format!("ozymem_test_res_bad_{}", std::process::id()));
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
        handle_request(&backend, init_req, None, None).await.unwrap();

        let read_req = mcp_common::JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(2)),
            method: "resources/read".to_string(),
            params: Some(serde_json::json!({ "uri": "ozymem://nonexistent" })),
        };
        let response = handle_request(&backend, read_req, None, None).await.unwrap();
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

        let uri_templates: Vec<&str> = templates.iter().filter_map(|t| t["uriTemplate"].as_str()).collect();
        assert!(uri_templates.contains(&"ozymem://file/{path}"), "should include file template");
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
        assert!(names.contains(&"analyze-file"), "should include analyze-file");
        assert!(names.contains(&"review-lessons"), "should include review-lessons");
    }

    #[tokio::test]
    async fn test_prompts_get_before_initialize_returns_error() {
        let backend: Arc<Mutex<Option<GraphBackend>>> = Arc::new(Mutex::new(None));
        let request = mcp_common::JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(1)),
            method: "prompts/get".to_string(),
            params: Some(serde_json::json!({ "name": "review-lessons", "arguments": { "file_path": "/test.rs" } })),
        };
        let response = handle_request(&backend, request, None, None).await.unwrap();
        let resp = response.unwrap();
        assert!(resp.error.is_some());
        assert_eq!(resp.error.as_ref().unwrap().code, -32000);
    }

    #[tokio::test]
    async fn test_prompts_get_analyze_file() {
        let backend: Arc<Mutex<Option<GraphBackend>>> = Arc::new(Mutex::new(None));
        let tmp_root = std::env::temp_dir().join(format!("ozymem_test_prompt_af_{}", std::process::id()));
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
        handle_request(&backend, init_req, None, None).await.unwrap();
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
            params: Some(serde_json::json!({ "name": "analyze-file", "arguments": { "path": main_abs, "depth": 1 } })),
        };
        let response = handle_request(&backend, prompt_req, None, None).await.unwrap();
        let resp = response.unwrap();
        assert!(resp.error.is_none(), "prompts/get analyze-file should not error: {:?}", resp.error);

        let result = resp.result.unwrap();
        let messages = result["messages"].as_array().unwrap();
        assert!(!messages.is_empty(), "should return at least one message");
        let text = messages[0]["content"]["text"].as_str().unwrap_or("");
        assert!(text.contains("Analysis of"), "should contain analysis header");
        assert!(text.contains("analyze_me"), "should mention function name");

        std::fs::remove_dir_all(&tmp_root).ok();
    }

    #[tokio::test]
    async fn test_prompts_get_review_lessons() {
        let backend: Arc<Mutex<Option<GraphBackend>>> = Arc::new(Mutex::new(None));
        let tmp_root = std::env::temp_dir().join(format!("ozymem_test_prompt_rl_{}", std::process::id()));
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
        handle_request(&backend, init_req, None, None).await.unwrap();
        let main_abs = tmp_root.join("main.rs").to_string_lossy().to_string();

        {
            let guard = backend.lock().unwrap();
            let gb = guard.as_ref().unwrap();
            gb.full_scan(&tmp_root.to_string_lossy(), None).ok();
            gb.record_lesson(&main_abs, Some("main"), "test error", "test solution").await.unwrap();
        }

        let prompt_req = mcp_common::JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(2)),
            method: "prompts/get".to_string(),
            params: Some(serde_json::json!({ "name": "review-lessons", "arguments": { "file_path": main_abs } })),
        };
        let response = handle_request(&backend, prompt_req, None, None).await.unwrap();
        let resp = response.unwrap();
        assert!(resp.error.is_none(), "prompts/get review-lessons should not error: {:?}", resp.error);

        let result = resp.result.unwrap();
        let messages = result["messages"].as_array().unwrap();
        let text = messages[0]["content"]["text"].as_str().unwrap_or("");
        assert!(text.contains("test solution"), "should contain lesson solution");
        assert!(text.contains("test error"), "should contain lesson error context");

        std::fs::remove_dir_all(&tmp_root).ok();
    }

    #[tokio::test]
    async fn test_prompts_get_unknown_prompt_returns_error() {
        let backend: Arc<Mutex<Option<GraphBackend>>> = Arc::new(Mutex::new(None));
        let tmp_root = std::env::temp_dir().join(format!("ozymem_test_prompt_unk_{}", std::process::id()));
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
        handle_request(&backend, init_req, None, None).await.unwrap();

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
        let response = handle_request(&backend, init_req, None, None).await.unwrap();
        let resp = response.unwrap();
        let caps = resp.result.unwrap()["capabilities"].clone();
        assert_eq!(caps["sampling"], json!({}), "sampling capability should be present");
        assert_eq!(caps["completions"], json!({}), "completions capability should be present");
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
        assert!(tools.len() > 10, "should have many tools");
        assert!(result.get("nextCursor").is_none(), "no cursor for first page without params");
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
        assert!(result.get("nextCursor").is_some() || result["tools"].as_array().unwrap().len() > 0);
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
        assert!(!results.is_empty(), "should find at least one matching file");
        assert!(results[0].contains("test_main"), "should match test_main.rs");

        let results = backend.complete_file_path("nonexistent_xyz", 5).unwrap();
        assert!(results.is_empty(), "should return empty for no matches");

        std::fs::remove_dir_all(&dir).ok();
    }
}
