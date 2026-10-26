# Changelog

All notable changes to Ozygram will be documented in this file.
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.6] - 2026-10-26

### Added
- **Generalized Hybrid Vector-Semantic & Topological Property Graph RAG**:
  - Eradicated domain hardcodes: dynamic prompt token extraction with multilingual stop-words filtering and 2+ letter acronym preservation (`OT`, `PO`, `PR`, `MR`, `UI`, `DB`).
  - AST symbol anchor enrichment querying `backend.search_ast_symbols`, boosting files that declare matched functions, classes, or interfaces.
  - Automatic topological BFS graph expansion (`backend.analyze_impact(anchor, 1)`) across callers and dependents when target file is omitted.
  - Multi-tier full-stack arc balancing in `planner.py` ensuring top-5 edit sequences cover Contracts (Tier 0), Services (Tier 2), UI/Endpoints (Tier 3), and Validation Scripts (Tier 4).
  - Source root immunity in `.ozymemignore` parser, protecting `src/`, `app/`, `lib/`, `components/` from generic ignore collisions.
  - Ultracompact output format (< 200 tokens) ensuring context windows remain lean and preventing output dump files.
- **One-Click Autonomous Project Initializer (`ozy init` / `ozymem init`)**:
  - Interactive and headless (`--auto`) CLI command that automatically discovers and configures installed AI coding clients: Antigravity IDE, Cursor (both user global and project-local), Claude Desktop (Windows, macOS, Linux), Windsurf, VS Code (Cline & Roo Code), and Zed.
  - Automatically initializes project-local `.ozymem` substrate, writes smart default `.ozymemignore`, and manages `.gitignore` protection without manual intervention.
  - Deploys executables to `~/.ozymem/bin` and updates user PATH automatically.
- **Zero-Python Embedded Standalone Reasoning Runtime (`ozy-brain.exe`)**:
  - Bundled PyInstaller standalone executable bundling Python 3.12, DuckDB, Pydantic, and all neuro-cognitive reasoning modules into a single zero-dependency binary.
  - Developers on React, Node.js, Go, or Rust can use Ozygram without installing or configuring Python.
  - Multi-tier resilient fallback: Embedded standalone binary (`ozy-brain.exe`) -> Python environment -> Rust fast-lane deterministic SQLite FTS5 fallback.
- **Deep Production Validation & Monorepo Hardening**:
  - Hardened Amygdala Reflex Substrate for Windows with strict string sanitization, eliminating `WindowsPath` parameter binding errors in SQLite WAL.
  - Implemented DuckDB connection reuse in batch event traces (`process_event_trace`), eliminating physical file lock latency and accelerating trace processing by > 400%.
  - Added bilingual semantic risk detection (Spanish keywords `migracion`, `seguridad`, `autenticación`) and candidate file directory path exclusion (`Ingenieria/`, `backups/`).
  - Fixed Windows release packaging in GitHub Actions CI by guaranteeing `upload/` directory creation.

## [0.1.5] - 2026-10-21

### Added
- **Distributed Neural Mesh & Collective Intelligence (Fase 6 — OzyBrain v2.4.0)**:
  - **Git-Native Heuristics Transport (`refs/notes/ozy-heuristics`)**: Deterministic line-by-line JSONL notes transport sorted by `pattern_id`, enabling conflict-free peer merges (`cat_sort_uniq`) without polluting the working tree or commit messages.
  - **Network Immunological Blood-Brain Barrier (BBB) & Anti-Injection Defense**: SHA-256 cryptographic provenance verification per tenant/author, antiseptic prompt-injection linter (`scan_context_poisoning`) neutralizing context-hijacking vectors (`ignore previous instructions`, `curl | sh`, `eval`), and quarantine isolation with `[ALERT: UNVERIFIED_HEURISTIC_QUARANTINE]`.
  - **Bayesian Consensus Reputation & Heuristic Apoptosis**: Distributed trust scoring ($R_{\text{consenso}} = \frac{\alpha_0 + V_{\text{success}}}{\alpha_0 + \beta_0 + V_{\text{success}} + F_{\text{failure}}}$) with automated apoptosis purging rules when $R_{\text{consenso}} < 0.35$ or when Amygdala threat scars are detected across $\ge 2$ peer environments (`[ALERT: HEURISTIC_APOPTOSIS_PURGED]`).
  - **Distributed Semantic Merger & Deduplication**: Token-cosine similarity engine in ChromaDB ($\cos \ge 0.88$) that merges complementary rules, suppresses duplicate node creation, increments peer confirmation counters, and reinforces synaptic weights ($w_{\text{new}} = \min(1.0, w_{\text{old}} + 0.08)$).
  - **Autonomous Dream-RSI Federated Synchronization**: Non-blocking offline-safe Git fetch with 4.0s timeout during the 30-minute idle sleep cycle, reconciling peer heuristics, updating `.ozymem/dream_latest_insights.json`, and exposing action `corpus_federated_sync` in MCP stdio.

