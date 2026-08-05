# Ozymem

**Ozymem** es un motor de memoria persistente para asistentes de código LLM vía el protocolo Model Context Protocol (MCP). Almacena lecciones, decisiones, convenciones y dependencias de cada proyecto en una base de datos SQLite local (`{project}/.ozymem/memory.db`), permitiendo que un agente LLM recupere contexto histórico sin depender de su ventana de tokens.

## Características

- **Memoria persistente por proyecto**: Lessons, decisiones, convenciones, gotchas y reglas de módulo. Cada proyecto tiene su propia `memory.db`.
- **Grafo de dependencias**: Indexa funciones, clases y relaciones entre archivos (soporta Python, Go, Rust, JS/TS, SQL).
- **Búsqueda semántica**: Embeddings locales (all-MiniLM-L6-v2) para encontrar lecciones por similitud de significado.
- **Servidor MCP simplificado**: 8 tools principales (`ozy_context`, `ozy_memory`, `ozy_graph`, `ozy_code_doctor`, `ozy_doctor`, `ozy_skills`, `ozy_brain`, `ozy_project`) con aliases legacy compatibles pero ocultos por defecto.
- **Smart Search y Ozy Query**: Búsqueda unificada MCP y traductor CLI seguro (`ozymem q`) para consultas compactas tipo `grep`, `find`, `ctx`, `trace`, `arch`, `doctor`, `code`, `skills`.
- **Learn from Changes**: Generación automática de lecciones desde git diff usando tree-sitter (detecta funciones nuevas, borradas o modificadas) con análisis de impacto en el grafo.
- **Impact Analysis enriquecido**: Severidad por heurísticas (`[BREAKING]`, `[WARN]`, `[INFO]`) y nombres de funciones afectadas por archivo.
- **File Context enriquecido**: Por archivo devuelve funciones, dependientes, dependencias, último commit git y lecciones asociadas.
- **Graph Path**: Encuentra caminos de dependencia entre dos archivos usando petgraph `all_simple_paths`.
- **Ignora ruido**: `node_modules/`, `target/`, `build/`, `dist/`, `.git/` excluidos automáticamente. Soporta `.ozymignore` personalizado.
- **Package management**: `create_project`, `add_package`, `remove_package`, `get_dependencies`, `run_script`.
- **Resource subscriptions**: El LLM puede suscribirse a recursos (`ozymem://summary`) y recibir notificaciones push cuando cambian.
- **Feedback real**: Notificaciones `notifications/methods/resources/updated` tras grabar lecciones o modificar el proyecto.
- **Ozy Doctor / Code Doctor**: Diagnóstico preview-safe del sistema, proyectos, memorias, duplicados, hotspots de arquitectura y recomendaciones de autosanado sin ejecutar cambios automáticos.
- **Skills oficiales**: `ozy_skills` expone metadata review-only de skills oficiales de `skills.sh` como contexto interno de buenas prácticas, sin ejecutar contenido externo.

## Inicio rápido

```bash
# Iniciar el servidor MCP (modo stdio)
cargo run -p ozymem-server
```

El servidor acepta conexiones vía stdio siguiendo el protocolo MCP. Se integra con clientes como Claude Desktop, Cursor, o cualquier agente que soporte MCP.

## Tools principales MCP (8)

Las tools históricas siguen funcionando como aliases internos para no romper clientes existentes, pero no aparecen en `tools/list` salvo que se active `OZYMEM_SHOW_LEGACY_TOOLS=true`.

```rust
ozy_context      // Contexto de tarea, archivo, resumen, files y memorias recientes
ozy_memory       // Record/search/list de lessons, decisions, conventions, gotchas y module rules
ozy_graph        // Summary, neighbors, impact, paths y architecture_report
ozy_code_doctor  // Duplicados, redundancias, hotspots, buenas prácticas y autosanado preview
ozy_doctor       // Salud de DB, registry, proyectos, memorias, embeddings, watchers e índices
ozy_skills       // Metadata oficial skills.sh review-only para buenas prácticas internas
ozy_brain        // Cerebro híbrido Rust/Python: plan, reflexión, recall profundo y revisión de riesgos
ozy_project      // Proyectos, packages, refresh index, stale projects e ignore rules
```

### Ozy Brain

`ozy_brain` combina contexto curado por Rust con razonamiento Python local en modo asesor. La respuesta incluye:

- **Structured Plan**: fases, archivos candidatos, comandos de validación y condiciones de parada.
- **Brain Context Pack**: estado git no destructivo, scoring de archivos candidatos, nivel de riesgo y recomendaciones persistibles.
- **Execution Policy**: acciones permitidas sin confirmación, acciones que requieren confirmación y límites explícitos del worker Python.

Python no modifica archivos, no ejecuta comandos del proyecto y no escribe en SQLite; Rust conserva la autoridad MCP, memoria, grafo e indexación.

### Aliases legacy

`analyze_impact`, `file_context`, `graph_summary`, `list_files`, `graph_neighbors`, `graph_path`, `context_for_task`, `record_*`, `search_lessons`, `similar_lessons`, `smart_search`, `find_symbol`, tools de proyectos, paquetes y git continúan disponibles durante la transición.


## Ozy Query CLI — traductor seguro para agentes

`ozymem q` permite escribir consultas cortas tipo shell, pero **no ejecuta shell externo**. Traduce la intención a consultas internas de Ozymem y devuelve salida destilada para ahorrar tokens.

```powershell
ozymem q grep auth
ozymem q grep "record_lesson" 220-260
ozymem q find GraphBackend
ozymem q ctx "duplicados en doctor"
ozymem q file crates/ozymem-server/src/main.rs
ozymem q trace crates/ozymem-server/src/main.rs
ozymem q tree crates/ozymem-server/src/main.rs
ozymem q arch
ozymem q d
ozymem q c
ozymem q sk react
```

Opciones útiles:

```powershell
ozymem q grep auth --limit 5
ozymem q arch --json
ozymem q ctx "migración" --tokens 800
```

Modo seguro:
- No ejecuta `grep`, `rg`, PowerShell, `cmd`, ni comandos arbitrarios.
- Si no entiende una consulta, devuelve sugerencias seguras.
- `--json` emite JSON compacto para scripts/agentes.

Alias recomendado en PowerShell:

```powershell
Set-Alias oz ozymem
oz q arch
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
- **`[BREAKING]`**: Schemas, modelos, DTOs, entidades, interfaces, types
- **`[WARN]`**: Archivos con lecciones registradas, +15 funciones, o profundidad ≤ 1
- **`[INFO]`**: Cambios en implementaciones sin efectos secundarios conocidos

Además lista las funciones clave de cada archivo impactado y muestra un resumen tipo `"3 [BREAKING] | 5 [WARN] | 2 [INFO]"`.

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
