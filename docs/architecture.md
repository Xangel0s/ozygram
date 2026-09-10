# Arquitectura Dual-Tier de Ozygram

Ozygram implementa una **Arquitectura Dual-Tier Asimétrica** diseñada para maximizar la velocidad de respuesta del asistente de código, eliminar la latencia de arranque y asegurar que las operaciones críticas del IDE jamás se bloqueen por cálculos cognitivos pesados.

---

## 1. Visión General: Los Dos Carriles

```text
       ┌────────────────────────────────────────────────────────┐
       │             Cliente MCP (IDE / Asistente LLM)          │
       └──────────────────────────┬─────────────────────────────┘
                                  │ JSON-RPC (Stdio)
                                  ▼
 ┌────────────────────────────────────────────────────────────────────────┐
 │                   CARRIL RÁPIDO (Fast Lane — Rust)                     │
 │                                                                        │
 │  • Servidor MCP de ultra-baja latencia (< 5ms)                         │
 │  • Almacenamiento persistente ACID en SQLite local                     │
 │  • Grafo de dependencias con Petgraph y AST nativo (Tree-Sitter)       │
 │  • Tabla Determinista de Engrams O(1) con rkyv + memmap2               │
 │  • Patrón Transaccional Outbox (SQLite Triggers)                       │
 │  • Circuit Breaker y Auto-Spawn de demonios auxiliares                 │
 └───────────────────┬────────────────────────────────┬───────────────────┘
                     │ Triggers                       │ Fallback / RPC
                     ▼                                ▼
       ┌──────────────────────────┐     ┌─────────────────────────────────┐
       │   memory_outbox Table    │     │  CARRIL DE POTENCIA (Python)    │
       │   (Transaccional SQLite) │     │                                 │
       └─────────────┬────────────┘     │  • SupervisorAgent              │
                     │ Consumo          │  • RiskCriticAgent              │
                     ▼ Asíncrono        │  • DataEngine (DuckDB + Polars) │
       ┌──────────────────────────┐     │  • OutboxConsumer & FastEmbed   │
       │   Motor Vectorial ONNX   │◄────┤  • ChromaDB Local               │
       │   (bge-m3 / FastEmbed)   │     │  • Decaimiento Temporal Exp.    │
       └──────────────────────────┘     └─────────────────────────────────┘
```

---

## 2. Estructura del Monorepo

```text
ozygram/
├── crates/
│   ├── ozymem-core/       # Motor persistente (SQLite), Outbox triggers, grafo Petgraph, Engram store
│   ├── ozymem-parser/     # Parsers AST Tree-Sitter nativos multi-lenguaje (Rust, Python, TS/JS, Go, SQL)
│   ├── ozymem-cli/        # CLI standalone con subcomandos (scan, dashboard, projects, etc.)
│   └── ozymem-server/     # Servidor MCP stdio modular de alta concurrencia (<5ms)
├── python/
│   └── ozy-brain/         # Motor cognitivo: Supervisor, RiskCritic, DataEngine (DuckDB), OutboxConsumer
└── docs/                  # Documentación técnica modular por secciones
```

---

## 3. El Carril Rápido (Fast Lane — Rust)

El carril rápido está compuesto por los crates `ozymem-core`, `ozymem-parser`, `ozymem-server` y `ozymem-cli`:

1. **Latencia Sub-Milisegundo**:
   - Responde inmediatamente a las solicitudes de contexto del IDE.
   - Las lecturas de firmas, dependencias y reglas de archivo se resuelven en $\approx 15\text{ ns}$ mediante memoria mapeada (`memmap2` y `rkyv`).
2. **Autoridad Transaccional Única**:
   - SQLite (`{project}/.ozymem/memory.db`) es la **única fuente de verdad** (*Single Source of Truth*).
   - Ninguna escritura depende de que un servicio externo o proceso de Python esté activo. Si Python no responde o no está corriendo, la persistencia en Rust continúa sin degradarse.
