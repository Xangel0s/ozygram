# Sistema de Engrams Deterministas, Prefill Especulativo y Validación Test-Time

Este documento detalla la arquitectura de **Engrams**, **Decodificación Especulativa de Contexto**, **Sandbox Test-Time** y **Sincronización P2P con Git Notes** en Ozygram.

---

## 1. Almacenamiento Determinista $O(1)$ con `rkyv` y `memmap2`

### Motivación
Para repositorios de gran escala o tareas de edición continua por agentes LLM, consultar firmas y contratos de símbolos a través de SQLite o parsers de texto introduce latencia acumulativa.

### Implementación
- **`IncrementalEngramStore` / `FastEngramReader`**: Motor de almacenamiento binario sin copia (*zero-copy*) basado en `rkyv` mapeado en memoria vía `memmap2`.
- **Estructura de Contrato (`EngramContract`)**:
  - `symbol_path`: Ruta canónica calificada (`crate::module::func`).
  - `signature_hash`: Hash BLAKE3/SHA-256 de la firma para invalidación rápida.
  - `input_types` y `return_type`: Tipos de entrada y retorno normalizados.
  - `doc_summary`: Resumen semántico del contrato.
  - `outgoing_calls` / `type_dependencies`: Enlaces directos a otros tipos dependientes.
- **Rendimiento**: Búsqueda por símbolo en $\approx 15\text{ ns}$ con consumo de memoria constante $O(1)$ sin allocations en el heap.

---

## 2. Prefill Predictivo y Speculative Decoding

### Motivación
Los agentes LLM tradicionalmente requieren múltiples *roundtrips* sucesivos (`get_file_context` $\rightarrow$ `find_symbol` $\rightarrow$ `lookup_engram`) para descubrir tipos en archivos importados.

### Implementación
- Al consultar `get_file_context` o `context_for_task`, `ozymem-core` utiliza el grafo de dependencias de `petgraph` para identificar los archivos vecinos de primer orden y los commits recientes en `GitBackend`.
- El servidor MCP inyecta un encabezado determinista `[ENGRAM_CACHE: Deterministic Symbol Contracts]` con los contratos adyacentes de mayor probabilidad de edición antes del cuerpo del archivo.
- **Beneficio**: Reduce los *roundtrips* del agente a cero y maximiza la tasa de acierto del **Prompt Cache** (>90%) en modelos con caching por prefijo (Claude 3.7 / GPT-4o / DeepSeek V3).

---

## 3. Sandbox de Validación Test-Time en Bucle Cerrado (`ozy_verify_diff`)

### Flujo de Verificación
```
┌─────────────────────────────────────────────────────────────┐
│                    Agente MCP / Tool Call                   │
│                     `ozy_verify_diff`                       │
└──────────────────────────────┬──────────────────────────────┘
                               │
                               ▼
┌─────────────────────────────────────────────────────────────┐
│                 Sandbox Fast-Path (Rust)                    │
│  - Chequeo de sintaxis Tree-Sitter                          │
│  - Verificación de contratos Engram                         │
│  - Verificación AST sin tocar el disco principal             │
└──────────────────────────────┬──────────────────────────────┘
                               │ (Si hay advertencias o cambios)
                               ▼
┌─────────────────────────────────────────────────────────────┐
│                 Reflexión Heurística (Python)               │
│  - `reflector.py` destila causa raíz                        │
│  - Generación de Reglas Procedimentales [TRIGGER]->[ACTION] │
└─────────────────────────────────────────────────────────────┘
```

- **Herramienta `ozy_verify_diff`**: Permite al agente enviar un parche o diff propuesto antes de guardarlo en disco.
- **Detección Rápida**: Identifica rupturas de contratos, incompatibilidad de tipos y errores de sintaxis en $<200\text{ ms}$.

---

## 4. Sincronización Distribuida P2P con Git Notes (`refs/notes/ozymem`)

### Motivación
Permitir que múltiples agentes o ramas compartan lecciones aprendidas y reglas procedimentales sin requerir bases de datos remotas dedicadas.

### Operación
- **Exportación (`ozy_export_memory_notes`)**: Empaqueta lecciones episódicas, contratos engram y reglas procedimentales en un payload JSON y lo escribe en `refs/notes/ozymem` sobre el commit actual de Git.
- **Importación (`ozy_import_memory_notes`)**: Lee la nota del commit actual o especificado, deduplica contra el SQLite local e inserta nuevas lecciones automáticamente.
- **Portabilidad**: Sincronizable con `git push origin refs/notes/ozymem` y `git fetch origin refs/notes/ozymem:refs/notes/ozymem`.
