# Hybrid Semantic Search & RRF Fusion

Ozygram combines the precision of exact lexical matching with the deep contextual understanding of language models using a **Hybrid Search with Reciprocal Rank Fusion (RRF)** strategy.

---

## 1. The Dilemma: Lexical Search vs. Vector Search?

In real-world codebases, relying exclusively on either approach causes retrieval failures:

- **Pure Vector Search (Dense Retrieval)**:
  - Excellent for abstract conceptual queries (e.g., "authentication with expired tokens").
  - Fails on exact code identifiers (e.g., specific environment variables like `AUTH_JWT_EXP_SECS` or exact function names like `verify_jwt_token`).
- **Pure Lexical Search (BM25 / Full-Text Search)**:
  - Excellent for exact class names, methods, and error strings.
  - Fails when a developer or AI agent searches by intent without knowing the exact symbol name.

**Ozygram's Solution**: Execute both retrieval pipelines concurrently and fuse their ordered result lists using the mathematical **RRF (Reciprocal Rank Fusion)** algorithm.

---

## 2. Hybrid Search Architecture

```text
                     User / Agent Query
                              │
              ┌───────────────┴───────────────┐
              ▼                               ▼
    [Sparse Lexical Lane]           [Dense Semantic Lane]
   SQLite FTS5 (Native BM25)       FastEmbed (Local ONNX)
              │                               │
       Top-K Candidates                Top-K Candidates
              │                               │
              └───────────────┬───────────────┘
                              ▼
            [Reciprocal Rank Fusion (RRF)]
                              │
                              ▼
            Unified Re-Ranked List (Top-N)
```

---

## 3. Dense Lane: FastEmbed with ONNX Runtime

Unlike other systems that require heavy PyTorch installations (multiple gigabytes) or cloud API calls (such as OpenAI Embeddings):

- **FastEmbed**: Uses the optimized **ONNX Runtime in C++** with quantization for ultra-fast CPU inference.
- **Supported Models**:
  - `BAAI/bge-m3` (Dense + sparse multilingual support, 8,192 token context window, 1024 dimensions).
  - `BAAI/bge-base-en-v1.5` / `all-MiniLM-L6-v2` (Ultra-lightweight mode for minimal RAM footprint).
- **Token Consumption**: **Zero API tokens**. 100% of inference executes locally on your machine.
- **Storage**: Vector persistence in local **ChromaDB** collections located at `{project}/.ozymem/vector_store/`.

---

## 4. Fusion Algorithm: Reciprocal Rank Fusion (RRF)

The RRF algorithm normalizes candidate ranks across heterogeneous search engines without requiring cosine similarity scores and BM25 scores to share the same metric scale:

### Mathematical Formulation
$$RRF\_Score(d) = \sum_{m \in M} \frac{1}{k + r_m(d)}$$

Where:
- $d$: Candidate document or lesson.
- $M$: Set of search retrieval engines ($M = \{\text{BM25}, \text{Vector}\}$).
- $r_m(d)$: Rank (1-indexed position) of document $d$ in engine $m$. If absent from top results, its rank is treated as infinity.
- $k$: Smoothing constant (default $k = 60$, canonical standard in Information Retrieval literature) to prevent top positions from dominating the final score.

### Implementation in Ozygram ([`vector_store.py`](../python/ozy-brain/ozy_brain/vector_store.py))

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

## 5. Practical Benefits for AI Agents

1. **Terminology Mismatch Immunity**: If a developer refers to a problem as "serialization failure" while the codebase throws `serde::de::Error`, the dense matcher retrieves the semantic intent while the lexical engine reinforces the exact lesson.
2. **Zero Network Dependency**: The entire semantic search pipeline functions offline, in air-gapped corporate networks, or in high-security environments without internet access.
3. **Balanced Scoring**: A candidate present in the Top 3 of both engines will always outrank a false positive that appears at Top 1 in only one engine.
