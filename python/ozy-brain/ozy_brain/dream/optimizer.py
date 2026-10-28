"""
Dream-RSI Optimizer and Code-as-Policy Refactoring Loop.
Performs counterfactual evaluation, AST safety verification, and safe promotion.
"""

import ast
from typing import Any, Dict, List, Optional

from ozy_brain.dream.evaluator import SimulationReport
from ozy_brain.dream.policy import BaseExplorationPolicy, MctsExplorationPolicy
from ozy_brain.dream.simulator import ReplaySimulator


class AstSafetyAuditor(ast.NodeVisitor):
    """Audita el AST de código de políticas candidatas para bloquear operaciones inseguras."""

    FORBIDDEN_CALLS = {
        "system",
        "popen",
        "spawn",
        "exec",
        "eval",
        "unlink",
        "remove",
        "rmdir",
        "rmtree",
    }
    FORBIDDEN_MODULES = {"subprocess", "socket", "shutil", "ctypes"}

    def __init__(self):
        self.violations: List[str] = []

    def visit_Import(self, node: ast.Import):
        for alias in node.names:
            if alias.name in self.FORBIDDEN_MODULES:
                self.violations.append(f"Módulo no permitido importado: {alias.name}")
        self.generic_visit(node)

    def visit_ImportFrom(self, node: ast.ImportFrom):
        if node.module in self.FORBIDDEN_MODULES:
            self.violations.append(f"Módulo no permitido importado: {node.module}")
        self.generic_visit(node)

    def visit_Call(self, node: ast.Call):
        if isinstance(node.func, ast.Name) and node.func.id in self.FORBIDDEN_CALLS:
            self.violations.append(f"Función no permitida invocada: {node.func.id}")
        elif isinstance(node.func, ast.Attribute) and node.func.attr in self.FORBIDDEN_CALLS:
            self.violations.append(f"Método no permitido invocado: {node.func.attr}")
        self.generic_visit(node)


def verify_policy_code_safety(code: str) -> tuple[bool, List[str]]:
    """Valida la sintaxis AST y las restricciones de seguridad de una política candidata."""
    try:
        parsed = ast.parse(code)
    except SyntaxError as e:
        return False, [f"Error de sintaxis AST en política candidata: {e}"]

    auditor = AstSafetyAuditor()
    auditor.visit(parsed)

    if auditor.violations:
        return False, auditor.violations
    return True, []


class DreamRsiOptimizer:
    """
    Optimizador recursivo de políticas de exploración.
    Evalúa políticas contrafactualmente y promueve mejoras sin riesgo de regresión.
    """

    def __init__(
        self,
        db_path: Optional[str] = None,
        active_policy: Optional[BaseExplorationPolicy] = None,
    ):
        self.simulator = ReplaySimulator(db_path=db_path)
        self.active_policy = active_policy or MctsExplorationPolicy(version="v1.0.0")

    def run_optimization_round(
        self,
        trajectories: List[Dict[str, Any]],
        candidate_policy: Optional[BaseExplorationPolicy] = None,
        epsilon: float = 1.0,
        validation_split: float = 0.30,
    ) -> Dict[str, Any]:
        """
        Evalúa la política activa vs la candidata a través de un lote de trayectorias.
        Utiliza partición Train/Validation (Hold-Out Set) para evitar sobreajuste.
        Aplica promoción segura si J_val(candidate) > J_val(active) + epsilon sin errores de poda.
        """
        if not trajectories:
            return {
                "promoted": False,
                "reason": "No hay trayectorias históricas para evaluar.",
                "active_score": 0.0,
                "candidate_score": 0.0,
            }

        # Partición Train / Validation para evitar sobreajuste
        if len(trajectories) >= 4 and 0.0 < validation_split < 1.0:
            split_idx = max(1, int(len(trajectories) * (1.0 - validation_split)))
            train_trajs = trajectories[:split_idx]
            val_trajs = trajectories[split_idx:]
        else:
            train_trajs = trajectories
            val_trajs = trajectories

        # 1. Evaluar política activa sobre conjunto de validación
        active_reports: List[SimulationReport] = []
        for traj in val_trajs:
            rep = self.simulator.simulate(
                self.active_policy,
                trajectory_id=traj.get("id", "traj_0"),
                nodes=traj.get("nodes"),
            )
            active_reports.append(rep)

        avg_active_score = sum(r.fitness_score for r in active_reports) / len(
            active_reports
        )

        if candidate_policy is None:
            # Si no hay candidata, generar diagnósticos sobre la activa
            all_bottlenecks = [b for r in active_reports for b in r.bottlenecks]
            return {
                "promoted": False,
                "active_score": round(avg_active_score, 2),
                "active_policy_version": self.active_policy.version,
                "reports_evaluated": len(active_reports),
                "bottlenecks": list(set(all_bottlenecks)),
                "message": "Evaluación de línea base completada.",
            }

        # 2. Evaluar política candidata en conjunto de validación retenido
        candidate_reports: List[SimulationReport] = []
        for traj in val_trajs:
            rep = self.simulator.simulate(
                candidate_policy,
                trajectory_id=traj.get("id", "traj_0"),
                nodes=traj.get("nodes"),
            )
            candidate_reports.append(rep)

        avg_candidate_score = sum(r.fitness_score for r in candidate_reports) / len(
            candidate_reports
        )
        total_pruning_errors = sum(r.pruning_errors for r in candidate_reports)

        # 3. Regla Estricta de Promoción (No-Regresión)
        improvement = avg_candidate_score - avg_active_score
        is_strictly_better = improvement >= epsilon and total_pruning_errors == 0

        if is_strictly_better:
            self.active_policy = candidate_policy

        return {
            "promoted": is_strictly_better,
            "active_score": round(avg_active_score, 2),
            "candidate_score": round(avg_candidate_score, 2),
            "improvement": round(improvement, 2),
            "total_pruning_errors": total_pruning_errors,
            "active_version": self.active_policy.version,
            "reports": [r.to_dict() for r in candidate_reports],
            "message": (
                f"Política promovida exitosamente a {candidate_policy.version} (+{improvement:.2f} pts)"
                if is_strictly_better
                else f"Política candidata rechazada: mejora insuficiente ({improvement:.2f} pts) o errores de podado ({total_pruning_errors})."
            ),
        }
