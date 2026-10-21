"""
Cerebellum Predictive Forward Model Engine for OzyBrain.
Computes an internal kinematic forward model of proposed code modifications,
calculates Kullback-Leibler divergence (D_KL) against historical module co-change
distributions from DuckDB STDP synapses, and detects Code Dysmetria:
  - Motor Overshoot (Hypermetria): Candidate patch touches unexpectedly distant or excessive modules.
  - Motor Undershoot / Under-Coupled (Hypometria): Candidate patch neglects strongly coupled dependencies.
Zero-emoji compliant.
"""

from __future__ import annotations

import math
import os
from collections import Counter
from pathlib import Path
from typing import Any, Dict, List, Optional, Set, Tuple

try:
    import duckdb
    HAS_DUCKDB = True
except ImportError:
    HAS_DUCKDB = False


def normalize_rel_path(path: str) -> str:
    """Normalizes relative file path across Windows and POSIX."""
    normalized = path.strip().replace("\\", "/")
    while normalized.startswith("./"):
        normalized = normalized[2:]
    return normalized


def extract_subsystem(file_path: str) -> str:
    """Extracts top-level or secondary subsystem/module from file path."""
    norm = normalize_rel_path(file_path)
    parts = norm.split("/")
    if len(parts) <= 1:
        return "root"
    if parts[0] in ("crates", "python", "crmnew", "src", "packages") and len(parts) > 2:
        return f"{parts[0]}/{parts[1]}"
    return parts[0]


