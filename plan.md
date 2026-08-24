**Fase 1: Capa de Datos y Telemetría Analítica (Python)**

- **Actualización del entorno:** Añadir `polars`, `duckdb`, `pydantic-ai` y `litellm` al archivo `pyproject.toml` en `python/ozy-brain`.

- **Motor analítico local (`ozy_brain/data_engine.py`):** Implementar ingestión de logs de Git mediante Polars para calcular métricas de _churn_, frecuencia de cambios por archivo y complejidad de dependencias en memoria.

- **Persistencia analítica embebida:** Configurar DuckDB para ejecutar consultas OLAP sobre el histórico de fallos, parches y mapas de acoplamiento de código sin sobrecargar SQLite.

---

**Fase 2: Despliegue del Sistema Multi-Agente (Supervisor & Crítico)**

- **Supervisor orquestador (`ozy_brain/brain.py`):** Sustituir la lógica monolítica determinista por un despachador jerárquico basado en esquemas de `pydantic-ai`.

- **Segundo Agente Auditor (`ozy_brain/agents/risk_agent.py`):** Reemplazar las heurísticas estáticas de `risk.py` por un agente crítico adversarial que simula regresiones y audita violaciones de diseño antes de confirmar operaciones.

- **Sintetizador de memoria (`ozy_brain/agents/memory_agent.py`):** Implementar algoritmos de consolidación de lecciones para agrupar patrones redundantes y aplicar factores de decaimiento temporal en SQLite.

---

**Fase 3: Bucle Reactivo Proactivo (Rust Core)**

- **Workers asíncronos en segundo plano:** Incorporar tareas periódicas con Tokio dentro de `ozymem-server` para indexación y mantenimiento no bloqueante.

- **Vigilancia del sistema de archivos:** Añadir `notify` en `ozymem-server` para monitorear eventos de guardado y activar al agente crítico de forma silenciosa.

- **Canal Push MCP:** Extender `crates/ozymem-server/src/mcp.rs` para emitir notificaciones push proactivas hacia el cliente sin requerir una consulta manual.

---

**Fase 4: Exposición de Herramientas y Validación E2E**

- **Catálogo de herramientas MCP:** Registrar nuevas herramientas en `crates/ozymem-server/src/tools.rs` (ej. `audit_changes_with_critic`, `get_repository_hotspots`, `consolidate_memory`).

- **Pruebas integradas:** Crear suites de validación cruzada en `tests/test_brain.py` y `crates/ozymem-server/tests/mcp_server_tests.rs` para medir latencias de subprocesos y validar que el _second-agent_ no bloquee el hilo principal.

**Fase 1: Capa de Datos y Telemetría Analítica (Python)**

- **Actualización del entorno:** Añadir `polars`, `duckdb`, `pydantic-ai` y `litellm` al archivo `pyproject.toml` en `python/ozy-brain`.

- **Motor analítico local (`ozy_brain/data_engine.py`):** Implementar ingestión de logs de Git mediante Polars para calcular métricas de _churn_, frecuencia de cambios por archivo y complejidad de dependencias en memoria.

- **Persistencia analítica embebida:** Configurar DuckDB para ejecutar consultas OLAP sobre el histórico de fallos, parches y mapas de acoplamiento de código sin sobrecargar SQLite.

---

**Fase 2: Despliegue del Sistema Multi-Agente (Supervisor & Crítico)**

- **Supervisor orquestador (`ozy_brain/brain.py`):** Sustituir la lógica monolítica determinista por un despachador jerárquico basado en esquemas de `pydantic-ai`.

- **Segundo Agente Auditor (`ozy_brain/agents/risk_agent.py`):** Reemplazar las heurísticas estáticas de `risk.py` por un agente crítico adversarial que simula regresiones y audita violaciones de diseño antes de confirmar operaciones.

- **Sintetizador de memoria (`ozy_brain/agents/memory_agent.py`):** Implementar algoritmos de consolidación de lecciones para agrupar patrones redundantes y aplicar factores de decaimiento temporal en SQLite.