- **Neuro-Cognitive Substrate, STDP Plasticity & Cerebellar Kinematics (Fase 5 — OzyBrain v2.3.0)**:
  - **Continuous STDP Hebbian Plasticity & SQLite Export**: Chronological Git commit sequence mining in DuckDB with automatic materialization of strong causal synapses ($w \ge 0.65$) into SQLite `memory_edges` (`COUPLED_WITH`) for sub-millisecond graph traversals.
  - **Neuromodulator Chemical Regulation**: Global 4D vector $\mathbf{N} = [D, NE, ACh, 5\text{-HT}]$ dynamically modulating RPE reward, flashbulb one-shot consolidation ($D \ge 0.75$), and Amygdala threat sensitivity ($NE \ge 0.60$).
  - **Cerebellar Forward Model & Code Dysmetria**: Kinematic motor model with Kullback-Leibler divergence ($D_{KL}$) detecting motor overshoot (`[ALERT: CODE_DYSMETRIA - MOTOR_OVERSHOOT]`) and missing coupled dependencies (`[ALERT: CODE_DYSMETRIA - UNDER_COUPLED]`).
  - **Corpus Callosum & Blood-Brain Barrier (BBB)**: Multi-workspace meta-pattern transfer between `crmnew` and `default` protected by Shannon entropy filtering ($H(S) \ge 4.5$) and regex secret/path scrubbing.

- **Neuro-Cognitive Architecture & Offline Dreaming (Fases 1–4 — v2.2.0)**:
  - **Amygdala Reflex Substrate**: Preemptive SQLite WAL reflex cache (`amygdala_reflexes`) with sub-millisecond (< 1ms) `ozy_context` interception (`[ALERT: AMYGDALA_REFLEX - HIGH_FRAGILITY]`).
  - **Glial & Glymphatic Homeostasis**: Background phagocytosis of dead file vectors/synapses across SQLite and ChromaDB, plus periodic DuckDB WAL compaction (`CHECKPOINT;`).
  - **Basal Ganglia Sequence Playbooks & STDP**: Co-occurrence choreography mining over Git commits with automatic sequence injection (`[PROCEDURAL_PLAYBOOK_DETECTED]`) and Spike-Timing-Dependent Plasticity in `analytics.duckdb`.
  - **Autonomous 30-Minute Idle Dreaming Watcher**: Zero-CPU Rust inactivity watcher (`idle_watcher.rs`) with edge-triggered latch triggering `DreamOrchestrator` to generate `.ozymem/dream_latest_insights.json`, exposed proactively via `ozygram://dream/insights` MCP resource.
  - **Thalamic Selective Attention**: Dynamic attention modulation (`thalamus.rs`) balancing lexical BM25 ($\alpha$), Amygdala threat scars ($\beta$), and AST graph topology ($\gamma$) based on task intent (`HOTFIX_DEBUG` vs `ARCHITECTURE_REFACTOR` vs `BALANCED`).

## [0.1.4] - 2026-10-20

### Added
- **MCP Catalog Consolidation (14 Core Tools)**:
  - Unified Git notes memory persistence into `ozy_memory` with actions `export_git_notes` and `import_git_notes` (supporting `commit_ref` and `note_ref`).
  - Streamlined exploration lifecycle in `ozy_exploration`: added `goal` alias in `action="start"`, and natural aliases `action="step"` and `action="finish"`.
  - Added full end-to-end integration test `test_consolidated_mcp_actions_exploration_and_git_notes`.
- **Registry Auto-Hydration & Scan Sync**:
  - `ozymem scan` now writes back synchronized stats (`file_count`, `vector_count`, `last_scan`) directly to `~/.ozymem/registry.db`.
  - `list_projects` automatically self-heals by hydrating project file counts directly from project `.ozymem/memory.db` when central stats are unpopulated.
- **Accurate AST Definition Parsing**:
  - Enhanced Tree-Sitter regex definitions in `queries.rs` to strictly distinguish definitions (`interface`, `type`, `class`, `function`) from imports, eliminating false positive fallback to imports.
  - Enabled multilingual import dependency parsing for TypeScript/JavaScript and Python during full scans.
