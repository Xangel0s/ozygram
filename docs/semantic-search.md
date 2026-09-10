# Búsqueda Semántica Híbrida y Fusión RRF

Ozygram combina la precisión del emparejamiento léxico exacto con la comprensión semántica profunda de modelos de lenguaje mediante una estrategia de **Búsqueda Híbrida con Reciprocal Rank Fusion (RRF)**.

---

## 1. El Dilema: ¿Búsqueda Léxica o Búsqueda Vectorial?

En bases de código reales, depender exclusivamente de una sola técnica produce fallos:

- **Búsqueda Vectorial Pura (Dense Retrieval)**:
  - Excelente para conceptos abstractos ("autenticación con tokens expirados").
  - Falla en identificadores exactos de código (variables como `AUTH_JWT_EXP_SECS` o nombres de funciones exactas `verify_jwt_token`).
- **Búsqueda Léxica Pura (BM25 / Full-Text Search)**:
  - Excelente para nombres exactos de clases, métodos y errores.
  - Falla cuando el desarrollador o agente busca por intención sin recordar el nombre exacto del símbolo.

**Solución de Ozygram**: Ejecutar ambas en paralelo y fusionar sus listas ordenadas con el algoritmo matemático **RRF (Reciprocal Rank Fusion)**.

---

## 2. Arquitectura de Búsqueda Híbrida

```text
                     Consulta del Usuario / Agente
                                  │
                  ┌───────────────┴───────────────┐
                  ▼                               ▼
     [Carril Léxico Disperso]         [Carril Semántico Denso]
      SQLite FTS5 (BM25 Nativo)       FastEmbed (ONNX Local)
                  │                               │
         Top-K Candidatos                 Top-K Candidatos
                  │                               │
                  └───────────────┬───────────────┘
                                  ▼
                [Reciprocal Rank Fusion (RRF)]
                                  │
                                  ▼
                Lista Unificada Re-Rankeada (Top-N)
```

---

## 3. Carril Denso: FastEmbed con ONNX Runtime

A diferencia de otros sistemas que requieren descargar PyTorch pesado (varios gigabytes) o llamar a APIs de pago (como OpenAI Embeddings):

- **FastEmbed**: Utiliza el motor optimizado **ONNX Runtime en C++** con cuantización para inferencia en CPU de ultra-alta velocidad.
- **Modelos Soportados**:
  - `BAAI/bge-m3` (Soporte multilingüe denso + disperso, ventana de 8,192 tokens, 1024 dimensiones).
  - `BAAI/bge-base-en-v1.5` / `all-MiniLM-L6-v2` (Modo ultra-ligero para bajo consumo de RAM).
- **Consumo de Tokens**: **Cero tokens de API**. 100% de la inferencia ocurre en tu máquina local.
- **Almacenamiento**: Persistencia vectorial en colecciones locales de **ChromaDB** en `{project}/.ozymem/vector_store/`.

---

## 4. Algoritmo de Fusión: Reciprocal Rank Fusion (RRF)

El algoritmo RRF normaliza las posiciones de los candidatos de ambas fuentes sin requerir que las puntuaciones de similitud de coseno y las de BM25 estén en la misma escala métrica:

### Fórmula Matemática
$$RRF\_Score(d) = \sum_{m \in M} \frac{1}{k + r_m(d)}$$

Donde:
- $d$: Documento o lección candidata.
- $M$: Conjunto de motores de búsqueda ($M = \{\text{BM25}, \text{Vectorial}\}$).
- $r_m(d)$: Rango (posición 1-indexada) del documento $d$ en el motor $m$. Si no aparece en los primeros resultados de ese motor, su rango es tratado como infinito.
- $k$: Constante de suavizado (por defecto $k = 60$, estándar de la literatura académica de Information Retrieval) para evitar que las primeras posiciones monopolicen la puntuación.

### Implementación en Ozygram ([`vector_store.py`](../python/ozy-brain/ozy_brain/vector_store.py))

```python
def reciprocal_rank_fusion(
    bm25_results: list[dict[str, Any]],
    vector_results: list[dict[str, Any]],
    k: int = 60,
) -> list[dict[str, Any]]:
    scores: dict[str, float] = {}
    doc_map: dict[str, dict[str, Any]] = {}

    for rank, item in enumerate(bm25_results):
        doc_id = str(item.get("id"))
        doc_map[doc_id] = item
        scores[doc_id] = scores.get(doc_id, 0.0) + 1.0 / (k + rank + 1)

    for rank, item in enumerate(vector_results):
        doc_id = str(item.get("id"))
        if doc_id not in doc_map:
            doc_map[doc_id] = item
        scores[doc_id] = scores.get(doc_id, 0.0) + 1.0 / (k + rank + 1)

    sorted_docs = sorted(scores.items(), key=lambda x: x[1], reverse=True)
    return [doc_map[doc_id] for doc_id, _ in sorted_docs]
```

---

## 5. Ventajas Prácticas para el Agente

1. **Inmunidad a Desfases Terminológicos**: Si el desarrollador llama a un error "falla en serialización" y el código dice `serde::de::Error`, el emparejador denso lo captura mientras el léxico refuerza la lección exacta.
2. **Cero Dependencia de Red**: Todo el pipeline de búsqueda semántica funciona en aviones, redes corporativas aisladas o entornos de alta seguridad sin acceso a internet.
3. **Puntuaciones Equilibradas**: Un elemento que esté en el Top 3 de ambos motores siempre rankeará por encima de un falso positivo que esté en el Top 1 de solo uno de ellos.
