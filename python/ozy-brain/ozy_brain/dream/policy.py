"""
Exploration Policy Interface and Monte Carlo Tree Search (MCTS) Implementation.
Defines the Code-as-Policy contract for offline trajectory evaluation and auto-refactoring.
"""

from abc import ABC, abstractmethod
import math
from typing import Any, Dict, List, Optional


class BaseExplorationPolicy(ABC):
    """Interfaz abstracta para políticas de exploración de árbol (Code-as-Policy)."""

    def __init__(self, name: str = "base_policy", version: str = "v1.0.0"):
        self.name = name
        self.version = version

    @abstractmethod
    def compute_uct_score(self, node: Dict[str, Any], parent_visits: int) -> float:
        """Calcula el score de prioridad UCT / UCB1 para un nodo."""
        pass

    @abstractmethod
    def should_prune(self, node: Dict[str, Any]) -> bool:
        """Determina si una rama debe ser podada de la exploración."""
        pass

    @abstractmethod
    def select_best_child(
        self, parent_node: Dict[str, Any], children: List[Dict[str, Any]]
    ) -> Optional[Dict[str, Any]]:
        """Selecciona el mejor nodo hijo a expandir o profundizar según la política."""
        pass


class MctsExplorationPolicy(BaseExplorationPolicy):
    """
    Política de exploración basada en Monte Carlo Tree Search (MCTS).
    Aplica el algoritmo UCB1 (Upper Confidence Bound 1 for Trees)
    con penalización de podado de ramas de riesgo.
    """

    def __init__(
        self,
        name: str = "mcts_policy",
        version: str = "v1.0.0",
        exploration_constant: float = math.sqrt(2.0),
        prune_threshold: float = -3.0,
        max_depth_without_progress: int = 8,
    ):
        super().__init__(name=name, version=version)
        self.exploration_constant = exploration_constant
        self.prune_threshold = prune_threshold
        self.max_depth_without_progress = max_depth_without_progress

    def compute_uct_score(self, node: Dict[str, Any], parent_visits: int) -> float:
        """
        Fórmula UCT: Q(s, a) + c * sqrt(ln(N_parent) / N_node)
        """
        visit_count = max(1, int(node.get("visit_count", 1)))
        value_estimate = float(node.get("value_estimate", node.get("reward_score", 0.0)))
        p_visits = max(1, parent_visits)

        exploitation = value_estimate
        exploration = self.exploration_constant * math.sqrt(math.log(p_visits) / visit_count)

        # Bonus si el nodo está marcado como solución
        if node.get("is_solution"):
            exploitation += 50.0

        return exploitation + exploration

    def should_prune(self, node: Dict[str, Any]) -> bool:
        """Poda ramas marcadas explícitamente, con Q-value degradado o profundidad estancada."""
        if node.get("is_pruned", False):
            return True

        val = float(node.get("value_estimate", node.get("reward_score", 0.0)))
        if val <= self.prune_threshold:
            return True

        depth = int(node.get("depth", 0))
        if depth >= self.max_depth_without_progress and not node.get("is_solution", False):
            return True

        return False

    def select_best_child(
        self, parent_node: Dict[str, Any], children: List[Dict[str, Any]]
    ) -> Optional[Dict[str, Any]]:
        """Elige el nodo hijo no podado con el mayor score UCT."""
        if not children:
            return None

        p_visits = int(parent_node.get("visit_count", 1))
        best_child = None
        best_score = -float("inf")

        for child in children:
            if self.should_prune(child):
                continue

            score = self.compute_uct_score(child, p_visits)
            if score > best_score:
                best_score = score
                best_child = child

        # Si todos están podados o ninguno superó el filtro, tomar el de menor penalización
        if best_child is None and children:
            best_child = max(
                children,
                key=lambda c: float(c.get("value_estimate", c.get("reward_score", 0.0))),
            )

        return best_child
