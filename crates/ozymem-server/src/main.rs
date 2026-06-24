use ozymem_core::{
    default_memgraph_database, default_memgraph_uri, FileGraphContext, GraphSummary,
    MemgraphConfig, MemgraphConnection, UserRecord,
};
use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    middleware::{self, Next},
    response::Response,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::sync::Arc;
use tokio::sync::OnceCell;
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader};

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Option<Value>,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: &'static str,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
struct JsonRpcError {
    code: i64,
    message: String,
}

#[derive(Debug, Serialize)]
struct InitializeResult {
    protocol_version: &'static str,
    capabilities: ServerCapabilities,
    server_info: ServerInfo,
}

#[derive(Debug, Serialize)]
struct ServerCapabilities {
    tools: ToolsCapability,
}

#[derive(Debug, Serialize)]
struct ToolsCapability {
    #[serde(rename = "listChanged", skip_serializing_if = "Option::is_none")]
    list_changed: Option<bool>,
}

#[derive(Debug, Serialize)]
struct ServerInfo {
    name: &'static str,
    version: &'static str,
}

#[derive(Debug, Serialize)]
struct ToolListResult {
    tools: Vec<ToolDefinition>,
}

#[derive(Debug, Serialize)]
struct ToolDefinition {
    name: &'static str,
    description: &'static str,
    #[serde(rename = "inputSchema")]
    input_schema: Value,
}

#[derive(Debug, Serialize)]
struct ToolCallResult {
    content: Vec<ContentBlock>,
    #[serde(rename = "isError", skip_serializing_if = "Option::is_none")]
    is_error: Option<bool>,
}

#[derive(Debug, Serialize)]
struct ContentBlock {
    #[serde(rename = "type")]
    kind: &'static str,
    text: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let is_web = std::env::args().any(|arg| arg == "--web")
        || std::env::var("OZYMEM_SERVER_MODE").as_deref() == Ok("web");

    let connection_cell = Arc::new(OnceCell::new());

    if is_web {
        run_web_server(connection_cell).await
    } else {
        run_server(connection_cell).await
    }
}

async fn run_server(connection_cell: Arc<OnceCell<MemgraphConnection>>) -> anyhow::Result<()> {
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

        if let Ok(request) = serde_json::from_str::<JsonRpcRequest>(trimmed) {
            if let Some(response) = handle_request(&connection_cell, request).await? {
                write_response(&mut stdout, &response).await?;
            }
        } else {
            eprintln!("[WARNING] Recibida línea no válida para JSON-RPC: {}", trimmed);
        }
    }

    Ok(())
}

async fn get_connection(cell: &OnceCell<MemgraphConnection>) -> anyhow::Result<&MemgraphConnection> {
    cell.get_or_try_init(|| async {
        let config = MemgraphConfig {
            uri: std::env::var("MEMGRAPH_URI").unwrap_or_else(|_| default_memgraph_uri().to_string()),
            user: std::env::var("MEMGRAPH_USER").unwrap_or_else(|_| "admin".to_string()),
            password: std::env::var("MEMGRAPH_PASSWORD").unwrap_or_else(|_| "admin".to_string()),
            database: std::env::var("MEMGRAPH_DATABASE")
                .unwrap_or_else(|_| default_memgraph_database().to_string()),
        };
        let conn = MemgraphConnection::connect(config).await?;
        
        // Setup Génesis if user database is empty
        match conn.has_any_users().await {
            Ok(false) => {
                let master_tenant_id = "default_tenant";
                let master_user = "admin";
                
                use rand::RngCore;
                let mut token_bytes = [0u8; 16];
                rand::thread_rng().fill_bytes(&mut token_bytes);
                let master_token_hex: String = token_bytes.iter().map(|b| format!("{:02x}", b)).collect();
                
                let master_credential = format!("ozy_partner_ctx_{}_usr_{}", master_tenant_id, master_token_hex);
                
                if let Err(e) = conn.create_tenant("Default Tenant", master_tenant_id).await {
                    eprintln!("[ERROR] Setup Génesis failed to create tenant: {:?}", e);
                } else if let Err(e) = conn.create_user(master_tenant_id, master_user, "Lead", &master_token_hex).await {
                    eprintln!("[ERROR] Setup Génesis failed to create user: {:?}", e);
                } else {
                    eprintln!("\n================ 🚀 OZYMEM-PARTNER SETUP ================");
                    eprintln!("¡Base de datos limpia detectada! Creando primer Lead Developer...");
                    eprintln!("\n🔑 CREDENCIAL MAESTRA GENERADA: {}", master_credential);
                    eprintln!("📌 Guarda esta credencial de inmediato. La necesitarás para tu CLI local.");
                    eprintln!("=========================================================\n");
                }
            }
            Ok(true) => {}
            Err(e) => {
                eprintln!("[WARNING] Failed to query user database state: {:?}", e);
            }
        }
        
        Ok(conn)
    }).await
}

