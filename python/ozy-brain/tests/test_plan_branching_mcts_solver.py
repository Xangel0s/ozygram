"""
Unit tests for Fase 3: Función de Recompensa Multi-Dimensional SOTA y Búsqueda MCTS.
Standard: Zero-Emoji, SOLID, SOTA in Tree Search for Agents (LATS ICML 2024 / RAP NeurIPS).
"""

import time
import pytest
from ozy_brain.dream.plan_branching import (
    BranchAction,
    PlanExecutionState,
    PlanBranchRewardEvaluator,
    LocusCoeruleusPuctModulator,
    PlanBranchingMctsOptimizer,
    BranchOptimizationResult,
)
from ozy_brain.scanner.compute_config import MonsterComputeConfig, ComputeMode


def test_plan_branch_reward_evaluator_formula():
    state = PlanExecutionState(
        completed_phases=["context", "leaf_contracts"],
        active_phase="domain_services",
    )

    dual_act = BranchAction(
        action_id="act_dual",
        strategy="DualActiveContract",
        phase="domain_services",
        target_files=["api-geofal-crm/app/api/endpoints/probetas.py"],
        projected_churn=15,
        description="Dual active routes",
        metadata={"is_breaking": False, "protects_frontend": True},
    )

    sync_act = BranchAction(
        action_id="act_sync",
        strategy="FrontendCallerSync",
        phase="domain_services",
        target_files=["api-geofal-crm/app/api/endpoints/probetas.py"],
        projected_churn=45,
        description="Breaking caller sync",
        metadata={"is_breaking": True, "protects_frontend": False},
    )

    # 1. Safe DualActiveContract evaluation
    dual_eval = PlanBranchRewardEvaluator.evaluate(
        action=dual_act,
        state=state,
        blast_radius_count=6,
        swr_hazard_detected=False,
    )
    assert dual_eval["q_score"] >= 0.85
    assert dual_eval["badge"] == "[OPTIMAL_BRANCH_CANDIDATE]"
    assert dual_eval["effective_blast"] == 0  # Fully contained by dual contract (zero breaking radius)

    # 2. Breaking FrontendCallerSync evaluation (uncontained blast radius)
    sync_eval = PlanBranchRewardEvaluator.evaluate(
        action=sync_act,
        state=state,
        blast_radius_count=6,
        swr_hazard_detected=False,
    )
    assert sync_eval["q_score"] < dual_eval["q_score"]
    assert sync_eval["effective_blast"] == 6

    # 3. Hazard penalty
    hazard_eval = PlanBranchRewardEvaluator.evaluate(
        action=sync_act,
        state=state,
        blast_radius_count=6,
        swr_hazard_detected=True,
    )
    assert hazard_eval["q_score"] < sync_eval["q_score"]
    assert hazard_eval["badge"] == "[ALERT: SUBOPTIMAL_HIGH_FRICTION]"


def test_locus_coeruleus_puct_modulation():
    # leaf_contracts: high discipline exploitation
    c_leaf = LocusCoeruleusPuctModulator.get_c_puct("leaf_contracts")
    assert round(c_leaf, 3) == 1.414

    # domain_services: divergent exploration
    c_domain = LocusCoeruleusPuctModulator.get_c_puct("domain_services")
    assert c_domain == 2.400

    # learning: consolidation
    c_learn = LocusCoeruleusPuctModulator.get_c_puct("learning")
    assert c_learn == 1.000

    # high risk dampening on divergent phase
    c_high_risk = LocusCoeruleusPuctModulator.get_c_puct("domain_services", risk_level="high")
    assert c_high_risk == 1.850


def test_mcts_plan_optimizer_convergence_and_latency():
    state = PlanExecutionState(
        completed_phases=["context", "leaf_contracts"],
        active_phase="domain_services",
        working_edits=[{"file": "schemas.py", "action": "add_dto_v2"}],
        modified_files=["schemas.py"],
    )
    cands = [
        "api-geofal-crm/app/api/endpoints/probetas.py",
        "crm-geofal/src/hooks/useProbetas.ts",
    ]

    # Balanced config (25 simulations, no throttle)
    config = MonsterComputeConfig(
        mode=ComputeMode.BALANCED,
        mcts_simulations=25,
        mcts_max_depth=5,
        parallel_workers=1,
        blast_radius_max_depth=4,
        throttle_sleep_ms=0,
        max_memory_mb=1024,
    )

    t0 = time.perf_counter()
    res = PlanBranchingMctsOptimizer.optimize_plan(
        state=state,
        candidate_files=cands,
        compute_config=config,
        blast_radius_count=5,
    )
    elapsed = time.perf_counter() - t0

    # Latency benchmark: must finish under 80ms
    assert elapsed < 0.080, f"MCTS optimization took {elapsed:.4f}s, exceeding 80ms SLA"

    # Strategy convergence
    assert res.best_strategy == "DualActiveContract"
    assert res.best_score >= 0.85
    assert res.badge == "[OPTIMIZED_BY_DREAM_RSI]"
    assert res.simulations_count == 25

    # Simulated state checks
    assert "domain_services" in res.simulated_final_state.completed_phases
    assert "api-geofal-crm/app/api/endpoints/probetas.py" in res.simulated_final_state.modified_files

    # Trajectory rankings
    trajectories = res.evaluated_trajectories
    assert len(trajectories) >= 2
    best_entry = next(t for t in trajectories if t["is_best"])
    assert best_entry["strategy"] == "DualActiveContract"

    # Serialization
    res_dict = res.to_dict()
    assert res_dict["badge"] == "[OPTIMIZED_BY_DREAM_RSI]"
    assert res_dict["best_strategy"] == "DualActiveContract"


def test_mcts_plan_optimizer_monster_mode():
    state = PlanExecutionState(
        completed_phases=["context"],
        active_phase="leaf_contracts",
    )
    config = MonsterComputeConfig(
        mode=ComputeMode.MONSTER,
        mcts_simulations=60,
        mcts_max_depth=8,
        parallel_workers=4,
        blast_radius_max_depth=6,
        throttle_sleep_ms=0,
        max_memory_mb=4096,
    )
    res = PlanBranchingMctsOptimizer.optimize_plan(
        state=state,
        candidate_files=["api-geofal-crm/app/schemas/probetas.py"],
        compute_config=config,
    )
    assert res.simulations_count == 60
    assert res.compute_mode == "ComputeMode.MONSTER"
    assert len(res.evaluated_trajectories) >= 2
