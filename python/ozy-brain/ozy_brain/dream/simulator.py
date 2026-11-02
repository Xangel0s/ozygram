"""
Offline Replay Simulator for Dream-RSI.
Simulates counterfactual exploration policies across historical trees at zero LLM token cost.
"""

import sqlite3
from typing import Any, Dict, List, Optional, Set

from ozy_brain.dream.evaluator import SimulationReport, evaluate_trajectory_simulation
from ozy_brain.dream.policy import BaseExplorationPolicy


class ReplaySimulator:
    """
    Simulador determinista de reproducción contrafactual (Etapa 3).
    Navega la frontera de árboles de exploración preexistentes evaluando decisiones de políticas.
    Mide los costos en tokens, pasos y latencia hasta la primera solución verificada.
    """

    def __init__(self, db_path: Optional[str] = None):
        self.db_path = db_path

    def load_trajectory_from_sqlite(
        self, trajectory_id: str
    ) -> List[Dict[str, Any]]:
        """Carga los nodos de una trayectoria desde la base de datos SQLite de Ozymem."""
        if not self.db_path:
            raise ValueError("No se ha configurado db_path para SQLite.")

        conn = sqlite3.connect(self.db_path)
        conn.row_factory = sqlite3.Row
        cursor = conn.cursor()

        cursor.execute(
            """
            SELECT id, trajectory_id, parent_id, depth, action_type, action_payload,
                   observation, cost_tokens, latency_ms, reward_score, visit_count,
                   value_estimate, is_solution, is_pruned, created_at
            FROM exploration_nodes
            WHERE trajectory_id = ?
            ORDER BY depth ASC, created_at ASC
            """,
            (trajectory_id,),
        )

        rows = [dict(row) for row in cursor.fetchall()]
        conn.close()
        return rows

    def simulate(
        self,
        policy: BaseExplorationPolicy,
        trajectory_id: str,
        nodes: Optional[List[Dict[str, Any]]] = None,
        max_steps: int = 50,
    ) -> SimulationReport:
        """
        Ejecuta la simulación contrafactual navegando la frontera activa del árbol registrado.
        Evalúa decisiones en base a profundidad, recompensas, tokens, blast radius y señales de fallo.
        """
        if nodes is None:
            nodes = self.load_trajectory_from_sqlite(trajectory_id)

        if not nodes:
            return evaluate_trajectory_simulation(
                trajectory_id=trajectory_id,
                policy_name=policy.name,
                policy_version=policy.version,
                solved=False,
                simulated_path=[],
                baseline_nodes=[],
                pruning_errors=0,
            )

        # 1. Mapeo de nodos e hijos
        nodes_by_id: Dict[str, Dict[str, Any]] = {n["id"]: n for n in nodes}
        children_by_parent: Dict[Optional[str], List[Dict[str, Any]]] = {}
        for n in nodes:
            pid = n.get("parent_id")
            children_by_parent.setdefault(pid, []).append(n)

        # 2. Identificar ancestros de nodos que contienen soluciones verificadas
        solution_node_ids: Set[str] = {
            n["id"] for n in nodes if n.get("is_solution")
        }
        solution_ancestors: Set[str] = set()

        for sol_id in solution_node_ids:
            curr = nodes_by_id.get(sol_id)
            while curr and curr.get("parent_id"):
                p_id = curr.get("parent_id")
                if p_id:
                    solution_ancestors.add(p_id)
                    curr = nodes_by_id.get(p_id)
                else:
                    break

        # 3. Inicialización de la frontera con las raíces
        roots = children_by_parent.get(None, [])
        if not roots:
            roots = [min(nodes, key=lambda n: int(n.get("depth", 0)))]

        frontier: List[Dict[str, Any]] = list(roots)
        simulated_path: List[Dict[str, Any]] = []
        visited_ids: Set[str] = set()
        pruning_errors = 0
        solved = False
        total_tree_visits = len(nodes)

        # 4. Navegación contrafactual sobre la frontera del árbol
        for _ in range(max_steps):
            if not frontier:
                break

            # Comprobar podas sobre la frontera actual
            for candidate in list(frontier):
                if policy.should_prune(candidate):
                    if (
                        candidate["id"] in solution_node_ids
                        or candidate["id"] in solution_ancestors
                    ):
                        pruning_errors += 1
                    frontier.remove(candidate)

            if not frontier:
                break

            # Seleccionar el mejor nodo candidato en la frontera según la política
            if hasattr(policy, "compute_frontier_score"):
                chosen = max(
                    frontier,
                    key=lambda n: policy.compute_frontier_score(
                        n,
                        parent=nodes_by_id.get(n.get("parent_id")),
                        tree_visits=total_tree_visits,
                    ),
                )
            else:
                chosen = max(
                    frontier,
                    key=lambda n: policy.compute_uct_score(
                        n,
                        parent_visits=int(
                            nodes_by_id.get(n.get("parent_id"), {}).get(
                                "visit_count", total_tree_visits
                            )
                        ),
                    ),
                )

            frontier.remove(chosen)
            visited_ids.add(chosen["id"])
            simulated_path.append(chosen)

            # Si el nodo es una solución verificada, terminar con éxito
            if chosen.get("is_solution"):
                solved = True
                break

            # Expandir los hijos del nodo elegido hacia la frontera
            children = children_by_parent.get(chosen["id"], [])
            for child in children:
                if child["id"] not in visited_ids and child not in frontier:
                    frontier.append(child)

        return evaluate_trajectory_simulation(
            trajectory_id=trajectory_id,
            policy_name=policy.name,
            policy_version=policy.version,
            solved=solved,
            simulated_path=simulated_path,
            baseline_nodes=nodes,
            pruning_errors=pruning_errors,
        )
