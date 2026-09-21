# Historial de Versiones y Novedades (Changelog)

Este documento recopila de forma cronológica la evolución y mejoras clave de **Ozygram**.

---

## 🌙 Versión v1.1.0 — Dream-RSI: Zero-Friction MCTS Engine & Auto-Parenting

### 1. Auto-Parenting Inteligente en Rust Core
- Eliminación de la necesidad de que el agente memorice y propague IDs hash padre (`parent_id`) en tareas secuenciales.
- Cuando `parent_id` es omitido, el motor en Rust localiza atómicamente el último nodo hoja activo (`ORDER BY depth DESC, created_at DESC LIMIT 1`) y calcula `depth = parent.depth + 1`.

### 2. Inserción Atómica por Lotes (`record_batch`)
- Nueva acción MCP `record_batch` para registrar múltiples pasos técnicos en una única llamada JSON-RPC.
- Reduce radicalmente la latencia percibida por el usuario (*turn tax*) en tareas lineales mientras mantiene la topología MCTS intacta.

### 3. Auditoría Objetiva de Recompensas (Anti-Alucinación)
- El analizador determinista `detect_failure_signals` audita observaciones en busca de códigos de error de proceso (`exit code != 0`), errores de sintaxis (`SyntaxError`, `TypeError`) o fallos de tests (`FAILED`, `timed out`).
- Clampea automáticamente recompensas positivas alucinadas a `-1.0` con la bandera `objective_reward_override: true`.

### 4. Diagnóstico Nativo de Trayectorias (`diagnose` y `ozymem dream diagnose`)
- Detección de cuellos de botella de latencia (> 3000 ms) y consumo de tokens (> 2000 tokens).
- Detección de oportunidades de poda (*pruning*) no ejecutadas y sugerencia matemática de constante de exploración UCB1 ($c$).
- Métricas de eficiencia de tokens en la rama de la solución final.

### 5. Resiliencia de Instalación en Windows
- Script `install.ps1` con reemplazo seguro de binarios mediante staging temporal (`.old`), evitando bloqueos de archivos en uso (`Resource busy / File in use`).

---

## ⚡ Versión v0.4.0 — Dual-Tier Engine, SQLite Outbox & FastEmbed RRF

### 1. Arquitectura Dual-Tier Asimétrica
- **Carril Rápido (Rust)**: Servidor MCP de ultra-baja latencia (<5 ms), almacenamiento transaccional en SQLite y tabla de engrams $O(1)$ con `rkyv` y `memmap2`.
- **Carril de Potencia (Python)**: Motor cognitivo asíncrono con `SupervisorAgent`, `RiskCriticAgent`, `DataEngine` (DuckDB + Polars) y `OutboxConsumer`.

### 2. Patrón Transaccional Outbox (Cero Vectores Fantasma)
- Triggers automáticos en SQLite para inserción, actualización y borrado de lecciones y observaciones.
- Drenado en segundo plano (`OutboxConsumer`) hacia ChromaDB.

### 3. Búsqueda Semántica Híbrida con RRF
- Nueva tool MCP: `deep_semantic_search` / `ozy_deep_search`.
- Fusión de búsqueda léxica FTS5 (BM25) y embeddings densos locales (`FastEmbed` con `BAAI/bge-m3` o `bge-base`) mediante el algoritmo matemático **Reciprocal Rank Fusion (RRF)**. Inferencia vectorial 100% local con ONNX Runtime en C++.

### 4. Resiliencia y Fallback Determinista
- **Auto-Spawn de Demonios**: `ensure_ozy_brain_running` inicia automáticamente el worker de Python si no se encuentra activo.
- **Circuit Breaker**: Si Python falla o excede el timeout (3s), conmuta inmediatamente al fallback determinista de SQLite sin interrumpir al agente.

### 5. Integración con el Dream Team
- Integración nativa con **`tgrep`** (Microsoft) para búsquedas trigram ultra-rápidas.
- Integración con **`rtk`** (Rust Token Killer) para compresión de payloads y ahorro de contexto.

---

## 🤖 Versión v0.3.0 — Cognición Multi-Agente & Telemetría Analítica

### 1. Motor Analítico de Telemetría (`DataEngine` DuckDB + Polars)
- Ingestión del historial de Git para calcular frecuencia de cambios, volumen de líneas modificadas (*churn*), número de autores y densidad de fallos/fixes.
- Persistencia analítica embebida en `.ozymem/analytics.duckdb` para consultas OLAP instantáneas.

### 2. Sistema Multi-Agente Cognitivo (`ozy-brain`)
- **Supervisor Orquestador (`SupervisorAgent`)**: Despacho jerárquico y estructurado con `pydantic-ai`.
- **Agente Crítico Adversarial (`RiskCriticAgent`)**: Simulación de vectores de regresión, veto de operaciones destructivas (`DROP TABLE`, `DELETE FROM`, blast radius > 8 archivos) y análisis cruzado con hotspots.
- **Sintetizador y Decaimiento de Memoria (`MemoryConsolidationAgent`)**: Agrupación de engrams redundantes y cálculo de decaimiento temporal exponencial ($S = C \cdot e^{-\lambda \Delta t}$).

### 3. Capa de Modelos Universal & OpenRouter (`config.py`)
- Detección automática en cascada de `OPENROUTER_API_KEY`, `OLLAMA_HOST`, `GEMINI_API_KEY`, `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`.
- Cadena de tolerancia a fallos con modelos gratuitos (`gemini-2.0-flash`, `nemotron-reasoning:free`, `qwen2.5-coder`).
- **Fallback Heurístico Local Offline ($0 Costo)**: Cero fallos en ausencia de red o claves API.

---

## 🚀 Versión v0.2.0 — Resolución en Cascada, AST y Robustez

### 1. Resolución de Rutas en Cascada (`resolve_target_path`)
- Resolvedor de 5 etapas para rutas relativas, normalizadas y coincidencia de sufijos, eliminando búsquedas vacías en monorepos complejos.

### 2. Fallback a Símbolos AST
- Búsqueda automática en AST cuando `context_for_task` o `ozy_context` no encuentra lecciones explícitas, devolviendo contratos y dependientes inmediatos.

### 3. Clasificación Inteligente de Duplicados
- `ozy_code_doctor` categoriza `[High-Priority Refactor Candidates]` (lógica de negocio repetida) vs `[Structural Boilerplate]` (modelos de datos y DTOs).

### 4. Diagnósticos AST / Linter Integrado
- Detección estática de errores y advertencias de sintaxis con Tree-Sitter reportados en `ozy_doctor`.

### 5. Soporte de Subdirectorios (`subpath`)
- Filtrado granular por subrutas para navegación eficiente en monorepos.
