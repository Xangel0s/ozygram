"""
Real-world deep algorithm & stress tests for OzyBrain against crmnew.
Tests:
1. STDP Hebbian Plasticity & commit session causality mining on real Git history.
2. Basal Ganglia procedural sequence mining & playbook discovery.
3. Monte Carlo Tree Search (MCTS) exploration trajectory optimization (UCT).
4. Cerebellar Forward Model & Code Dysmetria (D_KL) detection on real crmnew modules.
5. Dream-RSI offline consolidation cycle & postmortem vaccine ingestion.
"""

from __future__ import annotations

import os
import sys
import time
import unittest
from collections import Counter
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from ozy_brain.cerebellum import CerebellumEngine
from ozy_brain.dream.orchestrator import DreamOrchestrator
from ozy_brain.dream.policy import MctsExplorationPolicy
from ozy_brain.neuromodulators import NeuromodulatorEngine, NeuromodulatorState
from ozy_brain.procedural_habits import BasalGangliaEngine
from ozy_brain.stdp_synapse import StdpSynapseEngine

LOCAL_CRMNEW_CANDIDATES = [
    Path(__file__).resolve().parents[3] / "crmnew",
    Path(r"C:\Users\Lenovo\Documents\crmnew"),
]
WORKSPACE_DIR = next(
    (p for p in LOCAL_CRMNEW_CANDIDATES if p.exists() and (p / ".git").exists()),
    Path(__file__).resolve().parents[3],
)


class CrmnewDeepAlgorithmsTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.has_git = WORKSPACE_DIR.exists() and (WORKSPACE_DIR / ".git").exists()

    def test_stdp_hebbian_mining_on_real_crmnew_git(self):
        """Stress-tests STDP Hebbian Plasticity on real crmnew commit history."""
        if not self.has_git:
            self.skipTest("git repository not found for deep algorithm testing")

        duckdb_scratch = Path(__file__).resolve().parent / "scratch_crmnew_stdp.duckdb"
        if duckdb_scratch.exists():
            try:
                duckdb_scratch.unlink()
            except Exception:
                pass

        start = time.perf_counter()
        engine = StdpSynapseEngine(
            workspace_root=WORKSPACE_DIR,
            duckdb_path=duckdb_scratch,
            tenant_id="crmnew",
        )

        # 1. Mine real git commit sessions (last 60 commits) and apply STDP
        res = engine.mine_git_history_stdp(max_commits=60)
        mining_duration = time.perf_counter() - start

        self.assertIsInstance(res, dict)
        self.assertEqual(res.get("status"), "ok")
        self.assertGreater(res.get("events_processed", 0), 0, "Expected crmnew to contain commit events")

        # 2. Query strongest coupled synapses across the workspace
        strongest = engine.get_strongest_synapses(min_weight=0.10, limit=10)
        self.assertIsInstance(strongest, list)
        self.assertLess(mining_duration, 10.0, "STDP replay must complete in < 10.0s")

        # Cleanup scratch duckdb
        try:
            if duckdb_scratch.exists():
                duckdb_scratch.unlink()
        except Exception:
            pass

    def test_basal_ganglia_procedural_habits_on_real_crmnew(self):
        """Tests Basal Ganglia sequence mining over real crmnew commits."""
        if not self.has_git:
            self.skipTest("git repository not found for deep algorithm testing")

        engine = BasalGangliaEngine(
            workspace_root=WORKSPACE_DIR,
            tenant_id="crmnew",
        )

        sessions = engine.extract_git_commit_sessions(max_commits=50)
        self.assertGreater(len(sessions), 0)

        # Build co-occurrence choreographies
        playbooks = engine.mine_procedural_playbooks(sessions, min_support=2)
        self.assertIsInstance(playbooks, list)

    def test_mcts_exploration_policy_on_quimicos_unification(self):
        """Simulates MCTS Tree Search evaluating candidate refactoring steps for chemical assays."""
        policy = MctsExplorationPolicy(
            version="v2.4.0",
            exploration_constant=1.4142,
            prune_threshold=-2.5,
        )

        # Candidate refactor branches:
        parent_node = {"id": "root_task", "visit_count": 25, "value_estimate": 1.0}

        # Child A: Leaf-to-Root (Contract Schemas first) -> Positive value, 4 visits
        child_a_contract = {
            "id": "node_contract_schemas",
            "action_type": "patch_contract",
            "target": "api-geofal-crm/app/modules/schemas.py",
            "value_estimate": 3.8,
            "visit_count": 4,
            "depth": 1,
            "is_solution": False,
        }

        # Child B: Inverted Broken Refactor (Modifies UI without updating schemas, crashes)
        child_b_broken_ui = {
            "id": "node_broken_ui",
            "action_type": "patch_ui",
            "target": "crm-geofal/src/components/dashboard/SulfatosSolublesForm.tsx",
            "value_estimate": -3.5,
            "visit_count": 1,
            "depth": 1,
            "is_solution": False,
        }

        # Evaluate through MCTS UCT formula
        score_a = policy.compute_uct_score(child_a_contract, parent_visits=25)
        score_b = policy.compute_uct_score(child_b_broken_ui, parent_visits=25)

        self.assertGreater(score_a, score_b)
        self.assertFalse(policy.should_prune(child_a_contract))
        self.assertTrue(policy.should_prune(child_b_broken_ui))

        # Best child selection must pick Child A and reject Child B
        best_child = policy.select_best_child(parent_node, [child_a_contract, child_b_broken_ui])
        self.assertIsNotNone(best_child)
        self.assertEqual(best_child["id"], "node_contract_schemas")

    def test_neuromodulators_modulate_mcts_c_puct(self):
        """Verifies that high Acetylcholine (novelty) raises MCTS exploration constant c_puct."""
        scratch_state = Path(__file__).resolve().parent / "scratch_neuro_state.json"
        engine = NeuromodulatorEngine(state_file=scratch_state)

        # Routine task (high familiarity)
        res_routine = engine.record_code_novelty(is_new_subsystem=False, visit_count=15)
        c_puct_routine = engine.get_effective_hyperparameters()["mcts_c_puct"]

        # Highly novel unmapped assay (new subsystem)
        res_novel = engine.record_code_novelty(is_new_subsystem=True, visit_count=0)
        c_puct_novel = engine.get_effective_hyperparameters()["mcts_c_puct"]

        self.assertGreater(c_puct_novel, c_puct_routine)
        self.assertEqual(res_novel["mode"], "EXPLORATION_MCTS")

        if scratch_state.exists():
            try:
                scratch_state.unlink()
            except Exception:
                pass

    def test_cerebellar_forward_model_dysmetria_detection(self):
        """Verifies Cerebellar Kinematic Forward Model detects motor overshoot and under-coupling."""
        cerebellum = CerebellumEngine(workspace_root=WORKSPACE_DIR)

        # Case 1: Healthy coordinated patch (schemas + frontend form)
        healthy_patch = [
            "api-geofal-crm/app/modules/schemas.py",
            "crm-geofal/src/components/dashboard/SulfatosSolublesForm.tsx",
        ]
        res_healthy = cerebellum.evaluate_patch_dysmetria(
            target_file="api-geofal-crm/app/modules/schemas.py",
            proposed_files=healthy_patch,
        )
        self.assertIn(res_healthy["status"], ["calibrated", "under_coupled"])

        # Case 2: Motor Overshoot (Hypermetria) - touches disparate untracked/unrelated folders
        overshoot_patch = [
            "api-geofal-crm/app/modules/schemas.py",
            "Comercial-Control-CRM/lead.tsx",
            "backups/db.sql",
            "Ingenieria/perforacion.py",
            "docs/manual.md",
            "scripts/extra.py",
            "tests/test_x.py",
            "infra/setup.sh",
            "data/export.csv",
        ]
        res_overshoot = cerebellum.evaluate_patch_dysmetria(
            target_file="api-geofal-crm/app/modules/schemas.py",
            proposed_files=overshoot_patch,
        )
        self.assertEqual(res_overshoot["status"], "overshoot")
        self.assertIn("MOTOR_OVERSHOOT", res_overshoot["badge"])

    def test_dream_orchestrator_cycle_on_real_crmnew(self):
        """Runs an end-to-end idle Dream-RSI cycle against crmnew workspace."""
        if not self.has_git:
            self.skipTest("git repository not found for deep algorithm testing")

        orchestrator = DreamOrchestrator(
            workspace_root=WORKSPACE_DIR,
            tenant_id="crmnew",
        )

        start = time.perf_counter()
        insights = orchestrator.run_dream_cycle(max_commits=20)
        duration = time.perf_counter() - start

        self.assertIsInstance(insights, dict)
        self.assertIn("status", insights)
        self.assertIn("duration_ms", insights)
        self.assertIn("completed_at", insights)
        self.assertLess(duration, 15.0, "Dream cycle must execute in < 15s")


if __name__ == "__main__":
    unittest.main()
