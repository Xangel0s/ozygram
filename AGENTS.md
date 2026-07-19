# Ozymem Developer Agent Guidelines

## Arquitectura y Monorepo
- `crates/ozymem-core`: Almacenamiento persistente (SQLite), indexación multi-lenguaje (tree-sitter), búsqueda semántica (fastembed), análisis de grafo (petgraph).
- `crates/ozymem-parser`: Parsers de código fuente estructurado (Python, Go, Rust, JS/TS, SQL).
- `crates/ozymem-cli`: Herramienta de línea de comandos del ecosistema.
- `crates/ozymem-server`: Servidor de comunicación MCP.

## Principios y Convenciones
- **SOLID, DRY, KISS**: Mantener el código acoplado lo mínimo posible, extraer lógica reutilizable y no sobrediseñar.
- **Git y Commits**: Realizar commits limpios por característica siguiendo la convención de `conventional commits` (`feat:`, `fix:`, `docs:`, `test:`, `refactor:`).
- **Pruebas**: Cobertura de pruebas superior al 80% en cualquier funcionalidad nueva. Ejecutar `cargo test` antes de dar por completado cualquier desarrollo.
- **Cambios en Código**: Solicitar confirmación del usuario mostrando un diff descriptivo de los cambios antes de editarlos en disco.
