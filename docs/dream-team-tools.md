# Herramientas de Alto Rendimiento ("The Dream Team")

Ozygram se integra con un conjunto selecto de herramientas de alto rendimiento escritas en Rust para formar el **"Dream Team"** de asistencia a agentes: velocidad instantánea, mínimo consumo de memoria y ahorro agresivo de tokens.

---

## 1. Composición del Dream Team

```text
 ┌──────────────────┬────────────────────────────────────────────────────────┐
 │ Herramienta      │ Rol Estratégico en Ozygram                             │
 ├──────────────────┼────────────────────────────────────────────────────────┤
 │ tgrep            │ Búsqueda instantánea de expresiones regulares con     │
 │ (Microsoft)      │ índices trigram indexados para bases de código masivas.│
 ├──────────────────┼────────────────────────────────────────────────────────┤
 │ rtk              │ Rust Token Killer: Compresión agresiva de payloads,   │
 │ (rtk-ai)         │ eliminación de caracteres ANSI y minimización de JSON. │
 ├──────────────────┼────────────────────────────────────────────────────────┤
 │ fastembed        │ Embeddings densos locales con ONNX Runtime en C++     │
 │                  │ (cero dependencia de PyTorch y cero costo de API).     │
 └──────────────────┴────────────────────────────────────────────────────────┘
```

---

## 2. `tgrep` — Búsqueda Trigram de Expresiones Regulares

Desarrollado por Microsoft, `tgrep` acelera las búsquedas complejas mediante índices trigram invertidos:

### ¿Por qué es superior para Ozygram?
- **Indexación Trigram**: Mientras que herramientas tradicionales como `grep` escanean cada línea secuencialmente, `tgrep` descarta el 95% de los archivos irrelevantes antes de evaluar la expresión regular.
- **Rendimiento en Proyectos Gigantes**: Ideal para monorepos con cientos de miles de líneas donde el agente necesita localizar firmas o patrones de llamada en milisegundos.

### Uso en Terminal
```powershell
# Búsqueda por patrón de expresión regular
tgrep "fn [a-z_]+\(ctx: &Context\)"

# Búsqueda insensible a mayúsculas
tgrep -i "class SupervisorAgent"
```

---

## 3. `rtk` (Rust Token Killer) — Compresión de Tokens para LLMs

Desarrollado por `rtk-ai`, `rtk` es una herramienta especializada en reducir el volumen de tokens que se inyectan en las ventanas de contexto de los modelos:

### Capacidades Clave
1. **Sanitización de Secuencias de Escape**: Elimina códigos de color ANSI, caracteres de control de terminal y secuencias de escape que inflan el conteo de tokens sin aportar valor semántico.
2. **Minificación Inteligente de Estructuras**: Compacta respuestas JSON, logs y salidas de terminal manteniendo intacta la información semántica.
3. **Ahorro de Ventana de Contexto**: Reduce entre un 20% y un 45% el uso de tokens en prompts y transcripciones de herramientas.

### Uso en Terminal
```powershell
# Filtrar y comprimir la salida de un comando largo
cargo test | rtk

# Limpiar un log ruidoso antes de enviarlo a un LLM
Get-Content build.log | rtk
```

---

## 4. Sinergia con Ozygram

Cuando un agente LLM trabaja con Ozygram:
1. **`ozymem-server`** suministra el contexto estructurado de la memoria y el grafo de dependencias en <5 ms.
2. **`tgrep`** permite al agente rastrear patrones transversales en el código a velocidad trigram sin saturar el sistema de archivos.
3. **`rtk`** limpia y compacta cualquier salida densa de terminal o linter antes de que llegue a la ventana de contexto del LLM.
4. **`fastembed`** garantiza que las búsquedas semánticas y vectoriales se resuelvan de forma local sin consumir tokens ni generar gastos de API.
