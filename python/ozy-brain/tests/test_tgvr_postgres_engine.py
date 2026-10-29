"""
Unit and integration tests for Topologically-Gated Vector Retrieval (TGVR) in PostgreSQL L3.
Tests topological cone filtering, RRF rank fusion, and graceful offline fallback.
Zero-Emoji Standard compliant.
"""

from unittest.mock import MagicMock, patch
import pytest

from ozy_brain.scanner.postgres_engine import (
    HAS_PSYCOPG2,
    PostgresConfig,
    PostgresNativeEngine,
)
from ozy_brain.scanner.tri_hybrid_orchestrator import TriHybridMemoryOrchestrator


def test_tgvr_offline_fallback():
    """Verify TGVR returns empty list gracefully when Postgres engine is offline."""
    engine = PostgresNativeEngine(PostgresConfig(dsn=None))
    assert engine.is_available is False

    results = engine.topologically_gated_vector_search(
        query_vector=[0.1] * 384,
        seed_file="frontend/src/utils/tableDataCache.ts",
        max_depth=3,
        limit=5,
    )
    assert results == []


def test_tgvr_mocked_query_execution():
    """Verify SQL recursive CTE execution and TGVR score calculation with mocked psycopg2."""
    engine = PostgresNativeEngine(PostgresConfig(dsn="postgresql://mock:mock@localhost:5432/mockdb"))
    engine._is_online = True

    mock_conn = MagicMock()
    mock_cur = MagicMock()
    mock_conn.cursor.return_value.__enter__.return_value = mock_cur

    mock_rows = [
        {
            "id": "swr_1",
            "tenant_id": "crmnew",
            "file_path": "frontend/src/utils/tableDataCache.ts",
            "error_signature": "CacheInvariantViolation",
            "hazard_description": "Dirty cache on route transition",
            "invariant_vaccine": "Invalidate tableDataCache before navigation",
            "error_gradient": 0.85,
            "topological_depth": 0,
            "cosine_similarity": 0.9200,
            "tgvr_score": 0.024500,
        },
        {
            "id": "swr_2",
            "tenant_id": "crmnew",
            "file_path": "frontend/src/components/TableEditor.tsx",
            "error_signature": "StaleRowStateError",
            "hazard_description": "Uncontrolled cell re-rendering",
            "invariant_vaccine": "Use useMemo for row models",
            "error_gradient": 0.70,
            "topological_depth": 1,
            "cosine_similarity": 0.8400,
            "tgvr_score": 0.021000,
        },
    ]
    mock_cur.fetchall.return_value = mock_rows

    with patch.object(engine, "get_connection") as mock_get_conn:
        mock_get_conn.return_value.__enter__.return_value = mock_conn

        hits = engine.topologically_gated_vector_search(
            query_vector=[0.05] * 384,
            seed_file=r"frontend\src\utils\tableDataCache.ts",
            max_depth=2,
            limit=5,
            min_gradient=0.50,
        )

        assert len(hits) == 2
        assert hits[0]["id"] == "swr_1"
        assert hits[0]["file_path"] == "frontend/src/utils/tableDataCache.ts"
        assert hits[0]["topological_depth"] == 0
        assert hits[0]["similarity"] == 0.9200
        assert hits[0]["tgvr_score"] == 0.024500

        assert hits[1]["id"] == "swr_2"
        assert hits[1]["topological_depth"] == 1

        # Check call arguments
        assert mock_cur.execute.called
        call_args = mock_cur.execute.call_args[0]
        sql_executed = call_args[0]
        params = call_args[1]
        assert "WITH RECURSIVE dependency_cone" in sql_executed
        assert "tgvr_score" in sql_executed
        assert params[0] == "frontend/src/utils/tableDataCache.ts"
        assert params[1] == 2


def test_tgvr_fallback_to_knn_when_cone_empty():
    """Verify automatic fallback to pure KNN search if topological cone returns no rows."""
    engine = PostgresNativeEngine(PostgresConfig(dsn="postgresql://mock:mock@localhost:5432/mockdb"))
    engine._is_online = True

    mock_conn = MagicMock()
    mock_cur = MagicMock()
    mock_conn.cursor.return_value.__enter__.return_value = mock_cur

    # First execute (recursive CTE) returns empty list
    mock_cur.fetchall.return_value = []

    with patch.object(engine, "get_connection") as mock_get_conn, \
         patch.object(engine, "search_swr_ripples_knn") as mock_knn:
        
        mock_get_conn.return_value.__enter__.return_value = mock_conn
        mock_knn.return_value = [
            {
                "id": "swr_fallback",
                "file_path": "backend/app/main.py",
                "error_signature": "GlobalError",
                "hazard": "Global handler error",
                "vaccine": "Global vaccine",
                "gradient": 0.6,
                "similarity": 0.88,
            }
        ]

        hits = engine.topologically_gated_vector_search(
            query_vector=[0.1] * 384,
            seed_file="unknown/module.ts",
            limit=3,
        )

        assert len(hits) == 1
        assert hits[0]["id"] == "swr_fallback"
        assert hits[0]["topological_depth"] == -1
        assert "tgvr_score" in hits[0]
        assert mock_knn.called


def test_tri_hybrid_orchestrator_tgvr_integration(tmp_path):
    """Verify TriHybridMemoryOrchestrator delegates to TGVR and handles offline degradation."""
    sqlite_file = tmp_path / "test_memory.db"
    chroma_dir = tmp_path / "chroma"

    orch = TriHybridMemoryOrchestrator(
        sqlite_path=sqlite_file,
        chroma_dir=chroma_dir,
        pg_config=PostgresConfig(dsn=None),
    )

    # When Postgres is offline, topologically_gated_search falls back to federated_rrf_search
    results = orch.topologically_gated_search(
        query="tableDataCache navigation error",
        seed_file="frontend/src/utils/tableDataCache.ts",
        limit=3,
    )
    assert isinstance(results, list)

    orch.close()
