# Ozymem / Ozygram — Cerebro Híbrido y Memoria Persistente para Agentes IA

> **Ozygram no solo recuerda; entiende el proyecto, aprende del usuario, anticipa riesgos y guía al agente con planes, memoria y criterio técnico.**

**Ozymem / Ozygram** es un motor de memoria persistente, grafo de código y razonamiento pesado para asistentes de código LLM vía **Model Context Protocol (MCP)**. Combina un núcleo en **Rust** de ultra-alta velocidad y baja latencia con un motor auxiliar de razonamiento en **Python (`ozy-brain`)**.

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

## ⚡ Instalación Fácil para Release

### Windows (PowerShell)

Ejecuta el instalador automático para compilar e instalar Ozygram en tu sistema:

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

## ⚙️ Configuración MCP Server (`mcp_servers`)

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
MCP Client
   ↓
ozymem-server (Rust — Autoridad MCP, SQLite, Grafo, Seguridad, CLI)
   ↓ (Stdio JSON Payload)
ozy-brain (Python — Worker de Razonamiento Pesado en ozy_brain/)
   ↓ (Stdio JSON Response)
ozymem-server (Rust — Formateador Markdown)
   ↓
MCP Client
```

### Garantías de Seguridad (Sección 7)
- **Rust tiene la Autoridad**: Administra SQLite, MCP stdio, lectura de archivos, validación y compatibilidad en Windows.
- **Python es un Asesor Consultivo**: `ozy-brain` no puede borrar archivos directamente, no hace git commits, no hace push, no ejecuta comandos arbitrarios y no modifica SQLite sin pasar por Rust.

---

## 🛠️ Tools MCP Principales (8 Unificadas)

```rust
ozy_brain        // Cerebro híbrido: plan, reflect, recall_deep, risk_review, build_mental_model, detect_patterns
ozy_context      // Contexto de tarea, archivos, resúmenes y memorias recientes
ozy_memory       // Guardar/buscar lecciones, decisiones, convenciones, gotchas y reglas de módulo
ozy_graph        // Grafo de código, vecinos, análisis de impacto, caminos y reporte de arquitectura
ozy_code_doctor  // Diagnóstico preview-safe, duplicados, hotspots y autosanado
ozy_doctor       // Salud de base de datos, registro de proyectos, embeddings e índices
ozy_skills       // Metadata review-only de skills oficiales para mejores prácticas
ozy_project      // Gestión de proyectos, paquetes, scripts, ignore rules y actualización de índice
```

### Acciones de `ozy_brain` (11)

- `plan`: Plan estructurado en 5 fases con puntuación de archivos candidatos, validación y *"Qué No Tocar"*.
- `reflect` / `analyze_failure`: Análisis de causa raíz y detección de scope creep.
- `risk_review`: Evaluación de riesgos de Auth, Pérdida de Datos, Migraciones SQL y Refactorizaciones.
- `build_mental_model`: Síntesis de arquitectura, módulos, flujos de control y *"Dónde mirar primero"*.
- `recall_deep`: Recuperación priorizada con clustering semántico y deduplicación.
- `rank_memories`: Clasificación y orden de relevancia de memorias pasadas.
- `summarize_project`: Resumen técnico compacto y estadísticas del grafo.
- `detect_patterns`: Detección de patrones del usuario, reglas de dominio CRM y restricciones de plataforma.
- `suggest_next_steps`: Recomendación de siguientes hitos seguros.
- `compress_session`: Compactación de contexto de sesión antes de resets.

---

## 🔍 Ozy Query CLI (`ozymem q`)

Traductor seguro de comandos tipo shell para agentes de IA sin ejecución de shell externo:

```powershell
ozymem q grep auth                # Buscar símbolos o lecciones
ozymem q find GraphBackend        # Buscar definición de símbolos
ozymem q ctx "refactor cotizador" # Contexto priorizado de tarea
ozymem q file crates/server.rs    # Contexto enriquecido de archivo
ozymem q trace crates/server.rs   # Camino de impacto en el grafo
ozymem q arch                     # Reporte de arquitectura del proyecto
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

## 📄 Licencia y Requisitos

- **SO**: Windows, Linux, macOS.
- **Requisitos**: Rust stable (edición 2021) y Python 3.9+.
- **Cero Dependencias Externas**: SQLite integrado, sin necesidad de Docker ni bases de datos remotas.
