# Plan Maestro de Desarrollo: Ozygram Dual-Tier Engine (v0.4.0)

## 1. Visión y Arquitectura Dual-Tier

Ozygram se estructura en **Dos Carriles Cognitivos** complementarios:

```
┌─────────────────────────────────────────────────────────────┐
│  IDE / Agente LLM (Claude, Gemini, Cursor)                  │
└──────────────┬──────────────────────────────▲───────────────┘
               │ JSON-RPC (<100 ms)           │ Respuesta Inmediata
┌──────────────▼──────────────────────────────┴───────────────┐
│ CARRIL 1: Fast Lane (Rust Core - Ligero y Concurrente)      │
│ • Handshake MCP instantáneo (CWD, proyectos, herramientas)  │
│ • Hot cache en RAM + SQLite WAL transaccional               │
│ • Filtro sanitario: bloquea archivos >256KB, .venv, etc.    │
│ • Encolado asíncrono hacia el Worker de Python              │
└──────────────┬──────────────────────────────▲───────────────┘
               │ IPC / Local Socket           │ Lecciones Consolidadas
┌──────────────▼──────────────────────────────┴───────────────┐
│ CARRIL 2: Power Lane (Python Heavy Daemon - Potencia Bruta) │
│ • ChromaDB: Vectores densos de alta fidelidad               │
│ • AI Noise Gate: Crítico que destruye logs y datos basura   │
│ • Cross-Encoder Re-Ranker: Relevancia semántica > 95%       │
│ • Multi-Agent Supervisor: Pydantic-AI + DuckDB OLAP         │
└─────────────────────────────────────────────────────────────┘
```

---

## 2. Diagnóstico de Causas Raíz Resueltas

1. **Corrupción del índice de código (`files`, `functions`, `file_dependencies`):**
   - Causa: `full_scan()` en `indexing.rs` no invocaba `check_noise_or_huge_file`. Archivos gigantescos (`.bundle.js`, `.sql` de 10MB) y carpetas `.venv`, `scratch/`, `.supabase/` eran leídos e insertados en SQLite, bloqueando transacciones.
   - Estado: Las memorias sobrevivieron porque residen en tablas aisladas (`observations`, `lessons`).
2. **Ruido en captura pasiva:**
   - Causa: `passive_capture` en `lessons.rs` admitía cualquier línea con longitud >= 8 caracteres bajo `## Key Learnings`, absorbiendo volcados crudos de terminal y tablas markdown.
3. **Latencia en llamadas MCP:**
   - Causa: Procesos síncronos pesados bloqueaban el canal JSON-RPC en el hilo principal.

---

## 3. Hoja de Ruta Detallada por Fases (4 Fases × 4 Tareas)

### Fase 1: Blindaje Sanitario y Resiliencia en Rust Core (Fast Lane)
- [x] **Task 1.1:** Conectar `check_noise_or_huge_file` dentro de `indexing.rs::full_scan()` y `reload_if_stale()`, descartando archivos > 256 KB.
- [x] **Task 1.2:** Ampliar `is_noise_dir()` en `helpers.rs` con `.venv`, `venv`, `env`, `.tox`, `scratch`, `.supabase`, `.turbo`, `coverage`, `.output`, `target`.
- [x] **Task 1.3:** Configurar PRAGMAs de SQLite en `schema.rs`: `PRAGMA busy_timeout = 5000;`, `PRAGMA synchronous = NORMAL;`, `PRAGMA journal_mode = WAL;`.
- [x] **Task 1.4:** Sanitizar `passive_capture()` en `lessons.rs` para rechazar volcados de terminal, tablas markdown y salidas de compilación crudas.

### Fase 2: Power Engine en Python (ChromaDB + Vector Store + Re-ranking)
- [x] **Task 2.1:** Configurar cliente persistente de **ChromaDB** en `python/ozy-brain` (almacenamiento en `.ozymem/chroma`).
- [x] **Task 2.2:** Implementar pipeline de embeddings densos de alta dimensionalidad (`sentence-transformers` con aceleración por hardware CUDA/DirectML).
- [x] **Task 2.3:** Integrar el modelo **Neural Cross-Encoder Re-ranker** (`bge-reranker`) para filtrar falsos positivos antes de entregar respuestas al LLM.
- [x] **Task 2.4:** Crear acción unificada `deep_semantic_search` que combine FTS5 léxico de SQLite con búsqueda vectorial densa y re-ranking.

### Fase 3: AI Noise Gate y Crítico Antiruido (Blindaje de Datos)
- [x] **Task 3.1:** Implementar el **AI Noise Gate** en `python/ozy-brain/agents/memory_agent.py` para clasificar y evaluar la calidad semántica de cada memoria antes de indexar.
- [x] **Task 3.2:** Desviar volcados crudos de terminal, trazas de stack trace y logs a `analytics.duckdb` como telemetría, impidiendo la contaminación del índice vectorial.
- [x] **Task 3.3:** Crear rutina de **Clustering y Consolidación de Memorias** para sintetizar múltiples observaciones en 1 regla canónica maestra.
- [x] **Task 3.4:** Implementar factor de decaimiento temporal (*Memory Decay*) para rebajar la relevancia de lecciones obsoletas con más de 90 días sin confirmación.

### Fase 4: Orquestación Multi-Agente y Validación E2E
- [ ] **Task 4.1:** Conectar el `SupervisorAgent` en `brain.py` para coordinar el `RiskCritic`, el `MemoryConsolidationAgent` y el motor `DataEngine` (DuckDB + Polars).
- [ ] **Task 4.2:** Formatear salidas para el agente LLM con resúmenes ejecutivos "Token-Budget Aware" (< 1 KB) para no saturar la ventana de contexto.
- [ ] **Task 4.3:** Incorporar soporte para manifiesto `.ozy.toml` en repositorios para configuración instantánea sin fricción.
- [ ] **Task 4.4:** Ejecutar suites de pruebas cruzadas (`cargo test --all`, `python -m unittest discover`) y validar latencias (< 100 ms en Fast Lane y < 2.5 s en Power Lane).

---

## 4. Métricas de Éxito
1. **Latencia Fast Lane:** Respuestas a `mem_current_project` y `mem_context` en menos de 100 ms.
2. **Cero Polución:** 0 archivos mayores a 256 KB o de carpetas virtuales (`.venv`, `scratch`) ingeridos en `memory.db`.
3. **Relevancia Semántica:** Precisión de Re-ranking > 90% en pruebas de búsqueda con preguntas ambiguas.
4. **Integridad de Base de Datos:** 100% de transacciones atómicas libres de bloqueos `database is locked`.
