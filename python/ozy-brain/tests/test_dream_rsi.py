"""Unit tests for Dream-RSI and Monte Carlo Tree Search (MCTS) replay simulation."""

from __future__ import annotations

import unittest

from ozy_brain.brain import run
from ozy_brain.dream.evaluator import SimulationReport
from ozy_brain.dream.optimizer import DreamRsiOptimizer, verify_policy_code_safety
from ozy_brain.dream.policy import (
    MctsExplorationPolicy,
    ParametricExplorationPolicy,
    PolicyParameters,
)
from ozy_brain.dream.simulator import ReplaySimulator


class TestDreamRsi(unittest.TestCase):
    def test_mcts_policy_uct_score(self):
        policy = MctsExplorationPolicy(exploration_constant=1.4142)

        node_promising = {
            "id": "node_1",
            "visit_count": 2,
            "value_estimate": 8.0,
            "reward_score": 8.0,
            "is_solution": False,
        }

        node_unpromising = {
            "id": "node_2",
            "visit_count": 5,
            "value_estimate": -3.0,
            "reward_score": -3.0,
            "is_solution": False,
        }

        score_promising = policy.compute_uct_score(node_promising, parent_visits=10)
        score_unpromising = policy.compute_uct_score(node_unpromising, parent_visits=10)

        self.assertGreater(score_promising, score_unpromising)
        self.assertGreater(score_promising, 8.0)

    def test_mcts_policy_pruning(self):
        policy = MctsExplorationPolicy(prune_threshold=-3.0)

        node_bad = {"id": "n1", "value_estimate": -4.0, "is_pruned": False}
        node_marked = {"id": "n2", "value_estimate": 1.0, "is_pruned": True}
        node_good = {"id": "n3", "value_estimate": 2.5, "is_pruned": False}

        self.assertTrue(policy.should_prune(node_bad))
        self.assertTrue(policy.should_prune(node_marked))
        self.assertFalse(policy.should_prune(node_good))

    def test_replay_simulator_counterfactual_execution(self):
        simulator = ReplaySimulator()

        # Synthetic tree:
        # root
        #  ├── branch_a (fails): a1 (-2.0) -> a2 (-5.0)
        #  └── branch_b (succeeds): b1 (+2.0) -> b2 (+10.0, is_solution)
        nodes = [
            {
                "id": "root",
                "parent_id": None,
                "depth": 0,
                "action_type": "goal",
                "cost_tokens": 50,
                "value_estimate": 0.0,
                "reward_score": 0.0,
                "visit_count": 5,
                "is_solution": False,
            },
            {
                "id": "a1",
                "parent_id": "root",
                "depth": 1,
                "action_type": "bad_try",
                "cost_tokens": 150,
                "value_estimate": -2.0,
                "reward_score": -2.0,
                "visit_count": 2,
                "is_solution": False,
            },
            {
                "id": "a2",
                "parent_id": "a1",
                "depth": 2,
                "action_type": "worse_try",
                "cost_tokens": 200,
                "value_estimate": -5.0,
                "reward_score": -5.0,
                "visit_count": 1,
                "is_solution": False,
                "is_pruned": True,
            },
            {
                "id": "b1",
                "parent_id": "root",
                "depth": 1,
                "action_type": "good_try",
                "cost_tokens": 120,
                "value_estimate": 3.0,
                "reward_score": 3.0,
                "visit_count": 3,
                "is_solution": False,
            },
            {
                "id": "b2",
                "parent_id": "b1",
                "depth": 2,
                "action_type": "solution_try",
                "cost_tokens": 100,
                "value_estimate": 10.0,
                "reward_score": 10.0,
                "visit_count": 2,
                "is_solution": True,
            },
        ]

        policy = MctsExplorationPolicy(version="v1.0.0")
        report = simulator.simulate(policy, trajectory_id="synth_traj_1", nodes=nodes)

        self.assertIsInstance(report, SimulationReport)
        self.assertTrue(report.solved)
        self.assertEqual(report.steps_taken, 3)  # root -> b1 -> b2
        self.assertEqual(report.baseline_steps, 5)
        self.assertEqual(report.steps_saved, 2)
        self.assertEqual(report.pruning_errors, 0)
        self.assertGreater(report.fitness_score, 70.0)

    def test_replay_simulator_pruning_error_penalty(self):
        simulator = ReplaySimulator()

        nodes = [
            {
                "id": "root",
                "parent_id": None,
                "depth": 0,
                "action_type": "start",
                "cost_tokens": 50,
                "value_estimate": 0.0,
                "reward_score": 0.0,
                "visit_count": 2,
                "is_solution": False,
            },
            {
                "id": "sol_branch",
                "parent_id": "root",
                "depth": 1,
                "action_type": "fix",
                "cost_tokens": 100,
                "value_estimate": -4.0,  # Negative initial score
                "reward_score": 10.0,
                "visit_count": 1,
                "is_solution": True,
            },
        ]

        # Hyper-aggressive policy that prunes at -3.0
        aggressive_policy = MctsExplorationPolicy(prune_threshold=-3.0)
        report = simulator.simulate(
            aggressive_policy, trajectory_id="synth_traj_2", nodes=nodes
        )

        # The policy mistakenly pruned the only solution branch
        self.assertGreaterEqual(report.pruning_errors, 1)
        # Severe penalty applied: fitness score should be negative
        self.assertLess(report.fitness_score, 0.0)
        self.assertGreaterEqual(len(report.bottlenecks), 1)

    def test_ast_safety_audit(self):
        safe_code = """
class CustomPolicy:
    def compute_uct(self, q, n_p, n_c):
        import math
        return q + 1.414 * math.sqrt(math.log(n_p) / n_c)
"""
        is_safe, violations = verify_policy_code_safety(safe_code)
        self.assertTrue(is_safe)
        self.assertEqual(len(violations), 0)

        malicious_code = """
class BadPolicy:
    def exploit(self):
        import subprocess
        subprocess.run("rm -rf /", shell=True)
"""
        is_safe_bad, violations_bad = verify_policy_code_safety(malicious_code)
        self.assertFalse(is_safe_bad)
        self.assertTrue(any("subprocess" in v for v in violations_bad))

    def test_dream_rsi_optimizer_promotion(self):
        nodes = [
            {"id": "root", "parent_id": None, "depth": 0, "visit_count": 3, "reward_score": 0.0, "is_solution": False},
            {"id": "b1", "parent_id": "root", "depth": 1, "visit_count": 2, "reward_score": 5.0, "is_solution": False},
            {"id": "b2", "parent_id": "b1", "depth": 2, "visit_count": 1, "reward_score": 10.0, "is_solution": True},
        ]

        trajectories = [{"id": "traj_test_1", "nodes": nodes}]

        active_policy = MctsExplorationPolicy(version="v1.0.0", exploration_constant=2.0)
        candidate_policy = MctsExplorationPolicy(version="v1.1.0", exploration_constant=1.4142)

        optimizer = DreamRsiOptimizer(active_policy=active_policy)
        result = optimizer.run_optimization_round(trajectories, candidate_policy=candidate_policy, epsilon=0.0)

        self.assertIn("active_score", result)
        self.assertIn("candidate_score", result)
        self.assertIn(result["active_version"], ("v1.0.0", "v1.1.0"))

    def test_brain_dream_rsi_action(self):
        payload = {
            "trajectories": [
                {
                    "id": "t1",
                    "nodes": [
                        {"id": "r", "parent_id": None, "depth": 0, "visit_count": 2, "reward_score": 0.0, "is_solution": False},
                        {"id": "s", "parent_id": "r", "depth": 1, "visit_count": 1, "reward_score": 10.0, "is_solution": True},
                    ]
                }
            ],
            "candidate_policy": {
                "version": "v1.2.0",
                "exploration_constant": 1.4142,
                "prune_threshold": -2.5
            }
        }

        res = run("dream_rsi", payload)
        self.assertEqual(res["action"], "dream_rsi")
        self.assertEqual(res["engine"], "ozy-brain-python")
        self.assertIn("structured_plan", res)
        self.assertGreaterEqual(res["confidence"], 0.70)

    def test_synthetic_world_known_better_policy(self):
        """
        Mundo Sintético 1 (Auditoría Etapa 3): Existe una política mejor conocida
        (ahorro de tokens y pasos con calibración de costes), y el simulador contrafactual
        la descubre superando a la política bruta.
        """
        simulator = ReplaySimulator()

        # Árbol con 2 ramas:
        # Rama A (bruta/costosa): a1 (2.0, cost: 400) -> a2 (2.0, cost: 400) -> a3 (10.0, is_solution, cost: 400)
        # Rama B (óptima/eficiente): b1 (1.0, cost: 50) -> b2 (10.0, is_solution, cost: 50)
        nodes = [
            {"id": "root", "parent_id": None, "depth": 0, "visit_count": 5, "reward_score": 0.0, "is_solution": False, "cost_tokens": 10},
            {"id": "a1", "parent_id": "root", "depth": 1, "visit_count": 2, "reward_score": 2.0, "is_solution": False, "cost_tokens": 400},
            {"id": "a2", "parent_id": "a1", "depth": 2, "visit_count": 1, "reward_score": 2.0, "is_solution": False, "cost_tokens": 400},
            {"id": "a3", "parent_id": "a2", "depth": 3, "visit_count": 1, "reward_score": 10.0, "is_solution": True, "cost_tokens": 400},
            {"id": "b1", "parent_id": "root", "depth": 1, "visit_count": 2, "reward_score": 1.0, "is_solution": False, "cost_tokens": 50},
            {"id": "b2", "parent_id": "b1", "depth": 2, "visit_count": 1, "reward_score": 10.0, "is_solution": True, "cost_tokens": 50},
        ]

        # Política ciega al consumo de tokens: w_tokens = 0.0
        # Prefiere Rama A porque reward 2.0 > 1.0
        blind_policy = ParametricExplorationPolicy(w_tokens=0.0, c_exploration=0.1)
        rep_blind = simulator.simulate(blind_policy, "synth_better_1", nodes=nodes)
        self.assertTrue(rep_blind.solved)
        self.assertEqual(rep_blind.steps_taken, 4)
        self.assertEqual(rep_blind.tokens_consumed, 10 + 400 + 400 + 400)

        # Política calibrada: w_tokens = 1.0
        # Penaliza a1 (-4.0 pts) vs b1 (-0.5 pts), prefiriendo b1 y resolviendo en 3 pasos con 110 tokens
        calibrated_policy = ParametricExplorationPolicy(w_tokens=1.0, c_exploration=0.1)
        rep_calibrated = simulator.simulate(calibrated_policy, "synth_better_1", nodes=nodes)
        self.assertTrue(rep_calibrated.solved)
        self.assertEqual(rep_calibrated.steps_taken, 3)
        self.assertEqual(rep_calibrated.tokens_consumed, 10 + 50 + 50)
        self.assertGreater(rep_calibrated.fitness_score, rep_blind.fitness_score)

    def test_synthetic_world_null_no_better_policy(self):
        """
        Mundo Sintético 2 (Auditoría Etapa 3 y 4): Mundo nulo donde ninguna política es mejor.
        La puerta de promoción responde 'INSUFICIENTE_MEJORA' y NO promueve nada.
        """
        nodes = [
            {"id": "root", "parent_id": None, "depth": 0, "visit_count": 2, "reward_score": 0.0, "is_solution": False},
            {"id": "s1", "parent_id": "root", "depth": 1, "visit_count": 1, "reward_score": 10.0, "is_solution": True},
        ]
        trajectories = [
            {"id": f"traj_{i}", "nodes": nodes} for i in range(4)
        ]

        active_policy = ParametricExplorationPolicy(version="v1.0.0", c_exploration=1.4142)
        candidate_policy = ParametricExplorationPolicy(version="v1.0.1-null", c_exploration=1.4142)

        optimizer = DreamRsiOptimizer(active_policy=active_policy)
        result = optimizer.run_optimization_round(trajectories, candidate_policy=candidate_policy, epsilon=1.0)

        self.assertFalse(result["promoted"])
        self.assertEqual(result["verdict"], "INSUFICIENTE_MEJORA")
        self.assertEqual(optimizer.active_policy.version, "v1.0.0")

    def test_synthetic_world_trap_greedy_fails(self):
        """
        Mundo Sintético 3 (Auditoría Etapa 3): Árbol trampa donde greedy se estanca
        en una rama engañosa podada, mientras que la política exploratoria (c > 0)
        descubre la solución alternativa verificada.
        """
        simulator = ReplaySimulator()

        # Trampa:
        # Rama trampa: t1 (reward: 5.0) -> t2 (reward: -5.0, is_pruned: True)
        # Rama solución: s1 (reward: 0.5) -> s2 (reward: 10.0, is_solution: True)
        nodes = [
            {"id": "root", "parent_id": None, "depth": 0, "visit_count": 5, "reward_score": 0.0, "is_solution": False},
            {"id": "t1", "parent_id": "root", "depth": 1, "visit_count": 1, "reward_score": 5.0, "is_solution": False},
            {"id": "t2", "parent_id": "t1", "depth": 2, "visit_count": 1, "reward_score": -5.0, "is_solution": False, "is_pruned": True},
            {"id": "s1", "parent_id": "root", "depth": 1, "visit_count": 1, "reward_score": 0.5, "is_solution": False},
            {"id": "s2", "parent_id": "s1", "depth": 2, "visit_count": 1, "reward_score": 10.0, "is_solution": True},
        ]

        # Greedy puro (c = 0.0): cae en la trampa t1 por reward 5.0 y no resuelve en 2 pasos
        greedy_policy = ParametricExplorationPolicy(c_exploration=0.0, prune_threshold=-3.0)
        rep_greedy = simulator.simulate(greedy_policy, "synth_trap_greedy", nodes=nodes, max_steps=2)
        self.assertFalse(rep_greedy.solved)

        # Exploratorio (c = 2.0): explora s1 y s2 alcanzando la solución
        exploratory_policy = ParametricExplorationPolicy(c_exploration=2.0, prune_threshold=-3.0)
        rep_exploratory = simulator.simulate(exploratory_policy, "synth_trap_exploratory", nodes=nodes, max_steps=5)
        self.assertTrue(rep_exploratory.solved)

    def test_promotion_gate_requires_minimum_trees_and_holdout(self):
        """
        Etapa 4: Exige al menos 3 árboles para evaluar, y si hay evidencia y mejora en holdout pareado, promueve.
        """
        nodes = [
            {"id": "root", "parent_id": None, "depth": 0, "visit_count": 2, "reward_score": 0.0, "is_solution": False},
            {"id": "b1", "parent_id": "root", "depth": 1, "visit_count": 1, "reward_score": 5.0, "is_solution": False, "cost_tokens": 100},
            {"id": "b2", "parent_id": "b1", "depth": 2, "visit_count": 1, "reward_score": 10.0, "is_solution": True, "cost_tokens": 50},
        ]

        # 1. Con solo 2 árboles (< 3 requeridos) -> INSUFICIENTE_EVIDENCIA
        trajs_insufficient = [{"id": f"t_{i}", "nodes": nodes} for i in range(2)]
        optimizer = DreamRsiOptimizer()
        cand = ParametricExplorationPolicy(version="v2.0.0")
        res_insufficient = optimizer.run_optimization_round(trajs_insufficient, candidate_policy=cand, min_trees=3)
        self.assertFalse(res_insufficient["promoted"])
        self.assertEqual(res_insufficient["verdict"], "INSUFICIENTE_EVIDENCIA")

        # 2. Con 4 árboles y mejora real -> PROMOVIDO
        trajs_sufficient = [{"id": f"t_{i}", "nodes": nodes} for i in range(4)]
        active = ParametricExplorationPolicy(version="v1.0.0", w_tokens=0.0)
        cand_better = ParametricExplorationPolicy(version="v2.0.0", w_tokens=0.5)
        optimizer2 = DreamRsiOptimizer(active_policy=active)
        res_sufficient = optimizer2.run_optimization_round(trajs_sufficient, candidate_policy=cand_better, epsilon=0.0, min_trees=3)
        self.assertTrue(res_sufficient["promoted"])
        self.assertEqual(res_sufficient["verdict"], "PROMOVIDO")
        self.assertEqual(optimizer2.active_policy.version, "v2.0.0")


if __name__ == "__main__":
    unittest.main()
