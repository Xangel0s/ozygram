# Changelog

All notable changes to Ozygram will be documented in this file.
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
