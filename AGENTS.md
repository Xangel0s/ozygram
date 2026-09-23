# Ozygram Developer Agent Guidelines

## Dual-Tier Architecture (v0.4.0 & v1.1.0)
- `crates/ozymem-core`: Persistent ACID storage (SQLite), `memory_outbox` triggers, multi-language AST indexing (tree-sitter), hybrid semantic search (fastembed), graph analysis (petgraph).  
  `GraphBackend` (per-project memory at `{proj}/.ozymem/memory.db`) and `ProjectRegistry` (global registry at `~/.ozymem/registry.db`).
- `crates/ozymem-parser`: Native source code AST parsers (Python, Go, Rust, JS/TS, SQL) using Tree-Sitter and text heuristics.
- `crates/ozymem-cli`: Standalone CLI tool with subcommands `scan`, `lessons`, `dashboard`, `register`, `list`, `ignore`, `dream`, etc.
- `crates/ozymem-server`: Ultra-low latency MCP stdio server with 30+ tools, resources, prompts, resource subscriptions, and push notifications.
- `python/ozy-brain`: Asynchronous cognitive engine in Python with `SupervisorAgent`, `RiskCriticAgent`, `DataEngine` (DuckDB + Polars), `OutboxConsumer` (ChromaDB + FastEmbed ONNX), and `MemoryConsolidationAgent`.

## Key Features
- **Comprehensive Modular Documentation**: See [`docs/`](docs/INDEX.md) for dedicated domain guides:
  - [`docs/architecture.md`](docs/architecture.md): Dual-Tier Architecture & Transactional Outbox Pattern.
  - [`docs/semantic-search.md`](docs/semantic-search.md): Hybrid Semantic Search & RRF Fusion.
  - [`docs/supervision-and-validation.md`](docs/supervision-and-validation.md): Cognitive Supervision & Deterministic Validation.
  - [`docs/dream-team-tools.md`](docs/dream-team-tools.md): High-Performance Dream Team Tools (`tgrep`, `rtk`, `fastembed`).
  - [`docs/mcp-integration.md`](docs/mcp-integration.md): Comprehensive MCP Endpoint & Tool Reference.
  - [`docs/engram_system.md`](docs/engram_system.md): Deterministic $O(1)$ Engram System, Speculative Prefill & Git Notes P2P.
  - [`docs/dream-rsi.md`](docs/dream-rsi.md): Dream-RSI: Zero-Friction MCTS Exploration & Offline Self-Improvement.
  - [`docs/changelog.md`](docs/changelog.md): Chronological Version History & Release Notes.
- **Transactional Outbox Pattern (`memory_outbox`)**:
  - Automatic SQLite triggers (`lessons_outbox_ai`, `lessons_outbox_ad`, `observations_outbox_ai`, `observations_outbox_au`, `observations_outbox_ad`).
  - Background draining via `OutboxConsumer` in `ozy-brain` to ChromaDB. Zero orphan vectors.
- **Hybrid Semantic Search with RRF**:
  - `deep_semantic_search` / `ozy_deep_search`: Fuses lexical BM25 results from SQLite FTS5 with dense ONNX embeddings (`FastEmbed` with `BAAI/bge-m3` or `bge-base`) using Reciprocal Rank Fusion (RRF). Zero API token expenditures.
- **Daemon Auto-Spawn & Circuit Breaker**:
  - `ensure_ozy_brain_running()` automatically starts the Python process if not active.
  - If Python fails or exceeds the timeout (3s), the Circuit Breaker switches immediately to deterministic SQLite FTS5 fallback.
- **Cognitive Multi-Agent System (`ozy-brain`)**:
  - **Orchestration Supervisor (`SupervisorAgent`)**: Hierarchical and structured dispatch using `pydantic-ai`.
  - **Adversarial Critic (`RiskCriticAgent`)**: Regression vector simulation, destructive operation vetoes (`DROP TABLE`, `DELETE FROM`, blast radius > 8 files), and Git hotspot correlation.
  - **Memory Consolidation & Decay (`MemoryConsolidationAgent`)**: Clusters redundant engrams and computes exponential temporal decay ($S = C \cdot e^{-\lambda \Delta t}$).
- **Universal Model Cascade & OpenRouter Support (`config.py`)**:
  - Automatic fallback cascading across `OPENROUTER_API_KEY`, `OLLAMA_HOST`, `GEMINI_API_KEY`, `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`.
  - Zero-cost model tier (`gemini-2.0-flash`, `nemotron-reasoning:free`, `qwen2.5-coder`).
  - **Offline Local Heuristic Fallback ($0 Cost)**: Zero downtime when offline or lacking API keys.
- **Analytical Telemetry Engine (`DataEngine` DuckDB + Polars)**:
  - Ingests Git history to compute change frequency, code churn, contributor counts, and bug-fix density.
  - Embedded analytical storage in `.ozymem/analytics.duckdb`.
- **High-Performance Dream Team Integration**:
  - `tgrep` (Microsoft): Trigram inverted index regular expression search over massive repositories.
  - `rtk` (Rust Token Killer): ANSI sequence stripping and terminal payload compression.
