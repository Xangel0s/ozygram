use ozymem_core::mcp_common;
use serde_json::{json, Value};
use crate::tools::mark_legacy_tools_deprecated;
use crate::state::ok_response;

pub fn handle_tools_list(
    id: Value,
    request: &mcp_common::JsonRpcRequest,
) -> anyhow::Result<Option<mcp_common::JsonRpcResponse>> {
            let tools = vec![
                mcp_common::ToolDefinition {
                    name: "ozy_context",
                    description: "Unified context tool: task bundle, file context, project memory context, project schema, indexed files, and recent lessons. Replaces context_for_task, file_context, graph_summary, list_files, recent_lessons.",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "action": { "type": "string", "enum": ["task", "file", "project", "summary", "files", "recent"], "default": "task" },
                            "query": { "type": "string", "description": "Task/search query for action=task" },
                            "file_path": { "type": "string", "description": "File path for action=file" },
                            "project_name": { "type": "string", "description": "Registered project name for action=project" },
                            "max_tokens": { "type": "integer", "default": 4000 },
                            "limit": { "type": "integer", "default": 20 }
                        },
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "ozy_memory",
                    description: "Unified Ozy memory tool: legacy lessons plus sessions, observations, prompts, timeline, soft delete, topic upserts, dedupe, and passive capture.",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "action": { "type": "string", "enum": ["record", "search", "file", "symbol", "recent", "similar", "session_start", "session_end", "save", "get", "timeline", "delete", "prompt", "passive"], "default": "search" },
                            "kind": { "type": "string", "enum": ["lesson", "decision", "convention", "gotcha", "module_rule", "architecture", "bugfix", "pattern", "config", "discovery", "learning", "session_summary"] },
                            "session_id": { "type": "string" },
                            "project": { "type": "string" },
                            "directory": { "type": "string" },
                            "scope": { "type": "string", "enum": ["project", "personal"], "default": "project" },
                            "topic_key": { "type": "string" },
                            "title": { "type": "string" },
                            "id": { "type": "integer" },
                            "query": { "type": "string" },
                            "file_path": { "type": "string" },
                            "symbol_name": { "type": "string" },
                            "context": { "type": "string" },
                            "content": { "type": "string" },
                            "before": { "type": "integer", "default": 5 },
                            "after": { "type": "integer", "default": 5 },
                            "limit": { "type": "integer", "default": 10 },
                            "min_score": { "type": "number", "default": 0.5 }
                        },
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "ozy_graph",
                    description: "Unified architecture graph tool: summary, neighbors, impact, paths, and architecture report. Replaces graph_summary, graph_neighbors, analyze_impact, graph_path.",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "action": { "type": "string", "enum": ["summary", "neighbors", "impact", "path", "architecture_report"], "default": "summary" },
                            "file_path": { "type": "string" },
                            "from": { "type": "string" },
                            "to": { "type": "string" },
                            "depth": { "type": "integer", "default": 3 },
                            "max_paths": { "type": "integer", "default": 1 },
                            "max_hops": { "type": "integer", "default": 10 }
                        },
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "ozy_code_doctor",
                    description: "Preview-safe code doctor: duplicates, redundancy, architecture smells, best-practice feedback, dependency risks, and refactor suggestions.",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "mode": { "type": "string", "enum": ["preview"], "default": "preview" },
                            "scope": { "type": "string", "description": "Optional file or directory scope" },
                            "min_duplicate_lines": { "type": "integer", "default": 6 },
                            "max_findings": { "type": "integer", "default": 20 }
                        },
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "ozy_doctor",
                    description: "Ozymem/Ozygram system doctor: DB, registry, projects, memories, embeddings, watchers, indexes, and preview-safe repair suggestions.",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "format": { "type": "string", "enum": ["text", "json"], "default": "text" },
                            "include_projects": { "type": "boolean", "default": true }
                        },
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "ozy_skills",
                    description: "Official skills.sh integration: sync/list/search/apply imported skill metadata as internal best-practice context; never executes external skill content.",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "action": { "type": "string", "enum": ["sync", "list", "search", "apply"], "default": "list" },
                            "query": { "type": "string" },
                            "category": { "type": "string" },
                            "limit": { "type": "integer", "default": 20 }
                        },
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "ozy_brain",
                    description: "Hybrid Ozy brain: Rust-curated project context plus Python reasoning for plans, reflection, deep recall, risk review, memory ranking, and mental models. Advisory only; it does not modify files or execute commands.",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "action": { "type": "string", "enum": ["plan", "reflect", "recall_deep", "summarize_project", "detect_patterns", "suggest_next_steps", "analyze_failure", "compress_session", "rank_memories", "build_mental_model", "risk_review"], "default": "plan" },
                            "goal": { "type": "string" },
                            "query": { "type": "string" },
                            "project": { "type": "string" },
                            "max_tokens": { "type": "integer", "default": 4000 },
                            "limit": { "type": "integer", "default": 20 },
                            "failures": { "type": "array", "items": { "type": "string" } },
                            "changes": { "type": "array", "items": { "type": "string" } }
                        },
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "ozymem_map_api_routes",
                    description: "Extracts HTTP API endpoints, methods, parameters, and DTOs across the project (FastAPI, Express, Axum).",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "file_path": { "type": "string", "description": "Optional specific file to extract routes from" }
                        },
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "detect_code_drift",
                    description: "Audits code changes or diff against stored architecture rules and conventions, alerting if new code violates saved patterns.",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "changed_files": { "type": "array", "items": { "type": "string" } },
                            "diff_content": { "type": "string" }
                        },
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "rank_memories",
                    description: "Evaluates memory staleness and confidence scores, ranking active memories and identifying stale or deprecated ones.",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "min_confidence": { "type": "number", "default": 0.5 }
                        },
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "export_knowledge_bundle",
                    description: "Exports project knowledge, lessons, conventions, and mapped API routes to a portable, verifiable .ozymem bundle file.",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "output_path": { "type": "string", "description": "Destination file path (.ozymem)" },
                            "project_name": { "type": "string", "description": "Optional project identifier name" }
                        },
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "import_knowledge_bundle",
                    description: "Imports knowledge from a .ozymem bundle into the current project, with deduplication and checksum verification.",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "file_path": { "type": "string", "description": "Source .ozymem bundle path" },
                            "merge": { "type": "boolean", "default": true, "description": "Merge into existing memories or overwrite" }
                        },
                        "required": ["file_path"],
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "cross_repo_query",
                    description: "Performs cross-repository memory and lesson searches across all registered projects or a linked subset.",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "query": { "type": "string", "description": "Search query across repositories" },
                            "project_names": { "type": "array", "items": { "type": "string" }, "description": "Optional list of project names to scope" },
                            "limit": { "type": "integer", "default": 20 }
                        },
                        "required": ["query"],
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "link_projects",
                    description: "Links two projects in the registry to represent dependency or API consumer/provider relations.",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "source": { "type": "string", "description": "Source project name or path" },
                            "target": { "type": "string", "description": "Target project name or path" },
                            "relation": { "type": "string", "default": "depends_on", "description": "Relation type (e.g. depends_on, api_consumer, shared_lib)" }
                        },
                        "required": ["source", "target"],
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "ozymem_python_typecheck",
                    description: "Optional semantic type checker and contract auditor for Python codebases using Pyrefly or AST compiler.",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "file_path": { "type": "string", "description": "Optional specific Python file path to check" },
                            "strict": { "type": "boolean", "default": false, "description": "Enforce strict typing checks" }
                        },
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "ozy_project",
                    description: "Unified project/package tool: registered projects, package inspection, scripts, refresh index, stale projects, and ignore rules.",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "action": { "type": "string", "enum": ["list", "memories", "delete", "stale", "refresh", "create_ignore", "dependencies", "verify_dependencies"], "default": "list" },
                            "project_name": { "type": "string" },
                            "project_path": { "type": "string" },
                            "query": { "type": "string" },
                            "patterns": { "type": "array", "items": { "type": "string" } },
                            "force": { "type": "boolean", "default": false },
                            "days": { "type": "integer", "default": 90 }
                        },
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "search",
                    description: "Búsqueda unificada híbrida de Ozygram (Texto literal estilo grep + Símbolos AST + Lecciones + Contratos Excel/HTTP)",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "query": { "type": "string", "description": "Término o consulta de búsqueda" },
                            "scope": { "type": "string", "description": "Ámbito: 'code', 'lessons', 'contracts', 'all'", "default": "all" },
                            "mode": { "type": "string", "description": "Modo: 'text', 'semantic', 'hybrid'", "default": "hybrid" },
                            "limit": { "type": "integer", "description": "Límite de resultados", "default": 20 }
                        },
                        "required": ["query"],
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "verify_contracts",
                    description: "Auditar contratos de exportación Excel, routers HTTP y cabeceras Content-Disposition",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "target": { "type": "string", "description": "Objetivo de auditoría ('export', 'all')", "default": "export" }
                        },
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "context",
                    description: "Contexto unificado de archivo/símbolo (funciones, grafo de dependencias, lecciones y git)",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "file_path": { "type": "string", "description": "Ruta del archivo" }
                        },
                        "required": ["file_path"],
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "impact",
                    description: "Análisis de impacto transitivo y dependientes directos de un archivo",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "file_path": { "type": "string", "description": "Ruta del archivo" },
                            "depth": { "type": "integer", "description": "Profundidad máxima", "default": 3 }
                        },
                        "required": ["file_path"],
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "memory",
                    description: "Registrar o consultar memorias arquitectónicas (lesson, decision, convention, gotcha, module_rule)",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "action": { "type": "string", "description": "'record' o 'search'" },
                            "kind": { "type": "string", "description": "Tipo: 'lesson', 'decision', 'convention', 'gotcha', 'module_rule'" },
                            "file_path": { "type": "string", "description": "Ruta del archivo" },
                            "solution": { "type": "string", "description": "Contenido/solución de la memoria" },
                            "error_context": { "type": "string", "description": "Contexto de la lección o decisión" }
                        },
                        "required": ["action"],
                        "additionalProperties": false
                    }),
                },
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
                    name: "ozymem_get_schema",
                    description: "Obtener esquema general de archivos e idiomas del proyecto actual.",
                    input_schema: json!({
                        "type": "object",
                        "properties": {},
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "ozymem_find_symbol",
                    description: "Buscar la ubicación de un símbolo/función específico por nombre dentro del proyecto.",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "symbol_name": { "type": "string", "description": "Nombre del símbolo o función a buscar" }
                        },
                        "required": ["symbol_name"],
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "ozymem_hybrid_search",
                    description: "Perform hybrid search combining vector embeddings on code snippets and rels.",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "query": { "type": "string", "description": "The search query to match semantically" },
                            "limit": { "type": "integer", "description": "Maximum number of results to return" }
                        },
                        "required": ["query"],
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
                mcp_common::ToolDefinition {
                    name: "ozy_lookup_engram",
                    description: "Busca contratos deterministas, firmas de funciones e invariantes AST en tiempo O(1) y zero-copy usando DeepSeek Engram",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "symbol_path": { "type": "string", "description": "Ruta o nombre del símbolo (ej: 'src/auth.rs::verify' o 'open_db')" }
                        },
                        "required": ["symbol_path"],
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "lookup_engram",
                    description: "Busca contratos deterministas y firmas AST exactas en tiempo O(1) con DeepSeek Engram",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "symbol_path": { "type": "string", "description": "Ruta o nombre del símbolo (ej: 'src/auth.rs::verify' o 'open_db')" }
                        },
                        "required": ["symbol_path"],
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "ozy_verify_diff",
                    description: "Ejecuta una validación ligera en sandbox (cargo check / py_compile / tsc) con auto-corrección guiada por ozy-brain ante errores de compilación o tipos",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "file_path": { "type": "string", "description": "Ruta del archivo a verificar o sobre el que se aplica el diff" },
                            "proposed_code": { "type": "string", "description": "Fragmento o contenido propuesto opcional a validar" },
                            "project_path": { "type": "string", "description": "Ruta raíz del proyecto opcional" }
                        },
                        "required": ["file_path"],
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "verify_diff",
                    description: "Ejecuta validación sandbox ultrarrápida (cargo check / py_compile) y auto-diagnóstico",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "file_path": { "type": "string", "description": "Ruta del archivo a verificar" },
                            "proposed_code": { "type": "string", "description": "Fragmento o contenido propuesto opcional a validar" },
                            "project_path": { "type": "string", "description": "Ruta raíz del proyecto opcional" }
                        },
                        "required": ["file_path"],
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "ozy_export_memory_notes",
                    description: "Exporta y consolida el estado de memoria actual (lecciones, contratos y reglas procedimentales) en una Git Note vinculada al commit actual",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "commit_ref": { "type": "string", "description": "SHA del commit o ref (default: HEAD)" },
                            "note_ref": { "type": "string", "description": "Ref de la nota Git (default: refs/notes/ozymem)" }
                        },
                        "additionalProperties": false
                    }),
                },
                mcp_common::ToolDefinition {
                    name: "ozy_import_memory_notes",
                    description: "Importa y fusiona conocimiento, contratos Engram y reglas desde refs/notes/ozymem del commit especificado",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "commit_ref": { "type": "string", "description": "SHA del commit o ref a importar (default: HEAD)" },
                            "note_ref": { "type": "string", "description": "Ref de la nota Git (default: refs/notes/ozymem)" }
                        },
                        "additionalProperties": false
                    }),
                },
            ];
            // Pagination support for tools/list
            let page_size = 100;
            let params_cursor = request
                .params
                .as_ref()
                .and_then(|p| p.get("cursor"))
                .and_then(Value::as_str)
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(0);
            let show_legacy = std::env::var("OZYMEM_SHOW_LEGACY_TOOLS")
                .map(|v| matches!(v.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
                .unwrap_or(false);
            let visible_tools: Vec<_> = if show_legacy {
                tools
            } else {
                tools
                    .into_iter()
                    .filter(|tool| tool.name.starts_with("ozy_"))
                    .collect()
            };
            let total = visible_tools.len();
            let tools_slice: Vec<_> = visible_tools
                .into_iter()
                .skip(params_cursor * page_size)
                .take(page_size)
                .collect();
            let has_more = params_cursor * page_size + page_size < total;
            let mut result_value = serde_json::to_value(mcp_common::ToolListResult {
                tools: tools_slice,
                next_cursor: if has_more {
                    Some((params_cursor + 1).to_string())
                } else {
                    None
                },
            })?;
            mark_legacy_tools_deprecated(&mut result_value);
            Ok(Some(ok_response(id, result_value)))
}
