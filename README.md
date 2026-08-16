# Ozymem / Ozygram — Cerebro Híbrido y Memoria Persistente para Agentes IA

> **Ozygram no solo recuerda; entiende el proyecto, aprende del usuario, anticipa riesgos, previene desviaciones de arquitectura y guía al agente con planes, memoria y criterio técnico.**

**Ozymem / Ozygram** es un motor de memoria persistente, grafo de código multi-lenguaje y razonamiento cognitivo para asistentes de código LLM vía **Model Context Protocol (MCP)**. Combina un núcleo en **Rust** de ultra-alta velocidad y baja latencia con un motor auxiliar de razonamiento en **Python (`ozy-brain`)**.

---

## 🚀 Capacidades Destacadas (v0.1.0)

- **⚡ Live Delta Indexing & Watcher Reactivo**: Detección e indexación incremental en caliente (<50ms) sobre eventos de archivos con debouncing de 300ms, filtrado de ruido/bloqueos (`cargo.lock`, minificados, >256KB) y cálculo SHA-256 por archivo.
- **🛣️ Mapeo Sintético de Rutas HTTP (`ozymem_map_api_routes`)**: Extracción estructurada de endpoints y DTOs para **FastAPI**, **Express** y **Axum** sin dependencias externas.
- **🛡️ Detección Proactiva de Code Drift (`detect_code_drift`)**: Auditoría de diffs contra reglas de negocio y convenciones registradas (`record_convention`), alertando discrepancias antes de romper estándares.
- **📦 Portabilidad de Conocimiento (.ozymem Bundles)**: Exportación e importación (`ozymem export` / `ozymem import`) de memorias y rutas con verificación criptográfica SHA-256 y deduplicación inteligente.
- **🌐 Malla Multi-Repositorio (Cross-Repo Graph)**: Vinculación relacional de proyectos (`ozymem link`) y consultas transversales de memoria (`cross_repo_query`).
- **🧠 Memoria Cognitiva de Dos Capas (Working Cache vs Engram Store)**: Extracción de tríadas `[Sujeto -> Relación -> Objeto]` y consolidación a largo plazo activada por hitos (`git commit`, `test pass`, `bugfix`).
- **🔍 Adaptador de Tipado Semántico (`ozymem_python_typecheck`)**: Inferencia profunda de contratos Python / Pydantic con Pyrefly o validación AST nativa.

---

## 💡 ¿Qué Facilita, Mejora y Reduce Ozygram?

### 🚀 ¿Qué FACILITA a los Agentes de IA?
- **Memoria Contextual Permanente**: Almacena decisiones de arquitectura, lecciones aprendidas, convenciones, gotchas y reglas por módulo en SQLite local (`{project}/.ozymem/memory.db`).
- **Planes de Trabajo Seguros**: Genera planes guiados en 5 fases con puntuación de archivos candidatos, condiciones de parada y checklists de verificación.
- **Recall Profundo**: Recupera contexto relevante agrupado semánticamente sin saturar la ventana de tokens.
- **Navegación en Grafo de Código**: Encuentra dependientes, dependencias y caminos de impacto entre archivos vía `petgraph` y `tree-sitter`.
- **Navegación CLI Ultra-Rápida (`ozymem q`)**: Consultas compactas destiladas tipo shell sin riesgo de ejecución arbitraria.

### 📈 ¿Qué MEJORA en el Desarrollo?
- **Precisión de las Respuestas del Agente**: La IA toma decisiones informadas basadas en decisiones pasadas reales y no en suposiciones.
- **Calidad y Mantenibilidad del Código**: Aplica activamente principios **SOLID**, **DRY** y **KISS** en las recomendaciones del planificador.
- **Previsibilidad de Cambios Transversales**: Clasifica el impacto en 4 niveles de severidad (`[BREAKING]`, `[WARN]`, `[INFO]`) antes de tocar código.
- **Continuidad entre Sesiones**: Mantiene un modelo mental vivo del proyecto disponible entre reinicios o traspasos de contexto.

### 🛡️ ¿Qué REDUCE y Previene?
- **Reduce Alucinaciones de la LLM**: Proporciona evidencias empíricas desde el grafo y SQLite.
- **Elimina la Repetición de Errores Pasados**: Recupera automáticamente gotchas y lecciones anteriores sobre los mismos módulos.
- **Reduce el Desperdicio de Tokens**: Evita leer repositorios enteros o incluir `node_modules/`, `target/` y archivos ruidosos.
- **Previene Pérdidas de Datos o Borrados Accidentales**: Python opera en modo estrictamente consultivo/asesor; Rust exige confirmación explícita para acciones destructivas.
- **Reduce el Tiempo de Onboarding del Agente**: El comando `build_mental_model` indica exactamente *"Dónde mirar primero"*.

---

## ⚡ Instalación Rápida

### Windows (PowerShell)

```powershell
powershell -ExecutionPolicy Bypass -File .\install.ps1
```

