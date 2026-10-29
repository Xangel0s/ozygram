import pytest
from unittest.mock import MagicMock, patch
from ozy_brain.planner import _compute_blast_radius, plan


def test_blast_radius_postgres_offline_graceful_fallback():
    # When PostgreSQL is offline, _compute_blast_radius finishes with 0 errors
    cands = ["src/components/TableEditor.tsx", "src/utils/tableDataCache.ts"]
    res = _compute_blast_radius(cands, graph_context=[])
    assert res["candidate_files_count"] == 2
    assert res["risk_tier"] == "low"
    assert res["safe_to_edit_in_parallel"] is True


def test_blast_radius_postgres_l3_enrichment():
    cands = ["app/routers/orders.py"]
    mock_hits = [
        {"source_file": "app/services/checkout.py", "target_file": "app/routers/orders.py", "depth": 1},
        {"source_file": "frontend/src/hooks/useOrders.ts", "target_file": "app/routers/orders.py", "depth": 2},
        {"source_file": "frontend/src/views/CheckoutModal.tsx", "target_file": "app/routers/orders.py", "depth": 3},
    ]

    with patch("ozy_brain.scanner.tri_hybrid_orchestrator.TriHybridMemoryOrchestrator.query_reverse_blast_radius", return_value=mock_hits):
        res = _compute_blast_radius(cands, graph_context=[])
        assert res["candidate_files_count"] == 1
        assert res["transitive_dependents_count"] == 3
        assert res["total_blast_radius"] == 4
        assert "frontend/src/views/CheckoutModal.tsx" in res["high_severity_files"]


def test_e2e_ozybase_scenario_simulation():
    # Simulates the exact prompt from OzyBase that motivated this enhancement
    payload = {
        "goal": "Modularize full frontend application into clean feature folders with strict types, custom hooks, and isolated subcomponents for TableEditor, Layout, SqlTerminal, StorageManager, Overview, AuthManager, EdgeFunctions, ApiDocs, Workspaces, and Login.",
        "files": [
            "frontend/src/components/TableEditor.tsx",
            "frontend/src/components/SqlTerminal.tsx",
            "frontend/src/utils/tableDataCache.ts",
            ".dockerignore",
            ".env.example",
            ".gitattributes"
        ],
    }

    res = plan(payload)

    # 1. Verify dotfiles were completely purged from candidates
    cand_files = res.structured_plan.get("candidate_files", [])
    assert not any(f.startswith(".env") or f.endswith("ignore") or f.endswith("attributes") for f in cand_files)
    assert any("TableEditor" in f or "SqlTerminal" in f for f in cand_files)

    # 2. Verify phase auto-inferred as domain_services
    assert res.structured_plan.get("current_phase") == "domain_services"

    # 3. Verify MCTS Dream-RSI optimizer evaluated branches and promoted DualActiveContract
    dream_opt = res.structured_plan.get("dream_mcts_optimization", {})
    assert dream_opt.get("best_strategy") == "DualActiveContract"
    assert dream_opt.get("best_score", 0.0) >= 0.85
    assert dream_opt.get("badge") == "[OPTIMIZED_BY_DREAM_RSI]"

    # 4. Verify step 6 injected with [DREAM_RSI] badge
    dream_steps = [s for s in res.plan if "[DREAM_RSI]" in s]
    assert len(dream_steps) == 1
    assert "DualActiveContract" in dream_steps[0]

    # 5. Verify recommendation injected
    dream_recs = [r for r in res.recommendations if "[OPTIMIZED_BY_DREAM_RSI]" in r]
    assert len(dream_recs) >= 1
    assert "DualActiveContract" in dream_recs[0]
