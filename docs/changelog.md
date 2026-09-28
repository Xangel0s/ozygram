# Version History & Changelog

This document tracks the chronological evolution, architectural milestones, and key improvements in **Ozygram**.

## Version v1.2.0 — Dream-RSI: State Rollback, Action Simulation, Visual Trees & Stale Memory Guard

### 1. State Rollback and Backtracking with File Snapshots
- **Atomic File Snapshots**: Added `rollback_snapshot` action in `ozy_exploration` and Rust Core to capture physical file states at key exploration nodes.
- **Automated Disk Restoration**: Introduced `rollback_node` and `rollback_to_parent` to instantly revert tracked files on disk to prior states when an exploration branch is pruned or rejected, leaving the codebase pristine without manual reversions.

### 2. Action Simulation & Blast Radius Pre-Flight (`simulate_action`)
- **AST Dependency Blast Radius**: Added `simulate_action` / `critique_hypothesis` in `ozy_brain` to calculate the blast radius of proposed edits up to depth 2 using the dependency graph.
- **Preventive Safety Directives**: Issues `[ALERT: HIGH_BLAST_RADIUS: SNAPSHOT REQUIRED]` whenever proposed modifications touch core interfaces or multiple dependent files.
- **Smart Backtracking Advice**: Automatically recommends optimal fallback nodes (`recommended_resume_node`) when hypotheses are assessed as high-risk or unviable.

### 3. Objective Auto-Reward Calculation & Anti-Hallucination Rejection
- **Deterministic Auto-Reward**: Evaluates objective signals from test runs and execution logs:
  - Exit code 0 / passing tests: `+1.0` (`[OBJECTIVE: TEST_PASSED]`).
  - Syntax or lint errors: `-0.8` (`[ALERT: SYNTAX_OR_LINT_ERROR]`).
  - False claim rejection: Flags attempts to claim `is_solution: true` without clean test execution, clamping rewards to `-1.0` and pruning the branch (`[ALERT: FALSE_SOLUTION_REJECTED]`).

### 4. Emoji-Free Mermaid Tree Visualizer (`get_tree`)
- **Clean Interactive Graph Output**: Enhanced `ozy_exploration(action="get_tree")` and CLI `ozymem dream tree` to generate clean Mermaid diagrams with textual status tags:
  - `[STATUS: ACTIVE]` for active leaf nodes.
  - `[ALERT: PRUNED]` for dead-end or pruned branches.
  - `[OBJECTIVE: SOLVED / TEST_PASSED]` for validated solution nodes.
  - `[ALERT: HIGH_BLAST_RADIUS: SNAPSHOT REQUIRED]` for high-risk modifications.

### 5. Stale Memory Reality Check against File MTime
- **Physical Disk MTime Auditing**: Integrated file modification time and existence checks into `ozy_brain` and `ozy_context`.
- **Anti-Staleness Alert**: Injects `[ALERT: STALE_MEMORY]` whenever the underlying source file on disk has a modification time more recent than the recorded lesson, ensuring the agent prioritizes active code over outdated recollections.

### 6. Architecture Streamlining (Excel Engine Pruning)
- **Dead Code Pruning**: Pruned 507 lines of unused legacy Excel template engine and calamine parser code, reducing compile overhead and binary footprint to focus solely on high-performance semantic memory and reasoning.

---

## 🌙 Version v1.1.0 — Dream-RSI: Zero-Friction MCTS Engine, Trajectory Resumption & Auto-Pruning

### 1. Resilient Trajectory Diagnostics (`diagnose`)
- **Cycle-Breaking & Deadlock Elimination**: Ensured ancestor path traversal uses a visited set (`sol_path_ids.insert(...)`), preventing infinite loops and timeouts when inspecting cyclic or re-entrant exploration graphs.
- **Automatic Fallback to Latest Trajectory**: When `trajectory_id` is omitted in MCP calls, `diagnose` automatically resolves the most recent exploration trajectory.
- **Resilient Stdio Error Handling**: Exploration handlers return structured JSON-RPC errors (`ToolCallResult { is_error: Some(true) }`) instead of bubbling errors up to the stdio loop, preventing server crashes and stdio EOFs.

### 2. Autonomous Self-Pruning on Objective Test Failures
- Automatically detects failing objective signals (`exit code != 0`, `FAILED`, `SyntaxError`, `TypeError`, `timed out`) and triggers immediate node pruning (`is_pruned: true`, penalty reward `< 0.0`, `auto_pruned: true`).
- Prunes failing search branches early to conserve agent context window and computational budget.

### 3. Trajectory Resumption (`resume` Action)
- Added `resume` action to `ozy_exploration` and Rust Core (`get_recommended_resume_node`).
- Identifies the highest-value active unpruned leaf node across an interrupted trajectory, allowing coding agents to recover instantly without restarting tasks from scratch.

### 4. Smart Auto-Parenting in Rust Core
- Eliminates the requirement for AI agents to memorize and forward parent hash IDs (`parent_id`) during sequential coding tasks.
- When `parent_id` is omitted, the Rust core atomically locates the most recent active leaf node (`ORDER BY depth DESC, created_at DESC LIMIT 1`) and computes `depth = parent.depth + 1`.

