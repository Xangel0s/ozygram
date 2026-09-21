"""
Unit tests for Dream-RSI and Monte Carlo Tree Search (MCTS) replay simulation.
"""

import pytest
from ozy_brain.dream.policy import MctsExplorationPolicy
from ozy_brain.dream.simulator import ReplaySimulator
from ozy_brain.dream.evaluator import SimulationReport


def test_mcts_policy_uct_score():
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

    assert score_promising > score_unpromising
    assert score_promising > 8.0


def test_mcts_policy_pruning():
    policy = MctsExplorationPolicy(prune_threshold=-3.0)

    node_bad = {"id": "n1", "value_estimate": -4.0, "is_pruned": False}
    node_marked = {"id": "n2", "value_estimate": 1.0, "is_pruned": True}
    node_good = {"id": "n3", "value_estimate": 2.5, "is_pruned": False}

    assert policy.should_prune(node_bad) is True
    assert policy.should_prune(node_marked) is True
    assert policy.should_prune(node_good) is False


def test_replay_simulator_counterfactual_execution():
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

    assert isinstance(report, SimulationReport)
    assert report.solved is True
    assert report.steps_taken == 3  # root -> b1 -> b2
    assert report.baseline_steps == 5
    assert report.steps_saved == 2
    assert report.pruning_errors == 0
    assert report.fitness_score > 70.0


def test_replay_simulator_pruning_error_penalty():
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
    assert report.pruning_errors >= 1
    # Severe penalty applied: fitness score should be negative
    assert report.fitness_score < 0.0
    assert len(report.bottlenecks) >= 1