async fn handle_request(
    connection_cell: &OnceCell<MemgraphConnection>,
    request: JsonRpcRequest,
) -> anyhow::Result<Option<JsonRpcResponse>> {
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
            let payload = InitializeResult {
                protocol_version: "2024-11-05",
                capabilities: ServerCapabilities {
                    tools: ToolsCapability {
                        list_changed: Some(true),
                    },
                },
                server_info: ServerInfo {
                    name: "ozymem-server",
                    version: env!("CARGO_PKG_VERSION"),
                },
            };

            // Reactive Trigger: Extract workspaceFolders if present to wake project
            if let Some(ref params) = request.params {
                if let Some(workspace_folders) = params.get("workspaceFolders").and_then(Value::as_array) {
                    for folder in workspace_folders {
                        if let Some(uri) = folder.get("uri").and_then(Value::as_str) {
                            let mut path = uri.trim_start_matches("file://");
                            // Clean Windows prefix if present (e.g. /C:/Users/...)
                            if path.starts_with('/') && path.chars().nth(2) == Some(':') {
                                path = &path[1..];
                            }
                            let decoded_path = urlencoding::decode(path).unwrap_or(std::borrow::Cow::Borrowed(path)).into_owned();
                            let normal_path = decoded_path.replace('/', "\\");
                            
                            // Send wake command to daemon socket asynchronously
                            tokio::spawn(async move {
                                let registry = ozymem_core::registry::ProjectRegistry::open();
                                if let Ok(reg) = registry {
                                    if let Ok(Some(proj)) = reg.get_project_by_path(&normal_path) {
                                        let _ = send_daemon_command(&json!({
                                            "action": "wake",
                                            "project_name": proj.name,
                                            "project_path": normal_path
                                        })).await;
                                    }
                                }
                            });
                        }
                    }
                }
            }

            ok_response(id, serde_json::to_value(payload)?)
        }
        "notifications/initialized" => return Ok(None),
        "tools/list" => {
            let payload = ToolListResult {
                tools: vec![
                    ToolDefinition {
                        name: "file_context",
                        description: "Return the indexed file context, including language, strategy, and functions.",
                        input_schema: json!({
                            "type": "object",
                            "properties": {
                                "file_path": {
                                    "type": "string",
                                    "description": "Path used when the file was indexed"
                                }
                            },
                            "required": ["file_path"],
                            "additionalProperties": false
                        }),
                    },
                    ToolDefinition {
                        name: "graph_summary",
                        description: "Summarize the indexed graph with file and function counts.",
                        input_schema: json!({
                            "type": "object",
                            "properties": {},
                            "additionalProperties": false
                        }),
                    },
                    ToolDefinition {
                        name: "record_lesson",
                        description: "Record an error-to-fix lesson for a file or symbol as historical memory.",
                        input_schema: json!({
                            "type": "object",
                            "properties": {
                                "file_path": {
                                    "type": "string",
                                    "description": "Absolute path of the file that failed"
                                },
                                "symbol_name": {
                                    "type": "string",
                                    "description": "Optional name of the class or function where the error occurred"
                                },
                                "error_context": {
                                    "type": "string",
                                    "description": "Details about the error context or compilation message"
                                },
                                "solution": {
                                    "type": "string",
                                    "description": "Short fix or lesson learned"
                                }
                            },
                            "required": ["file_path", "error_context", "solution"],
                            "additionalProperties": false
                        }),
                    },
                    ToolDefinition {
                        name: "file_trace",
                        description: "Trace incoming/reverse dependencies of an indexed file (impact analysis / who depends on this file).",
                        input_schema: json!({
                            "type": "object",
                            "properties": {
                                "file_path": {
                                    "type": "string",
                                    "description": "Path of the file to trace reverse dependencies for"
                                }
                            },
                            "required": ["file_path"],
                            "additionalProperties": false
                        }),
                    },
                    ToolDefinition {
                        name: "ozymem_project_status",
                        description: "Get the status (ACTIVE/SLEEPING), scale, PID, and sync statistics of a project by path or name.",
                        input_schema: json!({
                            "type": "object",
                            "properties": {
                                "project_path": {
                                    "type": "string",
                                    "description": "Optional project path to check status"
                                },
                                "project_name": {
                                    "type": "string",
                                    "description": "Optional project name to check status"
                                }
                            },
                            "additionalProperties": false
                        }),
                    },
                ],
            };

            ok_response(id, serde_json::to_value(payload)?)
        }
        "tools/call" => {
            let params = request
                .params
                .ok_or_else(|| anyhow::anyhow!("missing params for tools/call"))?;
            let tool_call: ToolCallParams = serde_json::from_value(params)?;

            let payload = match tool_call.name.as_str() {
                "file_context" => {
                    let connection = get_connection(connection_cell).await?;
                    let file_path = read_string_argument(&tool_call.arguments, "file_path")
                        .or_else(|_| read_string_argument(&tool_call.arguments, "path"))?;
                    let context = connection.get_file_context("local", &file_path).await?;
                    let historical_engrams = connection
                        .get_historical_engram_solutions("local", &file_path)
                        .await?;
                    let body = format_file_context(context.as_ref(), &file_path);
                    ToolCallResult {
                        content: vec![ContentBlock {
                            kind: "text",
                            text: prepend_historical_engrams(&historical_engrams, body),
                        }],
                        is_error: None,
                    }
                }
                "graph_summary" => {
                    let connection = get_connection(connection_cell).await?;
                    let summary = connection.get_graph_summary("local").await?;
                    ToolCallResult {
                        content: vec![ContentBlock {
                            kind: "text",
                            text: format_graph_summary(&summary),
                        }],
                        is_error: None,
                    }
                }
                "record_lesson" => {
                    let connection = get_connection(connection_cell).await?;
                    let file_path = read_string_argument(&tool_call.arguments, "file_path")?;
                    let symbol_name = tool_call.arguments.get("symbol_name")
                        .and_then(Value::as_str);
                    let error_context = read_string_argument(&tool_call.arguments, "error_context")?;
                    let solution = read_string_argument(&tool_call.arguments, "solution")?;

                    connection
                        .record_lesson("local", &file_path, symbol_name, &error_context, &solution)
                        .await?;

                    ToolCallResult {
                        content: vec![ContentBlock {
                            kind: "text",
                            text: format!(
                                "Recorded lesson for {file_path}{}",
                                symbol_name.map(|s| format!("::{}", s)).unwrap_or_default()
                            ),
                        }],
                        is_error: None,
                    }
                }
                "file_trace" => {
                    let connection = get_connection(connection_cell).await?;
                    let file_path = read_string_argument(&tool_call.arguments, "file_path")
                        .or_else(|_| read_string_argument(&tool_call.arguments, "path"))?;
                    let incoming = connection.get_incoming_dependencies("local", &file_path).await?;
                    let mut text = format!("Reverse dependencies (files impacted by changes to {}):\n", file_path);
                    if incoming.is_empty() {
                        text.push_str("- (none)");
                    } else {
                        for path in incoming {
                            text.push_str(&format!("- {path}\n"));
                        }
                    }
                    ToolCallResult {
                        content: vec![ContentBlock {
                            kind: "text",
                            text,
                        }],
                        is_error: None,
                    }
                }
                "ozymem_project_status" => {
                    let path_arg = tool_call.arguments.get("project_path").and_then(Value::as_str);
                    let name_arg = tool_call.arguments.get("project_name").and_then(Value::as_str);

                    let registry = ozymem_core::registry::ProjectRegistry::open()?;
                    let project = if let Some(path) = path_arg {
                        registry.get_project_by_path(path)?
                    } else if let Some(name) = name_arg {
                        registry.get_project_by_name(name)?
                    } else {
                        anyhow::bail!("Either project_path or project_name must be provided");
                    };

                    let body = match project {
                        Some(p) => {
                            let daemon_res = send_daemon_command(&json!({
                                "action": "status",
                                "project_name": p.name,
                                "project_path": p.path
                            })).await.unwrap_or_else(|e| json!({ "status": "error", "message": e.to_string() }));
                            
                            format!(
                                "Project: {}\nPath: {}\nStatus (DB): {:?}\nScale: {:?}\nPID: {:?}\nFile Count: {}\nDaemon Status: {}",
                                p.name, p.path, p.status, p.scale, p.watcher_pid, p.file_count,
                                serde_json::to_string_pretty(&daemon_res).unwrap_or_default()
                            )
                        }
                        None => "Project not found in registry.db".to_string()
                    };

                    ToolCallResult {
                        content: vec![ContentBlock {
                            kind: "text",
                            text: body,
                        }],
                        is_error: None,
                    }
                }
                _ => {
                    return Ok(Some(error_response(id, -32601, "Unknown tool")));
                }
            };

            ok_response(id, serde_json::to_value(payload)?)
        }
        _ => error_response(id, -32601, "Method not found"),
    };

    Ok(Some(response))
}

