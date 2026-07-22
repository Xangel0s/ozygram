# Ozymem

**Ozymem** es un motor de memoria persistente para asistentes de código LLM vía el protocolo Model Context Protocol (MCP). Almacena lecciones, decisiones, convenciones y dependencias de cada proyecto en una base de datos SQLite local (`{project}/.ozymem/memory.db`), permitiendo que un agente LLM recupere contexto histórico sin depender de su ventana de tokens.

## Características

- **Memoria persistente por proyecto**: Lessons, decisiones, convenciones, gotchas y reglas de módulo. Cada proyecto tiene su propia `memory.db`.
- **Grafo de dependencias**: Indexa funciones, clases y relaciones entre archivos (soporta Python, Go, Rust, JS/TS, SQL).
- **Búsqueda semántica**: Embeddings locales (all-MiniLM-L6-v2) para encontrar lecciones por similitud de significado.
- **Servidor MCP**: 38 tools para exploración, memoria, paquetes, git y búsqueda inteligente.
- **Smart Search**: Búsqueda unificada que combina símbolos (tree-sitter), texto completo (FTS5) y embeddings semánticos en una sola llamada.
- **Learn from Changes**: Generación automática de lecciones desde git diff usando tree-sitter (detecta funciones nuevas, borradas o modificadas) con análisis de impacto en el grafo.
- **Impact Analysis enriquecido**: Severidad por heurísticas (🔴 breaking, 🟡 warning, 🟢 info) y nombres de funciones afectadas por archivo.
- **File Context enriquecido**: Por archivo devuelve funciones, dependientes, dependencias, último commit git y lecciones asociadas.
- **Graph Path**: Encuentra caminos de dependencia entre dos archivos usando petgraph `all_simple_paths`.
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

## Tools principales (38)

```rust
// Exploración
analyze_impact    // Impacto transitivo con severidad + funciones por archivo
file_context      // Contexto enriquecido: funciones + dependientes + git + lecciones
graph_summary     // Resumen del proyecto (archivos, funciones, lecciones)
list_files        // Lista archivos indexados
graph_neighbors   // Dependencias directas (incoming/outgoing)
graph_path        // Camino de dependencia entre dos archivos (all_simple_paths)
context_for_task  // Contexto empaquetado para una tarea (símbolos + lecciones + impacto)

// Memoria
record_lesson, record_decision, record_convention
record_gotcha, record_module_rule
learn_from_changes  // Auto-genera lecciones desde git diff con tree-sitter + preview
search_lessons      // Búsqueda texto completo (FTS5)
similar_lessons     // Búsqueda semántica por embeddings (all-MiniLM-L6-v2)
get_file_lessons, get_symbol_lessons, recent_lessons

// Búsqueda inteligente
smart_search      // Busca en símbolos + lessons FTS5 + embeddings, todo en paralelo
find_symbol       // Busca funciones/clases indexadas por tree-sitter (LIKE + filtro kind)

// Proyectos
list_projects, get_project_memories, delete_project
suggest_stale_projects, create_ozymignore

// Paquetes (npm/pnpm)
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

## Funcionalidades destacadas

### `analyze_impact` — Severidad y funciones
El análisis de impacto ahora clasifica cada archivo afectado con severidad heurística:
- **🔴 breaking**: Schemas, modelos, DTOs, entidades, interfaces, types
- **🟡 warning**: Archivos con lecciones registradas, +15 funciones, o profundidad ≤ 1
- **🟢 info**: Cambios en implementaciones sin efectos secundarios conocidos

Además lista las funciones clave de cada archivo impactado y muestra un resumen tipo `"3 🔴 breaking | 5 🟡 warning | 2 🟢 info"`.

### `learn_from_changes` — Lecciones desde git diff
Usa tree-sitter para comparar el AST del código antes y después de un commit:
- **Funciones nuevas** → `lesson`
- **Funciones borradas** → `decision`
- **Funciones modificadas** (cambio de línea) → `convention`
- **Preview mode**: `preview: true` muestra los cambios sin escribirlos
- **Impacto**: Incluye dependientes vía `graph_neighbors`
- **Embeddings**: Se calculan automáticamente → `similar_lessons` funciona inmediatamente

### `smart_search` — Búsqueda unificada
Ejecuta tres búsquedas en paralelo y las combina en una respuesta:
1. **Símbolos** (tree-sitter) — busca nombres de funciones/clases con LIKE
2. **Lessons** (FTS5) — búsqueda de texto completo sobre lecciones registradas
3. **Semántica** (all-MiniLM-L6-v2) — embeddings para similitud de significado

### `file_context` enriquecido
Además de las funciones del archivo, ahora devuelve:
- Dependientes (archivos que lo importan)
- Dependencias (archivos que importa)
- Último commit git que tocó el archivo
- Lecciones registradas para ese archivo

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
