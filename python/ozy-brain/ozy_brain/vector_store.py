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
    import importlib
    _fastembed_mod = importlib.import_module("fastembed")
    TextEmbedding = getattr(_fastembed_mod, "TextEmbedding", None)
    HAS_FASTEMBED = TextEmbedding is not None
except Exception:
    HAS_FASTEMBED = False
    TextEmbedding = None


class VectorMemoryStore:
    """Persistent ChromaDB vector store for Ozy-Brain.

    Provides dense semantic embeddings, 4 specialized neuro-cognitive collections:
    1. episodic_raw: Ingestion buffer for MCP calls and raw errors.
    2. code_graph_ast: Embeddings of functions/classes enriched with AST metadata.
    3. semantic_heuristics: Distilled rules and architectural gotchas.
    4. dream_insights: Proactive audit reports and recommendations.
    """

    PRIMARY_COLLECTIONS = (
        "episodic_raw",
        "code_graph_ast",
        "semantic_heuristics",
        "dream_insights",
    )

    def __init__(self, persist_dir: str | Path | None = None):
        self.persist_dir = Path(persist_dir or Path.home() / ".ozymem" / "chroma")
        self.persist_dir.mkdir(parents=True, exist_ok=True)
        self.client = None
        self.collection = None
        self._collections: dict[str, Any] = {}

        if HAS_CHROMADB:
            try:
                self.client = chromadb.PersistentClient(
                    path=str(self.persist_dir),
                    settings=Settings(anonymized_telemetry=False),
                )
                for name in self.PRIMARY_COLLECTIONS:
                    self._collections[name] = self.client.get_or_create_collection(
                        name=name,
                        metadata={"hnsw:space": "cosine"},
                    )
                # Backward-compatibility alias
                self.collection = self._collections.get("semantic_heuristics") or self.client.get_or_create_collection(
                    name="ozy_lessons",
                    metadata={"hnsw:space": "cosine"},
                )
                self._collections["ozy_lessons"] = self.collection
            except Exception:
                self.client = None
                self.collection = None
                self._collections = {}

        self._embedder = None
        if HAS_FASTEMBED:
            try:
                self._embedder = TextEmbedding(model_name="BAAI/bge-small-en-v1.5")
            except Exception:
                self._embedder = None

    def is_available(self) -> bool:
        return self.client is not None and self.collection is not None

    def get_collection(self, name: str) -> Any | None:
        """Retrieves or creates a named ChromaDB collection."""
        if not self.client:
            return None
        if name in self._collections:
            return self._collections[name]
        try:
            coll = self.client.get_or_create_collection(
                name=name,
                metadata={"hnsw:space": "cosine"},
            )
            self._collections[name] = coll
            return coll
        except Exception:
            return None

    def close(self):
        """Releases Chroma client references for safe cleanup on Windows."""
        self.collection = None
        self._collections.clear()
        if self.client is not None:
            del self.client
            self.client = None

    def upsert_item(
        self,
        collection_name: str,
        item_id: str,
        content: str,
        metadata: dict[str, Any] | None = None,
    ) -> bool:
        """Upserts a sanitized memory document into a target collection."""
        coll = self.get_collection(collection_name)
        if coll is None:
            return False

        clean_meta: dict[str, str | int | float | bool] = {}
        if metadata:
            for k, v in metadata.items():
                if isinstance(v, (str, int, float, bool)):
                    clean_meta[k] = v
                elif v is not None:
                    clean_meta[k] = str(v)

        try:
            coll.upsert(
                ids=[str(item_id)],
                documents=[content],
                metadatas=[clean_meta] if clean_meta else None,
            )
            return True
        except Exception:
            return False

    def upsert_lesson(
        self,
        lesson_id: str,
        content: str,
        metadata: dict[str, Any] | None = None,
    ) -> bool:
        """Upserts a sanitized memory into semantic_heuristics (and ozy_lessons)."""
        ok = self.upsert_item("semantic_heuristics", lesson_id, content, metadata)
        if self.collection is not None and self.collection.name == "ozy_lessons":
            self.upsert_item("ozy_lessons", lesson_id, content, metadata)
        return ok

    def ingest_episodic_event(
        self,
        event_id: str,
        content: str,
        metadata: dict[str, Any] | None = None,
    ) -> bool:
        """Ingests raw MCP tool calls and terminal logs into episodic_raw."""
        return self.upsert_item("episodic_raw", event_id, content, metadata)

    def upsert_code_ast(
        self,
        symbol_id: str,
        code_content: str,
        metadata: dict[str, Any] | None = None,
    ) -> bool:
        """Upserts function/class embedding enriched with AST metadata into code_graph_ast."""
        return self.upsert_item("code_graph_ast", symbol_id, code_content, metadata)

    def upsert_heuristic(
        self,
        rule_id: str,
        content: str,
        metadata: dict[str, Any] | None = None,
    ) -> bool:
        """Upserts distilled architectural rule into semantic_heuristics."""
        return self.upsert_item("semantic_heuristics", rule_id, content, metadata)

    def upsert_insight(
        self,
        insight_id: str,
        content: str,
        metadata: dict[str, Any] | None = None,
    ) -> bool:
        """Upserts proactive diagnostic insight into dream_insights."""
        return self.upsert_item("dream_insights", insight_id, content, metadata)

    def delete_item(self, entity_id: str, collection_name: str | None = None) -> bool:
        """Deletes a document from the vector collection by ID."""
        coll = self.get_collection(collection_name) if collection_name else self.collection
        if coll is None:
            return False
        try:
            coll.delete(ids=[str(entity_id)])
            return True
        except Exception:
            return False

    def search_similar(
        self,
        query: str,
        limit: int = 5,
        where: dict[str, Any] | None = None,
        collection_name: str | None = None,
    ) -> list[dict[str, Any]]:
        """Queries ChromaDB for semantic similarity using cosine distance."""
        coll = self.get_collection(collection_name) if collection_name else self.collection
        if coll is None:
            return []

        try:
            count = coll.count()
            if count == 0:
                return []

            results = coll.query(
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
