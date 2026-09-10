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
| Herramienta | Parámetros | Descripción |
| :--- | :--- | :--- |
| `lookup_engram` | `symbol`, `file` | Búsqueda determinista $O(1)$ de firmas y contratos en memoria mapeada. |
| `record_lesson` | `category`, `lesson`, `context` | Registra una lección aprendida en el SQLite del proyecto. |
| `record_decision` | `title`, `decision`, `rationale` | Registra una decisión de diseño arquitectónico. |
| `record_gotcha` | `gotcha`, `workaround` | Registra un problema conocido o comportamiento no obvio con su solución. |
| `record_convention` | `rule`, `rationale` | Registra una regla de estilo o convención técnica del proyecto. |
| `search_lessons` | `query`, `category` | Búsqueda léxica FTS5 en la base de lecciones. |
| `deep_semantic_search` / `ozy_deep_search` | `query`, `top_k` | Búsqueda híbrida con RRF combinando FTS5 y embeddings FastEmbed. |

### B. Grafo de Código y Navegación AST
| Herramienta | Parámetros | Descripción |
| :--- | :--- | :--- |
| `file_context` / `ozy_context` | `file_path`, `task` | Entrega prefill predictivo, reglas de archivo, firmas adyacentes y lecciones. |
| `analyze_impact` | `target_file` | Mapea archivos dependientes directos e indirectos y clasifica el riesgo. |
| `graph_neighbors` | `file_path`, `direction` | Obtiene vecinos inmediatos (`incoming`, `outgoing`, `both`) en el grafo. |
| `graph_summary` | `project_path` | Resumen del grafo: nodos, aristas, tipos de relación y archivos clave. |

### C. Cerebro Cognitivo y Auditoría (`ozy_brain`)
| Acción | Parámetros Clave | Descripción |
| :--- | :--- | :--- |
| `plan` | `goal`, `context` | Genera un plan estructurado en 5 fases con checklist de parada. |
| `audit_changes_with_critic` | `diff`, `files`, `plan_steps` | Auditoría adversaria con detección de hotspots y guardias DDL/DML. |
| `get_repository_hotspots` | `limit` | Identifica archivos con mayor churn, commits de fix y riesgo de regresión. |
| `consolidate_memory` | `threshold` | Sintetiza tópicos y depura memorias con decaimiento temporal. |
| `build_mental_model` | `scope` | Proporciona un mapa mental del proyecto ("por dónde empezar a leer"). |

### D. Diagnósticos y Salud del Código
| Herramienta | Descripción |
| :--- | :--- |
| `ozy_verify_diff` | Sandbox de validación previa: chequea sintaxis AST antes de persistir cambios. |
| `ozy_doctor` | Diagnóstico general de salud del proyecto, integridad de SQLite y dependencias. |
| `ozy_code_doctor` | Detección de duplicados, boilerplate vs refactorización y errores AST. |

---

## 3. Recursos MCP (`resources/list` y `resources/read`)

Ozygram expone URIs de recursos accesibles por los agentes:
- `ozymem://summary`: Estado global del proyecto, conteo de lecciones, nodos y archivos.
- `ozymem://recent-lessons`: Las últimas lecciones y decisiones registradas.
- `ozymem://file/{path}`: Contexto integral y reglas específicas asociadas a un archivo.
- `ozymem://file/{path}/neighbors`: Vecinos de grafo y dependencias del archivo.

Los clientes MCP pueden suscribirse a estos recursos (`resources/subscribe`) para recibir notificaciones automáticas cuando se registren nuevas lecciones o cambie el grafo del proyecto.
