# Documentación de Ozygram / Ozymem

Bienvenido a la documentación oficial y detallada de **Ozygram** (v0.2.0), el sistema de grafo de dependencias de código, memoria contextual persistente y razonamiento híbrido para desarrollo asistido por IA mediante el Model Context Protocol (MCP).

---

## 📚 Índice de Secciones

1. [**Arquitectura Global (`docs/overview.md`)**](overview.md)
   - Componentes del monorepo (`ozymem-core`, `ozymem-parser`, `ozymem-cli`, `ozymem-server`, `ozy-brain`).
   - Esquemas de almacenamiento SQLite (por proyecto y registro global).
   - Motor de embeddings semánticos y sincronización en tiempo real.

2. [**Organización Modular del Servidor (`docs/architecture_modular.md`)**](architecture_modular.md)
   - Descomposición modular de `ozymem-server` y `ozymem-core`.
   - Router central ligero y submódulos especializados (`schemas`, `graph`, `memory`, `git`, `unified`, `prompts`, `resources`, `verifier`, `brain`).

3. [**Sistema Engram, Prefill Especulativo y Sandbox (`docs/engram_system.md`)**](engram_system.md)
   - Almacenamiento determinista $O(1)$ con `rkyv` y `memmap2`.
   - Prefill predictivo y speculative decoding en `get_file_context`.
   - Sandbox de validación test-time (`ozy_verify_diff`) con `reflector.py`.
   - Sincronización descentralizada P2P con Git Notes (`refs/notes/ozymem`).

4. [**Referencia de Herramientas MCP (`docs/mcp_tools.md`)**](mcp_tools.md)
   - Guía exhaustiva de los 30+ endpoints MCP.
   - Herramientas de memoria (`lookup_engram`, `record_lesson`, `search_lessons`, `similar_lessons`).
   - Herramientas de grafo e impacto (`file_context`, `analyze_impact`, `graph_neighbors`, `graph_summary`).
   - Herramientas de validación y sandbox (`ozy_verify_diff`, `ozy_doctor`, `ozy_code_doctor`).
   - Herramientas Git colaborativas (`ozy_export_memory_notes`, `ozy_import_memory_notes`, `learn_from_changes`).
   - Motor de razonamiento (`ozy_brain`).

5. [**Novedades de Ozygram v0.2.0 (`docs/features_v02.md`)**](features_v02.md)
   - Resolución de rutas en cascada (rutas relativas, normalizadas y coincidencia de sufijo).
   - Fallback automático a símbolos AST cuando no existen memorias explícitas.
   - Clasificación inteligente de duplicados (`[High-Priority Refactor Candidates]` vs `[Structural Boilerplate]`).
   - Diagnósticos estáticos y linter de sintaxis AST mediante Tree-Sitter.
   - Soporte de subdirectorios / subpath para monorepos (`subpath`).

6. [**Novedades de Ozygram v0.3.0 (`docs/features_v03.md`)**](features_v03.md)
   - Motor analítico de telemetría y Git Churn embebido (`DuckDB` + `Polars`).
   - Sistema multi-agente (`Supervisor` y `Adversarial Risk Critic`).
   - Fusión y decaimiento exponencial temporal de memoria ($S = C \cdot e^{-\lambda \Delta t}$).
   - Integración nativa con `OpenRouter` y modelos gratuitos (`nemotron-reasoning`, `nemotron-120b`, `north-code`).
   - Fallback heurístico offline garantizado ($0 costo).

---

## 🚀 Instalación y Uso Rápido

### En Windows (PowerShell)
```powershell
powershell -ExecutionPolicy Bypass -File .\install.ps1
```

### En Linux / macOS (Bash)
```bash
chmod +x ./install.sh
./install.sh
```

### Configuración MCP (`claude_desktop_config.json` o configuración de IDE)
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
