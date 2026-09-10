# Documentación Oficial de Ozygram

Bienvenido a la documentación oficial y completa de **Ozygram** (Dual-Tier Engine v0.4.0), el sistema operativo cognitivo, memoria contextual persistente y grafo de código para agentes y asistentes de desarrollo asistidos por IA.

---

## 📚 Índice Modular de Secciones

### 1. [Arquitectura Dual-Tier (`docs/architecture.md`)](architecture.md)
- Desacoplamiento en dos carriles: **Carril Rápido (Rust)** y **Carril de Potencia (Python)**.
- Desglose de crates en el monorepo (`crates/`, `python/`).
- Autoridad transaccional única con SQLite local.
- Patrón Transaccional Outbox (`memory_outbox`) con triggers nativos y sincronización asíncrona.
- Resiliencia: Auto-spawn de demonios en segundo plano y Circuit Breaker de fallback determinista.

### 2. [Búsqueda Semántica Híbrida y Fusión RRF (`docs/semantic-search.md`)](semantic-search.md)
- Fusión de búsqueda léxica dispersa (SQLite FTS5 / BM25) y búsqueda semántica densa.
- Motor local `FastEmbed` con ONNX Runtime en C++ (`BAAI/bge-m3` / `bge-base-en-v1.5`).
- Almacenamiento vectorial en colecciones locales de ChromaDB.
- Algoritmo matemático **Reciprocal Rank Fusion (RRF)**: fórmula, ranking y ventajas.

### 3. [Supervisión Cognitiva y Validación Determinista (`docs/supervision-and-validation.md`)](supervision-and-validation.md)
- Roles de `SupervisorAgent` y el crítico adversarial `RiskCriticAgent`.
- Validación determinista sin LLM ($0 costo, <5 ms):
  - Telemetría de Git Churn y detección de Hotspots con `DuckDB` + `Polars`.
  - Guardia de seguridad anti-destrucción DDL/DML (`DROP TABLE`, `DELETE FROM`, etc.).
  - Control de radio de explosión (*Blast Radius* > 8 archivos).
  - Poda matemática de memoria por decaimiento temporal exponencial ($S = C \cdot e^{-\lambda \Delta t}$).
- Supervisión semántica opcional con modelos gratuitos: Google AI Studio (`Gemini 2.0 Flash`), Ollama local (`qwen2.5-coder`), y OpenRouter.
- Presupuesto estricto de tokens (*Zero Token Bloat*).

### 4. [Herramientas de Alto Rendimiento ("The Dream Team") (`docs/dream-team-tools.md`)](dream-team-tools.md)
- Integración de `tgrep` (Microsoft): Búsqueda trigram de expresiones regulares en milisegundos.
- Integración de `rtk` (Rust Token Killer): Compresión agresiva de payloads, limpieza ANSI y ahorro de tokens.
- Integración de `fastembed`: Inferencia vectorial local sin consumo de cuotas de API.

### 5. [Integración y Referencia MCP (`docs/mcp-integration.md`)](mcp-integration.md)
- Guía de configuración para Antigravity IDE, Claude Desktop, Cursor y VS Code.
- Catálogo completo de herramientas MCP: memoria, grafo, cerebro cognitivo, diagnósticos y salud de código.
- Recursos MCP (`ozymem://summary`, `recent-lessons`, etc.) y suscripciones dinámicas.

### 6. [Sistema Engram y Prefill Especulativo (`docs/engram_system.md`)](engram_system.md)
- Tabla determinista de firmas y contratos $O(1)$ con `rkyv` y `memmap2`.
- Prefill predictivo para maximizar la tasa de acierto de prompt cache (>90%).
- Sandbox de validación previa test-time (`ozy_verify_diff`).
- Sincronización descentralizada P2P con Git Notes (`refs/notes/ozymem`).

### 7. [Historial de Versiones y Novedades (`docs/changelog.md`)](changelog.md)
- Registro cronológico detallado de cambios y mejoras desde la v0.2.0 hasta la v0.4.0.

---

## ⚡ Guía Rápida de Instalación

### Windows (PowerShell)
```powershell
powershell -ExecutionPolicy Bypass -File .\install.ps1
```

### Linux / macOS (Bash)
```bash
chmod +x ./install.sh
./install.sh
```

### Configuración MCP Básica
```json
{
  "mcpServers": {
    "ozygram": {
      "command": "ozymem-server",
      "args": []
    }
  }
}
```
