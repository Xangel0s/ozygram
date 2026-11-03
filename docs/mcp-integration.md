# MCP (Model Context Protocol) Reference & Integration

Ozygram exposes its complete feature set as a standard **Model Context Protocol (MCP)** server over `stdio`, providing native out-of-the-box compatibility with **Antigravity IDE**, **Claude Desktop**, **Cursor**, **Windsurf**, and MCP extensions for **VS Code**.

---

## 1. MCP Server Configuration

Add the following entry to your MCP client configuration file (`claude_desktop_config.json`, `.gemini/antigravity-ide/mcp_config.json`, etc.):

### Windows (Single-Binary ozygram.exe)
```json
{
  "mcpServers": {
    "ozygram": {
      "command": "C:\\Users\\YOUR_USER\\.ozymem\\bin\\ozygram.exe",
      "args": ["serve"]
    }
  }
}
```
*(Legacy configurations pointing to `ozymem-server.exe` with `args: []` remain 100% supported).*

### Linux / macOS (Single-Binary ozygram)
```json
{
  "mcpServers": {
    "ozygram": {
      "command": "/home/YOUR_USER/.ozymem/bin/ozygram",
      "args": ["serve"]
    }
  }
}
```
*(Legacy configurations pointing to `ozymem-server` with `args: []` remain 100% supported).*

---

## 2. Featured MCP Tools

### A. Persistent Contextual Memory
| Tool | Key Parameters | Description |
| :--- | :--- | :--- |
| `lookup_engram` / `ozy_lookup_engram` | `symbol`, `file` | Deterministic $O(1)$ lookup of function signatures and contracts via memory-mapped tables (`rkyv` + `memmap2`). |
| `ozy_memory` | `action`, `kind`, `topic_key`, `query` | Unified memory manager: lessons, decisions, conventions, gotchas, sessions, timelines, and passive observation capture. |
| `record_lesson` / `record_decision` | `title`, `lesson` / `decision` | Direct endpoints for registering key lessons learned and architectural design decisions. |
| `record_gotcha` / `record_convention` | `gotcha`, `workaround` / `rule` | Records non-obvious runtime behaviors and team code conventions. |
| `deep_semantic_search` / `ozy_deep_search` | `query`, `limit`, `project` | Hybrid search with RRF combining FTS5 BM25 lexical retrieval and local FastEmbed ONNX dense embeddings. |

### B. Code Graph & AST Navigation
| Tool | Key Parameters | Description |
| :--- | :--- | :--- |
| `ozy_context` / `file_context` | `action`, `file_path`, `task` | Predictive prefill, adjacent function contracts, file-level rules, and dependent symbols. |
| `ozy_get_symbol` | `file_path`, `symbol_name`, `line` | Surgical retrieval of exact symbol source code (function, class, React component, hook, interface) without reading entire files into context. |
| `ozy_file_tree` | `directory`, `max_depth` | Native zero-process file tree exploration with file sizes and node types, avoiding terminal/PowerShell execution overhead. |
| `ozy_parse` | `file_path` | Native AST parser exposing definitions, line ranges, and dependency hints directly over JSON-RPC. |
| `ozy_ast_patch` | `file_path`, `symbol_name`, `new_code`, `dry_run` | **[RECOMMENDED]** Surgical AST-guided symbol replacement with auto-indentation and pre-flight Tree-Sitter validation. |
| `ozy_replace_symbol` | `file_path`, `symbol_name`, `new_code`, `dry_run` | *[DEPRECATED]* Legacy alias to `ozy_ast_patch`. |
| `ozy_search_literal` | `query`, `path_prefix`, `token_budget` | **[RECOMMENDED]** Fast (< 15ms) Rust code search with 800-token budget guard against context explosion. |
| `ozy_graph` | `action`, `file_path`, `depth` | Unified architectural navigation: `summary`, `neighbors`, `incoming` (blast radius), `impact_mermaid`, and structural reports. |
| `analyze_impact` | `target_file` | Maps direct and transitive dependent files, calculating architectural blast radius. |
| `graph_neighbors` | `file_path`, `direction` | Retrieves immediate incoming, outgoing, or bidirectional AST neighbors. |

### C. Exploration Tree & Self-Improvement (`ozy_exploration` / Dream-RSI v1.1.0)
| Action | Key Parameters | Description |
| :--- | :--- | :--- |
| `start` | `trajectory_id`, `task_description` | Initializes a new MCTS exploration trajectory persisted in SQLite. |
| `record_step` | `trajectory_id`, `action_type`, `observation`, `reward_score` | Records an exploration node with automatic parenting to the active trajectory leaf. |
| `record_batch` | `trajectory_id`, `steps` | Atomic bulk insertion of multiple technical milestones to eliminate agent turn tax. |
| `resume` | `trajectory_id` | Identifies the optimal unpruned leaf node to resume an interrupted exploration trajectory. |
| `complete` | `trajectory_id`, `status` | Marks the trajectory status as `completed`, `failed`, or `abandoned`. |
| `diagnose` | `trajectory_id` | Audits trajectory bottlenecks: high-latency nodes, excessive token usage, dead branches, and auto-pruning. |
| `get_tree` | `trajectory_id` | Returns the hierarchical tree topology with UCB1/UCT value scoring. |

### D. Cognitive Engine & Supervision (`ozy_brain`)
| Action | Key Parameters | Description |
| :--- | :--- | :--- |
| `plan` | `goal`, `context` | Generates a structured 5-phase plan with a deterministic completion checklist. |
| `audit_changes_with_critic` | `diff`, `files`, `plan_steps` | Adversarial code audit with hotspot correlation and destructive DDL/DML guards. |
| `get_repository_hotspots` | `limit` | Identifies repository files with the highest churn, bug-fix commits, and regression risk using DuckDB. |
| `consolidate_memory` | `threshold` | Consolidates related topics and prunes stale memories via exponential time decay. |
| `build_mental_model` | `scope` | Produces an architectural mental map ("where to start reading the codebase"). |

