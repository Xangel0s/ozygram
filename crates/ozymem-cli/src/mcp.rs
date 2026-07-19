use anyhow::Context;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::sync::Arc;
use tokio::sync::OnceCell;
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader};
use std::path::PathBuf;
use std::sync::Mutex;

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
    #[serde(rename = "protocolVersion")]
    protocol_version: &'static str,
    capabilities: ServerCapabilities,
    #[serde(rename = "serverInfo")]
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

#[derive(Debug, Deserialize)]
struct ToolCallParams {
    name: String,
    #[serde(default)]
    arguments: Map<String, Value>,
}

struct McpSession {
    current_project: Option<String>,
    project_path: Option<String>,
}

lazy_static::lazy_static! {
    static ref SESSION: Mutex<McpSession> = Mutex::new(McpSession {
        current_project: None,
        project_path: None,
    });
}

fn parse_root_uri(root_uri: &str) -> Option<String> {
    let mut path = root_uri.trim();
    if path.starts_with("file://") {
        path = &path[7..];
    }
    if path.starts_with('/') {
        path = &path[1..];
    }
    let decoded = path.replace("%3A", ":").replace("%3a", ":");
    let win_path = decoded.replace('/', "\\");
    Some(win_path)
}

fn clean_path_str(path_str: &str) -> String {
    if path_str.starts_with(r"\\?\") {
        path_str[4..].to_string()
    } else {
        path_str.to_string()
    }
}

fn validate_project_path(path_str: &str) -> anyhow::Result<(String, String)> {
    let target_buf = PathBuf::from(path_str);
    let canonical_target = target_buf.canonicalize()
        .context("Failed to canonicalize client root path")?;
    let clean_target = clean_path_str(&canonical_target.to_string_lossy());

    let (_, config) = crate::load_config()?;
    for (name, registered_path_str) in &config.projects {
        if let Ok(reg_path_buf) = PathBuf::from(registered_path_str).canonicalize() {
            let clean_reg_path = clean_path_str(&reg_path_buf.to_string_lossy());
            if clean_target.to_lowercase() == clean_reg_path.to_lowercase() {
                return Ok((name.clone(), registered_path_str.clone()));
            }
        }
    }

    Err(anyhow::anyhow!("Proyecto no registrado en ozymem.toml"))
}

pub async fn run_mcp_server() -> anyhow::Result<()> {
    // Force logs to stderr to protect stdout data channel
    eprintln!("[INFO] Iniciando servidor MCP de Ozymem... (OZYMEM_DAEMON={:?})", std::env::var("OZYMEM_DAEMON"));

    let connection_cell = Arc::new(OnceCell::new());
    let mut stdin = BufReader::new(io::stdin());
    let mut stdout = io::stdout();

    let mut line = String::new();
    while {
        line.clear();
        match stdin.read_line(&mut line).await {
            Ok(bytes) => bytes > 0,
            Err(e) => {
                eprintln!("[ERROR] Error leyendo stdin: {:?}", e);
                false
            }
        }
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

    eprintln!("[INFO] Bucle de lectura de stdin terminado.");
    if std::env::var("OZYMEM_DAEMON").is_ok() {
        eprintln!("[INFO] Entrando en modo daemon de bucle de sueño infinito...");
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(3600)).await;
        }
    }

    Ok(())
}

async fn get_connection(cell: &OnceCell<crate::BackendClient>) -> anyhow::Result<&crate::BackendClient> {
    cell.get_or_try_init(|| async {
        crate::build_backend_client().await
    }).await
}