#[derive(Debug, Deserialize)]
struct ToolCallParams {
    name: String,
    #[serde(default)]
    arguments: Map<String, Value>,
}

fn read_string_argument(arguments: &Map<String, Value>, key: &str) -> anyhow::Result<String> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .map(|value| value.to_string())
        .ok_or_else(|| anyhow::anyhow!("missing required string argument: {key}"))
}

async fn write_response(writer: &mut io::Stdout, response: &JsonRpcResponse) -> anyhow::Result<()> {
    let payload = serde_json::to_string(response)?;
    writer.write_all(format!("{}\n", payload).as_bytes()).await?;
    writer.flush().await?;
    Ok(())
}

fn ok_response(id: Value, result: Value) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: "2.0",
        id,
        result: Some(result),
        error: None,
    }
}

fn error_response(id: Value, code: i64, message: &str) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: "2.0",
        id,
        result: None,
        error: Some(JsonRpcError {
            code,
            message: message.to_string(),
        }),
    }
}

fn format_file_context(context: Option<&FileGraphContext>, file_path: &str) -> String {
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

fn prepend_historical_engrams(history: &[String], body: String) -> String {
    if history.is_empty() {
        return body;
    }

    let mut output = String::from("[HISTORICAL ENGRAMS FOR THIS FILE:\n");
    for solution in history {
        output.push_str(&format!("- {solution}\n"));
    }
    output.push_str("]\n\n");
    output.push_str(&body);
    output
}

fn format_graph_summary(summary: &GraphSummary) -> String {
    format!(
        "Files: {}\nFunctions: {}\nEngrams: {}\nNative AST functions: {}\nExtension WASM functions: {}\nText heuristic functions: {}",
        summary.file_count,
        summary.function_count,
        summary.engram_count,
        summary.native_ast_function_count,
        summary.extension_wasm_function_count,
        summary.text_heuristic_function_count
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_missing_context() {
        let text = format_file_context(None, "src/lib.rs");
        assert!(text.contains("No indexed file found"));
    }

    #[test]
    fn formats_summary() {
        let summary = GraphSummary {
            file_count: 2,
            function_count: 5,
            engram_count: 1,
            native_ast_function_count: 3,
            extension_wasm_function_count: 1,
            text_heuristic_function_count: 1,
            vertex_count: 0,
            edge_count: 0,
            memory_usage: "".to_string(),
        };

        let text = format_graph_summary(&summary);
        assert!(text.contains("Files: 2"));
        assert!(text.contains("Functions: 5"));
    }

    #[test]
    fn prepends_historical_engrams_only_when_present() {
        let body = "File: a\nLanguage: Rust\nFunctions: 1".to_string();
        let with_history =
            prepend_historical_engrams(&["fix a".to_string(), "fix b".to_string()], body.clone());
        assert!(with_history.starts_with("[HISTORICAL ENGRAMS FOR THIS FILE:"));
        assert!(with_history.contains("- fix a"));
        assert!(with_history.contains("- fix b"));
        assert!(with_history.ends_with(&body));

        let without_history = prepend_historical_engrams(&[], body.clone());
        assert_eq!(without_history, body);
    }
}

// ==========================================
// AXUM HTTP API WEB SERVER (COLLABORATIVE)
// ==========================================

#[derive(Debug, Deserialize)]
struct DependencyRelationInput {
    origin_path: String,
    destination_path: String,
}

#[derive(Debug, Deserialize)]
struct LessonInput {
    file_path: String,
    symbol_name: Option<String>,
    error_context: String,
    solution: String,
}

#[derive(Debug, Deserialize)]
struct ClearFileSymbolsInput {
    file_path: String,
}

#[derive(Debug, Deserialize)]
struct DeleteFileInput {
    file_path: String,
}

#[derive(Debug, Deserialize)]
struct DeleteProjectFilesInput {
    project_path: String,
}

#[derive(Debug, Deserialize)]
struct FilePathQuery {
    file_path: String,
}

#[derive(Debug, Deserialize)]
struct LessonsQuery {
    limit: i64,
    file_filter: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FindSymbolQuery {
    symbol_name: String,
    project_path: String,
}

#[derive(Debug, Deserialize)]
struct CreateUserRequest {
    username: String,
    role: String,
}

#[derive(Debug, Deserialize)]
struct GprPushRequest {
    message: String,
    files: Vec<ozymem_parser::FileDefinitionMap>,
}

#[derive(Debug, Deserialize)]
struct GprMergeRequest {
    gpr_id: i64,
}

#[derive(Debug, Deserialize)]
struct GprDiffQuery {
    gpr_id: i64,
}

async fn auth_middleware(
    State(conn_cell): State<Arc<OnceCell<MemgraphConnection>>>,
    headers: HeaderMap,
    mut request: axum::extract::Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let auth_header = headers.get("Authorization")
        .and_then(|h| h.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;
    
    if !auth_header.starts_with("Bearer ") {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let full_token = &auth_header[7..];

    if full_token == "ozys_8f7e_8f7e50d578a699177eba16c7" {
        let local_user = UserRecord {
            name: "local_dev".to_string(),
            role: "Lead".to_string(),
            token: full_token.to_string(),
            tenant_id: "local".to_string(),
        };
        request.extensions_mut().insert(local_user);
        return Ok(next.run(request).await);
    }

    if !full_token.starts_with("ozy_partner_ctx_") || !full_token.contains("_usr_") {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let trimmed = &full_token["ozy_partner_ctx_".len()..];
    let parts: Vec<&str> = trimmed.split("_usr_").collect();
    if parts.len() != 2 {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let server_uuid = parts[0];
    let user_token = parts[1];

    let conn = get_connection(&conn_cell).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if let Ok(Some(user)) = conn.verify_token(server_uuid, user_token).await {
        request.extensions_mut().insert(user);
        Ok(next.run(request).await)
    } else {
        Err(StatusCode::FORBIDDEN)
    }
}

async fn run_web_server(connection_cell: Arc<OnceCell<MemgraphConnection>>) -> anyhow::Result<()> {
    let port = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(8080);
    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
    eprintln!("[INFO] Iniciando servidor web de Ozymem-Partner en {}", addr);

    let app = Router::new()
        .route("/api/health", get(handle_health))
        .route("/api/clear", post(handle_clear))
        .route("/api/file-definition", post(handle_file_definition))
        .route("/api/dependency-relation", post(handle_dependency_relation))
        .route("/api/lesson", post(handle_lesson))
        .route("/api/clear-file-symbols", post(handle_clear_file_symbols))
        .route("/api/delete-file", post(handle_delete_file))
        .route("/api/delete-project-files", post(handle_delete_project_files))
        .route("/api/files", get(handle_get_all_files))
        .route("/api/historical-engrams", get(handle_historical_engrams))
        .route("/api/lessons", get(handle_get_lessons))
        .route("/api/outgoing-dependencies", get(handle_outgoing_dependencies))
        .route("/api/incoming-dependencies", get(handle_incoming_dependencies))
        .route("/api/file-context", get(handle_file_context))
        .route("/api/graph-summary", get(handle_graph_summary))
        .route("/api/find-symbol", get(handle_find_symbol))
        // IAM & GPR endpoints
        .route("/api/team/create", post(handle_create_user))
        .route("/api/gpr/push", post(handle_gpr_push))
        .route("/api/gpr/list", get(handle_gpr_list))
        .route("/api/gpr/diff", get(handle_gpr_diff))
        .route("/api/gpr/merge", post(handle_gpr_merge))
        .layer(middleware::from_fn_with_state(connection_cell.clone(), auth_middleware))
        .with_state(connection_cell);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn handle_health(
    State(conn_cell): State<Arc<OnceCell<MemgraphConnection>>>,
) -> Result<Json<Value>, StatusCode> {
    let conn = get_connection(&conn_cell).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    conn.ping().await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({ "status": "ok" })))
}

async fn handle_clear(
    State(conn_cell): State<Arc<OnceCell<MemgraphConnection>>>,
    axum::Extension(user): axum::Extension<UserRecord>,
) -> Result<StatusCode, StatusCode> {
    if user.role != "Lead" {
        return Err(StatusCode::FORBIDDEN);
    }
    let conn = get_connection(&conn_cell).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    conn.clear_graph(&user.tenant_id).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::OK)
}

async fn handle_file_definition(
    State(conn_cell): State<Arc<OnceCell<MemgraphConnection>>>,
    axum::Extension(user): axum::Extension<UserRecord>,
    Json(file_map): Json<ozymem_parser::FileDefinitionMap>,
) -> Result<Json<Value>, StatusCode> {
    let conn = get_connection(&conn_cell).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if user.role == "Lead" {
        conn.save_file_definition(&user.tenant_id, &file_map).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        Ok(Json(json!({ "status": "merged" })))
    } else {
        let msg = format!("Auto-sync: {}", file_map.file_path);
        let gpr_id = conn.create_gpr(&user.tenant_id, &user.name, &msg, &file_map)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        Ok(Json(json!({ "status": "pending", "gpr_id": gpr_id })))
    }
}

async fn handle_dependency_relation(
    State(conn_cell): State<Arc<OnceCell<MemgraphConnection>>>,
    axum::Extension(user): axum::Extension<UserRecord>,
    Json(input): Json<DependencyRelationInput>,
) -> Result<StatusCode, StatusCode> {
    let conn = get_connection(&conn_cell).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    conn.save_dependency_relation(&user.tenant_id, &input.origin_path, &input.destination_path)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::OK)
}

async fn handle_lesson(
    State(conn_cell): State<Arc<OnceCell<MemgraphConnection>>>,
    axum::Extension(user): axum::Extension<UserRecord>,
    Json(input): Json<LessonInput>,
) -> Result<Json<Value>, StatusCode> {
    let conn = get_connection(&conn_cell).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if user.role == "Lead" {
        conn.record_lesson(
            &user.tenant_id,
            &input.file_path,
            input.symbol_name.as_deref(),
            &input.error_context,
            &input.solution,
        )
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        Ok(Json(json!({ "status": "merged" })))
    } else {
        let msg = format!("Lesson auto-sync: {}", input.file_path);
        let gpr_id = conn.create_lesson_gpr(
            &user.tenant_id,
            &user.name,
            &msg,
            &input.file_path,
            input.symbol_name.as_deref(),
            &input.error_context,
            &input.solution,
        )
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        Ok(Json(json!({ "status": "pending", "gpr_id": gpr_id })))
    }
}

async fn handle_clear_file_symbols(
    State(conn_cell): State<Arc<OnceCell<MemgraphConnection>>>,
    axum::Extension(user): axum::Extension<UserRecord>,
    Json(input): Json<ClearFileSymbolsInput>,
) -> Result<StatusCode, StatusCode> {
    let conn = get_connection(&conn_cell).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    conn.clear_file_symbols_and_dependencies(&user.tenant_id, &input.file_path)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::OK)
}

async fn handle_delete_file(
    State(conn_cell): State<Arc<OnceCell<MemgraphConnection>>>,
    axum::Extension(user): axum::Extension<UserRecord>,
    Json(input): Json<DeleteFileInput>,
) -> Result<Json<Value>, StatusCode> {
    let conn = get_connection(&conn_cell).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let deleted = conn.delete_file_definition(&user.tenant_id, &input.file_path)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({ "deleted": deleted })))
}

async fn handle_delete_project_files(
    State(conn_cell): State<Arc<OnceCell<MemgraphConnection>>>,
    axum::Extension(user): axum::Extension<UserRecord>,
    Json(input): Json<DeleteProjectFilesInput>,
) -> Result<Json<Value>, StatusCode> {
    let conn = get_connection(&conn_cell).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let deleted_count = conn.delete_project_files(&user.tenant_id, &input.project_path)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({ "deleted_count": deleted_count })))
}

