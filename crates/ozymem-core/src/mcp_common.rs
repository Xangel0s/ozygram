use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use crate::{FileGraphContext, GraphSummary, graph_backend::{LessonEntry, NeighborInfo}};

// ---------------------------------------------------------------------------
// JSON-RPC wire types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Option<Value>,
}

#[derive(Debug, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: &'static str,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

// ---------------------------------------------------------------------------
// Initialize / Capabilities
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct InitializeResult {
    #[serde(rename = "protocolVersion")]
    pub protocol_version: &'static str,
    pub capabilities: ServerCapabilities,
    #[serde(rename = "serverInfo")]
    pub server_info: ServerInfo,
}

#[derive(Debug, Serialize)]
pub struct ServerCapabilities {
    pub tools: ToolsCapability,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<ResourcesCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompts: Option<PromptsCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logging: Option<LoggingCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sampling: Option<SamplingCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completions: Option<CompletionsCapability>,
}

#[derive(Debug, Serialize)]
pub struct ToolsCapability {
    #[serde(rename = "listChanged", skip_serializing_if = "Option::is_none")]
    pub list_changed: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct ResourcesCapability {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subscribe: Option<bool>,
    #[serde(rename = "listChanged", skip_serializing_if = "Option::is_none")]
    pub list_changed: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct PromptsCapability {
    #[serde(rename = "listChanged", skip_serializing_if = "Option::is_none")]
    pub list_changed: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct LoggingCapability {}

#[derive(Debug, Serialize)]
pub struct ServerInfo {
    pub name: &'static str,
    pub version: &'static str,
}

// ---------------------------------------------------------------------------
// Tools
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct ToolListResult {
    pub tools: Vec<ToolDefinition>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "nextCursor")]
    pub next_cursor: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ToolDefinition {
    pub name: &'static str,
    pub description: &'static str,
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
}

#[derive(Debug, Serialize)]
pub struct ToolCallResult {
    pub content: Vec<ContentBlock>,
    #[serde(rename = "isError", skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct ContentBlock {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub text: String,
}

#[derive(Debug, Deserialize)]
pub struct ToolCallParams {
    pub name: String,
    #[serde(default)]
    pub arguments: Map<String, Value>,
}

// ---------------------------------------------------------------------------
// Resources
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct ListResourcesResult {
    pub resources: Vec<ResourceDescription>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "nextCursor")]
    pub next_cursor: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ResourceDescription {
    pub uri: String,
    pub name: String,
    pub description: String,
    #[serde(rename = "mimeType", skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum ResourceContents {
    Text {
        uri: String,
        #[serde(rename = "mimeType")]
        mime_type: String,
        text: String,
    },
    Blob {
        uri: String,
        #[serde(rename = "mimeType")]
        mime_type: String,
        blob: String, // base64
    },
}

#[derive(Debug, Serialize)]
pub struct ReadResourceResult {
    pub contents: Vec<ResourceContents>,
}

#[derive(Debug, Deserialize)]
pub struct ReadResourceParams {
    pub uri: String,
}

#[derive(Debug, Deserialize)]
pub struct ListResourcesParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "cursor")]
    pub cursor: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ListResourceTemplatesResult {
    #[serde(rename = "resourceTemplates")]
    pub resource_templates: Vec<ResourceTemplate>,
}

#[derive(Debug, Serialize)]
pub struct ResourceTemplate {
    #[serde(rename = "uriTemplate")]
    pub uri_template: String,
    pub name: String,
    pub description: String,
    #[serde(rename = "mimeType", skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

// ---------------------------------------------------------------------------
// Prompts
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct ListPromptsResult {
    pub prompts: Vec<PromptDefinition>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "nextCursor")]
    pub next_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ListPromptsParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "cursor")]
    pub cursor: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PromptDefinition {
    pub name: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Vec<PromptArgument>>,
}

#[derive(Debug, Serialize)]
pub struct PromptArgument {
    pub name: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct GetPromptParams {
    pub name: String,
    #[serde(default)]
    pub arguments: Map<String, Value>,
}

#[derive(Debug, Serialize)]
pub struct GetPromptResult {
    pub messages: Vec<PromptMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PromptMessage {
    pub role: &'static str,
    pub content: PromptContent,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PromptContent {
    Text {
        #[serde(rename = "type")]
        kind: String,
        text: String,
    },
}

// ---------------------------------------------------------------------------
// Logging
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct LoggingMessageNotification {
    pub method: &'static str,
    pub params: LoggingMessageParams,
}

#[derive(Debug, Serialize)]
pub struct LoggingMessageParams {
    pub level: &'static str,
    pub logger: String,
    pub data: Value,
}

// ---------------------------------------------------------------------------
// Pagination
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct PaginatedResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "nextCursor")]
    pub next_cursor: Option<String>,
}

