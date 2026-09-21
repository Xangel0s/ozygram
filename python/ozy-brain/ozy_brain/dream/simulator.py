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
    Simulador determinista de reproducción contrafactual.
    Navega árboles de exploración preexistentes evaluando decisiones de políticas.
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
        Ejecuta la simulación contrafactual de la política sobre el árbol histórico.
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

        # 2. Identificar ancestros de nodos que contienen la solución
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

        # 3. Navegación contrafactual del árbol según la política
        roots = children_by_parent.get(None, [])
        if not roots:
            # Si no hay raíz con parent_id None, tomar el nodo con menor profundidad
            roots = [min(nodes, key=lambda n: int(n.get("depth", 0)))]

        current_node = policy.select_best_child({"visit_count": len(nodes)}, roots)
        if not current_node:
            current_node = roots[0]

        simulated_path: List[Dict[str, Any]] = [current_node]
        pruning_errors = 0
        solved = bool(current_node.get("is_solution"))

        visited_ids = {current_node["id"]}

        for _ in range(max_steps):
            if solved:
                break

            children = children_by_parent.get(current_node["id"], [])
            if not children:
                # Rama terminal u hoja sin hijos
                break

            # Comprobar si la política poda erróneamente una rama que conducía a la solución
            for child in children:
                if policy.should_prune(child):
                    if (
                        child["id"] in solution_node_ids
                        or child["id"] in solution_ancestors
                    ):
                        pruning_errors += 1

            # Seleccionar siguiente nodo con la política
            next_node = policy.select_best_child(current_node, children)
            if not next_node or next_node["id"] in visited_ids:
                break

            visited_ids.add(next_node["id"])
            simulated_path.append(next_node)
            current_node = next_node

            if current_node.get("is_solution"):
                solved = True
                break

        return evaluate_trajectory_simulation(
            trajectory_id=trajectory_id,
            policy_name=policy.name,
            policy_version=policy.version,
            solved=solved,
            simulated_path=simulated_path,
            baseline_nodes=nodes,
            pruning_errors=pruning_errors,
        )
