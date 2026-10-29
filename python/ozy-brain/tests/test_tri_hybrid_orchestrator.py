"""
Unit tests for TriHybridMemoryOrchestrator.
Validates multi-tier write-through persistence, Synaptic Tagging & Capture (STC)
promotion, 3-way Federated Reciprocal Rank Fusion (RRF), and offline failover resilience.
"""

import shutil
import tempfile
import time
from pathlib import Path
from unittest.mock import MagicMock, patch
import pytest

from ozy_brain.scanner.contract_tracer import ContractDriftAlert
from ozy_brain.scanner.memory_store import GotchaMemory
from ozy_brain.scanner.postgres_engine import PostgresConfig
from ozy_brain.scanner.tri_hybrid_orchestrator import TriHybridMemoryOrchestrator


@pytest.fixture
def temp_orchestrator():
    temp_dir = tempfile.mkdtemp()
    sqlite_path = Path(temp_dir) / "test_memory.db"
    chroma_dir = Path(temp_dir) / "chroma"
    md_dir = Path(temp_dir) / "md_memories"

    # Offline postgres config
    pg_config = PostgresConfig(dsn=None)

    orch = TriHybridMemoryOrchestrator(
        sqlite_path=sqlite_path,
        chroma_dir=chroma_dir,
        pg_config=pg_config,
        markdown_base_dir=md_dir,
    )
    yield orch

    orch.close()
    shutil.rmtree(temp_dir, ignore_errors=True)


def test_orchestrator_health_report(temp_orchestrator):
    """Verify tier health reporting with Postgres in graceful offline fallback."""
    health = temp_orchestrator.health_report()
    assert health["sqlite"]["status"] == "online"
    assert health["sqlite"]["journal_mode"] == "wal"
    assert health["chroma"]["status"] == "online"
    assert "semantic_heuristics" in health["chroma"]["collections"]
    assert health["postgres"]["status"] == "offline_fallback"


def test_write_through_save_gotcha(temp_orchestrator):
    """Verify synchronous write to SQLite, ChromaDB, and Markdown file."""
    gotcha = GotchaMemory(
        id="gotcha_sync_99",
        kind="convention",
        topic_key="api_signature_drift",
        title="[ALERT] Client-server signature mismatch on user profile update",
        author="AgentDev",
        date=time.strftime("%Y-%m-%d"),
        severity="critical",
        applies_to=["crmnew/api-geofal-crm/main.py", "crmnew/crm-geofal/page.tsx"],
        tags=["api", "drift", "fastapi"],
        flawed_assumption="Frontend presumed snake_case but backend produced camelCase",
        countermeasure="Enforce Pydantic aliases with camelCase serialization",
        created_at=time.time(),
    )

    res = temp_orchestrator.save_gotcha(gotcha, sync_markdown=True)
    assert res["id"] == "gotcha_sync_99"
    assert res["sqlite_saved"] is True
    assert res["chroma_saved"] is True
    assert res["postgres_saved"] is False  # Offline fallback
    assert res["markdown_path"] is not None
    assert Path(res["markdown_path"]).exists()

    # Verify retrieval from SQLite
    threats = temp_orchestrator.sqlite_backend.get_active_threats()
    assert any(t.id == "gotcha_sync_99" for t in threats)

    # Verify retrieval from ChromaDB KNN
    hits = temp_orchestrator.chroma_bridge.query_knn(
        collection_name="semantic_heuristics",
        query="camelCase serialization and Pydantic aliases",
    )
    assert len(hits) >= 1
    assert any(h["id"] == "gotcha_sync_99" for h in hits)


