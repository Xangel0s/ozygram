"""
Evaluation and Fitness Metrics for Dream-RSI Simulation.
Calculates objective policy fitness J(π) and diagnoses exploration bottlenecks.
"""

from dataclasses import dataclass, field
from typing import Any, Dict, List


@dataclass
class SimulationReport:
    trajectory_id: str
    policy_name: str
    policy_version: str
    solved: bool
    steps_taken: int
    baseline_steps: int
    steps_saved: int
    tokens_consumed: int
    tokens_saved: int
    pruning_errors: int
    fitness_score: float
    bottlenecks: List[str] = field(default_factory=list)

    def to_dict(self) -> Dict[str, Any]:
        return {
            "trajectory_id": self.trajectory_id,
            "policy_name": self.policy_name,
            "policy_version": self.policy_version,
            "solved": self.solved,
            "steps_taken": self.steps_taken,
            "baseline_steps": self.baseline_steps,
            "steps_saved": self.steps_saved,
            "tokens_consumed": self.tokens_consumed,
            "tokens_saved": self.tokens_saved,
            "pruning_errors": self.pruning_errors,
            "fitness_score": round(self.fitness_score, 2),
            "bottlenecks": self.bottlenecks,
        }


def evaluate_trajectory_simulation(
    trajectory_id: str,
    policy_name: str,
    policy_version: str,
    solved: bool,
    simulated_path: List[Dict[str, Any]],
    baseline_nodes: List[Dict[str, Any]],
    pruning_errors: int,
) -> SimulationReport:
    """
    Calcula el fitness score J(π) y diagnostica cuellos de botella de la política.
    """
    steps_taken = max(1, len(simulated_path))
    baseline_steps = max(1, len(baseline_nodes))
    steps_saved = baseline_steps - steps_taken

    tokens_consumed = sum(int(n.get("cost_tokens", 0)) for n in simulated_path)
    baseline_tokens = sum(int(n.get("cost_tokens", 0)) for n in baseline_nodes)
    tokens_saved = baseline_tokens - tokens_consumed

    # Formulación formal de Fitness J(π):
    # Base: 100 si resuelve, penalización por ratio de pasos y podados falsos
    base_reward = 100.0 if solved else 0.0
    step_penalty = 25.0 * (steps_taken / baseline_steps)
    prune_penalty = 150.0 * pruning_errors
    token_bonus = max(-20.0, min(30.0, tokens_saved / 500.0))

    fitness_score = base_reward - step_penalty - prune_penalty + token_bonus

    # Diagnóstico de cuellos de botella para el bucle de optimización
    bottlenecks = []
    if not solved:
        bottlenecks.append(
            "La política no alcanzó ningún nodo solución en las ramas exploradas."
        )
    if pruning_errors > 0:
        bottlenecks.append(
            f"La política podó prematuramente {pruning_errors} rama(s) que conducían a la solución."
        )
    if steps_taken > baseline_steps:
        bottlenecks.append(
            f"La búsqueda exploró {steps_taken - baseline_steps} paso(s) de más en comparación con el baseline."
        )

    # Detectar si se perdió tiempo en nodos con errores repetidos
    failed_steps = [
        n for n in simulated_path if float(n.get("reward_score", 0.0)) < 0.0
    ]
    if len(failed_steps) >= 2:
        bottlenecks.append(
            f"Se ejecutaron {len(failed_steps)} pasos con recompensa negativa antes de cambiar de rama."
        )

    return SimulationReport(
        trajectory_id=trajectory_id,
        policy_name=policy_name,
        policy_version=policy_version,
        solved=solved,
        steps_taken=steps_taken,
        baseline_steps=baseline_steps,
        steps_saved=steps_saved,
        tokens_consumed=tokens_consumed,
        tokens_saved=tokens_saved,
        pruning_errors=pruning_errors,
        fitness_score=fitness_score,
        bottlenecks=bottlenecks,
    )
