# Ozygram MCP Tools & Agent Best Practices Guide

## 1. Overview
Ozygram is an enterprise cognitive memory engine, code graph, and reasoning runtime designed for agentic pair programming. It provides high-precision code surgery, AST indexing, blast radius impact analysis, semantic memory with MMR deduplication, and MCTS exploration with rollback protection (Dream-RSI).

---

## 2. Tool Precedence Protocol (Native Ozygram First)

To avoid context bloat, syntax breaks, and latency, agents must adhere to the following precedence hierarchy:

1. **`ozy_search_literal` > `grep_search`**:
   - Native Rust execution (<15ms) with token-budgeted output (default 800 tokens).
   - Prevents regex compilation bugs and context window exhaustion on large codebases.
2. **`ozy_ast_patch` > `replace_file_content`**:
   - Tree-Sitter syntax validation in memory, context-aware auto-indentation, and caller contract validation.
   - Rejects malformed syntax before writing to disk.
3. **`ozy_graph(action="incoming" | "impact_mermaid")`**:
   - Mandatory blast radius check before modifying shared functions, API endpoints, or database models (>5 dependents requires careful phased migration).
4. **`ozy_context(action="file", lod="contracts")` > raw `view_file`**:
   - AST skeleton with signatures and types, saving >70% tokens on inspection, plus automatic `[ALERT: GOTCHA_PREVENTION]` injection.
5. **`ozy_brain(action="plan" | "reflect" | "risk_review", goal="...")`**:
   - High-level cognitive advisory for planning topological refactors and risk assessment.
6. **`ozy_exploration` (Dream-RSI & MCTS)**:
   - Exploration tree for complex multi-step tasks with automatic active trajectory resolution and rollback snapshots.
7. **`ozy_doctor(action="status" | "code" | "audit_migrations")`**:
   - Database WAL health, duplicate AST detection, and SQL migration idempotency linter.

---

## 3. Tool Usage Specifications

### `ozy_brain`
- **Mandatory Requirements**: `action` and `goal` (or `query`) are strictly required. Calls with empty arguments `{}` or missing `goal` will fail fast (`[ERROR: MISSING_GOAL]`) to prevent token burn.
- **Actions**:
  - `action="plan", goal="<exact target>"`: Returns topological edit sequence (leaves to root), blast radius, stop conditions, and candidate files.
  - `action="reflect", goal="...", failures=["..."], changes=["..."]`: Analyzes regression or scope creep.
  - `action="risk_review", goal="..."`: Adversarial pre-flight critique.
- **Boilerplate Gating (`verbose`)**:
  - By default (`verbose: false`), static execution policies are omitted, saving ~600 tokens per call.
  - Set `verbose: true` only when explicit execution policy details are required.

### `ozy_exploration` (Dream-RSI)
- **Starting a Trajectory**:
  - `action="start", task_description="<detailed task>"`: `task_description` is strictly mandatory.
- **Recording Steps**:
  - `action="record_step", action_type="tool_call" | "patch", observation="..."`:
  - `trajectory_id` is **optional**: Ozygram auto-resolves the active `in_progress` trajectory for the current project.
  - Automatic objective rewards calibration (`[OBJECTIVE: TEST_PASSED]`, `[ALERT: TEST_FAILED]`, `[ALERT: SYNTAX_ERROR]`).
- **Completing Trajectories**:
  - `action="complete", status="completed"`: Auto-resolves active trajectory and renders Mermaid tree.
- **Rollback Protection**:
  - `action="rollback", node_id="<id>"`: Restores file snapshots to a known good state.

### `ozy_context`
- **Actions**:
  - `action="file", path="<path>", lod="contracts" | "macro" | "full"`: Extracts AST skeleton and prepends active gotchas.
  - `action="task", query="<search query>"`: Injects relevant project memories, active conventions, and engrams.

### `ozy_memory`
- **MMR Diversity Deduplication**: Semantic search automatically suppresses near-duplicate memories (>0.88 cosine similarity) to keep context concise.
- **Actions**: `save`, `search`, `sync_markdown`, `export_markdown`.

---

## 4. Zero-Emoji Standard
All logs, tags, commit messages, Mermaid diagrams, and MCP outputs strictly use textual badges:
- `[STATUS: ACTIVE]`
- `[ALERT: STALE_MEMORY]`
- `[ALERT: GOTCHA_PREVENTION]`
- `[COUPLED_WITH]`
- `[OBJECTIVE: TEST_PASSED]`
- `[ERROR: MISSING_GOAL]`
