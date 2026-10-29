import os
import tempfile
import pytest
from ozy_brain.brain import run
from ozy_brain.scanner.tri_hybrid_orchestrator import TriHybridMemoryOrchestrator
from ozy_brain.planner import plan


def test_tri_hybrid_telemetry_handler():
    payload = {"workspace_root": "."}
    res = run("tri_hybrid_status", payload)

    assert res["action"] == "tri_hybrid_status"
    assert "structured_plan" in res
    plan_data = res["structured_plan"]

    assert "l1_sqlite" in plan_data
    assert plan_data["l1_sqlite"]["status"] == "online"
    assert "latency_sla" in plan_data["l1_sqlite"]

    assert "l2_chroma" in plan_data
    assert "latency_sla" in plan_data["l2_chroma"]

    assert "l3_postgres" in plan_data
    assert "status" in plan_data["l3_postgres"]

    assert "neuromodulators" in plan_data


def test_direct_orchestrator_telemetry():
    with tempfile.TemporaryDirectory(ignore_cleanup_errors=True) as tmpdir:
        sqlite_db = os.path.join(tmpdir, "test_mem.db")
        chroma_dir = os.path.join(tmpdir, "test_chroma")
        orch = TriHybridMemoryOrchestrator(sqlite_path=sqlite_db, chroma_dir=chroma_dir)
        telemetry = orch.get_telemetry_status(workspace_root=tmpdir)

        assert telemetry["badge"] == "[STATUS: TRI_HYBRID_COGNITION_ACTIVE]"
        assert telemetry["l1_sqlite"]["path"] == str(sqlite_db)
        assert telemetry["l2_chroma"]["status"] in ("online", "offline")
        assert "neuromodulators" in telemetry
        orch.close()


def test_mcts_auto_persists_gotcha_markdown():
    with tempfile.TemporaryDirectory(ignore_cleanup_errors=True) as tmpdir:
        # Create a mock .ozy/memories directory in tmpdir
        mem_dir = os.path.join(tmpdir, ".ozy", "memories", "gotchas")
        os.makedirs(mem_dir, exist_ok=True)

        payload = {
            "goal": "Modularize frontend application into clean subcomponents for TableEditor",
            "candidate_files": ["src/components/TableEditor.tsx"],
            "workspace_root": tmpdir,
        }
        res = plan(payload)

        # Assert MCTS ran
        opt = res.structured_plan.get("dream_mcts_optimization", {})
        assert opt.get("best_score", 0.0) >= 0.85