class CerebellumEngine:
    """Evaluates candidate actions and patches using a Cerebellar Forward Model."""

    D_KL_OVERSHOOT_THRESHOLD = 1.80
    HIGH_COUPLING_THRESHOLD = 0.70
    LAPLACE_EPSILON = 1e-5

    def __init__(
        self,
        workspace_root: Optional[str | Path] = None,
        duckdb_path: Optional[str | Path] = None,
        tenant_id: str = "local",
    ):
        self.workspace_root = Path(workspace_root or os.getcwd()).resolve()
        self.duckdb_path = Path(duckdb_path) if duckdb_path else self._resolve_duckdb_path()
        self.tenant_id = tenant_id

    def _resolve_duckdb_path(self) -> Path:
        local_dir = self.workspace_root / ".ozymem"
        local_dir.mkdir(parents=True, exist_ok=True)
        return local_dir / "analytics.duckdb"

    def get_historical_module_distribution(
        self, target_file: str
    ) -> Tuple[Dict[str, float], List[Dict[str, Any]], float]:
        """Queries historical STDP synapses for target_file to derive P(X) distribution over subsystems.

        Returns:
            (distribution_dict, strong_synapses_list, historical_mean_files)
        """
        norm_target = normalize_rel_path(target_file)
        target_subsystem = extract_subsystem(norm_target)

        subsystem_weights: Dict[str, float] = {target_subsystem: 1.0}
        strong_synapses: List[Dict[str, Any]] = []
        historical_files_count = 1.0

        if not HAS_DUCKDB or not self.duckdb_path.exists():
            # Return baseline prior centered on target subsystem
            return self._normalize_distribution(subsystem_weights), strong_synapses, historical_files_count

        try:
            con = duckdb.connect(str(self.duckdb_path))
            cur = con.cursor()
            cur.execute(
                """SELECT target_file, weight, sample_count
                   FROM stdp_synapses
                   WHERE source_file = ? AND tenant_id = ?
                   ORDER BY weight DESC""",
                [norm_target, self.tenant_id],
            )
            rows = cur.fetchall()
            con.close()

            if rows:
                historical_files_count = 1.0 + float(len(rows))
                for tgt_file, weight, sample_cnt in rows:
                    sub = extract_subsystem(str(tgt_file))
                    subsystem_weights[sub] = subsystem_weights.get(sub, 0.0) + float(weight)
                    if float(weight) >= self.HIGH_COUPLING_THRESHOLD:
                        strong_synapses.append({
                            "target_file": str(tgt_file),
                            "weight": float(weight),
                            "sample_count": int(sample_cnt),
                        })
        except Exception:
            pass

        return self._normalize_distribution(subsystem_weights), strong_synapses, historical_files_count

    def _normalize_distribution(self, weights: Dict[str, float]) -> Dict[str, float]:
        total = sum(weights.values())
        if total <= 0.0:
            return {k: 1.0 / len(weights) for k in weights}
        return {k: v / total for k, v in weights.items()}

    def compute_kullback_leibler_divergence(
        self,
        p_dist: Dict[str, float],
        q_dist: Dict[str, float],
    ) -> float:
        """Computes D_KL(Q || P) = sum_x Q(x) * ln((Q(x) + eps) / (P(x) + eps)).

        Measures information divergence of candidate patch distribution Q from prior P.
        """
        all_subsystems = set(p_dist.keys()) | set(q_dist.keys())
        if not all_subsystems:
            return 0.0

        kld = 0.0
        eps = self.LAPLACE_EPSILON

        for sub in all_subsystems:
            p_val = p_dist.get(sub, 0.0)
            q_val = q_dist.get(sub, 0.0)

            # Only terms where Q(x) > 0 contribute to D_KL(Q || P)
            if q_val > 0.0:
                ratio = (q_val + eps) / (p_val + eps)
                kld += q_val * math.log(ratio)

        return max(0.0, float(kld))

    def evaluate_patch_dysmetria(
        self,
        target_file: str,
        proposed_files: Optional[List[str]] = None,
        diff_lines: int = 0,
    ) -> Dict[str, Any]:
        """Evaluates a candidate patch or modification plan for Code Dysmetria."""
        norm_target = normalize_rel_path(target_file) if target_file else ""
        norm_proposed = [normalize_rel_path(f) for f in (proposed_files or []) if f.strip()]

        if norm_target and norm_target not in norm_proposed:
            norm_proposed.insert(0, norm_target)

        # 1. Historical prior P(X)
        p_dist, strong_synapses, hist_mean_files = self.get_historical_module_distribution(norm_target)

        # 2. Candidate patch distribution Q(X)
        if not norm_proposed:
            q_dist = {extract_subsystem(norm_target): 1.0} if norm_target else {"root": 1.0}
        else:
            counts = Counter(extract_subsystem(f) for f in norm_proposed)
            total_proposed = sum(counts.values())
            q_dist = {sub: count / total_proposed for sub, count in counts.items()}

        # 3. Kullback-Leibler Divergence
        kld = self.compute_kullback_leibler_divergence(p_dist, q_dist)

        # 4. Detect Dysmetria
        alerts: List[str] = []
        status = "calibrated"
        badge = "[STATUS: MOTOR_CALIBRATED]"

        # A. Check for Motor Overshoot (Hypermetria)
        proposed_count = len(norm_proposed)
        is_overshoot = (
            (kld >= self.D_KL_OVERSHOOT_THRESHOLD and proposed_count > 3)
            or (proposed_count >= 8 and proposed_count > (hist_mean_files * 2.5))
        )

        if is_overshoot:
            status = "overshoot"
            badge = "[ALERT: CODE_DYSMETRIA - MOTOR_OVERSHOOT]"
            divergent_subsystems = [
                sub for sub, q_val in q_dist.items()
                if q_val > 0.15 and p_dist.get(sub, 0.0) < 0.05
            ]
            divergent_msg = (
                f" Divergent subsystems touched: {divergent_subsystems}."
                if divergent_subsystems else ""
            )
            alerts.append(
                f"[ALERT: CODE_DYSMETRIA - MOTOR_OVERSHOOT] Proposed modification spans {proposed_count} files "
                f"(historical expected ~{hist_mean_files:.1f}) with divergence D_KL={kld:.2f}.{divergent_msg}"
            )

        # B. Check for Motor Undershoot / Under-Coupled (Hypometria)
        missing_coupled: List[str] = []
        proposed_set = set(norm_proposed)
        for syn in strong_synapses:
            coupled_file = syn["target_file"]
            if coupled_file not in proposed_set:
                missing_coupled.append(coupled_file)

        if missing_coupled:
            # If not already flagged as overshoot, flag as under-coupled
            if status == "calibrated":
                status = "under_coupled"
                badge = "[ALERT: CODE_DYSMETRIA - UNDER_COUPLED]"
            alerts.append(
                f"[ALERT: CODE_DYSMETRIA - UNDER_COUPLED] Proposed modification to '{norm_target}' "
                f"omits strongly coupled dependencies: {missing_coupled}. "
                f"Historical Hebbian weight >= {self.HIGH_COUPLING_THRESHOLD}."
            )

        return {
            "status": status,
            "badge": badge,
            "kld": round(kld, 4),
            "is_dysmetric": status != "calibrated",
            "historical_mean_files": hist_mean_files,
            "proposed_files_count": proposed_count,
            "p_distribution": {k: round(v, 4) for k, v in p_dist.items()},
            "q_distribution": {k: round(v, 4) for k, v in q_dist.items()},
            "missing_coupled_files": missing_coupled,
            "alerts": alerts,
        }
