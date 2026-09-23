# MCP (Model Context Protocol) Reference & Integration

Ozygram exposes its complete feature set as a standard **Model Context Protocol (MCP)** server over `stdio`, providing native out-of-the-box compatibility with **Antigravity IDE**, **Claude Desktop**, **Cursor**, **Windsurf**, and MCP extensions for **VS Code**.

---

## 1. MCP Server Configuration

Add the following entry to your MCP client configuration file (`claude_desktop_config.json`, `.gemini/antigravity-ide/mcp_config.json`, etc.):

### Windows
```json
{
  "mcpServers": {
    "ozygram": {
      "command": "C:\\Users\\YOUR_USER\\.ozymem\\bin\\ozymem-server.exe",
      "args": []
    }
  }
}
```

### Linux / macOS
```json
{
  "mcpServers": {
    "ozygram": {
      "command": "/home/YOUR_USER/.ozymem/bin/ozymem-server",
      "args": []
    }
  }
}
```

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
| `ozy_graph` | `action`, `file_path`, `depth` | Unified architectural navigation: `summary`, `neighbors`, `impact`, `path`, and structural dependency reports. |
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
| `ozy_doctor` | `format`, `include_projects` | Comprehensive operational diagnostics: SQLite integrity, embedding models, watchers, and registries. |
| `ozy_code_doctor` | `mode`, `scope`, `min_duplicate_lines` | Detects duplicated blocks, refactoring candidates, and structural boilerplate. |
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
