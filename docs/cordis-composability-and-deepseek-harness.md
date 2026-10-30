# Arquitectura de Componibilidad Estilo Cordis e Integración con DeepSeek Harness

## 1. Visión y Soberanía Tecnológica

Ozygram está diseñado con una premisa de **independencia absoluta, máxima velocidad (< 1 ms) y cero dependencias externas frágiles**. Todo el núcleo del sistema funciona como un binario único autocontenido (`ozygram.exe`), con SQLite WAL embebido, Tree-Sitter AST y embeddings Count-Sketch 384D en C/Rust.

El framework **DeepSeek Harness (`dsh`)** (desarrollado por [DeepSeek AI](https://github.com/deepseek-ai/deepseek-harness)) ofrece una propuesta modular de orquestación de agentes. Sin embargo, al encontrarse en versión **`0.2.2-alpha`**, acoplar el núcleo de Ozygram directamente a sus paquetes internos (`packages/`) introduciría vulnerabilidad ante *breaking changes* continuos.

### La Estrategia Óptima:
1. **Mantener Soberanía**: No adoptar `dsh` como dependencia obligatoria ni de bajo nivel.
2. **Adoptar el Patrón Cordis**: Replicar la arquitectura de **inyección de dependencias reactiva y efectos reversibles** que Cordis utiliza en DeepSeek Harness, permitiendo que los módulos, skills y memorias de Ozygram se carguen y descarguen en caliente sin reiniciar el servidor.
3. **Integración Universal vía MCP**: Exponer todas las capacidades de Ozygram a `dsh` a través del estándar abierto **Model Context Protocol (MCP)**, permitiendo que cualquier agente de DeepSeek aproveche el grafo y la memoria de Ozygram de inmediato.

---

## 2. Los Fundamentos del Patrón Cordis

El framework **Cordis** (creado originalmente en el ecosistema Koishi por Shigma y utilizado por DeepSeek Harness) propone un paradigma denominado **Componibilidad Espacio-Temporal**:

### A. Componibilidad Espacial (Reactive Coeffects)
En lugar de inicializar servicios en un orden rígido o depender de variables globales, los plugins declaran reactivamente los servicios que requieren:
- Un módulo de MCTS masivo o minería tensorial declara: `inject: ["ast_graph", "cuda"]`.
- Cuando el usuario conmuta la configuración a GPU (`ozygram config set compute.acceleration cuda`), el servicio `cuda` se vuelve disponible en el contexto y el plugin se monta automáticamente.
- Si se conmuta a `cpu` (`compute.acceleration cpu`), el servicio `cuda` se desactiva y el sistema degrada de inmediato al motor ligero de CPU sin arrojar excepciones.

### B. Componibilidad Temporal (Revertible Effects & LIFO Teardown)
Los sistemas de plugins convencionales suelen dejar *event listeners*, hilos o conexiones huérfanas al descargarse. Cordis soluciona esto registrando cada efecto junto con su función de desmontaje (`dispose`):
- Al descargar o actualizar un plugin, todos los listeners, temporizadores y conectores registrados se revierten exactamente en orden inverso (LIFO).
- Esto permite la recarga en caliente (**Hot Module Replacement**) de memorias, conectores y reglas heurísticas sin detener el proceso principal.

---

## 3. Implementación del Patrón en Ozygram

### En Rust (`ozymem-core`)
Ozygram implementa la modularidad reactiva a través del registro de servicios con guardas RAII:
```rust
pub trait LifecycleService: Send + Sync {
    fn name(&self) -> &'static str;
    fn dependencies(&self) -> &'static [&'static str];
    fn start(&mut self, ctx: &mut ServiceContext) -> anyhow::Result<()>;
    fn stop(&mut self) -> anyhow::Result<()>;
}
```
- **Servicios Modulares**:
  - `AstGraphService`: Parser incremental Tree-Sitter y cálculo de *blast radius*.
  - `MemoryDbService`: SQLite WAL local con índices FTS5.
  - `HardwareAccelerationService`: Gestión de modos `auto`, `cpu` y `cuda`.
  - `MctsEngineService`: Simulador contrafáctico en memoria RAM.

### En Python (`ozy-brain`)
- El comité deliberativo multi-agente (`Supervisor`, `RiskCritic`, `SecurityAuditor`) se monta sobre un `DynamicContext`.
- La actualización de vacunas Gotchas en `.ozy/memories/gotchas/*.md` o reglas de inmunidad se recarga en caliente sin reiniciar el worker IPC.

---

## 4. Integración Directa con DeepSeek Harness (`dsh`)

Cualquier proyecto o agente configurado con DeepSeek Harness puede utilizar Ozygram como su sustrato cognitivo de memoria y grafo de dependencias en 1 minuto:

### Configuración en `dsh` (vía MCP stdio)
En el archivo de configuración de plugins de `dsh` (ej. `dsh.config.json` o equivalente en el harness):

```json
{
  "plugins": {
    "mcp": {
      "servers": {
        "ozygram": {
          "command": "ozygram",
          "args": ["serve"]
        }
      }
    }
  }
}
```

### Capacidades Inmediatas que Ozygram Aporta a `dsh`:
1. **Grafo AST y Blast Radius**: Las 25 herramientas MCP oficiales (`ozy_context`, `ozy_graph`, `ozy_patch`, `ozy_doctor`, `ozy_memory`) quedan disponibles para que los modelos de DeepSeek (DeepSeek-V3, DeepSeek-Coder-V2) conozcan las dependencias de cada archivo antes de editar.
2. **Simulación en RAM (Ghost Patches)**: Capacidad de probar parches en memoria efímera sin tocar el disco (`ozy_simulate_patch`).
3. **Memoria Persistente de Vacunas (Gotchas)**: Prevención automática de errores históricos documentados en el repositorio.

---

## 5. Cita y Referencias

- **DeepSeek Harness**:
  > DeepSeek AI. (2026). *DeepSeek Harness (`dsh`): A Modular Agent Runtime and Evaluation Framework*.
  > Repositorio oficial: [https://github.com/deepseek-ai/deepseek-harness](https://github.com/deepseek-ai/deepseek-harness)
  > Directorio de paquetes modulares: [https://github.com/deepseek-ai/deepseek-harness/tree/master/packages](https://github.com/deepseek-ai/deepseek-harness/tree/master/packages)
- **Cordis Framework**:
  > Shigma et al. (2024-2026). *Cordis: The Modular Meta-Framework for TypeScript*.
  > Documentación y especificación: [https://cordis.moe](https://cordis.moe)