async fn handle_request(
    connection_cell: &OnceCell<crate::BackendClient>,
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
            let mut project_verified = false;
            if let Some(params) = &request.params {
                eprintln!("[DEBUG] params: {}", serde_json::to_string(&params).unwrap_or_default());
                let home_dir = home::home_dir().unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
                if let Err(e) = std::fs::write(
                    home_dir.join(".ozymem-init-params.log"),
                    serde_json::to_string_pretty(params).unwrap_or_default()
                ) {
                    eprintln!("[ERROR] Failed to write init params log: {:?}", e);
                }
                let root_uri = params.get("rootUri").and_then(Value::as_str)
                    .or_else(|| params.get("rootPath").and_then(Value::as_str))
                    .or_else(|| {
                        params.get("workspaceFolders")
                            .and_then(Value::as_array)
                            .and_then(|folders| folders.first())
                            .and_then(|folder| folder.get("uri"))
                            .and_then(Value::as_str)
                    });
                if let Some(uri) = root_uri {
                    eprintln!("[DEBUG] found rootUri/rootPath: {}", uri);
                    if let Some(decoded_path) = parse_root_uri(uri) {
                        eprintln!("[DEBUG] decoded_path: {}", decoded_path);
                        match validate_project_path(&decoded_path) {
                            Ok((name, path)) => {
                                let mut session = SESSION.lock().unwrap();
                                session.current_project = Some(name.clone());
                                session.project_path = Some(path.clone());
                                project_verified = true;
                                eprintln!("[INFO] MCP inicializado para proyecto registrado: {} ({})", name, path);
                            }
                            Err(e) => {
                                let home_dir = home::home_dir().unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
                                if let Err(err) = std::fs::write(
                                    home_dir.join(".ozymem-init-error.log"),
                                    format!("Path: {}\nError: {:?}", decoded_path, e)
                                ) {
                                    eprintln!("[ERROR] Failed to write init error log: {:?}", err);
                                }
                                eprintln!("[WARNING] MCP rechazó inicialización en ruta no registrada: {}. Detalle: {:?}", decoded_path, e);
                            }
                        }
                    }
                } else {
                    eprintln!("[DEBUG] rootUri/rootPath not found in params");
                }
            }

            let mut diag_msg = String::new();
            if !project_verified {
                let load_res = crate::load_config();
                diag_msg = format!(
                    "Fallback failed. load_config result: {:?}. ",
                    load_res.as_ref().map(|(_, c)| format!("Projects keys: {:?}", c.projects.keys().collect::<Vec<_>>()))
                );
                if let Ok((_, config)) = load_res {
                    if let Some((name, path)) = config.projects.iter().next() {
                        let mut session = SESSION.lock().unwrap();
                        session.current_project = Some(name.clone());
                        session.project_path = Some(path.clone());
                        project_verified = true;
                        eprintln!("[INFO] MCP inicializado por fallback a {}: {}", name, path);
                    }
                }
            }

            if !project_verified {
                let root_uri_str = request.params.as_ref()
                    .and_then(|p| p.get("rootUri").and_then(Value::as_str))
                    .unwrap_or("None");
                let err_msg = format!(
                    "Directorio no registrado. rootUri: {}. Diag: {}",
                    root_uri_str,
                    diag_msg
                );
                return Ok(Some(error_response(id, -32603, &err_msg)));
            }

            let payload = InitializeResult {
                protocol_version: "2024-11-05",
                capabilities: ServerCapabilities {
                    tools: ToolsCapability {
                        list_changed: Some(true),
                    },
                },
                server_info: ServerInfo {
                    name: "ozymem-mcp",
                    version: env!("CARGO_PKG_VERSION"),
                },
            };

            ok_response(id, serde_json::to_value(payload)?)
        }
        "notifications/initialized" => return Ok(None),
        "tools/list" => {
            let payload = ToolListResult {
                tools: vec![
                    ToolDefinition {
                        name: "ozymem_get_schema",
                        description: "Obtener esquema general de archivos e idiomas del proyecto actual indexado en SQLite.",
                        input_schema: json!({
                            "type": "object",
                            "properties": {},
                            "additionalProperties": false
                        }),
                    },
                    ToolDefinition {
                        name: "ozymem_find_symbol",
                        description: "Buscar la ubicación de un símbolo/función específico por nombre dentro del proyecto.",
                        input_schema: json!({
                            "type": "object",
                            "properties": {
                                "symbol_name": {
                                    "type": "string",
                                    "description": "Nombre del símbolo o función a buscar"
                                }
                            },
                            "required": ["symbol_name"],
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
                        name: "ozymem_hybrid_search",
                        description: "Perform hybrid search combining vector embeddings on code snippets and rels.",
                        input_schema: json!({
                            "type": "object",
                            "properties": {
                                "query": {
                                    "type": "string",
                                    "description": "The search query to match semantically"
                                },
                                "limit": {
                                    "type": "integer",
                                    "description": "Maximum number of results to return (default 5)"
                                }
                            },
                            "required": ["query"],
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

            let session = SESSION.lock().unwrap();
            let Some(proj_path) = &session.project_path else {
                return Ok(Some(error_response(id, -32603, "MCP no inicializado con un proyecto válido")));
            };

            // Establish connection lazily
            let connection = match get_connection(connection_cell).await {
                Ok(conn) => conn,
                Err(e) => {
                    return Ok(Some(error_response(id, -32603, &format!("Backend connection failed: {:?}", e))));
                }
            };

            let payload = match tool_call.name.as_str() {
                "ozymem_get_schema" => {
                    let summary = match connection.get_graph_summary().await {
                        Ok(s) => s,
                        Err(e) => {
                            return Ok(Some(error_response(id, -32603, &format!("Backend query error: {:?}", e))));
                        }
                    };

                    let body = format!(
                        "Proyecto Activo: {}\nRuta: {}\nTotal Archivos: {}\nTotal Funciones Mapeadas: {}\nTotal Engramas Formados: {}",
                        session.current_project.as_deref().unwrap_or(""),
                        proj_path,
                        summary.file_count,
                        summary.function_count,
                        summary.engram_count
                    );

                    ToolCallResult {
                        content: vec![ContentBlock {
                            kind: "text",
                            text: body,
                        }],
                        is_error: None,
                    }
                }
                "ozymem_find_symbol" => {
                    let symbol_name = tool_call.arguments.get("symbol_name")
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow::anyhow!("missing symbol_name argument"))?;

                    let results = match connection.find_symbol(symbol_name, proj_path).await {
                        Ok(res) => res,
                        Err(e) => {
                            return Ok(Some(error_response(id, -32603, &format!("Find symbol error: {:?}", e))));
                        }
                    };

                    let body = if results.is_empty() {
                        format!("Símbolo '{}' no encontrado en el proyecto '{}'.", symbol_name, session.current_project.as_deref().unwrap_or(""))
                    } else {
                        format!("Resultados para la búsqueda de '{}':\n{}", symbol_name, results.join("\n"))
                    };

                    ToolCallResult {
                        content: vec![ContentBlock {
                            kind: "text",
                            text: body,
                        }],
                        is_error: None,
                    }
                }
                "graph_summary" => {
                    let summary = connection.get_graph_summary().await?;
                    ToolCallResult {
                        content: vec![ContentBlock {
                            kind: "text",
                            text: format_graph_summary(&summary),
                        }],
                        is_error: None,
                    }
                }
                "file_context" => {
                    let file_path = read_string_argument(&tool_call.arguments, "file_path")
                        .or_else(|_| read_string_argument(&tool_call.arguments, "path"))?;
                    let context = connection.get_file_context(&file_path).await?;
                    let historical_engrams = connection
                        .get_historical_engram_solutions(&file_path)
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
                "record_lesson" => {
                    let file_path = read_string_argument(&tool_call.arguments, "file_path")?;
                    let symbol_name = tool_call.arguments.get("symbol_name")
                        .and_then(Value::as_str);
                    let error_context = read_string_argument(&tool_call.arguments, "error_context")?;
                    let solution = read_string_argument(&tool_call.arguments, "solution")?;
                    let ws_root = SESSION.lock().unwrap().project_path.clone().unwrap_or_default();

                    connection
                        .record_lesson(&file_path, symbol_name, &error_context, &solution, &ws_root)
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
                "ozymem_hybrid_search" => {
                    let query_str = read_string_argument(&tool_call.arguments, "query")?;
                    let limit = tool_call.arguments.get("limit")
                        .and_then(Value::as_u64)
                        .unwrap_or(5) as usize;
                    let category = tool_call.arguments.get("category")
                        .and_then(Value::as_str);

                    let embedding_vector = match get_embedding(&query_str).await {
                        Ok(v) => v,
                        Err(e) => {
                            return Ok(Some(error_response(id, -32603, &format!("Embedding error: {:?}", e))));
                        }
                    };

                    let matches = match search_vectors(proj_path, &embedding_vector, limit, category) {
                        Ok(m) => m,
                        Err(e) => {
                            return Ok(Some(error_response(id, -32603, &format!("Search error: {:?}", e))));
                        }
                    };

                    let mut body = format!("Resultados de búsqueda híbrida semántica para '{}':\n", query_str);
                    if matches.is_empty() {
                        body.push_str("No se encontraron coincidencias vectoriales en el workspace.");
                    } else {
                        for (i, (file_path, symbol, score)) in matches.iter().enumerate() {
                            body.push_str(&format!("{}. Símbolo: {} | Archivo: {} (Confianza: {:.4})\n", i + 1, symbol, file_path, score));
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

fn format_graph_summary(summary: &ozymem_core::GraphSummary) -> String {
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

fn generate_mock_embedding(text: &str) -> Vec<f32> {
    let dimension = 1536;
    let mut vector = vec![0.0; dimension];
    let mut hash: i32 = 0;
    for c in text.chars() {
        hash = (c as i32).wrapping_add((hash << 5).wrapping_sub(hash));
    }
    for i in 0..dimension {
        let angle = (hash + i as i32) as f64;
        vector[i] = angle.sin() as f32 / (i + 1) as f32;
    }
    vector
}

pub(crate) async fn get_embedding(query_str: &str) -> anyhow::Result<Vec<f32>> {
    let api_key = std::env::var("GEMINI_API_KEY").unwrap_or_default();
    if api_key.is_empty() {
        return Ok(generate_mock_embedding(query_str));
    }
    let client = reqwest::Client::new();
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/text-embedding-004:embedContent?key={}",
        api_key
    );
    let resp = client.post(&url)
        .json(&serde_json::json!({
            "model": "models/text-embedding-004",
            "content": {
                "parts": [{"text": query_str}]
            }
        }))
        .send()
        .await?;
    if !resp.status().is_success() {
        return Err(anyhow::anyhow!("Gemini API returned status {}", resp.status()));
    }
    let res: serde_json::Value = resp.json().await?;
    let values: Vec<f32> = serde_json::from_value(
        res["embedding"]["values"].clone()
    )?;
    Ok(values)
}

pub(crate) fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot_product = 0.0;
    let mut norm_a = 0.0;
    let mut norm_b = 0.0;
    for i in 0..a.len() {
        dot_product += a[i] * b[i];
        norm_a += a[i] * a[i];
        norm_b += b[i] * b[i];
    }
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot_product / (norm_a.sqrt() * norm_b.sqrt())
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct VectorRecord {
    pub id: String,
    pub category: String,
    pub project: String,
    pub tenant_id: String,
    pub source_path: String,
    pub timestamp: i64,
    pub hit_count: i64,
    pub text: String,
    pub embedding: Vec<f32>,
    pub schema_version: i64,
    pub parent_id: Option<String>,
}

pub(crate) fn search_vectors(project_path: &str, query_vector: &[f32], limit: usize, category: Option<&str>) -> anyhow::Result<Vec<(String, String, f32)>> {
    let db_path = std::path::Path::new(project_path)
        .join(".ozymem")
        .join("vectors")
        .join("vectors.json");
    if !db_path.exists() {
        return Ok(Vec::new());
    }
    let data = std::fs::read_to_string(&db_path)?;
    let mut records: Vec<VectorRecord> = serde_json::from_str(&data)?;
    
    // Metadata Pre-filtering
    if let Some(cat) = category {
        records.retain(|r| r.category.eq_ignore_ascii_case(cat));
    }
    records.retain(|r| r.schema_version == 1);
    
    let mut matches: Vec<(String, String, f32)> = records.iter()
        .map(|r| {
            let score = cosine_similarity(query_vector, &r.embedding);
            (r.source_path.clone(), r.id.clone(), score)
        })
        .collect();
    
    matches.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
    matches.truncate(limit);
    
    if !matches.is_empty() {
        let best_id = &matches[0].1;
        // Reload, update and save
        if let Ok(data) = std::fs::read_to_string(&db_path) {
            let records_res: Result<Vec<VectorRecord>, _> = serde_json::from_str(&data);
            if let Ok(mut records) = records_res {
                if let Some(r) = records.iter_mut().find(|rec| &rec.id == best_id) {
                    r.hit_count += 1;
                    if let Ok(serialized) = serde_json::to_string_pretty(&records) {
                        let _ = std::fs::write(&db_path, serialized);
                    }
                }
            }
        }
    }
    
    Ok(matches)
}



