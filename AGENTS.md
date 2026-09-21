# Ozymem / Ozygram Developer Agent Guidelines

## Arquitectura Dual-Tier (v0.4.0)
- `crates/ozymem-core`: Almacenamiento persistente (SQLite), triggers de `memory_outbox`, indexación multi-lenguaje (tree-sitter), búsqueda semántica (fastembed), análisis de grafo (petgraph).  
  `GraphBackend` (memoria por proyecto en `{proj}/.ozymem/memory.db`) y `ProjectRegistry` (global en `~/.ozymem/registry.db`).
- `crates/ozymem-parser`: Parsers de código fuente estructurado (Python, Go, Rust, JS/TS, SQL) con tree-sitter nativo y heurística de texto.
- `crates/ozymem-cli`: Herramienta de línea de comandos con subcomandos `scan`, `lessons`, `dashboard`, `register`, `list`, `ignore`, etc.
- `crates/ozymem-server`: Servidor MCP sobre stdio con 30+ tools, resources, prompts, resource subscriptions y notificaciones push.
- `python/ozy-brain`: Motor cognitivo auxiliar asíncrono en Python con `SupervisorAgent`, `RiskCriticAgent`, `DataEngine` (DuckDB + Polars), `OutboxConsumer` (ChromaDB + FastEmbed ONNX) y `MemoryConsolidationAgent`.

## Features Clave (v0.4.0, v0.3.0 & v0.2.0)
- **Documentación Modular Completa**: Consulte la carpeta [`docs/`](docs/INDEX.md) con guías dedicadas por secciones:
  - [`docs/architecture.md`](docs/architecture.md): Arquitectura Dual-Tier y Patrón Outbox.
  - [`docs/semantic-search.md`](docs/semantic-search.md): Búsqueda Semántica Híbrida y Fusión RRF.
  - [`docs/supervision-and-validation.md`](docs/supervision-and-validation.md): Supervisión Cognitiva y Validación Determinista.
  - [`docs/dream-team-tools.md`](docs/dream-team-tools.md): Herramientas del Dream Team (`tgrep`, `rtk`, `fastembed`).
  - [`docs/mcp-integration.md`](docs/mcp-integration.md): Referencia completa de endpoints MCP.
  - [`docs/engram_system.md`](docs/engram_system.md): Sistema Engram $O(1)$, prefill especulativo y Git Notes P2P.
- **Patrón Transaccional Outbox (`memory_outbox`)**:
  - Triggers automáticos en SQLite (`lessons_outbox_ai`, `lessons_outbox_ad`, `observations_outbox_ai`, `observations_outbox_au`, `observations_outbox_ad`).
  - Drenado en segundo plano por `OutboxConsumer` en `ozy-brain` hacia ChromaDB. Cero vectores huérfanos.
- **Búsqueda Semántica Híbrida con RRF**:
  - `deep_semantic_search` / `ozy_deep_search`: Combina resultados léxicos BM25 de SQLite FTS5 y embeddings densos ONNX (`FastEmbed` con `BAAI/bge-m3` o `bge-base`) mediante Reciprocal Rank Fusion (RRF). Cero tokens de API gastados.
- **Daemon Auto-Spawn & Circuit Breaker**:
  - `ensure_ozy_brain_running()` levanta automáticamente el proceso Python si no está activo.
  - Si Python falla o excede el timeout (3s), el Circuit Breaker conmuta de inmediato al fallback determinista de SQLite FTS5.
- **Sistema Multi-Agente Cognitivo (`ozy-brain`)**:
  - **Supervisor Orquestador (`SupervisorAgent`)**: Despacho jerárquico y estructurado con `pydantic-ai`.
  - **Agente Crítico Adversarial (`RiskCriticAgent`)**: Simulación de vectores de regresión, veto de operaciones destructivas (`DROP TABLE`, `DELETE FROM`, blast radius > 8 archivos) y análisis cruzado con hotspots.
  - **Sintetizador y Decaimiento de Memoria (`MemoryConsolidationAgent`)**: Agrupación de engrams redundantes y cálculo de decaimiento temporal exponencial ($S = C \cdot e^{-\lambda \Delta t}$).
