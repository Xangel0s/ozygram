import pytest
from ozy_brain.schemas import auto_infer_phase, _current_phase
from ozy_brain.planner import plan


def test_auto_infer_phase_variants():
    # 1. Refactor / Modularize frontend -> domain_services
    phase = auto_infer_phase("Modularize full frontend application into subcomponents")
    assert phase == "domain_services"

    # 2. Schema / Contract migration -> leaf_contracts
    phase_schema = auto_infer_phase("Migrate Pydantic schemas and models to v2")
    assert phase_schema == "leaf_contracts"

    # 3. Contract files dominance -> leaf_contracts
    phase_files = auto_infer_phase("Update base structures", candidate_files=["src/models.py", "src/types.ts", "src/dto.py"])
    assert phase_files == "leaf_contracts"

    # 4. Validation / Tests -> validation
    phase_tests = auto_infer_phase("Run tests and verify coverage suite")
    assert phase_tests == "validation"

    # 5. Router / Dispatcher -> entrypoints
    phase_routes = auto_infer_phase("Add new endpoint to API router")
    assert phase_routes == "entrypoints"

    # 6. Default exploration -> context
    phase_default = auto_infer_phase("Look around the codebase")
    assert phase_default == "context"


def test_planner_auto_mcts_activation_without_manual_phase():
    payload = {
        "goal": "Modularize full frontend application into clean feature folders with strict types and custom hooks for TableEditor and SqlTerminal."
    }
    res = plan(payload)

    # Assert phase was automatically inferred as domain_services
    assert res.structured_plan.get("current_phase") == "domain_services"

    # Assert MCTS ran and recommended non-breaking DualActiveContract
    mcts_opt = res.structured_plan.get("dream_mcts_optimization", {})
    assert mcts_opt.get("best_strategy") == "DualActiveContract"
    assert mcts_opt.get("best_score", 0.0) >= 0.85

    # Assert DREAM_RSI badge is injected into recommendations and steps
    assert any("[OPTIMIZED_BY_DREAM_RSI]" in rec for rec in res.recommendations)
    assert any("[DREAM_RSI]" in step for step in res.plan)