// ---------------------------------------------------------------------------
// Completions
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct CompletionsCapability {}

#[derive(Debug, Deserialize)]
pub struct CompletionParams {
    #[serde(default)]
    pub argument: CompletionArgument,
    #[serde(rename = "ref")]
    pub ref_param: CompletionRef,
}

#[derive(Debug, Default, Deserialize)]
pub struct CompletionArgument {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum CompletionRef {
    #[serde(rename = "ref/prompt")]
    Prompt { name: String },
    #[serde(rename = "ref/resource")]
    Resource { uri: String },
}

#[derive(Debug, Serialize)]
pub struct CompletionResult {
    pub completion: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub values: Option<Vec<String>>,
    #[serde(rename = "hasMore", skip_serializing_if = "Option::is_none")]
    pub has_more: Option<bool>,
    #[serde(rename = "total", skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
}

// ---------------------------------------------------------------------------
// Sampling
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct SamplingCapability {}

#[derive(Debug, Serialize)]
pub struct CreateMessageRequest {
    pub method: &'static str,
    pub params: CreateMessageParams,
}

#[derive(Debug, Serialize)]
pub struct CreateMessageParams {
    pub messages: Vec<SamplingMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "systemPrompt")]
    pub system_prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "includeContext")]
    pub include_context: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "temperature")]
    pub temperature: Option<f64>,
    #[serde(rename = "maxTokens")]
    pub max_tokens: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "stopSequences")]
    pub stop_sequences: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
    #[serde(rename = "modelPreferences", skip_serializing_if = "Option::is_none")]
    pub model_preferences: Option<ModelPreferences>,
}

#[derive(Debug, Serialize)]
pub struct SamplingMessage {
    pub role: &'static str,
    pub content: PromptContent,
}

#[derive(Debug, Serialize)]
pub struct ModelPreferences {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hints: Option<Vec<ModelHint>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "costPriority")]
    pub cost_priority: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "speedPriority")]
    pub speed_priority: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "intelligencePriority")]
    pub intelligence_priority: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct ModelHint {
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateMessageResult {
    pub role: String,
    pub content: PromptContent,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "stopReason")]
    pub stop_reason: Option<String>,
}

// ---------------------------------------------------------------------------
// McpBackend trait
// ---------------------------------------------------------------------------

#[async_trait::async_trait]
pub trait McpBackend: Send + Sync {
    fn tenant_id(&self) -> String;
    async fn get_graph_summary(&self) -> anyhow::Result<GraphSummary>;
    async fn get_file_context(&self, file_path: &str) -> anyhow::Result<Option<FileGraphContext>>;
    async fn get_historical_engram_solutions(&self, file_path: &str) -> anyhow::Result<Vec<String>>;
    async fn record_lesson(&self, file_path: &str, symbol_name: Option<&str>, error_context: &str, solution: &str) -> anyhow::Result<()>;
    async fn get_incoming_dependencies(&self, file_path: &str) -> anyhow::Result<Vec<String>>;
    async fn find_symbol(&self, symbol_name: &str, project_path: &str) -> anyhow::Result<Vec<String>>;

    async fn search_lessons(&self, query: &str, kind: Option<&str>, limit: usize) -> anyhow::Result<Vec<LessonEntry>>;
    async fn get_file_lessons(&self, file_path: &str) -> anyhow::Result<Vec<LessonEntry>>;
    async fn get_symbol_lessons(&self, file_path: &str, symbol_name: &str) -> anyhow::Result<Vec<LessonEntry>>;
    async fn recent_lessons(&self, kind: Option<&str>, limit: usize) -> anyhow::Result<Vec<LessonEntry>>;
    async fn get_graph_neighbors(&self, file_path: &str) -> anyhow::Result<NeighborInfo>;
    async fn record_entry(&self, file_path: &str, symbol_name: Option<&str>, error_context: &str, solution: &str, kind: &str) -> anyhow::Result<()>;
    async fn lookup_engram(&self, _symbol_path: &str) -> anyhow::Result<Option<ozymem_parser::EngramContract>> {
        Ok(None)
    }
    async fn get_speculative_engrams(&self, _file_path: &str, _limit: usize) -> anyhow::Result<Vec<ozymem_parser::EngramContract>> {
        Ok(vec![])
    }
}