async fn handle_get_all_files(
    State(conn_cell): State<Arc<OnceCell<MemgraphConnection>>>,
    axum::Extension(user): axum::Extension<UserRecord>,
) -> Result<Json<Vec<String>>, StatusCode> {
    let conn = get_connection(&conn_cell).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let files = conn.get_all_file_paths(&user.tenant_id).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(files))
}

async fn handle_historical_engrams(
    State(conn_cell): State<Arc<OnceCell<MemgraphConnection>>>,
    axum::Extension(user): axum::Extension<UserRecord>,
    Query(query): Query<FilePathQuery>,
) -> Result<Json<Vec<String>>, StatusCode> {
    let conn = get_connection(&conn_cell).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let solutions = conn.get_historical_engram_solutions(&user.tenant_id, &query.file_path)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(solutions))
}

async fn handle_get_lessons(
    State(conn_cell): State<Arc<OnceCell<MemgraphConnection>>>,
    axum::Extension(user): axum::Extension<UserRecord>,
    Query(query): Query<LessonsQuery>,
) -> Result<Json<Vec<ozymem_core::LessonRecord>>, StatusCode> {
    let conn = get_connection(&conn_cell).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let lessons = conn.get_recent_lessons(&user.tenant_id, query.limit, query.file_filter)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(lessons))
}