3. **Indexación Sintáctica en Caliente**:
   - Parser multi-lenguaje (Rust, TypeScript, JavaScript, Python, Go, SQL) que extrae funciones, structs, clases y rutas HTTP sin necesidad de compiladores externos.

---

## 4. Patrón Transaccional Outbox (`memory_outbox`)

Para evitar "vectores fantasma" y desfases entre la base relacional y la base de datos vectorial, Ozygram implementa el patrón **Transactional Outbox** gobernado por triggers nativos en SQLite:

### Esquema de la Tabla
```sql
CREATE TABLE IF NOT EXISTS memory_outbox (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    event_type TEXT NOT NULL,       -- 'UPSERT' o 'DELETE'
    entity_type TEXT NOT NULL,      -- 'lesson' u 'observation'
    entity_id INTEGER NOT NULL,     -- ID en la tabla original
    payload TEXT NOT NULL,          -- Contenido estructurado en JSON
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    processed_at TIMESTAMP NULL     -- NULL = pendiente de vectorización
);
```

### Triggers Automáticos
- **Inserción y Actualización**: Cada vez que se crea o actualiza una lección u observación en SQLite, un trigger automático inserta un evento `UPSERT` en `memory_outbox`.
- **Borrado / Soft-Delete**: Cuando se elimina un registro de SQLite, se genera un evento `DELETE` en la outbox.

### Ventajas del Patrón Outbox
- **Atomicidad Total**: Si una transacción de base de datos hace `ROLLBACK`, el evento de vectorización jamás llega a la outbox.
- **Cero Vectores Huérfanos**: La base vectorial en ChromaDB nunca retiene recuerdos que hayan sido eliminados en la base principal.
- **Desacoplamiento Temporal**: Rust escribe a disco en 1 ms; Python puede procesar los embeddings en segundo plano en lotes sin ralentizar al usuario.

---

## 4. El Carril de Potencia (Power Lane — Python `ozy-brain`)

El motor auxiliar en Python se encarga de las tareas analíticas complejas que se benefician del ecosistema de Data Science:

1. **`OutboxConsumer`**:
   - Hilo demonio en segundo plano (intervalo de 10s) que vacía eventos pendientes de la tabla `memory_outbox`.
   - Genera embeddings densos locales mediante `FastEmbed` y actualiza las colecciones en `ChromaDB`.
2. **`SupervisorAgent` & `RiskCriticAgent`**:
   - Orquesta la auditoría adversaria de planes de trabajo y cambios de código.
   - En modo sin conexión, utiliza heurísticas deterministas; con LLM configurado, evalúa vectores de regresión sutiles.
3. **`DataEngine` (DuckDB + Polars)**:
   - Analiza el historial de commits y cambios en Git para computar puntuaciones de *Churn*, detectar archivos propensos a errores (*hotspots*) y correlacionar riesgos.
4. **`MemoryConsolidationAgent`**:
   - Aplica fórmulas matemáticas de decaimiento exponencial ($S = C \cdot e^{-\lambda \Delta t}$) para podar lecciones obsoletas sin quemar tokens.

---

## 5. Resiliencia: Daemon Auto-Spawn & Circuit Breaker

La comunicación entre el servidor Rust y el motor Python incluye mecanismos de auto-sanación:

### Auto-Spawn de Daemon (`ensure_ozy_brain_running`)
Si el servidor Rust recibe una petición semántica profunda (`deep_semantic_search`) o cognitiva (`ozy_brain`) y el demonio Python no está activo:
1. Detecta automáticamente el entorno virtual local o global de Python.
2. Inicia el proceso `ozy-brain` en segundo plano desacoplado (`DETACHED_PROCESS` en Windows).
3. Espera el *health check* en el puerto RPC antes de dirigir la petición.

### Circuit Breaker y Fallback Determinista
Si el proceso de Python no responde, arroja un error o se agota el tiempo de espera (timeout de 3s):
- El **Circuit Breaker** entra en acción inmediatamente.
- Genera un resultado de fallback determinista basado en **SQLite FTS5 + BM25 local**.
- El agente MCP recibe una respuesta válida sin fallos catastróficos ni pantallas de error.
