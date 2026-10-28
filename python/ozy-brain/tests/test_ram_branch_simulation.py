"""
Unit tests for Fase 2: Generador de Ramas Contrafácticas y Espacio de Acciones por Fase.
Standard: Zero-Emoji, SOLID, SOTA in Tree Search for Agents (LATS ICML 2024 / RAP NeurIPS).
"""

import time
import pytest
from ozy_brain.dream.plan_branching import (
    BranchAction,
    PhaseActionGenerator,
    PlanExecutionState,
    RamBranchSimulator,
    SwrBranchFilter,
)


def test_phase_action_generator_domain_services():
    cands = ["api-geofal-crm/app/api/endpoints/probetas.py"]
    actions = PhaseActionGenerator.generate_actions("domain_services", cands)

    strategies = [a.strategy for a in actions]
    assert "DualActiveContract" in strategies
    assert "FrontendCallerSync" in strategies
    assert "BackendFallbackAdapter" in strategies

    dual_act = next(a for a in actions if a.strategy == "DualActiveContract")
    assert dual_act.metadata["is_breaking"] is False
    assert dual_act.metadata["protects_frontend"] is True

    sync_act = next(a for a in actions if a.strategy == "FrontendCallerSync")
    assert sync_act.metadata["is_breaking"] is True
    assert sync_act.metadata["blast_radius_risk"] == "HIGH"


def test_phase_action_generator_all_phases():
    for phase in ["leaf_contracts", "entrypoints", "validation", "learning", "custom_phase"]:
        actions = PhaseActionGenerator.generate_actions(phase, ["file_a.py"])
        assert len(actions) >= 1
        for a in actions:
            assert a.phase == phase
            assert "file_a.py" in a.target_files


def test_ram_branch_simulator_ephemeral_isolation():
    initial_state = PlanExecutionState(
        completed_phases=["context", "leaf_contracts"],
        active_phase="domain_services",
        working_edits=[{"file": "contracts.py", "action": "base_schema"}],
        modified_files=["contracts.py"],
    )

    action = BranchAction(
        action_id="act_test_01",
        strategy="DualActiveContract",
        phase="domain_services",
        target_files=["api-geofal-crm/app/api/endpoints/probetas.py"],
        projected_churn=15,
        description="Dual active route testing",
        metadata={"is_breaking": False},
    )

    t0 = time.perf_counter()
    simulated_state, report = RamBranchSimulator.simulate_mutation(initial_state, action)
    elapsed = time.perf_counter() - t0

    # Latency assertion (< 2ms SLA)
    assert elapsed < 0.005, f"RAM simulation took {elapsed:.5f}s, exceeding SLA"

    # Strict isolation: initial state must not have been mutated
    assert len(initial_state.working_edits) == 1
    assert len(initial_state.modified_files) == 1
    assert "domain_services" not in initial_state.completed_phases

    # Simulated state verification
    assert len(simulated_state.working_edits) == 2
    assert "api-geofal-crm/app/api/endpoints/probetas.py" in simulated_state.modified_files
    assert "domain_services" in simulated_state.completed_phases
    assert simulated_state.active_phase == "entrypoints"

    # Report verification
    assert report["simulated_in_ram"] is True
    assert report["disk_writes"] == 0
    assert report["invariants_satisfied"] is True


def test_swr_branch_filter_immunological_pruning():
    actions = [
        BranchAction(
            action_id="act_safe_dual",
            strategy="DualActiveContract",
            phase="domain_services",
            target_files=["api-geofal-crm/app/api/endpoints/probetas.py"],
            projected_churn=15,
            description="Safe dual contract",
            metadata={"is_breaking": False},
        ),
        BranchAction(
            action_id="act_breaking_sync",
            strategy="FrontendCallerSync",
            phase="domain_services",
            target_files=["api-geofal-crm/app/api/endpoints/probetas.py"],
            projected_churn=45,
            description="Breaking caller sync",
            metadata={"is_breaking": True},
        ),
    ]

    # Historical ripples simulating recurring regression
    ripples = [
        {
            "error_signature": "TypeError: Altered parameter signature broke caller arity in probetas view",
            "hazard": "Hazard: Altering parameter signatures without updating upstream call sites.",
        }
    ]

    approved, pruned = SwrBranchFilter.filter_actions(actions, ripples)

    assert len(approved) == 1
    assert approved[0].strategy == "DualActiveContract"

    assert len(pruned) == 1
    assert pruned[0]["strategy"] == "FrontendCallerSync"
    assert pruned[0]["badge"] == "[ALERT: SWR_TRAP_PRUNED]"
    assert "caller" in pruned[0]["reason"].lower() or "signature" in pruned[0]["reason"].lower()