El instalador:
1. Compila los binarios optimizados de release (`ozymem.exe` y `ozymem-server.exe`).
2. Instala los ejecutables en `%USERPROFILE%\.ozymem\bin` y los añade a tu `PATH`.
3. Instala el motor `ozy-brain` en `%USERPROFILE%\.ozymem\python\ozy-brain`.
4. Muestra la configuración exacta para copiar en tu cliente MCP.

### Linux / macOS (Bash)

```bash
chmod +x ./install.sh
./install.sh
```

---

## ⚙️ Configuración MCP Server (`mcpServers`)

Agrega Ozygram a tu cliente MCP favorito (Antigravity IDE, Claude Desktop, Cursor, VS Code):

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

## 🧠 Arquitectura Híbrida Rust / Python

```text
MCP Client (Antigravity, Claude, Cursor)
   ↓
ozymem-server (Rust — Autoridad MCP, SQLite, FastEmbed, AST Tree-Sitter, CLI)
   ↓ (Stdio JSON Payload)
ozy-brain (Python — Worker de Razonamiento Pesado en ozy_brain/)
   ↓ (Stdio JSON Response)
ozymem-server (Rust — Formateador y Sanitizador Markdown)
   ↓
MCP Client
```

### Garantías de Seguridad
- **Rust tiene la Autoridad**: Administra SQLite, MCP stdio, lectura de archivos con backoff exponencial contra locks de Windows (`EBUSY`), indexación vectorial y validación.
- **Python es un Asesor Consultivo**: `ozy-brain` no puede borrar archivos directamente, no hace commits, no hace push y no ejecuta comandos arbitrarios sin pasar por Rust.

---

## 🛠️ Herramientas MCP Disponibles

```rust
// Core y Memoria
ozy_brain                 // Razonamiento pesado: plan, reflect, risk_review, consolidate_engrams, build_mental_model
ozy_context               // Contexto de tarea, archivos, resúmenes y memorias recientes
ozy_memory                // Guardar/buscar lecciones, decisiones, convenciones, gotchas y reglas

// Análisis y Diagnóstico
ozy_graph                 // Grafo de código, vecinos, análisis de impacto y dependencias
ozymem_map_api_routes     // Extracción automática de rutas HTTP (FastAPI, Express, Axum)
detect_code_drift         // Detección de drift entre commits y convenciones guardadas
rank_memories             // Evaluación de vigencia y ranking temporal de memorias
ozymem_python_typecheck   // Inferencia semántica y tipado estricto Python / Pyrefly
ozy_code_doctor           // Diagnóstico preview-safe, duplicados y autosanado
ozy_doctor                // Salud de base de datos, embeddings e índices

// Portabilidad y Multi-Repo
export_knowledge_bundle   // Exportación de paquete portable .ozymem con SHA-256
import_knowledge_bundle   // Importación con deduplicación y verificación de integridad
cross_repo_query          // Búsqueda de memorias a través de múltiples repositorios
link_projects             // Vinculación de dependencias entre proyectos registrados
```

---

## 🔍 Ozy CLI Commands

```powershell
# Búsqueda y Navegación Rápida
ozymem q grep auth                # Buscar símbolos o lecciones
ozymem q find GraphBackend        # Buscar definición de símbolos
ozymem q ctx "refactor cotizador" # Contexto priorizado de tarea
ozymem q trace src/main.rs        # Camino de impacto en el grafo

# Portabilidad y Multi-Repo
ozymem export --output backup.ozymem  # Exportar paquete de conocimiento
ozymem import backup.ozymem --merge   # Importar con deduplicación
ozymem link --target api-backend      # Vincular proyecto relacionado
ozymem link --list                    # Listar enlaces multi-repo

# Watcher y Sincronización
ozymem watch                      # Iniciar watcher reactivo en primer plano
ozymem scan .                     # Escaneo completo e indexación del grafo
ozymem doctor                     # Diagnóstico de salud del entorno
```

---

## 📊 Recursos y Prompts MCP

### Recursos
- `ozymem://summary` — Resumen completo del proyecto.
- `ozymem://recent-lessons` — Últimas lecciones registradas.
- `ozymem://full-context` — Bundle completo (resumen + archivos + lecciones).
- `ozymem://file/{path}` — Contexto enriquecido de un archivo.
- `ozymem://file/{path}/neighbors` — Dependencias directas e indirectas.

### Prompts
- `analyze-file` — Análisis profundo de archivo e impacto.
- `review-lessons` — Revisión de lecciones registradas.
- `project-status` — Estado general del proyecto y salud.

---

## 🧪 Pruebas y Validación

```powershell
# Ejecutar todas las pruebas del workspace en Rust (134 tests)
cargo test --workspace

# Ejecutar las pruebas del motor de razonamiento Python
python -m unittest discover -s python/ozy-brain/tests -v

# Ejecutar el arnés de evaluación dorada (Golden Eval)
python python/ozy-brain/tests/eval.py
```

---

## 📄 Licencia y Requisitos

- **SO**: Windows, Linux, macOS (x86_64 y ARM64 Apple Silicon).
- **Requisitos**: Rust stable (edición 2021) y Python 3.9+.
- **Cero Dependencias Externas**: SQLite integrado, sin necesidad de Docker ni bases de datos remotas.
