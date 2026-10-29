"""
Unit tests for Active Inference Expected Free Energy (EFE) MCTS Solver
and volatile RAM Code World Model (GIF-MCTS).
Standard: Zero-Emoji, SOLID, High Performance (< 150ms).
"""

import time
import pytest
from ozy_brain.dream.generative_mcts import GenerativeMctsSearcher, MctsNode, RamCodeWorldModel


def test_ram_code_world_model_validation():
    """Verify volatile RAM World Model prunes invalid mutations before LLM evaluation."""
    state = {"symbol": "update_user_profile", "file_path": "app/routers/users.py"}

    # 1. Breaking cutover without backward compatibility shim is rejected
    action_breaking = {"type": "breaking_cutover", "has_shim": False}
    res_breaking = RamCodeWorldModel.validate_action(state, action_breaking)
    assert res_breaking["valid"] is False
    assert res_breaking["backward_compatible"] is False
    assert "rejected by World Model" in res_breaking["reason"]

    # 2. Syntax error in patch is caught by in-memory AST parser
    action_syntax_err = {
        "type": "patch",
        "patch_code": "def bad_syntax(x, y: return x + y",  # Invalid syntax
    }
    res_syntax = RamCodeWorldModel.validate_action(state, action_syntax_err)
    assert res_syntax["valid"] is False
    assert res_syntax["syntax_valid"] is False
    assert "Syntax error in RAM patch" in res_syntax["reason"]

    # 3. Valid non-breaking action passes
    action_valid = {
        "type": "add_default_param",
        "syntax_valid": True,
        "backward_compatible": True,
        "blast_ratio": 0.05,
    }
    res_valid = RamCodeWorldModel.validate_action(state, action_valid)
    assert res_valid["valid"] is True
    assert res_valid["backward_compatible"] is True
    assert res_valid["blast_ratio"] == 0.05


def test_expected_free_energy_calculation():
    """Verify pragmatic vs epistemic value computation in EFE."""
    searcher = GenerativeMctsSearcher()

    # Node with perfect pragmatic compliance and unvisited (high epistemic bonus)
    node_fresh = MctsNode(
        state={
            "syntax_valid": True,
            "backward_compatible": True,
            "guard_applied": True,
            "blast_ratio": 0.0,
            "poincare_distance": 2.5,
        },
        visits=0,
    )
    efe_fresh = searcher.compute_expected_free_energy(node_fresh)
    assert efe_fresh["pragmatic_value"] == 1.0
    assert efe_fresh["epistemic_value"] > 0.60
    assert efe_fresh["efe_score"] == 1.0

    # Node after many visits (epistemic uncertainty is resolved)
    node_visited = MctsNode(
        state={
            "syntax_valid": True,
            "backward_compatible": True,
            "guard_applied": True,
            "blast_ratio": 0.0,
            "poincare_distance": 2.5,
        },
        visits=25,
    )
    efe_visited = searcher.compute_expected_free_energy(node_visited)
    assert efe_visited["pragmatic_value"] == 1.0
    assert efe_visited["epistemic_value"] < efe_fresh["epistemic_value"]


def test_efe_mcts_search_convergence_and_token_savings(tmp_path):
    """Verify EFE-MCTS runs under 150ms and yields estimated token savings."""
    searcher = GenerativeMctsSearcher(
        simulations=15,
        max_depth=3,
        workspace_root=tmp_path,
    )

    root_state = {
        "symbol": "render_table_view",
        "file_path": "frontend/src/components/TableEditor.tsx",
        "syntax_valid": True,
        "backward_compatible": True,
        "poincare_distance": 1.8,
    }

    t0 = time.perf_counter()
    result = searcher.search(root_state=root_state, threat_scar=0.2)
    elapsed_ms = (time.perf_counter() - t0) * 1000

    assert elapsed_ms < 150.0, f"Search must complete in < 150ms: got {elapsed_ms:.2f}ms"
    assert result["root_visits"] == 15
    assert result["best_q_value"] > 0.5
    assert result["solutions_found"] >= 1
    assert result["tokens_saved_estimate"] == 15 * 450
    assert "best_pragmatic_value" in result
    assert "best_epistemic_value" in result
