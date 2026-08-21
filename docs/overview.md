# Arquitectura Global de Ozygram

Ozygram es una infraestructura de memoria y análisis estático de código de alto rendimiento, diseñada para operar localmente sin dependencias externas ni bases de datos pesadas en contenedores.

---

## 🏗️ Monorepo y Componentes

```
ozygram/
├── crates/
│   ├── ozymem-parser/       # AST Parsing multi-lenguaje (Tree-Sitter + Regex Fallbacks)
│   ├── ozymem-core/         # SQLite DB, Grafo Petgraph, FastEmbed, Registros
│   ├── ozymem-cli/          # CLI standalone y herramientas de terminal
│   └── ozymem-server/       # Servidor MCP stdio con JSON-RPC 2.0
├── python/
│   └── ozy-brain/           # Motor de razonamiento pesado (Advisory Worker)
└── docs/                    # Documentación técnica del proyecto
```

### 1. `ozymem-parser`
- Parser nativo basado en Tree-Sitter para **Rust**, **Python**, **JavaScript**, **TypeScript**, **Go** y **SQL**.
- Extracción de funciones, clases, métodos, rangos de líneas y dependencias de importación (`use`, `import`, `require`).
- **Novedad v0.2.0**: Extracción de diagnósticos y advertencias de sintaxis AST (`AstDiagnostic`) mediante recorrido de nodos de error.

### 2. `ozymem-core`
- **Almacenamiento**: Base de datos SQLite local (`{project}/.ozymem/memory.db`) para memorias por proyecto, y registro global en (`~/.ozymem/registry.db`).
- **Grafo**: Grafo dirigido en memoria (`petgraph::DiGraph`) que modela archivos como nodos y dependencias de importación como aristas.
- **Búsqueda Semántica**: Generación de embeddings con `fastembed` (`all-MiniLM-L6-v2`) para búsqueda vectorial por similitud coseno combinada con BM25 híbrido.
- **Watchers**: Sincronización en vivo con `notify` para indexación delta incremental de archivos modificados.

### 3. `ozymem-server`
- Implementa la especificación MCP sobre `stdio`.
- Expone más de 30 herramientas unificadas, 5 recursos (`ozymem://...`) y 3 prompts.
- Notificaciones push mediante `notifications/message` y soporte de subscripciones a recursos.

### 4. `ozy-brain` (Python)
- Servicio auxiliar de análisis cognitivo que apoya a los agentes en:
  - Planificación de tareas complejas (`plan`).
  - Reflexión post-ejecución (`reflect`).
  - Detección de riesgos y code drift (`risk_review`).
  - Construcción de modelos mentales del proyecto (`build_mental_model`).
