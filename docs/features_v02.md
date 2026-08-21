# Novedades y Mejoras de Ozygram v0.2.0

Esta versión introduce 5 mejoras clave diseñadas a partir de la experiencia real con proyectos monorepo complejos (ej. FastAPI + Next.js):

---

## 1. Resolución de Rutas en Cascada (`resolve_target_path`)

### Problema Anterior
Cuando un cliente MCP o agente enviaba una ruta relativa como `crm-geofal/src/main.py` o un nombre de archivo simple `router.py`, las consultas al grafo o lecciones no coincidían con la clave canónica indexada (ej. `C:\Users\...\crm-geofal\src\main.py`), devolviendo resultados vacíos.

### Solución Implementada
Se implementó un resolvedor en cascada de 5 etapas:
1. **Coincidencia Exacta**: Busca la clave directa en el índice en memoria.
2. **Coincidencia en SQLite**: Consulta `files` por ruta normalizada.
3. **Unión Relativa**: Concatena la ruta de entrada con el `project_path` o `workspace_root`.
4. **Coincidencia de Sufijo en Memoria**: Verifica si alguna clave indexada termina con el sufijo provisto.
5. **Fallback SQLite LIKE**: Busca coincidencias parciales de sufijo en la base de datos.

---

## 2. Fallback Automático a Símbolos AST en Búsquedas de Contexto

### Problema Anterior
Al consultar `context_for_task` o `ozy_context` para una tarea nueva sin lecciones previas, la respuesta simplemente indicaba `(none)` o `No matching memories found`, sin aportar información del código.

### Solución Implementada
Si `lessons.is_empty()`, el servidor consulta automáticamente los símbolos AST indexados (`backend.search_ast_symbols(query, 10)`), devolviendo:
- Nombres de funciones/clases coincidentes con su rango de líneas (`L10-L25`).
- Archivos fuente asociados con su contexto completo.
- Dependientes transitivos e impacto en el grafo.

---

## 3. Clasificación Inteligente de Duplicados en Code Doctor

### Problema Anterior
`ozy_code_doctor` agrupaba todas las coincidencias idénticas por igual, mezclando definiciones normales de DTOs/esquemas con duplicación real de lógica de negocio.

### Solución Implementada
Se introdujo `classify_duplicate_block`, que analiza el contenido del bloque:
- `[High-Priority Refactor Candidates]`: Bloques con lógica procedimental y flujo de control (`if`, `for`, `while`, `def`, `fn`, `return`).
- `[Structural Boilerplate]`: Modelos Pydantic (`BaseModel`), interfaces, clases de datos y DTOs repetidos por diseño.

---

## 4. Diagnósticos Estáticos de Sintaxis AST (Linter Integrado)

### Problema Anterior
Si un archivo contenía errores de sintaxis al momento del escaneo, el parser fallaba silenciosamente o usaba heurísticas sin advertir al usuario.

### Solución Implementada
- `extract_ast_diagnostics` recorre el árbol de Tree-Sitter en busca de nodos `ERROR` y `MISSING`.
- Los hallazgos se guardan en la tabla SQLite `ast_diagnostics`.
- `ozy_doctor` reporta el estado como `[ok]` si no hay errores o `[warning]` con el detalle de archivos afectados.

---

## 5. Soporte de Subdirectorios (`subpath`) para Monorepos

### Problema Anterior
En repositorios con backend y frontend juntos (ej. monorepos), `ozy_graph action=summary` solo permitía ver las estadísticas de todo el proyecto acumulado.

### Solución Implementada
- Se añadió `get_subpath_graph_summary(&self, subpath: &str)`.
- `ozy_graph` y `ozy_context` ahora aceptan el parámetro `subpath` (ej. `crm-backend/` o `crates/ozymem-parser/`), filtrando archivos, funciones, aristas y lecciones del sub-alcance solicitado.
