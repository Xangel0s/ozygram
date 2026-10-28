"""
Unit tests for Warm-Started Plan Branching MCTS (Fase 1).
Standard: Zero-Emoji, SOTA in Tree Search for Agents (LATS ICML 2024 / RAP NeurIPS).
"""

import time
import pytest
from ozy_brain.dream.plan_branching import (
    PlanExecutionState,
    WarmStartNode,
    WarmStartRootNodeBuilder,
    STANDARD_PHASES,
)
from ozy_brain.planner import plan, _structured_plan


def test_plan_execution_state_lifecycle():
    payload = {
        "completed_phases": ["context", "leaf_contracts"],
        "current_phase": "domain_services",
        "working_edits": [
            {"file": "api-geofal-crm/app/api/endpoints/probetas.py", "action": "update_schema_v2"}
        ],
        "modified_files": ["api-geofal-crm/app/api/endpoints/probetas.py"],
        "synaptic_tags": [
            {"node_id": "tag-1", "file_path": "api-geofal-crm/app/api/endpoints/probetas.py", "weight": 1.0, "timestamp": time.time()}
        ],
        "validation_history": [{"phase": "leaf_contracts", "status": "PASSED"}],
    }

    state = PlanExecutionState.from_payload(payload)
    assert state.is_phase_completed("context") is True
    assert state.is_phase_completed("leaf_contracts") is True
    assert state.is_phase_completed("domain_services") is False

    remaining = state.remaining_phases()
    assert remaining == ["domain_services", "entrypoints", "validation", "learning"]

    # Test record edit
    state.record_edit("crm-geofal/src/hooks/useProbetas.ts", "add_resilient_fetch")
    assert len(state.working_edits) == 2
    assert "crm-geofal/src/hooks/useProbetas.ts" in state.modified_files

    # Test advance phase
    state.advance_phase("entrypoints")
    assert state.active_phase == "entrypoints"
    assert "domain_services" in state.completed_phases
    assert state.remaining_phases() == ["entrypoints", "validation", "learning"]

    # Test serialization
    data = state.to_dict()
    assert data["completed_phases"] == ["context", "leaf_contracts", "domain_services"]
    assert data["active_phase"] == "entrypoints"
    assert data["working_edits_count"] == 2


def test_synaptic_tags_decay():
    state = PlanExecutionState(
        synaptic_tags=[
            {"node_id": "fresh", "file_path": "file1.py", "weight": 1.0, "timestamp": time.time()},
            {"node_id": "old", "file_path": "file2.py", "weight": 1.0, "timestamp": time.time() - 3600}, # 1 hour old (> 1800s)
        ]
    )
    active = state.get_active_tags(max_age_seconds=1800.0)
    assert len(active) == 1
    assert active[0]["node_id"] == "fresh"
    assert active[0]["effective_weight"] > 0.95


def test_warm_start_root_node_builder_latency_and_priors():
    state = PlanExecutionState(
        completed_phases=["context", "leaf_contracts"],
        active_phase="domain_services",
        synaptic_tags=[
            {"node_id": "tag-1", "file_path": "crm-geofal/src/app/probetas/page.tsx", "weight": 1.0, "timestamp": time.time()}
        ]
    )
    candidates = [
        "crm-geofal/src/app/probetas/page.tsx",
        "api-geofal-crm/app/api/endpoints/probetas.py",
    ]

    # Benchmark: 100 runs must execute in < 100ms total (< 1ms per run)
    t0 = time.perf_counter()
    for _ in range(100):
        root = WarmStartRootNodeBuilder.build_root(state, candidate_files=candidates)
    elapsed = time.perf_counter() - t0

    assert elapsed < 0.10, f"100 root builds took {elapsed:.4f}s, exceeding 1ms per run SLA"
    assert root.phase == "domain_services"
    assert root.visits == 1
    assert root.prior_score > 1.0  # Boosted due to synaptic tag overlap
    assert root.metadata["completed_count"] == 2
    assert root.metadata["remaining_count"] == 4


def test_warm_start_node_uct_selection():
    root = WarmStartNode(phase="domain_services", visits=10)
    child_a = WarmStartNode(phase="entrypoints", action="direct_refactor", visits=5, value_sum=3.0, prior_score=1.0)
    child_b = WarmStartNode(phase="entrypoints", action="dual_contract", visits=5, value_sum=4.5, prior_score=1.0)

    root.add_child(child_a)
    root.add_child(child_b)

    # child_b has Q=0.9, child_a has Q=0.6 -> child_b should win
    best = root.best_child(c_puct=1.414)
    assert best.action == "dual_contract"
    assert best.parent is root


def test_structured_plan_phase_annotation():
    payload = {
        "goal": "Refactor probetas endpoint to support dual active contract",
        "completed_phases": ["context", "leaf_contracts"],
        "current_phase": "domain_services",
        "files": ["api-geofal-crm/app/api/endpoints/probetas.py"],
    }

    struct = _structured_plan(payload, autonomy="supervised")
    assert struct["current_phase"] == "domain_services"
    assert set(struct["completed_phases"]) == {"context", "leaf_contracts"}

    phases = {p["name"]: p["status"] for p in struct["phases"]}
    assert phases["context"] == "COMPLETED"
    assert phases["leaf_contracts"] == "COMPLETED"
    assert phases["domain_services"] == "ACTIVE"
    assert phases["entrypoints"] == "PENDING"
    assert phases["validation"] == "PENDING"
    assert phases["learning"] == "PENDING"

    # Full plan() integration
    res = plan(payload)
    assert res.action == "plan"
    assert res.structured_plan is not None
    assert len(res.structured_plan["phases"]) == 6
    assert res.structured_plan["current_phase"] == "domain_services"
    assert "leaf_contracts" in res.structured_plan["completed_phases"]
