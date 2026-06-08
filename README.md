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

## Uso de la CLI

Una vez instalado, la herramienta global `ozymem` te permite escanear proyectos, registrar lecciones e inspeccionar dependencias:

* **Escanear código**: `ozymem scan <directorio>` (agrega `--reset` para limpiar el grafo actual).
* **Ver estado**: `ozymem status` (muestra la topología del grafo y estado de los watchers de proyectos).
* **Bitácora de Lecciones**: `ozymem lessons --limit 10` para leer soluciones aplicadas por el equipo.
* **Árbol de Dependencias**: `ozymem tree <archivo> --depth 2`.
* **Limpiar archivo del grafo**: `ozymem clean --path <archivo>`.

## Servidor MCP y Backend HTTP

Para usar Ozymem-Partner en tu IDE (Cursor o Claude Desktop) o levantar el backend que servirá de API centralizada al equipo:

### Servidor MCP Local (Stdio)
```bash
cargo run -p ozymem-server
```

### Backend API Colaborativo (Modo Web)
Para arrancar el backend en la nube que recibe las sincronizaciones:
```bash
cargo run -p ozymem-server -- --web
```
*(O configurando la variable de entorno `OZYMEM_SERVER_MODE=web`)*.

---

## Despliegue en Producción (Coolify)

Ozymem-Partner incluye soporte nativo para despliegue automatizado en Coolify o cualquier orquestador compatible con Docker Compose.

### Archivos de Despliegue
- [Dockerfile](file:///c:/Users/Lenovo/Documents/ozymem-partner/Dockerfile): Compila y empaqueta el binario de forma eficiente cacheando las dependencias.
- [docker-compose.prod.yml](file:///c:/Users/Lenovo/Documents/ozymem-partner/docker-compose.prod.yml): Coordina e integra la base de datos Memgraph y el servidor Axum de Ozymem.

### Pasos de Configuración en Coolify:
1. Crea un nuevo recurso de tipo **Docker Compose** en tu proyecto de Coolify.
2. Apunta a tu repositorio de GitHub, usa la rama `main` y selecciona el archivo `docker-compose.prod.yml`.
3. Configura las siguientes variables de entorno en el panel de Coolify para el servicio `server`:
   - `OZYMEM_SERVER_MODE`: `web`
   - `MEMGRAPH_URI`: `memgraph:7687`
   - `MEMGRAPH_USER`: `admin`
   - `MEMGRAPH_PASSWORD`: `<contraseña_segura>`
   - `MEMGRAPH_DATABASE`: `memgraph`
4. Configura el puerto expuesto del servidor (`8080`) para que Coolify genere el proxy inverso con HTTPS automático.
5. Haz clic en **Deploy**. Al realizar la primera consulta a `/api/health`, el servidor iniciará el *Setup Génesis* y mostrará tu credencial maestra en la terminal de logs en Coolify.