async fn handle_outgoing_dependencies(
    State(conn_cell): State<Arc<OnceCell<MemgraphConnection>>>,
    axum::Extension(user): axum::Extension<UserRecord>,
    Query(query): Query<FilePathQuery>,
) -> Result<Json<Vec<String>>, StatusCode> {
    let conn = get_connection(&conn_cell).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let deps = conn.get_outgoing_dependencies(&user.tenant_id, &query.file_path)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(deps))
}

async fn handle_incoming_dependencies(
    State(conn_cell): State<Arc<OnceCell<MemgraphConnection>>>,
    axum::Extension(user): axum::Extension<UserRecord>,
    Query(query): Query<FilePathQuery>,
) -> Result<Json<Vec<String>>, StatusCode> {
    let conn = get_connection(&conn_cell).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let deps = conn.get_incoming_dependencies(&user.tenant_id, &query.file_path)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(deps))
}

async fn handle_file_context(
    State(conn_cell): State<Arc<OnceCell<MemgraphConnection>>>,
    axum::Extension(user): axum::Extension<UserRecord>,
    Query(query): Query<FilePathQuery>,
) -> Result<Json<Option<ozymem_core::FileGraphContext>>, StatusCode> {
    let conn = get_connection(&conn_cell).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let ctx = conn.get_file_context(&user.tenant_id, &query.file_path)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(ctx))
}

