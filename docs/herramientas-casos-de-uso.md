# Guía Maestra de Herramientas, Casos de Uso y Arquitectura Operativa (v0.2.4)

Este documento detalla exhaustivamente cada una de las herramientas de Ozygram, tanto las **Meta-Herramientas MCP** que utilizan los agentes de Inteligencia Artificial como los **Comandos CLI** ejecutados por desarrolladores en la terminal, junto con sus casos de uso prácticos, ejemplos de prompts y los mecanismos de aislamiento y optimización que garantizan cero alucinaciones y mínimo consumo de tokens.

---

## 1. Arquitectura en Tres Sectores

El ecosistema de Ozygram opera mediante la colaboración de tres capas no redundantes:

```text
+-----------------------------------------------------------------------------------------+
|                                    CAPA DE INTERACCIÓN                                  |
|   Desarrollador (Terminal CLI)                       Agente AI (Chat / Protocolo MCP)   |
|          |                                                          |                   |
|          v                                                          v                   |
|   ozymem dream / ozymem doctor                       ozy_plan, ozy_context, ozy_patch   |
+-----------------------------------------------------------------------------------------+
                                           |
                                           v
+-----------------------------------------------------------------------------------------+
| 1. SECTOR OZY-BRAIN (Cerebro Python / Sistema 2 Deliberativo)                           |
|    - Generative MCTS Searcher: Búsqueda en árbol Monte Carlo y cálculo de Free Energy.  |
|    - Contratos Cross-Repo: Resolución bidireccional FastAPI (@router) <-> Next.js TSX.  |
|    - TGVR: Topologically-Gated Vector Retrieval combinando CTE recursivo con pgvector. |
+-----------------------------------------------------------------------------------------+
                                           ^
                     IPC Ultra-Baja Latencia (<0.1 ms / TCP_NODELAY)
                                           v
+-----------------------------------------------------------------------------------------+
| 2. SECTOR RUST CORE (Motor Nativo / ozymem-core, ozymem-parser, ozymem-server)         |
|    - Tree-Sitter AST: Indexación incremental de sintaxis (<40 us) y esqueletos LOD.     |
|    - Sandbox CoW en RAM: Simulación de parches con chequeo de aridad en memoria volátil.|
|    - EphemeralWorktree: Sandbox aislado de Git con política Default-Deny para CLI.      |
|    - SQLite WAL (L1): Base de datos transactional outbox con latencia <1 ms.            |
+-----------------------------------------------------------------------------------------+
                                           |
                              Ciclos de Reposo / Offline
                                           v
+-----------------------------------------------------------------------------------------+
| 3. SECTOR OZYDREAM / TRAJECTORY REPLAY TUNER (Consolidación y Aprendizaje)              |
|    - En línea (Modo Sombra): Medición de branching factor real, tokens y adherencia.    |
|    - Offline (Sleep Replay): Minería de hábitos (Basal Ganglia), cicatrices de amenaza  |
|      (Amygdala) y síntesis de vacunas permanentes (.ozy/memories/gotchas/*.md).         |
+-----------------------------------------------------------------------------------------+
```

---

## 2. Meta-Herramientas MCP (Para Agentes de Inteligencia Artificial)

Las herramientas MCP consolidan 25+ funciones dispersas en 5 meta-herramientas de alto nivel, reduciendo el esquema del prompt del sistema en más de 1,500 tokens por turno.

### 2.1. `ozy_plan` — Planificación Topológica y MCTS

* **Propósito:** Calcula la secuencia matemática exacta de edición de archivos y su radio de impacto (*blast radius*) antes de escribir una sola línea de código, ordenando las dependencias de raíz a hojas.
* **Mecanismo Interno:** Ejecuta en memoria RAM el algoritmo Active Inference EFE-MCTS (Expected Free Energy). Evalúa si la meta involucra modelos, esquemas, servicios o interfaces, y descarta rutas que violen dependencias hacia atrás.
* **Argumentos:**
  - `goal` (string, obligatorio): Descripción concisa y técnica del objetivo a resolver.
  - `critique_role` (string, opcional): Rol de crítica dialéctica en RAM (`architect`, `security_critic`, `code_reviewer`, `test_engineer`).
