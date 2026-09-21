# Integración y Referencia MCP (Model Context Protocol)

Ozygram expone todas sus capacidades como un servidor estándar **Model Context Protocol (MCP)** sobre `stdio`, haciéndolo compatible de forma nativa con **Antigravity IDE**, **Claude Desktop**, **Cursor**, **Windsurf** y extensiones MCP de **VS Code**.

---

## 1. Configuración del Servidor MCP

Agrega la siguiente entrada en tu archivo de configuración de cliente MCP (`claude_desktop_config.json`, `.gemini/antigravity-ide/mcp_config.json`, etc.):

### Windows
```json
{
  "mcpServers": {
    "ozygram": {
      "command": "C:\\Users\\TU_USUARIO\\.ozymem\\bin\\ozymem-server.exe",
      "args": []
    }
  }
}
```

### Linux / macOS
```json
{
  "mcpServers": {
    "ozygram": {
      "command": "/home/TU_USUARIO/.ozymem/bin/ozymem-server",
      "args": []
    }
  }
}
```

---

## 2. Herramientas MCP Destacadas

### A. Memoria Contextual Persistente
| Herramienta | Parámetros Clave | Descripción |
| :--- | :--- | :--- |
| `lookup_engram` / `ozy_lookup_engram` | `symbol`, `file` | Búsqueda determinista $O(1)$ de firmas y contratos en memoria mapeada (`rkyv` + `memmap2`). |
| `ozy_memory` | `action`, `kind`, `topic_key`, `query` | Herramienta unificada de memoria: lecciones, decisiones, convenciones, gotchas, sesiones, timelines y passive capture. |
| `record_lesson` / `record_decision` | `title`, `lesson` / `decision` | Endpoints directos para registrar lecciones y decisiones de diseño. |
| `record_gotcha` / `record_convention` | `gotcha`, `workaround` / `rule` | Registra comportamientos no obvios y convenciones de código. |
| `deep_semantic_search` / `ozy_deep_search` | `query`, `limit`, `project` | Búsqueda híbrida con RRF combinando FTS5 léxico y embeddings densos locales FastEmbed ONNX. |

### B. Grafo de Código y Navegación AST
| Herramienta | Parámetros Clave | Descripción |
| :--- | :--- | :--- |
| `ozy_context` / `file_context` | `action`, `file_path`, `task` | Prefill predictivo, contratos de funciones adyacentes, reglas de archivo y dependientes. |
| `ozy_graph` | `action`, `file_path`, `depth` | Navegación unificada de arquitectura: `summary`, `neighbors`, `impact`, `path` y reporte estructural. |
| `analyze_impact` | `target_file` | Mapea archivos dependientes directos e indirectos calculando el radio de dispersión. |
| `graph_neighbors` | `file_path`, `direction` | Obtiene vecinos inmediatos (`incoming`, `outgoing`, `both`) en el grafo AST. |

### C. Árbol de Exploración y Auto-Mejora (`ozy_exploration` / Dream-RSI v1.1.0)
| Acción | Parámetros Clave | Descripción |
| :--- | :--- | :--- |
| `start` | `trajectory_id`, `task_description` | Inicia una nueva trayectoria de exploración MCTS persistida en SQLite. |
| `record_step` | `trajectory_id`, `action_type`, `observation`, `reward_score` | Registra un nodo con auto-parenting automático al último nodo activo en Rust core. |
| `record_batch` | `trajectory_id`, `steps` | Inserción atómica en lote de múltiples hitos técnicos para eliminar turn tax. |
| `complete` | `trajectory_id`, `status` | Marca la trayectoria como `completed`, `failed` o `abandoned`. |
| `diagnose` | `trajectory_id` | Audita cuellos de botella: nodos de alta latencia, consumo excesivo de tokens y ramas muertas. |
| `get_tree` | `trajectory_id` | Devuelve la topología jerárquica del árbol con cálculo de scores UCB1/UCT. |

### D. Cerebro Cognitivo y Supervisión (`ozy_brain`)
| Acción | Parámetros Clave | Descripción |
| :--- | :--- | :--- |
| `plan` | `goal`, `context` | Genera un plan estructurado en 5 fases con checklist de parada determinista. |
| `audit_changes_with_critic` | `diff`, `files`, `plan_steps` | Auditoría adversaria con detección de hotspots y guardias DDL/DML destructivas. |
| `get_repository_hotspots` | `limit` | Identifica archivos con mayor churn, commits de fix y riesgo de regresión con DuckDB. |
| `consolidate_memory` | `threshold` | Sintetiza tópicos y depura memorias mediante decaimiento temporal exponencial. |
| `build_mental_model` | `scope` | Proporciona un mapa mental del proyecto ("por dónde empezar a leer"). |

### E. Diagnósticos y Salud del Código
| Herramienta | Parámetros Clave | Descripción |
| :--- | :--- | :--- |
| `ozy_verify_diff` | `file_path`, `diff` | Sandbox de validación previa test-time: chequea sintaxis AST antes de persistir cambios. |
| `ozy_doctor` | `format`, `include_projects` | Diagnóstico integral: integridad de SQLite, modelos de embedding, watchers y registro. |
| `ozy_code_doctor` | `mode`, `scope`, `min_duplicate_lines` | Detección de duplicados, candidatos a refactor vs boilerplate estructural. |
| `ozy_skills` | `action`, `query`, `category` | Integración oficial con skills.sh para búsqueda y aplicación de guías contextuales. |
| `ozy_export_memory_notes` | `notes_ref` | Exporta lecciones y decisiones a Git Notes (`refs/notes/ozymem`) para sincronización P2P. |
| `ozy_import_memory_notes` | `notes_ref` | Importa y fusiona memorias desde Git Notes sin colisiones. |

---

## 3. Recursos MCP (`resources/list` y `resources/read`)

Ozygram expone URIs de recursos accesibles por los agentes:
- `ozymem://summary`: Estado global del proyecto, conteo de lecciones, nodos y archivos.
- `ozymem://recent-lessons`: Las últimas lecciones y decisiones registradas.
- `ozymem://file/{path}`: Contexto integral y reglas específicas asociadas a un archivo.
- `ozymem://file/{path}/neighbors`: Vecinos de grafo y dependencias del archivo.

Los clientes MCP pueden suscribirse a estos recursos (`resources/subscribe`) para recibir notificaciones automáticas cuando se registren nuevas lecciones o cambie el grafo del proyecto.
