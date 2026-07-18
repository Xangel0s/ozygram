# Ozymem

**Ozymem** es un motor de memoria persistente para asistentes de código LLM vía el protocolo Model Context Protocol (MCP). Almacena lecciones, decisiones, convenciones y dependencias de cada proyecto en una base de datos SQLite local (`{project}/.ozymem/memory.db`), permitiendo que un agente LLM recupere contexto histórico sin depender de su ventana de tokens.

## Características

- **Memoria persistente por proyecto**: Lessons, decisiones, convenciones, gotchas y reglas de módulo. Cada proyecto tiene su propia `memory.db`.
- **Grafo de dependencias**: Indexa funciones, clases y relaciones entre archivos (soporta Python, Go, Rust, JS/TS, SQL).
- **Búsqueda semántica**: Embeddings locales (all-MiniLM-L6-v2) para encontrar lecciones por similitud de significado.
- **Servidor MCP**: 30+ tools para que el LLM explore el proyecto, registre lecciones, administre paquetes y reciba notificaciones en tiempo real.
- **Ignora ruido**: `node_modules/`, `target/`, `build/`, `dist/`, `.git/` excluidos automáticamente. Soporta `.ozymignore` personalizado.
- **Package management**: `create_project`, `add_package`, `remove_package`, `get_dependencies`, `run_script`.
- **Resource subscriptions**: El LLM puede suscribirse a recursos (`ozymem://summary`) y recibir notificaciones push cuando cambian.
- **Feedback real**: Notificaciones `notifications/methods/resources/updated` tras grabar lecciones o modificar el proyecto.

## Inicio rápido

```bash
# Iniciar el servidor MCP (modo stdio)
cargo run -p ozymem-server
```

El servidor acepta conexiones vía stdio siguiendo el protocolo MCP. Se integra con clientes como Claude Desktop, Cursor, o cualquier agente que soporte MCP.

## Tools principales

```rust
// Exploración
analyze_impact, file_context, graph_summary, list_files
graph_neighbors, search_lessons, similar_lessons

// Memoria
record_lesson, record_decision, record_convention
record_gotcha, record_module_rule

// Proyectos
list_projects, get_project_memories, delete_project
suggest_stale_projects, create_ozymignore

// Paquetes
create_project, add_package, remove_package
get_dependencies, run_script, analyze_package
verify_dependencies

// Git
git_recent_changes, git_diff_summary, git_diff_file
git_blame_line, recent_changes_with_impact
```

## Recursos MCP

| URI | Descripción |
|---|---|
| `ozymem://summary` | Resumen del proyecto indexado |
| `ozymem://recent-lessons` | Últimas lecciones registradas |
| `ozymem://full-context` | Bundle: summary + files + lessons |
| `ozymem://file/{path}` | Contexto de un archivo específico |
| `ozymem://file/{path}/neighbors` | Dependencias de un archivo |

## Prompts MCP

| Prompt | Descripción |
|---|---|
| `analyze-file` | Análisis completo de un archivo (contexto + impacto + lecciones) |
| `review-lessons` | Revisión de lecciones de un archivo |
| `project-status` | Visión general del proyecto |

## Arquitectura

```
LLM Agent ←→ MCP (stdio) ←→ ozymem-server
                                │
                    ┌───────────┴───────────┐
                    │                       │
          ProjectRegistry          GraphBackend
          (~/.ozymem/registry.db)  ({proj}/.ozymem/memory.db)
                    │                       │
                    │              ┌────────┴────────┐
                    │              │                 │
               Project           files +         lessons +
               metadata          functions       embeddings
```

## Requisitos

- Rust stable
- Sin dependencias externas (no requiere Docker, Memgraph, ni base de datos remota)
