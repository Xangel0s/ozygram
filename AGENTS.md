# Ozymem Developer Agent Guidelines

## Arquitectura y Monorepo
- `crates/ozymem-core`: Almacenamiento persistente (SQLite), indexación multi-lenguaje (tree-sitter), búsqueda semántica (fastembed), análisis de grafo (petgraph).  
  `GraphBackend` (memoria por proyecto en `{proj}/.ozymem/memory.db`) y `ProjectRegistry` (global en `~/.ozymem/registry.db`).
- `crates/ozymem-parser`: Parsers de código fuente estructurado (Python, Go, Rust, JS/TS, SQL) con tree-sitter nativo y heurística de texto.
- `crates/ozymem-cli`: Herramienta de línea de comandos con subcomandos `scan`, `lessons`, `dashboard`, `register`, `list`, `ignore`, etc.
- `crates/ozymem-server`: Servidor MCP sobre stdio con 30+ tools, resources, prompts, resource subscriptions y notificaciones push.

## Features Clave
- **MCP Tools (30+)**: Exploración (`analyze_impact`, `graph_summary`, `list_files`, `graph_neighbors`), memoria (`record_lesson`, `record_decision`, `record_convention`, `record_gotcha`, `record_module_rule`, `search_lessons`, `similar_lessons`), proyectos (`list_projects`, `get_project_memories`, `delete_project`, `suggest_stale_projects`), paquetes (`create_project`, `add_package`, `remove_package`, `get_dependencies`, `run_script`, `analyze_package`, `verify_dependencies`), git (5 tools), `.ozymignore` (`create_ozymignore`).
- **Recursos MCP**: 5 recursos (`ozymem://summary`, `recent-lessons`, `full-context`, `file/{path}`, `file/{path}/neighbors`) con soporte de templates y subscriptions.
- **Prompts MCP**: 3 prompts (`analyze-file`, `review-lessons`, `project-status`).
- **Resource subscriptions**: LLM puede suscribirse a recursos vía `resources/subscribe` y recibir `notifications/methods/resources/updated` cuando cambian.
- **`.ozymignore`**: Carga automática de `.ozymignore` + `.gitignore` en `full_scan`. `is_noise_dir` filtra `node_modules/`, `target/`, `.git/`, etc.
- **Package management**: `create_project` ejecuta `pnpm/npm init -y` + `add_package`/`remove_package` vía `pnpm add/remove`. `get_dependencies` lee `package.json` estructurado. `verify_dependencies` escanea imports vs declarados.
- **Sin dependencias externas**: SQLite local, no requiere Docker, Memgraph, ni base de datos remota.

## Principios y Convenciones
- **SOLID, DRY, KISS**: Mantener el código acoplado lo mínimo posible, extraer lógica reutilizable y no sobrediseñar.
- **Git y Commits**: Realizar commits limpios por característica siguiendo la convención de `conventional commits` (`feat:`, `fix:`, `docs:`, `test:`, `refactor:`).
- **Pruebas**: Cobertura de pruebas superior al 80% en cualquier funcionalidad nueva. Ejecutar `cargo test` antes de dar por completado cualquier desarrollo.
- **Cambios en Código**: Solicitar confirmación del usuario mostrando un diff descriptivo de los cambios antes de editarlos en disco.
- **Windows compat**: Usar `cmd /c` para comandos shell en Windows. Evitar `std::process::Command` directo para scripts `.ps1`.
- **Nueva tool MCP**: Registrar tool en `tools/list`, añadir handler en `handle_request` (o `handle_project_tool`/`handle_package_tool` para tools sin backend lock). Añadir aserciones en tests de integración.