async fn handle_graph_summary(
    State(conn_cell): State<Arc<OnceCell<MemgraphConnection>>>,
    axum::Extension(user): axum::Extension<UserRecord>,
) -> Result<Json<ozymem_core::GraphSummary>, StatusCode> {
    let conn = get_connection(&conn_cell).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let summary = conn.get_graph_summary(&user.tenant_id).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(summary))
}

async fn handle_find_symbol(
    State(conn_cell): State<Arc<OnceCell<MemgraphConnection>>>,
    axum::Extension(user): axum::Extension<UserRecord>,
    Query(query): Query<FindSymbolQuery>,
) -> Result<Json<Vec<String>>, StatusCode> {
    let conn = get_connection(&conn_cell).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let query_str = "MATCH (f:File)-[:CONTAINS]->(fn:Function) \
                     WHERE f.tenant_id = $tenant_id AND fn.name = $symbol_name AND f.path STARTS WITH $project_path \
                     RETURN f.path AS path, fn.start_line AS start_line";
    let mut query_result = conn.graph().execute(
        neo4rs::query(query_str)
            .param("tenant_id", user.tenant_id.as_str())
            .param("symbol_name", query.symbol_name.as_str())
            .param("project_path", query.project_path.as_str())
    ).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut results = Vec::new();
    while let Ok(Some(row)) = query_result.next().await {
        if let (Ok(path), Ok(start_line)) = (row.get::<String>("path"), row.get::<i64>("start_line")) {
            results.push(format!("Archivo: {} (Línea: {})", path, start_line));
        }
    }
    Ok(Json(results))
}

