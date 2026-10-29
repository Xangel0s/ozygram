"""
Unit tests for PostgreSQL Native Engine with pgvector and ltree.
Validates formatting, offline fallback resilience, MCTS forest persistence,
and SQL execution contracts without requiring an external running PostgreSQL instance.
"""

from unittest.mock import MagicMock, patch
import pytest

from ozy_brain.scanner.postgres_engine import (
    HAS_PSYCOPG2,
    MctsForestRepository,
    PostgresConfig,
    PostgresNativeEngine,
)


def test_postgres_config_initialization(monkeypatch):
    """Test PostgresConfig configuration from explicit parameters and environment."""
    cfg = PostgresConfig(dsn="postgresql://user:pass@localhost:5432/testdb", min_conn=2, max_conn=5)
    assert cfg.dsn == "postgresql://user:pass@localhost:5432/testdb"
    assert cfg.min_conn == 2
    assert cfg.max_conn == 5

    monkeypatch.setenv("OZY_PG_DSN", "postgresql://envuser:envpass@remote:5432/envdb")
    cfg_env = PostgresConfig.from_env()
    assert cfg_env.dsn == "postgresql://envuser:envpass@remote:5432/envdb"


def test_format_ltree_path_sanitization():
    """Verify that file paths and hierarchy names are safely transformed into valid ltree labels."""
    assert PostgresNativeEngine.format_ltree_path("root/phase1/task-2") == "root.phase1.task_2"
    assert PostgresNativeEngine.format_ltree_path(r"app\services\auth.py") == "app.services.auth.py"
    assert PostgresNativeEngine.format_ltree_path("...invalid--name...") == "invalid_name"
    assert PostgresNativeEngine.format_ltree_path("") == "root"


def test_format_vector():
    """Verify vector serialization for pgvector syntax."""
    vec = [0.1234567, -0.9876543, 0.0]
    formatted = PostgresNativeEngine.format_vector(vec)
    assert formatted == "[0.123457,-0.987654,0.000000]"


def test_offline_fallback_when_no_dsn():
    """Verify engine degrades gracefully to offline mode if no DSN is provided."""
    engine = PostgresNativeEngine(PostgresConfig(dsn=None))
    health = engine.check_connection()
    assert health["is_online"] is False
    assert health["status"] == "offline_fallback"
    assert engine.is_available is False

    # All methods return safe fallbacks when offline
    assert engine.execute_ddl()["status"] == "skipped"
    assert engine.insert_swr_ripple("r1", "file.py", "err", "haz", "vac", 0.9, [0.1] * 384) == ""
    assert engine.search_swr_ripples_knn([0.1] * 384) == []
    assert engine.query_reverse_blast_radius("target.py") == []

    repo = MctsForestRepository(engine)
    assert repo.save_node("n1", "t1", "root", "p1", "strat", 1, 0.8, {}) == "n1"
    assert repo.query_tree_trajectory("t1") == []


def test_offline_fallback_on_connection_error():
    """Verify engine returns offline fallback if host is unreachable."""
    if not HAS_PSYCOPG2:
        pytest.skip("psycopg2 not installed")

    # Point to an invalid local port that will fail immediately
    engine = PostgresNativeEngine(PostgresConfig(dsn="postgresql://baduser:badpass@127.0.0.1:59999/nodb", connect_timeout=1))
    health = engine.check_connection()
    assert health["is_online"] is False
    assert health["status"] == "offline_fallback"


def test_mocked_ddl_and_swr_insertion():
    """Verify query formatting and execution through mocked psycopg2 connection."""
    engine = PostgresNativeEngine(PostgresConfig(dsn="postgresql://mock:mock@localhost:5432/mockdb"))
    engine._is_online = True

    mock_conn = MagicMock()
    mock_cur = MagicMock()
    mock_conn.cursor.return_value.__enter__.return_value = mock_cur

    with patch.object(engine, "get_connection") as mock_get_conn:
        mock_get_conn.return_value.__enter__.return_value = mock_conn

        # Test execute_ddl
        res = engine.execute_ddl()
        assert res["status"] == "ok"
        assert res["statements_executed"] == len(PostgresNativeEngine.DDL_STATEMENTS)
        assert mock_cur.execute.call_count == len(PostgresNativeEngine.DDL_STATEMENTS)

        # Test SWR insertion
        mock_cur.reset_mock()
        ripple_id = engine.insert_swr_ripple(
            ripple_id="swr_test_1",
            file_path="app/api/endpoints.py",
            error_signature="ContractMismatchError",
            hazard_desc="Client expects dict but received list",
            invariant_vaccine="Always return dict with status key",
            error_gradient=0.92,
            embedding=[0.05] * 384,
            tenant_id="crmnew",
        )
        assert ripple_id == "swr_test_1"
        assert mock_cur.execute.call_count == 1
        args = mock_cur.execute.call_args[0]
        assert "INSERT INTO ozy_swr_ripples" in args[0]
        assert args[1][0] == "swr_test_1"
        assert args[1][6] == 0.92


def test_mocked_mcts_forest_repository():
    """Verify MCTS node saving and hierarchical trajectory queries with ltree."""
    engine = PostgresNativeEngine(PostgresConfig(dsn="postgresql://mock:mock@localhost:5432/mockdb"))
    engine._is_online = True
    repo = MctsForestRepository(engine)

    mock_conn = MagicMock()
    mock_cur = MagicMock()
    mock_conn.cursor.return_value.__enter__.return_value = mock_cur

    with patch.object(engine, "get_connection") as mock_get_conn:
        mock_get_conn.return_value.__enter__.return_value = mock_conn

        # Save node
        node_id = repo.save_node(
            node_id="node_123",
            tree_id="tree_abc",
            parent_path="root/phase_1",
            phase="Phase 1: Backend",
            action_strategy="DualActiveContract",
            visits=15,
            q_value=0.875,
            state_payload={"edits": ["file1.py"]},
            projected_churn=2,
        )
        assert node_id == "node_123"
        assert mock_cur.execute.call_count == 1
        sql_called = mock_cur.execute.call_args[0][0]
        params_called = mock_cur.execute.call_args[0][1]
        assert "INSERT INTO ozy_mcts_nodes" in sql_called
        assert params_called[2] == "root.phase_1"  # formatted ltree path
        assert params_called[4] == "DualActiveContract"

        # Query tree trajectory with prefix
        mock_cur.reset_mock()
        mock_cur.fetchall.return_value = [
            {
                "node_id": "node_123",
                "tree_id": "tree_abc",
                "parent_path": "root.phase_1",
                "phase": "Phase 1: Backend",
                "action_strategy": "DualActiveContract",
                "visits": 15,
                "q_value": 0.875,
                "projected_churn": 2,
                "state_payload": {"edits": ["file1.py"]},
            }
        ]

        nodes = repo.query_tree_trajectory("tree_abc", path_prefix="root/phase_1")
        assert len(nodes) == 1
        assert nodes[0]["node_id"] == "node_123"
        assert nodes[0]["strategy"] == "DualActiveContract"
        assert "<@ %s::ltree" in mock_cur.execute.call_args[0][0]