- **Windows Linker Protection in Installer**:
  - `install.ps1` now performs safe pre-build renames (`.exe.running`) to prevent linker conflicts (`LNK1104`) on Windows when background MCP servers or IDE instances are active.

### Changed
- **Ozy Doctor Service Indicators**:
  - Replaced phantom `ACTIVO (PID: 0)` watcher state with clean `STANDBY / GIT_DRIVEN` status badge when no continuous watcher PID is alive.
- **Context Action Auto-Inference**:
  - `ozy_context` now auto-infers `action="file"` when `file_path` is passed, and `action="project"` when `project_name` or numeric `project_id` is supplied.
- **Consolidation Badges**:
  - Deprecated and consolidated tools (`ozy_export_memory_notes`, `ozy_import_memory_notes`, `ozy_replace_symbol`, `ozy_code_doctor`) now feature clear deprecation notices pointing to their unified counterparts.

## [0.1.3] - 2026-10-19

### Added
- **Knowledge as Code (Git-First Markdown Memories)**:
  - Atomic markdown memory persistence under `.ozy/memories/<kind>/*.md` with YAML Frontmatter (`id`, `kind`, `topic_key`, `title`, `author`, `date`, `severity`, `applies_to`, `tags`).
  - Merge-conflict-free across Git branches and auditable in Pull Requests.
  - Bi-directional sync between atomic Git markdown files and local SQLite WAL cache via `ozy_memory(action="sync_markdown"|"export_markdown"|"scan_markdown")`.
- **Preventive Gotchas Injection**:
  - `ozy_context(action="file")` automatically extracts and prepends `[ALERT: GOTCHA_PREVENTION]` for any matching files before code inspection, ensuring agents and developers do not repeat previously discovered bugs.
- **Memory Lifecycle & Token Guards**:
  - Typed lifecycle statuses (`active`, `deprecated`, `superseded`).
  - Deterministic token budget limits (default 800 tokens) with pagination support.
  - Freshness stale memory warning badges (`[ALERT: STALE_MEMORY: This memory may be out of date; verify live code first.]`).
- **MMR (Maximal Marginal Relevance) Diversity Deduplication**:
  - Vector search now filters sister memories with cosine similarity > 0.88, suppressing redundant repetitive context and saving up to 50% tokens on memory recall.
- **Active Trajectory Auto-Resolution**:
  - `ozy_exploration` automatically resolves the current `in_progress` trajectory without requiring explicit manual trajectory ID passing from the agent.
- **Consolidated AST Code Diagnostics in `ozy_doctor`**:
  - Integrated AST duplicate block detection and static code analysis under `ozy_doctor(action="code")`, deprecating legacy standalone tools.
- **Canonical MCP Instructions**:
  - Added comprehensive `mcp/ozygram/instructions.md` with explicit tool precedence hierarchy (`ozy_search_literal` > `grep_search`, `ozy_ast_patch` > `replace_file_content`).

### Changed
- **Token-Efficient Reasoning Outputs**:
  - Standardized `ozy_brain` and `ozy_exploration` output headers and planner steps to concise technical English, eliminating translation dissonance and reducing token consumption by 25%–40%.
- **Boilerplate Suppression**:
  - Execution Policy boilerplate (~600 tokens) is now omitted by default from `ozy_brain` responses; accessible only when explicitly requesting `verbose: true`.
- **Topological Refactor Guidance**:
  - Clarified plan phases to enforce leaf-to-root edits (`[CONTRACTS] Base Types & Schemas` -> `[UTILITIES] Core Helpers & Parsing` -> `[DOMAIN] Business Logic & State` -> `[ENTRYPOINTS] Public Routing & MCP Handlers`).

### Fixed
- **Fail-Fast Parameter Verification**:
  - `ozy_brain` now returns a prompt, informative error if `goal` is omitted, preventing accidental broad dirty-file sweeps and context pollution.
  - `ozy_exploration(action="start")` now fails fast if `task_description` is omitted.
- **Windows Path Normalization**:
  - Normalized path matching with canonical forward slashes and uppercase drive letters across core indexing and graph backends.
- **CI / CD Automation**:
  - Updated macOS release runner to `macos-latest` with build timeouts.
  - Resolved nested Tokio runtime blocking in CLI start/stop lifecycle commands.

---

## [0.1.2] - 2026-10-15
- Initial public release of Ozymem Core, Parser, Server, and CLI tools with Tree-Sitter multi-language support.
