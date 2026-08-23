# Arquitectura Modular de Ozygram (v0.2.0)

Este documento describe la organización modular del monorepo, enfocándose en la descomposición de `ozymem-server`, `ozymem-core` y la integración con el worker `ozy-brain`.

---

## 1. Módulos del Monorepo

```text
Ozygram
├── crates/
│   ├── ozymem-core/       # Motor de persistencia SQLite, grafo petgraph, fastembed y engram store
│   │   ├── src/
│   │   │   ├── graph_backend/   # Submódulos: indexing, queries, lessons, sqlite_backend, git_notes
│   │   │   ├── engram_store/    # Lógica de serialización rkyv y FastEngramReader
│   │   │   ├── git_backend.rs   # Operaciones git2 y git notes
│   │   │   ├── registry.rs      # Registro global ~/.ozymem/registry.db
│   │   │   └── mcp_common.rs    # Tipos e interfaces comunes JSON-RPC
│   ├── ozymem-parser/     # Parsers AST Tree-Sitter nativos multi-lenguaje (Rust, Python, TS/JS, Go)
│   │   ├── src/
│   │   │   ├── engram.rs        # Extracción de contratos de símbolos
│   │   │   ├── api_routes.rs    # Detección de endpoints FastAPI, Express, Axum
│   │   │   └── excel_parser.rs  # Heurística y candidate detection
│   ├── ozymem-cli/        # CLI estructurada con comandos (scan, dashboard, projects, etc.)
│   └── ozymem-server/     # Servidor MCP stdio modular de alta concurrencia
│       ├── src/
│       │   ├── dispatch.rs      # Router central JSON-RPC (<300 líneas)
│       │   ├── schemas.rs       # Definiciones de esquema y paginación para tools/list
│       │   ├── graph.rs         # Herramientas de contexto de archivo, impacto y dependencias
│       │   ├── memory.rs        # Registro de lecciones, decisiones, gotchas y búsqueda BM25
│       │   ├── unified.rs       # Suite de herramientas compuestas ozy_*
│       │   ├── git.rs           # Integración git y sincronización de Git Notes
│       │   ├── prompts.rs       # Protocolo de prompts MCP y autocompletado
│       │   ├── resources.rs     # Recursos MCP y suscripciones en tiempo real
│       │   ├── brain.rs         # Puente hacia el worker de razonamiento ozy-brain
│       │   ├── verifier.rs      # Sandbox de comprobación rápida de diffs
│       │   ├── doctor.rs        # Diagnósticos del sistema y chequeo de salud
│       │   ├── packages.rs      # Gestión de paquetes y dependencias
│       │   ├── projects.rs      # Gestión del ciclo de vida de proyectos
│       │   └── state.rs         # Notificaciones, logging y estado global
└── python/
    └── ozy-brain/         # Worker de razonamiento cognitivo (planner, reflector, risk_review)
```

---

## 2. Principios de Modularidad
- **Bajo Acoplamiento**: Cada submódulo MCP maneja su propio subconjunto de herramientas o métodos JSON-RPC (`handle_*_tool`), retornando `Ok(Some(...))` o `Ok(None)` para encadenamiento limpio.
- **Archivos < 1,000 líneas**: Mantenimiento estricto de límites de tamaño para facilitar la auditoría y extensibilidad.
- **Cobertura Exhaustiva**: Integración de 146 tests unitarios y de integración (134 Rust + 12 Python) validados en CI multiplataforma.