* **Ejemplo de Invocación:**
  ```json
  {
    "goal": "Modificar el modelo ControlInforme para agregar campo estado_firma y sincronizar schema y router en FastAPI"
  }
  ```
* **Salida Típica:**
  ```text
  Ozy Brain: plan (Confidence: 0.88)
  Goal: Modificar el modelo ControlInforme...
  Target Files & Edit Sequence:
    Step 1: models.py -> Types / Data Contract [EXPORTS: ControlInforme]
    Step 2: schemas.py -> Data Contract [EXPORTS: ControlInformeResponse]
    Step 3: router.py -> API Endpoint [EXPORTS: obtener_informe]
  Blast Radius & Risks:
    Risk Tier: MODERATE | Impact: 3 file(s)
  Validation:
    pytest tests/test_informes.py
  ```

---

### 2.2. `ozy_context` — Inyección Extractive y Detección Preventiva

* **Propósito:** Inspecciona archivos con niveles de detalle progresivo (LOD) sin cargar miles de líneas innecesarias en la ventana de contexto del LLM, inyectando automáticamente advertencias de errores conocidos.
* **Mecanismo Interno:** Usa Tree-Sitter para podar cuerpos de funciones o métodos extensos, devolviendo únicamente la firma, tipos y docstrings. Consulta la tabla `amygdala_reflexes` en SQLite; si el archivo ha fallado anteriormente, antepone un aviso preventivo.
* **Argumentos:**
  - `action` (string, obligatorio): `"file"`, `"task"`, o `"project"`.
  - `path` (string, opcional): Ruta absoluta o relativa del archivo a inspeccionar.
  - `lod` (string, opcional): Nivel de detalle:
    - `"macro"`: Estructura general de clases y módulos.
    - `"contracts"`: Firmas de funciones exportadas, tipos de parámetros y retornos (recomendado por defecto).
    - `"full"`: Código fuente completo.
* **Salida Típica:**
  ```text
  [ALERT: GOTCHA_PREVENTION - HIGH_FRAGILITY]
  Atención: El archivo 'app/services/outbox.py' tiene historial de bloqueos de concurrencia en SQLite WAL.
  Asegurar timeout explicito y transacciones cortas.

  [EXPORTS & CONTRACTS]:
  class UniversalOutbox:
      def enqueue(self, event_type: str, payload: dict) -> str
      def process_pending(self, batch_size: int = 50) -> int
  ```

---

### 2.3. `ozy_patch` — Parcheo Quirúrgico y Simulación en RAM

* **Propósito:** Aplica modificaciones de código con validación sintáctica estricta o simula cambios en memoria volátil sin tocar el disco.
* **Mecanismo Interno:**
  - En modo `"simulate"`: Ejecuta `simulate_patch` en memoria RAM Copy-on-Write (`SANDBOX_INVARIANT_COW_RAM_ONLY`). Parsea la nueva firma con Tree-Sitter y consulta a todos los llamadores en SQLite. Si detecta cambio de aridad (faltan parámetros requeridos en llamadores), emite un veto (`[CRITICAL_VETO: INVARIANT_VIOLATION]`) en menos de 5 ms.
  - En modo `"apply"`: Realiza un parcheo AST difuso tolerante a espacios y saltos de línea (CRLF/LF) con auto-indentación, asegurando que el archivo resultante compile.
* **Argumentos:**
  - `action` (string, obligatorio): `"apply"`, `"simulate"`, o `"replace_symbol"`.
  - `path` (string, opcional): Ruta del archivo a modificar.
  - `target_symbol` (string, opcional): Nombre de la función o clase objetivo.
  - `candidate_code` (string, opcional): Código propuesto de reemplazo.
