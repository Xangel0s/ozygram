"""
Unit tests for ChromaVectorBridge.
Validates 384-dimensional embedding generation, unit normalization,
ChromaDB collection management, cross-collection KNN retrieval, and Windows handle safety.
"""

import math
import shutil
import tempfile
from pathlib import Path
import pytest

from ozy_brain.scanner.chroma_bridge import ChromaVectorBridge, HAS_CHROMADB


def test_deterministic_embedding_properties():
    """Verify deterministic embedding dimensions, determinism, and unit L2 norm."""
    text1 = "FastAPI endpoint /api/v1/projects requires authorization bearer token"
    text2 = "FastAPI endpoint /api/v1/projects requires authorization bearer token"
    text3 = "Next.js React Server Component fetcher"

    vec1 = ChromaVectorBridge.deterministic_embedding(text1, dim=384)
    vec2 = ChromaVectorBridge.deterministic_embedding(text2, dim=384)
    vec3 = ChromaVectorBridge.deterministic_embedding(text3, dim=384)

    assert len(vec1) == 384
    assert len(vec2) == 384
    assert len(vec3) == 384

    # Exact determinism
    assert vec1 == vec2
    assert vec1 != vec3

    # Unit L2 norm (||v||_2 = 1.0)
    norm1 = math.sqrt(sum(x * x for x in vec1))
    norm3 = math.sqrt(sum(x * x for x in vec3))
    assert abs(norm1 - 1.0) < 1e-5
    assert abs(norm3 - 1.0) < 1e-5

    # Empty string fallback
    empty_vec = ChromaVectorBridge.deterministic_embedding("", dim=384)
    assert len(empty_vec) == 384
    assert empty_vec[0] == 1.0
    assert abs(math.sqrt(sum(x * x for x in empty_vec)) - 1.0) < 1e-5


@pytest.mark.skipif(not HAS_CHROMADB, reason="ChromaDB not installed")
def test_chroma_bridge_lifecycle_and_knn():
    """Verify Chroma collections, upserting, and cosine KNN search."""
    temp_dir = tempfile.mkdtemp()
    bridge = ChromaVectorBridge(persist_dir=temp_dir)

    try:
        assert bridge.is_available is True
        for coll_name in bridge.ALL_COLLECTIONS:
            coll = bridge.get_collection(coll_name)
            assert coll is not None

        # Upsert into semantic_heuristics
        ok1 = bridge.upsert_document(
            collection_name=bridge.COLLECTION_HEURISTICS,
            doc_id="rule_crmnew_01",
            content="Never modify customer schema without backward compatible aliases in crmnew",
            metadata={"project": "crmnew", "severity": "high", "active": True},
        )
        assert ok1 is True

        # Upsert into dream_insights
        ok2 = bridge.upsert_document(
            collection_name=bridge.COLLECTION_INSIGHTS,
            doc_id="insight_crmnew_02",
            content="Cross-repo drift detected between FastAPI router and TanStack Table column keys",
            metadata={"project": "crmnew", "severity": "critical", "active": True},
        )
        assert ok2 is True

        # KNN Query in semantic_heuristics
        hits = bridge.query_knn(
            collection_name=bridge.COLLECTION_HEURISTICS,
            query="customer schema modifications",
            limit=5,
            where={"project": "crmnew"},
        )
        assert len(hits) == 1
        assert hits[0]["id"] == "rule_crmnew_01"
        assert hits[0]["similarity_score"] > 0.0
        assert hits[0]["metadata"]["project"] == "crmnew"

        # Cross-collection query
        cross_hits = bridge.query_cross_collections(
            collections=[bridge.COLLECTION_HEURISTICS, bridge.COLLECTION_INSIGHTS],
            query="customer and cross-repo schema drift",
            limit_per_coll=2,
            where={"project": "crmnew"},
        )
        assert len(cross_hits) >= 1
        # Results should be sorted descending by similarity_score
        scores = [h["similarity_score"] for h in cross_hits]
        assert scores == sorted(scores, reverse=True)

    finally:
        bridge.close()
        shutil.rmtree(temp_dir, ignore_errors=True)
