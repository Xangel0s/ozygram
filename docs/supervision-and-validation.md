# Supervisión Cognitiva y Validación Determinista

Ozygram incorpora un subsistema de supervisión arquitectónica diseñado para actuar como un revisor de código implacable (*Adversarial Critic*) que audita planes, detecta regresiones y bloquea operaciones destructivas antes de que se toquen los archivos del proyecto.

---

## 1. El Supervisor y el Crítico Adversarial

El sistema se estructura en dos roles complementarios:

- **`SupervisorAgent`** ([`supervisor.py`](../python/ozy-brain/ozy_brain/agents/supervisor.py)):
  - Orquesta las peticiones cognitivas del IDE.
  - Coordina la telemetría del repositorio, la consolidación de recuerdos y las auditorías de riesgo.
  - Genera respuestas estructuradas con planes de mitigación, niveles de confianza y recomendaciones de acción.
- **`RiskCriticAgent`** ([`risk_critic.py`](../python/ozy-brain/ozy_brain/agents/risk_critic.py)):
  - Adopta una postura adversaria: asume que cada cambio propuesto romperá algo en producción hasta que se demuestre lo contrario.
  - Identifica vectores de regresión sutiles, incompatibilidades de firmas y acoplamiento peligroso.

---

## 2. Validación Determinista Sin LLM (Modo Offline / $0 Costo)

Una de las mayores fortalezas de Ozygram es que **no requiere un modelo de lenguaje de pago para proteger la base de código**. Si no hay conexión o no tienes API keys configuradas, entra en acción `_audit_offline`:

```text
               Plan o Diff de Código Propuesto
                             │
                             ▼
 ┌───────────────────────────────────────────────────────────────┐
 │               MOTOR DE VALIDACIÓN DETERMINISTA                │
 ├───────────────────────────────────────────────────────────────┤
 │  1. Telemetría de Git (DuckDB + Polars)                       │
 │     → Cruce con archivos de alto Churn y Hotspots históricos  │
 │                                                               │
 │  2. Control de Radio de Explosión (Blast Radius)              │
 │     → ¿Modifica > 8 archivos concurrentemente?               │
 │                                                               │
 │  3. Guardia Anti-Destrucción DDL/DML                          │
 │     → Regex veto: DROP TABLE, DELETE FROM, TRUNCATE, etc.    │
 │                                                               │
 │  4. Decaimiento Temporal de Memoria                           │
 │     → S = C · e^(-λ·Δt) para poda matemática de contexto     │
 └───────────────────────────┬───────────────────────────────────┘
                             │
                             ▼
         Veredicto: [LOW | MEDIUM | HIGH | CRITICAL]
         (Bloqueo preventivo de la operación si is_blocked=True)
```

### A. Telemetría de Hotspots con DuckDB + Polars ([`data_engine.py`](../python/ozy-brain/ozy_brain/data_engine.py))
El motor analítico lee el log de Git local e indexa métricas históricas de cada archivo:
- **Churn Score**: Volumen acumulado de líneas agregadas y eliminadas.
- **Fix Commits**: Cuántas veces un archivo estuvo involucrado en commits con palabras como `fix`, `bug`, `issue` o `patch`.
- **Author Churn**: Número de autores distintos que han editado el archivo.

Si un cambio propone tocar un archivo clasificado como **CRITICAL HOTSPOT**, el sistema emite una alerta roja y exige tests de regresión específicos.

### B. Guardia Anti-Destrucción DDL/DML
El supervisor inspecciona el texto del diff buscando sentencias destructivas en bases de datos:
`DROP TABLE`, `DELETE FROM`, `ALTER TABLE`, `TRUNCATE`, `DROP COLUMN`.
Si alguna es detectada:
- **Bloquea la operación (`is_blocked = True`)**.
- Asigna nivel de riesgo **`CRITICAL`**.
- Exige validación con scripts de migración no destructivos y salvaguardas de *rollback*.

### C. Control de Radio de Impacto (*Blast Radius*)
Si un plan involucra más de **8 archivos simultáneamente**:
- Marca el riesgo como `HIGH` por dispersión arquitectónica.
- Emite la recomendación obligatoria de partir el trabajo en subtareas atómicas e independientes.

### D. Consolidación de Memoria por Decaimiento Exponencial
Para depurar y resumir memorias sin enviar miles de líneas a un LLM:
$$S = C \cdot e^{-\lambda \Delta t}$$
Donde:
- $S$: Relevancia residual de la lección.
- $C$: Puntuación de confianza inicial.
- $\lambda$: Factor de decaimiento temporal.
- $\Delta t$: Días transcurridos desde el último acceso o actualización.

---

## 3. Supervisión Semántica Opcional con Modelos Gratuitos

Si deseas agregar razonamiento semántico para que el supervisor actúe como un *Senior Tech Lead*, puedes activar proveedores sin costo:

### Opción A: Gemini 2.0 Flash (Google AI Studio Free Tier)
- **Cuota Gratuita Oficial**: 15 requests por minuto, 1 millón de tokens por minuto.
- **Rendimiento**: Velocidad ultrarrápida (~500 ms) y alta capacidad de seguimiento de reglas estructuradas en JSON.
- **Activación**:
  ```powershell
  $env:GEMINI_API_KEY = "tu_clave_de_google_ai_studio"
  ```

### Opción B: Ollama Local (`qwen2.5-coder` / `gemma2`)
- **100% Local y Privado**: Corre directamente en la GPU o CPU de tu máquina.
- **Activación**: Ozygram detecta automáticamente si Ollama está corriendo en `http://localhost:11434` o mediante `$env:OLLAMA_HOST`.

### Opción C: Modelos Gratuitos de OpenRouter
- Acceso a modelos como `openrouter/nvidia/nemotron-3-nano-omni-30b-a3b-reasoning:free` o `openrouter/cohere/north-mini-code:free`.
- **Activación**:
  ```powershell
  $env:OPENROUTER_API_KEY = "tu_clave_de_openrouter"
  ```

---

## 4. Presupuesto Estricto de Tokens (*Zero Token Bloat*)

Ozygram está diseñado específicamente para **no quemar tu ventana de contexto ni agotar tus cuotas**:

1. **Pre-Filtrado Analítico**: Antes de enviar cualquier texto al modelo, DuckDB y `tgrep` reducen el repositorio a un diff condensado y un conjunto de métricas numéricas (< 2 KB).
2. **Límite Estricto de Salida**: Configurado por defecto a `max_tokens = 1500` con `temperature = 0.2` para respuestas concisas y deterministas.
3. **Respuesta en JSON Puro**: Las auditorías se devuelven en formato estructurado sin prosa innecesaria, reduciendo drásticamente el consumo de tokens de entrada y salida.