// ---------------------------------------------------------------------------
// Tools list (legacy, for ozymem-cli)
// ---------------------------------------------------------------------------

pub fn get_tools_list() -> Vec<ToolDefinition> {
    vec![
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
    ]
}

// ---------------------------------------------------------------------------
// Tool call dispatching (legacy, for ozymem-cli)
// ---------------------------------------------------------------------------

pub async fn handle_mcp_tool_call(
    backend: &dyn McpBackend,
    tool_name: &str,
    arguments: &Map<String, Value>,
    current_project: Option<&str>,
    project_path: Option<&str>,
) -> anyhow::Result<ToolCallResult> {
    match tool_name {
        "ozymem_get_schema" => {
            let summary = backend.get_graph_summary().await?;
            let body = format!(
                "Proyecto Activo: {}\nRuta: {}\nTotal Archivos: {}\nTotal Funciones Mapeadas: {}\nTotal Engramas Formados: {}",
                current_project.unwrap_or(""),
                project_path.unwrap_or(""),
                summary.file_count,
                summary.function_count,
                summary.engram_count
            );
            Ok(ToolCallResult {
                content: vec![ContentBlock {
                    kind: "text",
                    text: body,
                }],
                is_error: None,
            })
        }
        "ozymem_find_symbol" => {
            let symbol_name = arguments.get("symbol_name")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("missing symbol_name argument"))?;
            let proj_path = project_path.unwrap_or("");
            let results = backend.find_symbol(symbol_name, proj_path).await?;
            let body = if results.is_empty() {
                format!("Símbolo '{}' no encontrado en el proyecto '{}'.", symbol_name, current_project.unwrap_or(""))
            } else {
                format!("Resultados para la búsqueda de '{}':\n{}", symbol_name, results.join("\n"))
            };
            Ok(ToolCallResult {
                content: vec![ContentBlock {
                    kind: "text",
                    text: body,
                }],
                is_error: None,
            })
        }
        "graph_summary" => {
            let summary = backend.get_graph_summary().await?;
            Ok(ToolCallResult {
                content: vec![ContentBlock {
                    kind: "text",
                    text: format_graph_summary(&summary),
                }],
                is_error: None,
            })
        }
        "file_context" => {
            let file_path = arguments.get("file_path")
                .and_then(Value::as_str)
                .or_else(|| arguments.get("path").and_then(Value::as_str))
                .ok_or_else(|| anyhow::anyhow!("missing file_path argument"))?;
            let context = backend.get_file_context(file_path).await?;
            let historical_engrams = backend.get_historical_engram_solutions(file_path).await?;
            let body = format_file_context(context.as_ref(), file_path);
            Ok(ToolCallResult {
                content: vec![ContentBlock {
                    kind: "text",
                    text: prepend_historical_engrams(&historical_engrams, body),
                }],
                is_error: None,
            })
        }
        "record_lesson" => {
            let file_path = arguments.get("file_path")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("missing file_path"))?;
            let symbol_name = arguments.get("symbol_name").and_then(Value::as_str);
            let error_context = arguments.get("error_context")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("missing error_context"))?;
            let solution = arguments.get("solution")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("missing solution"))?;

            backend.record_lesson(file_path, symbol_name, error_context, solution).await?;
            Ok(ToolCallResult {
                content: vec![ContentBlock {
                    kind: "text",
                    text: format!(
                        "Recorded lesson for {file_path}{}",
                        symbol_name.map(|s| format!("::{}", s)).unwrap_or_default()
                    ),
                }],
                is_error: None,
            })
        }
        "file_trace" => {
            let file_path = arguments.get("file_path")
                .and_then(Value::as_str)
                .or_else(|| arguments.get("path").and_then(Value::as_str))
                .ok_or_else(|| anyhow::anyhow!("missing file_path"))?;
            let incoming = backend.get_incoming_dependencies(file_path).await?;
            let mut text = format!("Reverse dependencies (files impacted by changes to {}):\n", file_path);
            if incoming.is_empty() {
                text.push_str("- (none)");
            } else {
                for path in incoming {
                    text.push_str(&format!("- {path}\n"));
                }
            }
            Ok(ToolCallResult {
                content: vec![ContentBlock {
                    kind: "text",
                    text,
                }],
                is_error: None,
            })
        }
        _ => Err(anyhow::anyhow!("Unknown tool: {}", tool_name)),
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