- **Deterministic $O(1)$ Engram Table (`rkyv` + `memmap2`)**: Memory-mapped binary lookup of signatures and symbol contracts in $\approx 15\text{ ns}$.
- **Predictive Speculative Prefill**: Automatic injection of adjacent 1st-order dependency contracts into prompt prefill.
- **Test-Time Validation Sandbox (`ozy_verify_diff`)**: AST syntax and contract verification prior to disk write operations.
- **Multi-Language Dependency Resolution (Python & TS/JS)**:
  - Native import/export extraction via Tree-Sitter for Python and JavaScript/TypeScript.
  - Resolves relative paths (`.`, `..`), aliases (`@/`, `~/`), and workspace modules to concrete physical files (`.py`, `.ts`, `.tsx`, `.js`, `index.ts`, `__init__.py`).
  - Accurate file relationship mapping (`edge_count > 0`) in mixed monorepos.
- **Non-Blocking Embedding Lifecycle & SQLite Batch Transactions**:
  - Background initialization and download (`std::thread::spawn` decoupled from Tokio JSON-RPC thread).
  - Explicit model states: `Ready`, `NotDownloaded`, `Downloading`, `CorruptedOrDeleted`, `Failed(String)`.
  - Sub-20ms lesson and observation recording in SQLite + `memory_outbox`.
  - `BEGIN IMMEDIATE ... COMMIT` batch transactions in `full_scan` avoiding disk fsync thrashing on Windows.
- **Automatic Knowledge Capture with Git Hooks (`ozymem hook`)**:
  - CLI subcommand `ozymem hook install|uninstall|status|run` and MCP tool `install_git_hook`.
  - Cross-platform `.git/hooks/post-commit` hook indexing commit deltas and capturing lessons automatically.
  - Verification in `ozy_doctor` to audit active hook status.
- **Dream-RSI & Monte Carlo Tree Search (MCTS) v1.1.0**:
  - **Persistent Discovery Tree (`exploration_trajectories`, `exploration_nodes`)**: Hierarchical SQLite storage with `parent_id`, `depth`, `action_payload`, `visit_count`, and running Q-value (`value_estimate`).
  - **Smart Auto-Parenting in Rust Core**: When `parent_id` is omitted, automatically links to the latest active unpruned node (`depth = parent.depth + 1`), eliminating the fragility of manual node hash forwarding.
  - **Atomic Batch Mode (`record_batch`)**: Ingests multiple technical milestones in a single MCP turn, reducing agent turn tax by ~70%.
  - **Objective Reward Verification & Self-Pruning**: Automatically detects failure signals (`exit code != 0`, `timeout`, `SyntaxError`, `build failed`, `tests failed`), clamps inflated rewards to `< 0.0`, and flags `is_pruned = true` with `auto_pruned: true`.
  - **Deadlock-Free Trajectory Diagnostics (`diagnose`)**: Endpoint `ozy_exploration(action="diagnose")` with cycle-breaking visited sets, fallback to the latest trajectory if `trajectory_id` is omitted, and token efficiency metrics.
  - **Trajectory Resumption (`resume`)**: Action `ozy_exploration(action="resume")` locates the highest-value active unpruned leaf node for frictionless recovery after context compaction.
  - **Noise Filter & Token Budget Bounds**: Automatic truncation of observations to **32 KB per step** (`MAX_OBSERVATION_BYTES`), and exhaustive filtering of binary files (`.onnx`, `.lock`, `.db`, `.duckdb`, `.parquet`, SHA hex blobs) and cache directories (`.fastembed_cache`, `node_modules`, `target`).
  - **Native MCTS Backpropagation**: Incremental ancestor update $Q \leftarrow Q + \frac{R - Q}{N}$.
  - **Offline Counterfactual Simulator (`ReplaySimulator`)**: Deterministic replay at **0 LLM tokens and 0 re-executions**, computing fitness score $J(\pi)$ and diagnosing exploration bottlenecks.
  - **AST Safety Validation**: Strict audit via `AstSafetyAuditor` (blocking `subprocess`, `eval`, `rmtree`) and non-regression guard ($J_{\text{cand}} > J_{\text{base}} + \epsilon$ with zero false prunes).
  - **MCP & CLI Interface**: Endpoint `ozy_exploration` (`start`, `record_step`, `record_batch`, `complete`, `get_tree`, `diagnose`, `list`, `delete`, `resume`), `ozy_brain(action="dream_rsi")`, and CLI subcommands `ozymem dream run|status|diagnose`.

## Principles & Conventions
- **SOLID, DRY, KISS**: Keep code loosely coupled, extract reusable logic, and avoid over-engineering.
- **Git & Commits**: Write clean, feature-scoped commits following conventional commits (`feat:`, `fix:`, `docs:`, `test:`, `refactor:`).
- **Testing**: Maintain test coverage above 80% on all new features. Run `cargo test` and Python test suites prior to completing tasks.
- **Code Changes**: Always show descriptive diff previews and confirm with the user before applying modifications.
- **Windows Compatibility**: Use `cmd /c` for shell execution on Windows. Avoid raw `std::process::Command` calls on `.ps1` scripts without powershell wrappers.
- **Adding MCP Tools**: Register tool in `tools/list`, add handler in `handle_request` (or `handle_project_tool`/`handle_package_tool` for lock-free tools), and include assertions in integration tests.
