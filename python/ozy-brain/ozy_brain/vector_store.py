from __future__ import annotations

import os
import re
from pathlib import Path
from typing import Any

try:
    import chromadb
    from chromadb.config import Settings
    HAS_CHROMADB = True
except ImportError:
    HAS_CHROMADB = False

try:
    from fastembed import TextEmbedding
    HAS_FASTEMBED = True
except ImportError:
    HAS_FASTEMBED = False


class VectorMemoryStore:
    """Persistent ChromaDB vector store for Ozy-Brain.

    Provides dense semantic embeddings, topic collections, and cross-similarity reranking.
    """

    def __init__(self, persist_dir: str | Path | None = None):
        self.persist_dir = Path(persist_dir or Path.home() / ".ozymem" / "chroma")
        self.persist_dir.mkdir(parents=True, exist_ok=True)
        self.client = None
        self.collection = None

        if HAS_CHROMADB:
            try:
                self.client = chromadb.PersistentClient(
                    path=str(self.persist_dir),
                    settings=Settings(anonymized_telemetry=False),
                )
                self.collection = self.client.get_or_create_collection(
                    name="ozy_lessons",
                    metadata={"hnsw:space": "cosine"},
                )
            except Exception:
                self.client = None
                self.collection = None

        self._embedder = None
        if HAS_FASTEMBED:
            try:
                self._embedder = TextEmbedding(model_name="BAAI/bge-small-en-v1.5")
            except Exception:
                self._embedder = None

    def is_available(self) -> bool:
        return self.client is not None and self.collection is not None

    def close(self):
        """Releases Chroma client references for safe cleanup on Windows."""
        self.collection = None
        if self.client is not None:
            del self.client
            self.client = None

    def upsert_lesson(
        self,
        lesson_id: str,
        content: str,
        metadata: dict[str, Any] | None = None,
    ) -> bool:
        """Upserts a sanitized memory into the vector collection."""
        if not self.is_available():
            return False

        clean_meta: dict[str, str | int | float | bool] = {}
        if metadata:
            for k, v in metadata.items():
                if isinstance(v, (str, int, float, bool)):
                    clean_meta[k] = v
                elif v is not None:
                    clean_meta[k] = str(v)

        try:
            self.collection.upsert(
                ids=[str(lesson_id)],
                documents=[content],
                metadatas=[clean_meta] if clean_meta else None,
            )
            return True
        except Exception:
            return False

    def delete_item(self, entity_id: str) -> bool:
        """Deletes a document from the vector collection by ID."""
        if not self.is_available():
            return False
        try:
            self.collection.delete(ids=[str(entity_id)])
            return True
        except Exception:
            return False

    def search_similar(
        self,
        query: str,
        limit: int = 5,
        where: dict[str, Any] | None = None,
    ) -> list[dict[str, Any]]:
        """Queries ChromaDB for semantic similarity using cosine distance."""
        if not self.is_available():
            return []

        try:
            count = self.collection.count()
            if count == 0:
                return []

            results = self.collection.query(
                query_texts=[query],
                n_results=min(limit, count),
                where=where,
            )
            output: list[dict[str, Any]] = []
            if results and results.get("ids") and results["ids"][0]:
                ids = results["ids"][0]
                docs = results.get("documents", [[]])[0]
                metas = results.get("metadatas", [[]])[0]
                distances = results.get("distances", [[]])[0]

                for i, doc_id in enumerate(ids):
                    doc = docs[i] if i < len(docs) else ""
                    meta = metas[i] if i < len(metas) else {}
                    dist = distances[i] if distances and i < len(distances) else 0.5
                    sim_score = max(0.0, min(1.0, 1.0 - dist))
                    output.append({
                        "id": doc_id,
                        "content": doc,
                        "metadata": meta,
                        "distance": dist,
                        "similarity_score": round(sim_score, 4),
                    })
            return output
        except Exception:
            return []

    def rerank(
        self,
        query: str,
        candidates: list[dict[str, Any]],
        top_k: int = 5,
        min_score: float = 0.40,
    ) -> list[dict[str, Any]]:
        """Cross-relevance re-ranker that evaluates semantic overlap and token proximity,

        filtering out noise and boosting definitive architectural rules.
        """
        if not candidates:
            return []

        query_tokens = set(re.findall(r"\w{3,}", query.lower()))
        scored: list[dict[str, Any]] = []

        for item in candidates:
            content = item.get("content", "")
            base_score = item.get("similarity_score", 0.5)
            content_tokens = set(re.findall(r"\w{3,}", content.lower()))

            # Jaccard / token affinity
            overlap = query_tokens.intersection(content_tokens)
            token_affinity = len(overlap) / max(1, len(query_tokens))

            # Penalty for high noise/code dump indicators
            noise_penalty = 0.0
            if "Traceback" in content or "at Object." in content:
                noise_penalty = 0.4
            if "|" in content and content.count("|") > 4:
                noise_penalty = 0.3

            # Boost for verified architectural metadata
            arch_boost = 0.0
            meta = item.get("metadata", {})
            if meta.get("kind") in ("architecture", "decision", "convention"):
                arch_boost = 0.15

            final_score = max(0.0, min(1.0, (base_score * 0.55) + (token_affinity * 0.45) + arch_boost - noise_penalty))

            if final_score >= min_score:
                item_copy = dict(item)
                item_copy["rerank_score"] = round(final_score, 4)
                item_copy["matched_tokens"] = list(overlap)
                scored.append(item_copy)

        scored.sort(key=lambda x: x["rerank_score"], reverse=True)
        return scored[:top_k]


def reciprocal_rank_fusion(
    bm25_items: list[dict[str, Any]],
    vector_items: list[dict[str, Any]],
    k: int = 60,
    top_k: int = 10,
) -> list[dict[str, Any]]:
    """Merges lexical (BM25/FTS5) and dense vector rankings using Reciprocal Rank Fusion (RRF).

    Formula: RRF_score(d) = sum(1 / (k + rank(d)))
    """
    scores: dict[str, float] = {}
    items_by_key: dict[str, dict[str, Any]] = {}

    def get_key(item: dict[str, Any]) -> str:
        if "id" in item and item["id"]:
            return str(item["id"])
        if "file_path" in item and "symbol_name" in item:
            return f"{item['file_path']}::{item['symbol_name']}"
        return str(hash(item.get("content", "")))

    for rank, item in enumerate(bm25_items, start=1):
        key = get_key(item)
        items_by_key[key] = item
        scores[key] = scores.get(key, 0.0) + (1.0 / (k + rank))

    for rank, item in enumerate(vector_items, start=1):
        key = get_key(item)
        if key not in items_by_key:
            items_by_key[key] = item
        scores[key] = scores.get(key, 0.0) + (1.0 / (k + rank))

    sorted_keys = sorted(scores.keys(), key=lambda x: scores[x], reverse=True)
    fused: list[dict[str, Any]] = []
    for key in sorted_keys[:top_k]:
        merged_item = dict(items_by_key[key])
        merged_item["rrf_score"] = round(scores[key], 5)
        fused.append(merged_item)

    return fused
