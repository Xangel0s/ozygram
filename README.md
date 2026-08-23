# Ozymem / Ozygram — Cerebro Híbrido y Memoria Persistente para Agentes IA

> **Ozygram no solo recuerda; entiende el proyecto, aprende del usuario, anticipa riesgos, previene desviaciones de arquitectura y guía al agente con planes, memoria y criterio técnico.**

**Ozymem / Ozygram** es un motor de memoria persistente, grafo de código multi-lenguaje y razonamiento cognitivo para asistentes de código LLM vía **Model Context Protocol (MCP)**. Combina un núcleo en **Rust** de ultra-alta velocidad y baja latencia con un motor auxiliar de razonamiento en **Python (`ozy-brain`)**.

---

## 🚀 Capacidades Destacadas (v0.2.0)

- **⚡ Tabla Determinista de Engrams $O(1)$ (`rkyv` + `memmap2`)**: Búsqueda binaria de firmas y contratos de símbolos en $\approx 15\text{ ns}$ directamente en memoria mapeada sin allocations en el heap.
- **🔮 Speculative Context Decoding / Prefill Predictivo**: Inyección automática de dependencias adyacentes de primer orden y contratos relevantes en el prefill de prompts, reduciendo *roundtrips* del agente a cero y maximizando la tasa de acierto de prompt cache (>90%).
- **🛡️ Sandbox de Validación Test-Time en Bucle Cerrado (`ozy_verify_diff`)**: Verificación y comprobación sintáctica/tipada de diffs en $<200\text{ ms}$ antes de persistir cambios, retroalimentando a `reflector.py` para generar reglas procedimentales automáticas `[TRIGGER] -> [ACTION]`.
- **🤝 Memoria Distribuida P2P con Git Notes (`refs/notes/ozymem`)**: Sincronización descentralizada y portabilidad de lecciones y reglas aprendidas entre agentes/desarrolladores directamente en Git sin alterar el árbol de commits.
- **⚡ Live Delta Indexing & Watcher Reactivo**: Detección e indexación incremental en caliente (<50ms) sobre eventos de archivos con debouncing de 300ms, filtrado de ruido/bloqueos (`cargo.lock`, minificados, >256KB) y cálculo SHA-256 por archivo.
- **🛣️ Mapeo Sintético de Rutas HTTP (`ozymem_map_api_routes`)**: Extracción estructurada de endpoints y DTOs para **FastAPI**, **Express** y **Axum** sin dependencias externas.
- **🛡️ Detección Proactiva de Code Drift (`detect_code_drift`)**: Auditoría de diffs contra reglas de negocio y convenciones registradas (`record_convention`), alertando discrepancias antes de romper estándares.
- **📦 Portabilidad de Conocimiento (.ozymem Bundles)**: Exportación e importación (`ozymem export` / `ozymem import`) de memorias y rutas con verificación criptográfica SHA-256 y deduplicación inteligente.
- **🌐 Malla Multi-Repositorio (Cross-Repo Graph)**: Vinculación relacional de proyectos (`ozymem link`) y consultas transversales de memoria (`cross_repo_query`).
- **🧠 Razonamiento Guiado (`ozy_brain`)**: Motor cognitivo con 11 acciones de planificación, reflexión, revisión de riesgos, recall profundo y modelo mental.
- **🧩 Arquitectura Modular Desacoplada**: Submódulos independientes y testeables para schemas, graph, memory, git, unified, prompts, resources, verifier y brain.

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

### Garantías de Seguridad y Calidad
- **Rust tiene la Autoridad**: Administra SQLite, MCP stdio, lectura de archivos con backoff exponencial contra locks de Windows (`EBUSY`), indexación vectorial y validación.
- **Python es un Asesor Consultivo**: Sin permisos de escritura en la base de datos principal, sin ejecuciones de comandos arbitrarios sin confirmación.
- **146 / 146 Tests Automatizados (100% OK)**: Suite exhaustiva de pruebas en Rust (`ozymem-parser`, `ozymem-core`, `ozymem-server`, `ozymem-cli`) y Python (`ozy-brain`).

---

## 📚 Documentación Completa

Para profundizar en la arquitectura y herramientas de Ozygram:
- [**Índice General de Documentación**](docs/INDEX.md)
- [**Arquitectura Modular**](docs/architecture_modular.md)
- [**Sistema Engram, Prefill y Sandbox**](docs/engram_system.md)
- [**Catálogo de Herramientas MCP**](docs/mcp_tools.md)
- [**Novedades de la Versión v0.2.0**](docs/features_v02.md)