---

**Fase 3: Bucle Reactivo Proactivo (Rust Core)**

- **Workers asíncronos en segundo plano:** Incorporar tareas periódicas con Tokio dentro de `ozymem-server` para indexación y mantenimiento no bloqueante.

- **Vigilancia del sistema de archivos:** Añadir `notify` en `ozymem-server` para monitorear eventos de guardado y activar al agente crítico de forma silenciosa.

- **Canal Push MCP:** Extender `crates/ozymem-server/src/mcp.rs` para emitir notificaciones push proactivas hacia el cliente sin requerir una consulta manual.

---

**Fase 4: Exposición de Herramientas y Validación E2E**

- **Catálogo de herramientas MCP:** Registrar nuevas herramientas en `crates/ozymem-server/src/tools.rs` (ej. `audit_changes_with_critic`, `get_repository_hotspots`, `consolidate_memory`).

- **Pruebas integradas:** Crear suites de validación cruzada en `tests/test_brain.py` y `crates/ozymem-server/tests/mcp_server_tests.rs` para medir latencias de subprocesos y validar que el _second-agent_ no bloquee el hilo principal.

El documento propuesto es **totalmente adecuado y tácticamente acertado** para transformar Ozygram de una herramienta MCP de consulta reactiva a un sistema cognitivo autónomo y proactivo.

---

**Verificación de Limitaciones Actuales (v0.2.0)**

Las limitaciones diagnosticadas son reales en la estructura actual del proyecto:

- **Heurísticas rígidas en `ozy-brain`:** Módulos como `risk.py`, `reflector.py` y `planner.py` operan con reglas estáticas y deterministas, perdiendo capacidad adaptativa frente a bases de código complejas o políglotas.

- **Modelo síncrono/reactivo:** `ozymem-server` solo procesa peticiones entrantes vía MCP; no ejecuta tareas de auditoría en segundo plano ni alerta sobre anomalías de forma proactiva.

- **Persistencia plana:** SQLite gestiona símbolos y grafos AST de primer orden, pero carece de análisis de series temporales (churn de Git, acoplamiento dinámico y decaimiento de memoria por desuso).

---

**Cómo potenciar el "Segundo Agente" (Patrón Supervisor-Crítico)**

Para estructurar un agente dentro de otro sin generar sobrecarga ni bucles infinitos:

1. **Patrón Actor-Critic (Supervisor & Adversary):**

- **Agente Principal (Planificador/Constructor):** Genera el plan, AST diffs y propuestas de refactorización o memoria.
- **Segundo Agente (Auditor/Crítico Autónomo):** Se ejecuta en una capa paralela o "sandbox". Su único objetivo es desafiar al agente principal: detecta vectores de regresión, violaciones de arquitectura, calcula el riesgo real en base a métricas de Git y veta cambios antes de escribirlos en memoria o código.

2. **Bucle Proactivo en Rust (`Tokio + Notify`):**

- El núcleo en Rust vigila el sistema de archivos (`notify`). Al detectar un guardado, despierta al segundo agente en `ozy-brain` mediante un worker de fondo.

- Esto permite emitir notificaciones MCP push automáticas sin requerir una consulta manual del usuario.

---

**Recomendaciones para Flexibilidad y Stack**

- **Rechazar Go:** Mantener **Rust** (rendimiento, parsing AST y servidor de memoria) + **Python** (razonamiento, manipulación de datos y LLMs). Agregar Go triplicaría la fricción de despliegue sin aportar ventajas sobre Rust.

- **Evitar sobrecarga de frameworks:** No combines `langgraph` y `crewai` simultáneamente. Utiliza **`pydantic-ai`** (para tipado estricto y validación de esquemas) junto a **`litellm`**.

- **Persistencia y Métricas:** La combinación de **DuckDB** (consultas OLAP sobre Git churn) y **Polars** (análisis vectorial/tabular de dependencias) otorga la máxima velocidad analítica local sin requerir servicios externos pesados.
