<div align="center">

# ⚡ Ozygram / Ozymem ⚡

### *El Sistema Operativo Cognitivo, Memoria Contextual Persistente y Grafo de Código para Agentes de IA*

[![Engine](https://img.shields.io/badge/Architecture-Dual--Tier_v0.4.0-blueviolet?style=for-the-badge&logo=rust)](docs/architecture.md)
[![Fast Lane](https://img.shields.io/badge/Fast_Lane-Rust_<5ms-orange?style=for-the-badge&logo=rust)](crates/ozymem-server)
[![Power Lane](https://img.shields.io/badge/Power_Lane-Python_Cognitive-blue?style=for-the-badge&logo=python)](python/ozy-brain)
[![Storage](https://img.shields.io/badge/Storage-SQLite_ACID_+_Outbox-003B57?style=for-the-badge&logo=sqlite)](docs/architecture.md)
[![Vector](https://img.shields.io/badge/Embeddings-FastEmbed_ONNX_Local-success?style=for-the-badge)](docs/semantic-search.md)
[![Analytics](https://img.shields.io/badge/Telemetry-DuckDB_+_Polars-FFF000?style=for-the-badge&logo=duckdb)](docs/supervision-and-validation.md)
[![MCP](https://img.shields.io/badge/Protocol-MCP_Native-green?style=for-the-badge)](docs/mcp-integration.md)

<p align="center">
  <b>Ozygram no solo recuerda:</b> entiende el proyecto, aprende del usuario, anticipa riesgos, previene desviaciones de arquitectura y guía al agente con memoria viva, telemetría y criterio técnico.
</p>

---

</div>

## 💡 ¿Qué es Ozygram y Qué Resuelve?

Los asistentes de desarrollo y agentes autónomos (como Claude, Cursor, Antigravity o agentes basados en LLMs) sufren de cinco debilidades críticas:
1. **Amnesia Crónica**: Pierden el contexto entre sesiones y repiten una y otra vez los mismos errores ya resueltos.
2. **Alucinación de Firmas**: Asumen contratos de funciones y tipos inexistentes, provocando errores en cascada.
3. **Falta de Criterio de Riesgo**: Modifican indiscriminadamente archivos críticos o esquemas sin evaluar el impacto transversal.
4. **Desperdicio de Tokens**: Leen repositorios enteros, saturando la ventana de contexto con código irrelevante o logs ruidosos.
5. **Fragilidad de Integración**: Sistemas de memoria tradicionales fallan si no hay conexión a internet o si la base vectorial se desincroniza.

### 🎯 Lo que Ozygram Aporta al Desarrollador y al Agente:
- **🧠 Memoria Contextual Persistente**: Almacena decisiones de arquitectura, lecciones aprendidas, convenciones, gotchas y reglas por módulo en SQLite local (`.ozymem/memory.db`).
- **⚡ Prefill Predictivo $O(1)$ (`rkyv` + `memmap2`)**: Resuelve firmas y contratos adyacentes en **$\approx 15\text{ ns}$** directo en memoria mapeada sin allocations, maximizando la tasa de acierto de prompt cache (>90%).
- **🛡️ Supervisión Adversaria y Detección de Riesgo**: Analiza telemetría histórica de Git con **DuckDB** para detectar *Hotspots*, calcula el radio de dispersión (*Blast Radius*) y **bloquea automáticamente modificaciones destructivas DDL/DML** (`DROP TABLE`, `TRUNCATE`, etc.).
- **🔍 Búsqueda Híbrida con RRF**: Combina búsqueda léxica exacta (SQLite FTS5 / BM25) con embeddings densos locales (**FastEmbed ONNX**) usando el algoritmo **Reciprocal Rank Fusion (RRF)**. Cero costo de API.
- **🚀 Integración con el "Dream Team"**: Equipado con `tgrep` (búsqueda de expresiones regulares trigram ultra-rápida de Microsoft) y `rtk` (compresor de tokens de terminal y sanitizador ANSI de `rtk-ai`).

---

## 🏗️ Arquitectura Dual-Tier (Asimétrica y Resiliente)

Ozygram separa sus responsabilidades en dos carriles desacoplados gobernados por el patrón **Transactional Outbox**:

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

### 1. El Carril Rápido (Rust)
- **Cero Latencia**: Responde al IDE en microsegundos.
- **Autoridad Única de Datos**: SQLite local (`.ozymem/memory.db`) es la única fuente de verdad. Si Python no está disponible, Rust sigue operando al 100%.
- **Transactional Outbox**: Triggers de SQLite registran automáticamente cada inserción, edición o borrado en `memory_outbox`, garantizando que jamás existan "vectores fantasma".

### 2. El Carril de Potencia (Python `ozy-brain`)
- **Worker Cognitivo Asíncrono**: Demonios en segundo plano drenan los eventos de la outbox y generan embeddings vectoriales locales.
- **Motor Analítico OLAP**: Ingesta la historia de Git con **DuckDB** y **Polars** para calcular métricas de *Churn* y riesgo de regresión.
- **Resiliencia Automática**: Si no está corriendo, Rust lo auto-inicia (`ensure_ozy_brain_running`). Si se produce un timeout, el **Circuit Breaker** entrega un fallback determinista inmediato sin romper la experiencia del agente.

---

## 🛡️ Validación Determinista vs Supervisión Semántica

Ozygram está diseñado para operar con **cero costo ($0.00)** sin requerir APIs externas:

| Capa | Componente | Modo Sin Conexión ($0 Costo) | Modo con LLM (Opcional) |
| :--- | :--- | :--- | :--- |
| **Búsqueda Semántica** | `FastEmbed` + ChromaDB + SQLite FTS5 | **100% Local ONNX** (`BAAI/bge-m3` / `bge-base`) | N/A (Inferencia local en CPU/GPU) |
| **Auditoría de Cambios** | `RiskCriticAgent` & `DataEngine` | **Determinista**: DuckDB Churn + DDL/DML Guard + Blast Radius | **Semántico**: Simulación de vectores de regresión sutiles |
| **Modelos Soportados** | `config.py` Cascade Fallback | Heurísticas matemáticas y reglas AST | **Gratis**: `gemini-2.0-flash`, Ollama (`qwen2.5-coder`), OpenRouter Free |
| **Presupuesto de Tokens** | `rtk` + Pre-filtrado analítico | **0 tokens gastados** | **Estricto**: `max_tokens = 1500`, diffs condensados |

---

## ⚡ Instalación Rápida

### Windows (PowerShell)
```powershell
powershell -ExecutionPolicy Bypass -File .\install.ps1
```

### Linux / macOS (Bash)
```bash
chmod +x ./install.sh
./install.sh
```

El instalador compila los binarios optimizados de release (`ozymem.exe` y `ozymem-server.exe`), los añade a tu `PATH` (`~/.ozymem/bin`), e instala el entorno cognitivo de Python.

---

## ⚙️ Configuración MCP Server (`mcpServers`)

Agrega Ozygram a la configuración de tu cliente MCP (**Antigravity IDE**, **Claude Desktop**, **Cursor**, **Windsurf**, **VS Code**):

```json
{
  "mcpServers": {
    "ozygram": {
      "command": "C:\\Users\\TU_USUARIO\\.ozymem\\bin\\ozymem-server.exe",
      "args": []
    }
  }
}
```

---

## 🛠️ Herramientas de Alto Rendimiento ("The Dream Team")

Ozygram se potencia mutuamente con las herramientas auxiliares instaladas en el sistema:

- **`tgrep` (Microsoft)**: Búsqueda trigram de expresiones regulares a velocidad extrema sobre monorepos masivos.
  ```powershell
  tgrep "fn [a-z_]+\(ctx: &Context\)"
  ```
- **`rtk` (Rust Token Killer)**: Purga de secuencias ANSI y compresión agresiva de payloads de terminal y logs para ahorrar tokens.
  ```powershell
  cargo test | rtk
  ```

---

## 📚 Documentación Modular por Secciones

Para explorar en profundidad cada subsistema de Ozygram, consulta las guías dedicadas en la carpeta [`docs/`](docs/INDEX.md):

1. 🏛️ [**Arquitectura Dual-Tier y Patrón Outbox**](docs/architecture.md): Detalles de sincronización transaccional, triggers y resiliencia.
2. 🔍 [**Búsqueda Semántica Híbrida y Fusión RRF**](docs/semantic-search.md): Algoritmo RRF, inferencia local con FastEmbed ONNX y ChromaDB.
3. 🛡️ [**Supervisión Cognitiva y Validación Determinista**](docs/supervision-and-validation.md): DuckDB Churn Telemetry, guardias DDL/DML y presupuesto de tokens.
4. 🚀 [**Herramientas del Dream Team (`tgrep`, `rtk`, `fastembed`)**](docs/dream-team-tools.md): Capacidades, benchmarks y sinergia.
5. 🔌 [**Referencia Completa de Herramientas MCP**](docs/mcp-integration.md): Catálogo de 30+ endpoints, recursos y suscripciones dinámicas.
6. ⚡ [**Sistema Engram y Prefill Especulativo**](docs/engram_system.md): Tablas $O(1)$ con `rkyv`, sandbox de diffs y Git Notes P2P.

---

<div align="center">
  <sub>Construido con ❤️ y obsesión por el rendimiento para la era de los Agentes de IA.</sub>
</div>