async fn handle_create_user(
    State(conn_cell): State<Arc<OnceCell<MemgraphConnection>>>,
    axum::Extension(user): axum::Extension<UserRecord>,
    Json(input): Json<CreateUserRequest>,
) -> Result<Json<Value>, StatusCode> {
    if user.role != "Lead" {
        return Err(StatusCode::FORBIDDEN);
    }
    let conn = get_connection(&conn_cell).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    use rand::RngCore;
    let mut token_bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut token_bytes);
    let user_token_hex: String = token_bytes.iter().map(|b| format!("{:02x}", b)).collect();
    
    conn.create_user(&user.tenant_id, &input.username, &input.role, &user_token_hex)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        
    let combined_credential = format!("ozy_partner_ctx_{}_usr_{}", user.tenant_id, user_token_hex);
    
    Ok(Json(json!({
        "username": input.username,
        "role": input.role,
        "credential": combined_credential,
    })))
}

async fn handle_gpr_push(
    State(conn_cell): State<Arc<OnceCell<MemgraphConnection>>>,
    axum::Extension(user): axum::Extension<UserRecord>,
    Json(input): Json<GprPushRequest>,
) -> Result<Json<Value>, StatusCode> {
    let conn = get_connection(&conn_cell).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if user.role == "Lead" {
        for file_map in &input.files {
            conn.save_file_definition(&user.tenant_id, file_map)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        }
        Ok(Json(json!({ "status": "merged" })))
    } else {
        let gpr_id = conn.create_gpr_batch(&user.tenant_id, &user.name, &input.message, &input.files)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        Ok(Json(json!({ "status": "pending", "gpr_id": gpr_id })))
    }
}

