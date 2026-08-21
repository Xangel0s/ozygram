# Referencia de Herramientas MCP

Ozygram expone un conjunto integral de herramientas accesibles para cualquier cliente MCP (Claude Desktop, Antigravity, Cursor, etc.).

---

## 🧠 1. Memoria Contextual y Engramas

| Herramienta | Parámetros Principales | Descripción |
| :--- | :--- | :--- |
| `ozy_memory` | `action`, `query`, `kind`, `limit` | Búsqueda y gestión de memorias (`lesson`, `decision`, `convention`, `gotcha`, `module_rule`). |
| `ozy_context` | `action`, `query`, `subpath`, `max_tokens` | Provee contexto enriquecido para tareas, con fallback inteligente a AST y filtrado por subpath. |
| `record_lesson` | `file_path`, `symbol_name`, `context`, `solution` | Registra una lección aprendida tras resolver un problema en un archivo o símbolo. |
| `record_decision` | `file_path`, `symbol_name`, `context`, `decision` | Documenta una decisión arquitectónica o de diseño. |
| `record_convention` | `file_path`, `symbol_name`, `context`, `convention` | Define una convención de código para un archivo o módulo. |
| `record_gotcha` | `file_path`, `symbol_name`, `context`, `gotcha` | Advierte sobre trampas conocidas o peculiaridades del entorno. |
| `record_module_rule` | `file_path`, `context`, `rule` | Establece una regla estricta para un módulo. |

---

## 🕸️ 2. Grafo de Dependencias e Impacto

| Herramienta | Parámetros Principales | Descripción |
| :--- | :--- | :--- |
| `ozy_graph` | `action` (`summary`, `neighbors`, `impact`, `path`), `subpath`, `depth` | Inspección del grafo de dependencias de archivos. Admite rutas relativas y subrutas monorepo. |
| `analyze_impact` | `file_path`, `depth` | Análisis de impacto de modificar un archivo, calculando dependientes transitivos y severidad. |
| `file_context` | `file_path` | Retorna el lenguaje, funciones indexadas, engramas históricos y vecinos en el grafo. |
| `ozymem_map_api_routes` | `file_path` | Mapea endpoints HTTP de FastAPI, Express o Axum con sus métodos y DTOs. |

---

## 🩺 3. Diagnóstico y Calidad de Código

| Herramienta | Parámetros Principales | Descripción |
| :--- | :--- | :--- |
| `ozy_doctor` | `include_projects`, `format` | Verifica el estado del sistema, bases de datos y advertencias de sintaxis AST registradas. |
| `ozy_code_doctor` | `min_duplicate_lines`, `max_findings`, `scope` | Detecta bloques de código duplicado separando `[High-Priority Refactor Candidates]` de `[Structural Boilerplate]`. |
| `detect_code_drift` | `changed_files`, `diff_content` | Compara un diff contra las convenciones registradas y alerta si hay desvíos. |
| `ozymem_python_typecheck` | `file_path`, `strict` | Ejecuta verificación de tipos y sintaxis Python usando Pyrefly o el compilador AST nativo. |

---

## 🧩 4. Cerebro Híbrido (`ozy_brain`)

| Acción (`action`) | Propósito |
| :--- | :--- |
| `plan` | Genera un plan de acción estructurado y seguro antes de realizar cambios en el código. |
| `reflect` | Analiza el impacto de los cambios completados y extrae lecciones aprendidas. |
| `risk_review` | Evalúa riesgos de regresión, acoplamiento y rotura de contratos de datos. |
| `summarize_project` | Sintetiza el propósito, arquitectura y estado del repositorio. |
| `recall_deep` | Recuperación profunda de decisiones históricas y contexto disperso. |
| `consolidate_engrams` | Sintetiza y compacta observaciones repetidas o fragmentadas. |

---

## 📦 5. Proyectos y Gestión de Paquetes

| Herramienta | Parámetros Principales | Descripción |
| :--- | :--- | :--- |
| `ozy_project` | `action` (`list`, `refresh`, `create_ignore`) | Administra proyectos registrados en el registry global. |
| `link_projects` | `source_project`, `target_project` | Establece enlaces lógicos entre proyectos en arquitecturas multi-repo. |
| `export_knowledge_bundle` | `project_name`, `output_path` | Exporta todo el conocimiento y grafo a un archivo empaquetado `.ozymem`. |
| `import_knowledge_bundle` | `file_path`, `target_project` | Importa conocimiento de un bundle a un proyecto local. |
| `rank_memories` | `min_confidence` | Audita y poda memorias obsoletas o de baja confianza. |