### 5. Atomic Batch Ingestion (`record_batch`)
- Introduced `record_batch` MCP action to ingest multiple technical milestones in a single JSON-RPC turn.
- Dramatically cuts user-perceived turn tax in linear execution phases while preserving the full MCTS tree topology.

### 6. Objective Reward Verification (Anti-Hallucination)
- Deterministic signal evaluator `detect_failure_signals` audits observations for process failure exit codes or test execution errors.
- Automatically clamps hallucinated positive rewards to `-1.0` and flags `objective_reward_override: true`.

### 7. Binary & Cache Noise Filtering
- Expanded `is_binary_file`, `is_noise_dir`, and `_is_noise_path` to filter `.onnx`, `.lock`, `.fastembed_cache`, `.db`, `.sqlite`, `.duckdb`, `.parquet`, `.so`, `.wasm`, `.pyc`, and hexadecimal hash blobs (≥32 hex chars) across Rust parser, Rust core, and Python brain.

---

## ⚡ Version v0.4.0 — Dual-Tier Engine, SQLite Outbox & FastEmbed RRF

### 1. Asymmetric Dual-Tier Architecture
- **Fast Lane (Rust)**: Ultra-low latency MCP server (< 5 ms), transactional ACID storage in SQLite, and $O(1)$ engram lookup tables via `rkyv` and `memmap2`.
- **Power Lane (Python)**: Asynchronous cognitive engine with `SupervisorAgent`, `RiskCriticAgent`, `DataEngine` (DuckDB + Polars), and `OutboxConsumer`.

### 2. Transactional Outbox Pattern (Zero Phantom Vectors)
- Automated SQLite triggers for insert, update, and delete events on lessons and observations.
- Background flushing (`OutboxConsumer`) to local ChromaDB collections.

### 3. Hybrid Semantic Search with RRF
- New MCP tool: `deep_semantic_search` / `ozy_deep_search`.
- Fuses lexical FTS5 (BM25) search with local dense embeddings (`FastEmbed` with `BAAI/bge-m3` or `bge-base`) using **Reciprocal Rank Fusion (RRF)**. 100% local C++ ONNX Runtime inference.

### 4. Resilience & Deterministic Fallbacks
- **Daemon Auto-Spawn**: `ensure_ozy_brain_running` launches the background Python worker automatically if not running.
- **Circuit Breaker**: If Python fails or exceeds the 3s timeout threshold, immediately switches to the local SQLite FTS5 fallback without disrupting the agent.

### 5. High-Performance Dream Team Integration
- Native integration with **`tgrep`** (Microsoft) for accelerated inverted trigram regex lookups.
- Integration with **`rtk`** (Rust Token Killer) for payload compression and context conservation.

---

## 🤖 Version v0.3.0 — Multi-Agent Cognition & Analytical Telemetry

### 1. Analytical Telemetry Engine (`DataEngine` DuckDB + Polars)
- Ingests Git history to compute change frequency, code churn, contributor counts, and bug-fix density.
- Embedded analytical storage in `.ozymem/analytics.duckdb` for instant OLAP queries.

### 2. Cognitive Multi-Agent System (`ozy-brain`)
- **Orchestration Supervisor (`SupervisorAgent`)**: Structured hierarchical dispatch powered by `pydantic-ai`.
- **Adversarial Critic (`RiskCriticAgent`)**: Simulates regression vectors, enforces destructive operation vetoes (`DROP TABLE`, `DELETE FROM`, blast radius > 8 files), and correlates Git hotspots.
- **Memory Consolidation (`MemoryConsolidationAgent`)**: Clusters redundant memories and applies exponential time decay formulas ($S = C \cdot e^{-\lambda \Delta t}$).

### 3. Universal Model Cascade & OpenRouter Support (`config.py`)
- Automatic cascading detection of `OPENROUTER_API_KEY`, `OLLAMA_HOST`, `GEMINI_API_KEY`, `OPENAI_API_KEY`, and `ANTHROPIC_API_KEY`.
- Fault-tolerant tier of zero-cost models (`gemini-2.0-flash`, `nemotron-reasoning:free`, `qwen2.5-coder`).
- **Offline Local Heuristic Fallback ($0 Cost)**: Zero downtime when offline or lacking API keys.

---

## 🚀 Version v0.2.0 — Cascading Path Resolution, AST & Code Health

### 1. Cascading Path Resolution (`resolve_target_path`)
- 5-stage path resolver for relative, normalized, and suffix matching, eliminating empty query returns in complex monorepos.

### 2. AST Symbol Fallback
- Automatic AST symbol retrieval when `context_for_task` or `ozy_context` finds no explicit lessons, returning adjacent contracts and immediate dependents.

### 3. Intelligent Duplicate Classification
- `ozy_code_doctor` categorizes `[High-Priority Refactor Candidates]` (duplicated business logic) vs `[Structural Boilerplate]` (data models and DTOs).

### 4. Integrated AST Diagnostics / Linter
- Static syntax error and warning detection using Tree-Sitter exposed in `ozy_doctor`.

### 5. Subpath Filtering Support
- Granular subpath filtering for fast targeted navigation in monorepos.
