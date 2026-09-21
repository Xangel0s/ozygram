"""
Dream-RSI & Monte Carlo Tree Search (MCTS) Engine for Ozygram / Ozymem.
Enables counterfactual offline simulation and Recursive Self-Improvement (RSI)
over historical trajectory trees at zero token cost.
"""

from ozy_brain.dream.policy import BaseExplorationPolicy, MctsExplorationPolicy
from ozy_brain.dream.evaluator import SimulationReport, evaluate_trajectory_simulation
from ozy_brain.dream.simulator import ReplaySimulator

__all__ = [
    "BaseExplorationPolicy",
    "MctsExplorationPolicy",
    "SimulationReport",
    "evaluate_trajectory_simulation",
    "ReplaySimulator",
]
