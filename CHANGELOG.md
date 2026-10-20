# Changelog

All notable changes to Ozygram will be documented in this file.
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.4] - 2026-10-20

### Added
- **Consolidación del Catálogo MCP (14 Herramientas Núcleo)**:
  - Unificación de notas Git dentro de `ozy_memory` con las acciones `export_git_notes` e `import_git_notes` (con soporte para `commit_ref` y `note_ref`).
  - Ciclo de vida ágil en `ozy_exploration`: alias `goal` en `action="start"`, y alias naturales `action="step"` y `action="finish"`.
  - Suite de integración completa `test_consolidated_mcp_actions_exploration_and_git_notes` (70 tests pasando).
- **Auto-Hidratación del Registry y Sync en Scan**:
  - `ozymem scan` sincroniza y persiste estadísticas (`file_count`, `vector_count`, `last_scan`) en `~/.ozymem/registry.db`.
  - `list_projects` implementa auto-reparación hidratando los conteos desde el `.ozymem/memory.db` local si el registro central está en cero.
- **Parsing Multilingüe Preciso de Definiciones AST**:
  - Distinción estricta de regex en Tree-Sitter entre definiciones (`interface`, `type`, `class`, `function`) e importaciones, eliminando falsos positivos.
  - Habilitada la extracción de dependencias de importación para TypeScript/JavaScript y Python durante escaneos.
- **Protección contra Bloqueos de Ejecutables en Windows**:
  - `install.ps1` implementa renombramiento preventivo (`.exe.running`) para evitar errores `LNK1104` del linker cuando hay procesos MCP activos en segundo plano.

### Changed
- **Indicadores de Servicio en Ozy Doctor**:
  - Reemplazado el estado fantasma `ACTIVO (PID: 0)` por la etiqueta limpia `STANDBY / GIT_DRIVEN`.
- **Auto-Inferencia en `ozy_context`**:
  - Inferencia automática de `action="file"` al enviar `file_path`, y `action="project"` al enviar `project_name` o `project_id`.
- **Badges de Consolidación**:
  - Herramientas secundarias (`ozy_export_memory_notes`, `ozy_import_memory_notes`, `ozy_replace_symbol`, `ozy_code_doctor`) señalizadas con `[CONSOLIDATED]` sin romper retrocompatibilidad.

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