- **Capa de Modelos Universal & OpenRouter (`config.py`)**:
  - Detección automática en cascada de `OPENROUTER_API_KEY`, `OLLAMA_HOST`, `GEMINI_API_KEY`, `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`.
  - Cadena de tolerancia a fallos con modelos gratuitos (`gemini-2.0-flash`, `nemotron-reasoning:free`, `qwen2.5-coder`).
  - **Fallback Heurístico Local Offline ($0 Costo)**: Cero fallos en ausencia de red o claves API.
- **Motor Analítico de Telemetría (`DataEngine` DuckDB + Polars)**:
  - Ingestión de historial de Git para calcular frecuencia de cambios, volumen de líneas modificadas (*churn*), número de autores y densidad de fixes.
  - Persistencia analítica embebida en `.ozymem/analytics.duckdb`.
- **Integración con "The Dream Team"**:
  - `tgrep` (Microsoft): Búsqueda trigram indexada de expresiones regulares sobre monorepos gigantes.
  - `rtk` (Rust Token Killer): Purga de secuencias ANSI y compresión de payloads de terminal.
- **Tabla Determinista de Engrams $O(1)$ (`rkyv` + `memmap2`)**: Búsqueda binaria de firmas y contratos de símbolos en $\approx 15\text{ ns}$.
- **Prefill Predictivo**: Inyección automática de dependencias adyacentes de primer orden en el prefill de prompts.
- **Sandbox de Validación Test-Time (`ozy_verify_diff`)**: Comprobación sintáctica de diffs antes de persistir cambios.
- **Resolución de Dependencias Multi-Lenguaje (Python & TS/JS)**:
  - Extracción nativa de imports/exports vía Tree-Sitter para Python y JavaScript/TypeScript.
  - Resolución inteligente de rutas relativas (`.`, `..`), alias (`@/`, `~/`) y módulos de workspace hacia archivos físicos concretos (`.py`, `.ts`, `.tsx`, `.js`, `index.ts`, `__init__.py`).
  - Detección precisa de relaciones entre archivos (`edge_count > 0`) en monorepos mixtos.
- **Ciclo de Vida de Embeddings No Bloqueante & Transacciones SQLite Batch**:
  - Descarga e inicialización en segundo plano (`std::thread::spawn` desacoplado del hilo JSON-RPC Tokio).
  - Estados catalogados del modelo: `Ready`, `NotDownloaded`, `Downloading`, `CorruptedOrDeleted`, `Failed(String)`.
  - Guardado de lecciones y observaciones inmediato (<20ms) en SQLite + cola `memory_outbox`.
  - Transacciones `BEGIN IMMEDIATE ... COMMIT` en `full_scan` evitando miles de llamadas a fsync en Windows.
- **Captura Automática de Conocimiento con Git Hooks (`ozymem hook`)**:
  - Subcomando CLI `ozymem hook install|uninstall|status|run` y tool MCP `install_git_hook`.
  - Hook nativo `.git/hooks/post-commit` multiplataforma (Windows y Unix) que indexa los deltas del commit y preserva lecciones/observaciones sin intervención manual.
  - Verificación en `ozy_doctor` para auditar si el hook está activo.

## Principios y Convenciones
- **SOLID, DRY, KISS**: Mantener el código acoplado lo mínimo posible, extraer lógica reutilizable y no sobrediseñar.
- **Git y Commits**: Realizar commits limpios por característica siguiendo la convención de `conventional commits` (`feat:`, `fix:`, `docs:`, `test:`, `refactor:`).
- **Pruebas**: Cobertura de pruebas superior al 80% en cualquier funcionalidad nueva. Ejecutar `cargo test` antes de dar por completado cualquier desarrollo.
- **Cambios en Código**: Solicitar confirmación del usuario mostrando un diff descriptivo de los cambios antes de editarlos en disco.
- **Windows compat**: Usar `cmd /c` para comandos shell en Windows. Evitar `std::process::Command` directo para scripts `.ps1`.
- **Nueva tool MCP**: Registrar tool en `tools/list`, añadir handler en `handle_request` (o `handle_project_tool`/`handle_package_tool` para tools sin backend lock). Añadir aserciones en tests de integración.