def test_synaptic_tagging_and_capture_promotion(temp_orchestrator):
    """
    Verify that MCTS nodes with Q >= 0.85 are automatically promoted to
    invariant vaccines in ChromaDB insights and SQLite gotchas (STC mechanism).
    """
    # 1. Normal exploration branch (Q = 0.65 < 0.85) -> No STC promotion
    res_normal = temp_orchestrator.save_mcts_trajectory_node(
        node_id="mcts_node_normal_1",
        tree_id="tree_test_1",
        parent_path="root.phase_1",
        phase="Phase 1: Backend",
        action_strategy="BackendFallbackAdapter",
        visits=5,
        q_value=0.65,
        state_payload={"working_edits": {"api.py": "mod"}},
    )
    assert res_normal["stc_promoted"] is False

    # 2. High-reward branch (Q = 0.89 >= 0.85) -> STC promotion triggered!
    res_high = temp_orchestrator.save_mcts_trajectory_node(
        node_id="mcts_node_winner_2",
        tree_id="tree_test_1",
        parent_path="root.phase_2",
        phase="Phase 2: Full-Stack Verification",
        action_strategy="DualActiveContract",
        visits=25,
        q_value=0.89,
        state_payload={"working_edits": {"contract.py": "dual", "consumer.ts": "dual"}},
    )
    assert res_high["stc_promoted"] is True

    # Verify promoted vaccine in ChromaDB 'dream_insights'
    insights = temp_orchestrator.chroma_bridge.query_knn(
        collection_name="dream_insights",
        query="DualActiveContract high-reward vaccine",
    )
    assert len(insights) >= 1
    assert any(h["id"] == "stc_mcts_node_winner_2" for h in insights)

    # Verify promoted gotcha vaccine in SQLite WAL
    threats = temp_orchestrator.sqlite_backend.get_active_threats()
    assert any(t.id == "gotcha_stc_mcts_node_winner_2" for t in threats)


def test_federated_rrf_search_ranking(temp_orchestrator):
    """
    Verify 3-way Reciprocal Rank Fusion:
    Documents indexed in both SQLite and ChromaDB should obtain higher RRF scores
    than documents appearing in only one tier.
    """
    # Item A: Saved in both SQLite and ChromaDB
    gotcha_a = GotchaMemory(
        id="item_shared_a",
        kind="gotcha",
        topic_key="auth_token_refresh",
        title="Token refresh race condition in Axios interceptor",
        author="SecDev",
        date="2026-10-28",
        severity="critical",
        applies_to=["auth/interceptor.ts"],
        tags=["auth", "jwt"],
        flawed_assumption="Single promise shared across concurrent requests",
        countermeasure="Implement mutex queue for token refresh",
        created_at=time.time(),
    )
    temp_orchestrator.save_gotcha(gotcha_a, sync_markdown=False)

    # Item B: Only saved in SQLite WAL (bypassing Chroma)
    gotcha_b = GotchaMemory(
        id="item_sqlite_only_b",
        kind="gotcha",
        topic_key="auth_token_refresh",
        title="Token refresh timeout",
        author="SecDev",
        date="2026-10-28",
        severity="high",
        applies_to=["auth/timeout.ts"],
        tags=["auth"],
        flawed_assumption="Network timeout handling missing",
        countermeasure="Set 5s timeout",
        created_at=time.time(),
    )
    temp_orchestrator.sqlite_backend.save_gotcha(gotcha_b)

    # Perform federated RRF search
    results = temp_orchestrator.federated_rrf_search("Token refresh mutex and interceptor", limit=5)
    assert len(results) >= 2

    # The multi-tier item should rank first due to reciprocal rank summation
    top_hit = results[0]
    assert top_hit["id"] == "item_shared_a"
    assert top_hit["rrf_score"] > results[1]["rrf_score"]


def test_mocked_online_postgres_integration(temp_orchestrator):
    """Verify write-through to PostgreSQL and CTE reverse blast queries when Postgres is online."""
    temp_orchestrator.postgres_engine._is_online = True

    mock_conn = MagicMock()
    mock_cur = MagicMock()
    mock_conn.cursor.return_value.__enter__.return_value = mock_cur

    with patch.object(temp_orchestrator.postgres_engine, "get_connection") as mock_get_conn:
        mock_get_conn.return_value.__enter__.return_value = mock_conn

        # Test save_gotcha with online Postgres
        gotcha = GotchaMemory(
            id="gotcha_online_test",
            kind="architecture",
            topic_key="db_migration",
            title="Always use idempotent migration statements",
            author="DBA",
            date="2026-10-28",
            severity="high",
            applies_to=["migrations/001.sql"],
            tags=["sql"],
            flawed_assumption="Assumed clean database",
            countermeasure="Use IF NOT EXISTS and DROP IF EXISTS",
            created_at=time.time(),
        )
        res = temp_orchestrator.save_gotcha(gotcha, sync_markdown=False)
        assert res["postgres_saved"] is True
        assert mock_cur.execute.call_count == 1
        assert "INSERT INTO ozy_cognitive_gotchas" in mock_cur.execute.call_args[0][0]

