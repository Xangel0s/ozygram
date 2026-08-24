# Ozygram v0.3.0: Cognición Multi-Agente, OpenRouter & Telemetría Analítica

La versión **v0.3.0** marca la transición de Ozygram de un indexador MCP pasivo a un **sistema cognitivo activo, analítico y proactivo**, incorporando razonamiento multi-agente (`Supervisor` y `Adversarial Critic`), analítica OLAP embebida con `DuckDB` y `Polars`, y soporte universal de modelos mediante `OpenRouter`.

---

## 🏗️ 1. Motor Analítico de Telemetría (`DataEngine` DuckDB + Polars)

- **Cálculo de Git Churn**: Ingesta el historial de commits y diffs numstat de Git en memoria con `Polars` para determinar:
  - Frecuencia de modificación por archivo (`commit_count`).
  - Volumen de líneas insertadas/borradas (`churn_score`).
  - Concurrencia de desarrolladores (`authors_count`).
  - Densidad de corrección de bugs (`fix_commits`).
- **Persistencia OLAP Embebida**: Almacena los agregados en `.ozymem/analytics.duckdb`.
- **Fórmula de Hotspot Scoring**:
  $$\text{HotspotScore} = \text{ChurnScore} \times \text{CommitCount} \times (1.0 + 0.5 \times \text{FixCommits})$$
- **Clasificación de Riesgo**: `CRITICAL`, `HIGH`, `MEDIUM`, `LOW`.

---

## 🤖 2. Sistema Multi-Agente (Supervisor & Crítico Adversarial)

- **`SupervisorAgent` (`ozy_brain/agents/supervisor.py`)**: Orquestador jerárquico para enrutar tareas analíticas y de auditoría.
- **`RiskCriticAgent` (`ozy_brain/agents/risk_critic.py`)**:
  - Simula vectores de regresión antes de aplicar cambios en código.
  - Cruza archivos involucrados con la telemetría de hotspots en DuckDB.
  - Veta cambios destructivos en esquemas (ej. `DROP TABLE`, `DELETE FROM`, `ALTER TABLE DROP COLUMN`).
- **`MemoryConsolidationAgent` (`ozy_brain/agents/memory_agent.py`)**:
  - Sintetiza lecciones redundantes por clusters temáticos.
  - Aplica factor de decaimiento temporal exponencial:
    $$S = C \cdot e^{-\lambda \cdot \Delta t}$$
  - Detecta y marca memorias obsoletas como candidatas a depuración.

---

## 🌐 3. Capa de Modelos Universal & OpenRouter (`config.py`)

- **Autodetección de Proveedores**:
  1. `OPENROUTER_API_KEY`: Soporte nativo para OpenRouter (gratuitos y comerciales).
  2. `OLLAMA_HOST` / `OLLAMA_BASE_URL`: Modelos locales (`qwen2.5-coder`, `deepseek-r1:8b`).
  3. `GEMINI_API_KEY` / `OPENAI_API_KEY` / `ANTHROPIC_API_KEY`.
  4. **Fallback Offline Heurístico ($0 Costo)**: Motor estático local para garantizar cero fallos sin conexión.
- **Cadena de Modelos Gratuitos Activa en OpenRouter**:
  - `openrouter/nvidia/nemotron-3-nano-omni-30b-a3b-reasoning:free`
  - `openrouter/nvidia/nemotron-3-super-120b-a12b:free`
  - `openrouter/cohere/north-mini-code:free`
  - `openrouter/z-ai/glm-5.2:free`
  - `openrouter/google/gemma-4-31b-it:free`

---

## 🔌 4. Nuevas Acciones Expuestas en `ozy_brain`

| Acción | Descripción |
| :--- | :--- |
| `audit_changes_with_critic` | Ejecuta auditoría adversarial simulando regresiones y cruzando hotspots. |
| `get_repository_hotspots` | Retorna los archivos con mayor churn y riesgo histórico desde DuckDB. |
| `consolidate_memory` | Agrupa engrams redundantes y calcula el decaimiento temporal de lecciones. |
