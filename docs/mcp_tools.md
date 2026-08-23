# Referencia de Herramientas MCP en Ozygram

Ozygram expone una suite completa de más de 30 herramientas a través del Model Context Protocol (MCP).

---

## 1. Herramientas de Memoria y Engrams

- **`lookup_engram` / `ozy_lookup_engram`**: Consulta un contrato determinista de símbolo $O(1)$ por nombre o ruta canónica (`file_path::symbol_name`).
- **`record_lesson`**: Registra una lección aprendida ante un error o solución técnica.
- **`record_decision`**: Registra una decisión de arquitectura o diseño.
- **`record_convention`**: Registra una convención de estilo o regla de código.
- **`record_gotcha`**: Registra una trampa técnica o caso borde detectado.
- **`record_module_rule`**: Registra una regla estricta aplicable a un módulo.
- **`search_lessons`**: Búsqueda textual y BM25 ponderada sobre lecciones y memorias.
- **`similar_lessons`**: Búsqueda de lecciones similares mediante embeddings semánticos (`FastEmbed`).

---

## 2. Herramientas de Grafo y Contexto de Código

- **`file_context`**: Retorna el contexto AST y prefill determinista de engrams para un archivo dado.
- **`context_for_task`**: Agrupa lecciones relevantes, símbolos AST y archivos críticos para una tarea de desarrollo.
- **`analyze_impact`**: Realiza un análisis de impacto transitivo a $N$ niveles de profundidad en el grafo.
- **`graph_summary`**: Retorna estadísticas de archivos, funciones, aristas y uso de memoria.
- **`graph_neighbors`**: Lista dependencias directas entrantes y salientes de un archivo.
- **`list_files`**: Lista todos los archivos indexados en el grafo del proyecto.

---

## 3. Sandbox de Validación Test-Time y Diagnósticos

- **`ozy_verify_diff`**: Evalúa un parche o diff propuesto en un sandbox antes de aplicarlo a disco, detectando errores de sintaxis y discrepancias de contratos.
- **`ozy_doctor`**: Diagnostica la salud del backend, integridad de la base de datos, estado del watcher y errores de sintaxis AST detectados.
- **`ozy_code_doctor`**: Detecta bloques de código duplicados clasificando candidatos a refactorización vs boilerplate estructural.
- **`detect_code_drift`**: Audita modificaciones contra convenciones registradas.

---

## 4. Herramientas Git y Colaboración P2P

- **`ozy_export_memory_notes`**: Exporta las memorias, contratos y reglas procedimentales hacia `refs/notes/ozymem` en el commit actual.
- **`ozy_import_memory_notes`**: Importa y fusiona notas de memoria desde `refs/notes/ozymem` de forma descentralizada.
- **`learn_from_changes`**: Analiza el diff de git y extrae automáticamente lecciones de código.
- **`git_recent_changes`**: Lista los archivos modificados recientemente en git.
- **`git_diff_summary`**: Resume los cambios del repositorio.

---

## 5. Herramientas Unificadas `ozy_*` y Razonamiento

- **`ozy_context`**: Punto de entrada unificado para búsqueda semántica, contratos, dependientes e historial git.
- **`ozy_graph`**: Acciones compuestas sobre el grafo (`summary`, `impact`, `neighbors`, `trace`, `path`).
- **`ozy_brain`**: Motor de razonamiento cognitivo con 11 acciones (`plan`, `reflect`, `recall_deep`, `summarize_project`, `detect_patterns`, `suggest_next_steps`, `analyze_failure`, `compress_session`, `rank_memories`, `build_mental_model`, `risk_review`).
- **`ozy_skills`**: Lista metadatos de habilidades oficiales de desarrollo integradas.