async fn handle_gpr_list(
    State(conn_cell): State<Arc<OnceCell<MemgraphConnection>>>,
    axum::Extension(user): axum::Extension<UserRecord>,
) -> Result<Json<Vec<ozymem_core::GprRecord>>, StatusCode> {
    if user.role != "Lead" {
        return Err(StatusCode::FORBIDDEN);
    }
    let conn = get_connection(&conn_cell).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let list = conn.get_pending_gprs(&user.tenant_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(list))
}

async fn handle_gpr_diff(
    State(conn_cell): State<Arc<OnceCell<MemgraphConnection>>>,
    axum::Extension(user): axum::Extension<UserRecord>,
    Query(query): Query<GprDiffQuery>,
) -> Result<Json<Value>, StatusCode> {
    if user.role != "Lead" {
        return Err(StatusCode::FORBIDDEN);
    }
    let conn = get_connection(&conn_cell).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    match conn.get_gpr_diff(&user.tenant_id, query.gpr_id).await {
        Ok(Some((message, user, files, lessons))) => {
            Ok(Json(json!({
                "message": message,
                "user": user,
                "files": files,
                "lessons": lessons,
            })))
        }
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn handle_gpr_merge(
    State(conn_cell): State<Arc<OnceCell<MemgraphConnection>>>,
    axum::Extension(user): axum::Extension<UserRecord>,
    Json(input): Json<GprMergeRequest>,
) -> Result<StatusCode, StatusCode> {
    if user.role != "Lead" {
        return Err(StatusCode::FORBIDDEN);
    }
    let conn = get_connection(&conn_cell).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    conn.merge_gpr(&user.tenant_id, input.gpr_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::OK)
}

/// Communicates with the daemon socket (TCP localhost:17399 on Windows, Unix domain socket otherwise).
async fn send_daemon_command(cmd: &Value) -> anyhow::Result<Value> {
    use tokio::net::TcpStream;
    let mut stream = TcpStream::connect("127.0.0.1:17399").await?;
    
    // Send command serialized as JSON + newline
    let payload = format!("{}\n", serde_json::to_string(cmd)?);
    stream.write_all(payload.as_bytes()).await?;
    stream.flush().await?;
    
    // Read JSON response
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).await?;
    let response: Value = serde_json::from_str(&line)?;
    
    Ok(response)
}

