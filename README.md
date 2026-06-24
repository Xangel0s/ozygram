# Ozymem-Partner 🚀

Ozymem-Partner es un motor de análisis arquitectónico políglota y un servidor de engramas de memoria basado en grafos de conocimiento, diseñado especialmente para el trabajo colaborativo en equipos de desarrollo. Utiliza una arquitectura unificada que conecta múltiples terminales a un cerebro centralizado en la nube (ej. Coolify) o localmente.

## Características de Ozymem-Partner

- **Arquitectura Colaborativa**: Si se configura con un host remoto HTTP/S, la CLI delega la persistencia del mapa de dependencias, definición de archivos y registro de lecciones (`record_lesson`) a través de APIs HTTP seguras.
- **Cerebro Compartido**: Las lecciones aprendidas y soluciones de errores aplicadas por un desarrollador están disponibles de forma instantánea para el resto del equipo en sus respectivos IDEs (vía MCP).
- **Multiplataforma Nativo**: Soporte certificado para Windows (PowerShell), Linux y macOS (Bash).
- **Modo Offline/Local**: Sigue permitiendo la conexión por defecto vía Bolt a una base de datos Memgraph local en caso de desarrollo aislado.

## Requisitos previos

Para poder ejecutar e instalar Ozymem-Partner localmente se requiere contar con:

- Rust (cargo, rustc en versión estable reciente)
- Docker (solo si ejecutas Memgraph de forma local)

## Instalación rápida

### Windows (PowerShell)
Abre PowerShell en la raíz del monorepo y ejecuta:
```powershell
Set-ExecutionPolicy Bypass -Scope Process -Force
.\init-ozymem.ps1
```

### Linux / macOS (Bash)
Abre tu terminal favorita y ejecuta:
```bash
chmod +x init-ozymem.sh
./init-ozymem.sh
```

## Configuración Colaborativa (`.ozymem.toml`)

Para conectar tu CLI al cerebro centralizado de tu equipo, edita el archivo `.ozymem.toml` ubicado en tu carpeta de usuario (Home):

```toml
current_brain = "central_brain"
token = "tu-token-seguro-mcp"

[brains.central_brain]
host = "https://tu-instancia-coolify.com"
port = 443
```

Si el host comienza con `http://` o `https://`, la CLI cambiará de inmediato al modo colaborativo HTTP/S autenticado mediante token.

## Arquitectura Hibrida de Memoria (Rust + Go + Memgraph + LanceDB)

Ozymem-Partner implementa una arquitectura hibrida para desacoplar las operaciones de escritura reactivas de las lecturas analiticas en el IDE:
1. **Escritura Determinista en Tiempo Real (Go Sidecar):** Un demonio recursivo en Go monitorea el workspace (`fsnotify`), invoca el analizador sintactico (AST) en Rust y almacena los recuerdos en una base de datos vectorial embebida (`.ozymem/vectors/vectors.json`) y el grafo de dependencias en Memgraph en milisegundos y con costo de tokens de indexacion $0.00.
2. **Lectura y Consultas MCP/CLI (Rust):** El servidor MCP y la CLI en Rust leen y consultan la base de datos vectorial embebida realizando busqueda de similitud de coseno con **Pre-filtrado de Metadatos** (`schema_version == 1` y filtro opcional por `category`).

---

## Panel de Control TUI Interactivo (`ozymem dashboard`)

Ozymem incluye una interfaz de terminal interactiva (TUI) profesional desarrollada con `ratatui` y `crossterm`. Puedes iniciarla ejecutando:
```bash
ozymem dashboard
```

### Funcionalidades y Navegacion por Pestanas:
- **Pestanas (Teclas `1`, `2`, `3` o `Tab`):**
  1. **Recuerdos (Memories):** Buscador de texto/semantico en tiempo real (`s`), olvido selectivo (`f`) para extirpar recuerdos obsoletos, y depuracion interactiva de huerfanos (`p`) con confirmacion visual. Soporta scroll de codigo con `,` y `.`.
  2. **Monitoreo (System Status):** Diagnostico del Docker, ping en vivo de Memgraph y visualizador/tail en tiempo real de los logs de los watchers activos para depurar sin salir de la TUI.
  3. **Graph PRs (GPR Audit):** Auditoria de solicitudes de integracion de grafos. Inspecciona el diff detallado de funciones y lecciones de cada PR y efectua fusiones (`m` para merge) directamente.
- **Salir:** Teclas `q` o `Esc`.
- **Refrescar Datos:** Tecla `r` para recargar logs, recuerdos o PRs desde la base de datos.

---

## Uso de la CLI

* **Escanear codigo**: `ozymem scan <directorio>` (agrega `--reset` para limpiar el grafo).
* **Ver estado**: `ozymem status` (topologia del grafo y watchers activos).
* **Bitacora de Lecciones**: `ozymem lessons --limit 10`.
* **Arbol de Dependencias**: `ozymem tree <archivo> --depth 2`.
* **Limpiar archivo del grafo**: `ozymem clean --path <archivo>`.
* **Buscar vectores (CLI)**: `ozymem vector search "<query>" --limit 5 --category <lesson|fact|context>`.
* **Listar vectores**: `ozymem vector list --category <categoria>`.
* **Inspeccionar recuerdo**: `ozymem vector inspect <id>`.
* **Eliminar recuerdo**: `ozymem vector forget <id>`.
* **Depurar huerfanos (CLI)**: `ozymem vector prune --apply`.
* **Top recuerdos**: `ozymem vector top` (muestra los recuerdos mas accedidos por la IA).

---

## Servidor MCP y Backend HTTP

### Servidor MCP Local (Stdio)
```bash
cargo run -p ozymem-server
```

### Backend API Colaborativo (Modo Web)
```bash
cargo run -p ozymem-server -- --web
```

---

## Despliegue en Produccion (Coolify)

Ozymem-Partner incluye soporte nativo para despliegue automatizado en Coolify o cualquier orquestador compatible con Docker Compose.

### Archivos de Despliegue
- [Dockerfile](file:///c:/Users/Lenovo/Documents/ozymem-partner/Dockerfile): Compila y empaqueta el binario de forma eficiente cacheando las dependencias.
- [docker-compose.prod.yml](file:///c:/Users/Lenovo/Documents/ozymem-partner/docker-compose.prod.yml): Coordina e integra la base de datos Memgraph y el servidor Axum de Ozymem.

### Pasos de Configuracion en Coolify:
1. Crea un nuevo recurso de tipo **Docker Compose** en tu proyecto de Coolify.
2. Apunta a tu repositorio de GitHub, usa la rama `main` y selecciona el archivo `docker-compose.prod.yml`.
3. Configura las siguientes variables de entorno en el panel de Coolify para el servicio `server`:
   - `OZYMEM_SERVER_MODE`: `web`
   - `MEMGRAPH_URI`: `memgraph:7687`
   - `MEMGRAPH_USER`: `admin`
   - `MEMGRAPH_PASSWORD`: `<contrasena_segura>`
   - `MEMGRAPH_DATABASE`: `memgraph`
4. Configura el puerto expuesto del servidor (`8080`) para que Coolify genere el proxy inverso con HTTPS automatico.
5. Haz clic en **Deploy**. Al realizar la primera consulta a `/api/health`, el servidor iniciara el *Setup Genesis* y mostrara tu credencial maestra en la terminal de logs en Coolify.
