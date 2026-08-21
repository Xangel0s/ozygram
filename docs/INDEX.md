# Documentación de Ozygram / Ozymem

Bienvenido a la documentación oficial y detallada de **Ozygram** (v0.2.0), el sistema de grafo de dependencias de código, memoria contextual y razonamiento híbrido para desarrollo asistido por IA mediante el Model Context Protocol (MCP).

---

## 📚 Índice de Secciones

1. [**Arquitectura Global (`docs/overview.md`)**](overview.md)
   - Componentes del monorepo (`ozymem-core`, `ozymem-parser`, `ozymem-cli`, `ozymem-server`, `ozy-brain`).
   - Esquemas de almacenamiento SQLite (por proyecto y registro global).
   - Motor de embeddings semánticos y sincronización en tiempo real.

2. [**Referencia de Herramientas MCP (`docs/mcp_tools.md`)**](mcp_tools.md)
   - Guía exhaustiva de los 30+ endpoints MCP.
   - Herramientas de memoria (`ozy_memory`, `record_lesson`, `record_decision`, `record_convention`, `record_gotcha`, `record_module_rule`).
   - Herramientas de grafo e impacto (`ozy_graph`, `analyze_impact`, `graph_neighbors`, `file_context`).
   - Herramientas de diagnóstico (`ozy_doctor`, `ozy_code_doctor`, `detect_code_drift`).
   - Motor de razonamiento (`ozy_brain`).
   - Herramientas de registro y monorepo (`ozy_project`, `export_knowledge_bundle`, `import_knowledge_bundle`, `link_projects`).

3. [**Novedades de Ozygram v0.2.0 (`docs/features_v02.md`)**](features_v02.md)
   - Resolución de rutas en cascada (rutas relativas, normalizadas y coincidencia de sufijo).
   - Fallback automático a símbolos AST cuando no existen memorias explícitas.
   - Clasificación inteligente de duplicados (`[High-Priority Refactor Candidates]` vs `[Structural Boilerplate]`).
   - Diagnósticos estáticos y linter de sintaxis AST mediante Tree-Sitter.
   - Soporte de subdirectorios / subpath para monorepos (`subpath`).

---

## 🚀 Instalación y Uso Rápido

### En Windows (PowerShell)
```powershell
.\install.ps1
```

### En Linux / macOS (Bash)
```bash
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