### E. Diagnostics & Code Health
| Tool | Key Parameters | Description |
| :--- | :--- | :--- |
| `ozy_verify_diff` | `file_path`, `diff` | Test-time sandboxed validation: verifies AST syntactic integrity prior to saving modifications. |
| `ozy_doctor` | `action` (`status`, `audit_migrations`, `code`), `scope`, `migrations_path` | **[UNIFIED DOCTOR]** Comprehensive operational diagnostics: SQLite integrity, embedding models, SQL migration linter, and static code analysis (duplicates/smells). |
| `ozy_code_doctor` | `scope`, `min_duplicate_lines` | *[CONSOLIDATED]* Legacy alias to `ozy_doctor(action="code")`. |
| `ozy_skills` | `action`, `query`, `category` | Official integration with skills.sh for searching and applying contextual guides. |
| `ozy_export_memory_notes` | `notes_ref` | Exports lessons and decisions to Git Notes (`refs/notes/ozymem`) for decentralized P2P synchronization. |
| `ozy_import_memory_notes` | `notes_ref` | Imports and merges memories from Git Notes without database conflicts. |

---

## 3. MCP Resources (`resources/list` & `resources/read`)

Ozygram exposes URI-addressable resources for AI agents:
- `ozymem://summary`: Global repository overview, counts of lessons, nodes, and tracked files.
- `ozymem://recent-lessons`: Latest lessons and decisions logged in the current project.
- `ozymem://file/{path}`: Complete context and domain rules associated with a specific file.
- `ozymem://file/{path}/neighbors`: Dependency graph relationships for a specified source file.

MCP clients can subscribe to these resources (`resources/subscribe`) to receive automatic notifications when new lessons are captured or when project graph structures are refreshed.


---

## 4. Autonomous Agent Precedence & Zero-Prompting Setup

A persistent challenge in AI agent workflows is that large language models (LLMs) are heavily biased towards built-in file tools (`read_file`, `edit_file`, terminal `dir`/`grep`). Without strict guidance, agents often ignore high-performance MCP tools, taking **23 minutes** on linear searches instead of **4 minutes** using Ozygram.

### 4.1. The 2-Tier Interception Engine

To enable 100% fluid, zero-prompting tool invocation (where the developer writes simple requests and the agent automatically chooses Ozygram without being reminded):

1. **Tier 1: Imperative Server Schemas (`schemas.rs`)**:
   Ozygram prefixes MCP tool schemas with strict cognitive directives:
   * `ozy_context`: `[PRIMARY CODE EXPLORATION TOOL - INVOKE BEFORE read_file OR grep]`
   * `ozy_patch`: `[MANDATORY PRE-FLIGHT PATCHER - ALWAYS SIMULATE BEFORE EDITING]`
   * `ozy_plan`: `[PRIMARY ARCHITECTURAL PLANNER]`
   These badges intercept the LLM's initial tool selection before it defaults to native read commands.

2. **Tier 2: Client Rule Header (`AGENTS.md` / `CLAUDE.md` / `GEMINI.md`)**:
   Add this mandatory header at the top of your project rules file:

   ```markdown
   ## Protocolo Obligatorio de Herramientas (Ozygram First)

   > [!IMPORTANT]
   > **REGLA DE PRECEDENCIA ESTRICTA PARA AGENTES**:
   > El espacio de trabajo está indexado en Ozygram. Para evitar exploraciones lentas de 20+ minutos y saturación de contexto:
   >
   > 1. **Exploración**: ESTÁ TERMINANTEMENTE PROHIBIDO usar `read_file` masivo o `grep` recursivo a ciegas. SIEMPRE invocar primero `ozy_context(action="task", query="<tarea o error>")` o `ozy_context(action="file", lod="contracts")`.
   > 2. **Modificación**: ANTES de escribir en disco, es OBLIGATORIO ejecutar `ozy_patch(action="simulate", file_path="...", candidate_code="...", target_symbol="...")` para verificar en memoria RAM (Copy-on-Write) que la firma no rompa llamadores en otros módulos.
   > 3. **Planificación**: Para tareas full-stack o que toquen múltiples componentes, ejecutar `ozy_plan(goal="...")` para obtener la secuencia topológica estricta y el blast radius en RAM.
   ```

### 4.2. Tested Benchmark: 23 Minutes vs. 4 Minutes

| Metric | Native LLM Exploration | Ozygram Fluid Interception |
| :--- | :--- | :--- |
| **Resolution Time** | **23 minutes** | **4 minutes** (5.75x faster) |
| **Token Budget** | > 45,000 tokens | < 6,000 tokens (>85% reduction) |
| **Disk Mutations** | Multi-turn disk trial & error | Volatile RAM simulation (CoW) -> Single surgical write |
| **Caller Arity Breakages** | Common in cross-module refactors | 0 (Pre-flight arity verification) |

### 4.3. Client Configuration Notes

* **Antigravity IDE**: Automatically loads `AGENTS.md` from workspace root and parent directories. The tool badges in `schemas.rs` are registered upon MCP handshake.
* **Claude Desktop & Claude Code**: Put the precedence protocol in `CLAUDE.md` at the project root.
* **Cursor**: Place the precedence block in `.cursorrules` or `.cursor/rules/ozygram.mdc`.
* **Windsurf**: Place the precedence block in `.windsurfrules`.