* **Ejemplo de Invocación:**
  ```json
  {
    "action": "simulate",
    "target_symbol": "calcular_resistencia_is",
    "candidate_code": "def calcular_resistencia_is(fuerza_kn: float, diametro_mm: float) -> float:\n    return (fuerza_kn * 1000.0) / (diametro_mm ** 2)"
  }
  ```

---

### 2.4. `ozy_memory` — Memoria Persistente Git-First y Reflexes

* **Propósito:** Almacena y recupera lecciones arquitectónicas, convenciones y patrones sin depender de servidores externos, guardando el conocimiento directamente como código en Git.
* **Mecanismo Interno:** Sincroniza archivos Markdown atómicos bajo `.ozy/memories/<kind>/*.md` con YAML Frontmatter. Combina búsqueda léxica FTS5 (BM25) en SQLite con búsqueda vectorial KNN en ChromaDB / PostgreSQL mediante Reciprocal Rank Fusion (RRF).
* **Argumentos:**
  - `action` (string, obligatorio): `"search"`, `"save"`, `"sync_markdown"`, o `"export_git_notes"`.
  - `query` (string, opcional): Texto de búsqueda semántica.
  - `topic_key` (string, opcional): Clave temática de la memoria.
  - `content` (string, opcional): Contenido en Markdown de la lección aprendida.

---

### 2.5. `ozy_doctor` — Diagnóstico de Entorno e Integridad

* **Propósito:** Supervisa la salud técnica de todo el entorno local, base de datos SQLite WAL, cola outbox y linteo de migraciones SQL.
* **Mecanismo Interno:** Valida que la base de datos no tenga bloqueos de WAL, verifica el esquema de tablas, audita que las migraciones SQL sean idempotentes (`IF NOT EXISTS`) y reporta métricas de latencia.
* **Argumentos:**
  - `action` (string, opcional): `"status"`, `"code"`, o `"audit_migrations"`.

---

## 3. Comandos CLI (Para Terminal y Desarrolladores)

Todos los comandos se invocan a través de los binarios nativos compilados `ozymem` u `ozygram`:

| Comando CLI | Sector Responsable | Descripción y Propósito |
| :--- | :--- | :--- |
| `ozymem dream status` | Dream-RSI | Muestra la política MCTS activa, constante UCT ($\sqrt{2} \approx 1.4142$), umbral de poda y límite de observación (32 KB). |
| `ozymem dream shadow` | Modo Sombra | Despliega métricas de modo sombra: factor de ramificación real, porcentaje de adherencia a los planes y contabilidad de tokens. |
| `ozymem dream run` | Dream-RSI Offline | Ejecuta manualmente una ronda de simulación offline: minería de hábitos en Git, cicatrices en Amygdala y compactación glial. |
| `ozymem dream diagnose` | Diagnóstico MCTS | Analiza una trayectoria de búsqueda pasada, detectando cuellos de botella y ramas que requirieron poda. |
| `ozymem dream tree` | Visualizador | Genera un diagrama Mermaid renderizable del árbol de decisiones explorado por MCTS. |
| `ozymem doctor` | Rust Core | Auditoría rápida del archivo `.ozymem.toml` y conectividad con la base SQLite local. |
| `ozygram serve` | Servidor MCP | Arranca el servidor monolítico de alto rendimiento vía stdio para clientes AI (Antigravity, Cursor, Claude). |

---

## 4. La Dualidad de Dream-RSI: Online vs. Offline

Dream-RSI no es un concepto abstracto, sino dos fases temporales de optimización rigurosa:

```text
                               +-----------------------------+
                               |     DREAM-RSI DUALIDAD      |
                               +-----------------------------+
                                              |
                   +--------------------------+--------------------------+
                   |                                                     |
                   v                                                     v
   FASE 1: ONLINE (MCTS en RAM)                         FASE 2: OFFLINE (Consolidación en Reposo)
   - Activo durante la sesión de chat.                  - Se activa tras 30 min de inactividad.
   - Búsqueda en árbol en RAM volátil (<10 ms).         - Cero modificación de archivos de código.
   - Poda sintáctica y arity check previo.              - Minería de commits y coreografías Git.
   - Radio de impacto y secuencia topológica.           - Cicatrices de fragilidad (Amygdala).
   - Cero I/O en disco durante la exploración.          - Síntesis de vacunas (.ozy/memories/gotchas/).
   - Reduce >85% de tokens de contexto del LLM.         - Phagocitosis glial y compactación SQLite.
```

