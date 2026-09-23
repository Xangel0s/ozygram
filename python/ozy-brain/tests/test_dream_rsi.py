"""Unit tests for Dream-RSI and Monte Carlo Tree Search (MCTS) replay simulation."""

from __future__ import annotations

import unittest

from ozy_brain.brain import run
from ozy_brain.dream.evaluator import SimulationReport
from ozy_brain.dream.optimizer import DreamRsiOptimizer, verify_policy_code_safety
from ozy_brain.dream.policy import MctsExplorationPolicy
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


if __name__ == "__main__":
    unittest.main()
