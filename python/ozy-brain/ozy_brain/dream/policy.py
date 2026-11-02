"""
Exploration Policy Interface and Monte Carlo Tree Search (MCTS) Implementation.
Defines the Code-as-Policy contract for offline trajectory evaluation and auto-refactoring.
"""

from abc import ABC, abstractmethod
from dataclasses import dataclass
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


@dataclass
class PolicyParameters:
    """Parámetros cuantitativos de la política de exploración en frontera."""
    c_exploration: float = 1.4142
    w_tokens: float = 0.001          # Penalización por token gastado (escala /100)
    w_depth: float = 0.1             # Penalización por profundidad en árbol
    w_blast: float = 0.5             # Penalización por radio de impacto/blast radius
    w_failure_class: float = 2.0     # Penalización por clases de fallo o recompensas negativas
    prune_threshold: float = -3.0
    max_depth_without_progress: int = 8


class ParametricExplorationPolicy(BaseExplorationPolicy):
    """
    Política de exploración paramétrica evaluable sobre la frontera de árboles (Etapa 3).
    Fórmula de scoring sobre la frontera del árbol:
    Score(u) = Q(u) + c * sqrt(ln(N_p) / N_u) - w_tokens * (cost / 100) - w_depth * depth - w_blast * blast - w_fail * is_fail + bonus_sol
    """

    def __init__(
        self,
        name: str = "parametric_mcts",
        version: str = "v1.0.0",
        params: Optional[PolicyParameters] = None,
        c_exploration: float = 1.4142,
        w_tokens: float = 0.001,
        w_depth: float = 0.1,
        w_blast: float = 0.5,
        w_failure_class: float = 2.0,
        prune_threshold: float = -3.0,
        max_depth_without_progress: int = 8,
    ):
        super().__init__(name=name, version=version)
        if params is not None:
            self.params = params
        else:
            self.params = PolicyParameters(
                c_exploration=c_exploration,
                w_tokens=w_tokens,
                w_depth=w_depth,
                w_blast=w_blast,
                w_failure_class=w_failure_class,
                prune_threshold=prune_threshold,
                max_depth_without_progress=max_depth_without_progress,
            )

    @property
    def exploration_constant(self) -> float:
        return self.params.c_exploration

    def compute_frontier_score(
        self,
        node: Dict[str, Any],
        parent: Optional[Dict[str, Any]] = None,
        tree_visits: int = 1,
    ) -> float:
        """
        Calcula el score de prioridad sobre cualquier nodo candidato de la frontera.
        Evalúa: Q-value, exploración UCT, consumo de tokens, profundidad, radio de blast y señales de fallo.
        """
        val = float(node.get("value_estimate", node.get("reward_score", 0.0)))
        n_node = max(1, int(node.get("visit_count", 1)))
        n_parent = max(1, int(parent.get("visit_count", 1))) if parent else max(1, tree_visits)
        exploration = self.params.c_exploration * math.sqrt(math.log(n_parent) / n_node)

        tokens = float(node.get("cost_tokens", 0))
        token_penalty = self.params.w_tokens * (tokens / 100.0)

        depth = float(node.get("depth", 0))
        depth_penalty = self.params.w_depth * depth

        blast = float(node.get("blast_ratio", 0.1))
        blast_penalty = self.params.w_blast * blast

        is_fail = 1.0 if (node.get("failure_class") or float(node.get("reward_score", 0.0)) < 0.0) else 0.0
        fail_penalty = self.params.w_failure_class * is_fail

        sol_bonus = 50.0 if node.get("is_solution") else 0.0

        return val + exploration + sol_bonus - token_penalty - depth_penalty - blast_penalty - fail_penalty

    def compute_uct_score(self, node: Dict[str, Any], parent_visits: int) -> float:
        return self.compute_frontier_score(node, parent={"visit_count": parent_visits}, tree_visits=parent_visits)

    def should_prune(self, node: Dict[str, Any]) -> bool:
        if node.get("is_pruned", False):
            return True
        val = float(node.get("value_estimate", node.get("reward_score", 0.0)))
        if val <= self.params.prune_threshold:
            return True
        depth = int(node.get("depth", 0))
        if depth >= self.params.max_depth_without_progress and not node.get("is_solution", False):
            return True
        return False

    def select_best_child(
        self, parent_node: Dict[str, Any], children: List[Dict[str, Any]]
    ) -> Optional[Dict[str, Any]]:
        if not children:
            return None
        valid = [c for c in children if not self.should_prune(c)]
        if not valid:
            valid = children
        return max(valid, key=lambda c: self.compute_frontier_score(c, parent=parent_node))