---

## 5. Los Dos Niveles de Aislamiento y Sandboxing

Para garantizar que un agente de IA nunca rompa tu entorno de desarrollo mientras explora hipótesis, Ozygram aplica dos barreras de contención:

### Nivel 1: Volátil en Memoria RAM Pura (CoW)
* **Objetivo:** Pruebas ultra-rápidas a nivel de firma y dependencias ($< 5\text{ ms}$).
* **Garantía:** `SANDBOX_INVARIANT_COW_RAM_ONLY`.
* **Comportamiento:** El código candidato se analiza en estructuras de memoria volátil. Si un cambio en una función rompe a 5 componentes que la invocan, el explorador MCTS la descarta de inmediato sin haber tocado el disco ni generado un archivo temporal.

### Nivel 2: Sandbox Git Worktree Efímero (`EphemeralWorktree`)
* **Objetivo:** Pruebas que requieren ejecución de comandos CLI reales (compiladores, linters, tests).
* **Garantía:** Desacoplamiento total del árbol de trabajo de Git con política **Default-Deny**.
* **Comportamiento:**
  1. Ejecuta `git worktree add --detach <temp_dir> HEAD`.
  2. Aplica el parche exclusivamente en `<temp_dir>`.
  3. Ejecuta comandos de validación restringidos únicamente a binarios permitidos: `cargo`, `pytest`, `npm`, `python`, `node`, `tsc`, `rg`.
  4. Bloquea de forma inmediata cualquier patrón de destrucción de datos (`rm`, `drop table`, `format`).
  5. Mediante el trait `Drop` de Rust (RAII), el directorio efímero se destruye y limpia automáticamente al finalizar la prueba, dejando el repositorio principal intacto.

---

## 6. Casos de Uso y Prompts Recomendados

Para obtener el máximo rendimiento y menor consumo de tokens de Ozygram, utiliza los siguientes patrones de instrucción con tu agente de IA:

### Caso A: Corrección de Bugs con Mínimo Consumo de Tokens
> **Prompt:**  
> *"Hay un bug en [descripción del fallo o error]. Utiliza `ozy_context(lod='contracts')` y `ozy_plan` con MCTS para diagnosticar el problema y calcular el blast radius mínimo antes de editar ningún archivo. Aplica la solución mediante `ozy_patch` quirúrgico."*

### Caso B: Refactorización de Funciones Compartidas
> **Prompt:**  
> *"Necesito modificar la firma de la función [nombre_funcion] en [archivo]. Antes de escribir en disco, ejecuta `ozy_patch(action='simulate')` para comprobar si rompe la aridad de los llamadores en el resto del proyecto y muéstrame los resultados de la simulación en RAM."*

### Caso C: Implementación Full-Stack (Backend FastAPI + Frontend Next.js)
> **Prompt:**  
> *"Vamos a implementar la nueva característica [nombre_feature]. Usa `ozy_plan` para determinar la secuencia topológica estricta (modelo -> schema -> router -> componente frontend) y reporta los riesgos antes de comenzar la implementación."*

### Caso D: Auditoría y Consolidación de Lecciones Post-Sesión
En tu terminal:
```powershell
# 1. Ver cómo se comportó la exploración en la sesión
ozymem dream shadow

# 2. Correr una ronda de consolidación y minería de hábitos
ozymem dream run

# 3. Verificar estado de salud general
ozymem doctor
```

---

## 7. Registro de Versión

* **Versión del documento:** v0.2.4
* **Crates asociados:** `ozymem-core v0.2.4`, `ozymem-parser v0.2.4`, `ozymem-server v0.2.4`, `ozymem-cli v0.2.4`.
* **Python Brain:** `ozy-brain v0.2.4`.
