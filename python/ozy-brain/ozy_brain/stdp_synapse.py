"""
Spike-Timing-Dependent Plasticity (STDP) Synapse Engine for OzyBrain.
Calculates directional synaptic weight updates between code modules based on temporal causality:
LTP (Long-Term Potentiation): Delta_w = A_+ * exp(-Delta_t / tau_+) if Delta_t > 0
LTD (Long-Term Depression):   Delta_w = -A_- * exp(Delta_t / tau_-) if Delta_t <= 0
Persists synaptic matrices in analytics.duckdb to discover implicit cross-module coupling.
Zero-emoji compliant.
"""

from __future__ import annotations

import math
import os
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple

try:
    import duckdb
    HAS_DUCKDB = True
except ImportError:
    HAS_DUCKDB = False


def normalize_rel_path(path: str) -> str:
    normalized = path.strip().replace("\\", "/")
    while normalized.startswith("./"):
        normalized = normalized[2:]
    return normalized


class StdpSynapseEngine:
    """Discovers hidden Hebbian connections between modules using STDP."""

    A_PLUS = 1.0       # Max potentiation amplitude
    A_MINUS = 0.5      # Max depression amplitude
    TAU_PLUS = 600.0   # 10 minute decay constant for causal transitions
    TAU_MINUS = 300.0  # 5 minute decay constant for anti-causal transitions
    LEARNING_RATE = 0.20

    def __init__(
        self,
        workspace_root: Optional[str | Path] = None,
        duckdb_path: Optional[str | Path] = None,
    ):
        self.workspace_root = Path(workspace_root or os.getcwd()).resolve()
        self.duckdb_path = Path(duckdb_path) if duckdb_path else self._resolve_duckdb_path()
        self._init_duckdb_schema()

    def _resolve_duckdb_path(self) -> Path:
        local_dir = self.workspace_root / ".ozymem"
        local_dir.mkdir(parents=True, exist_ok=True)
        return local_dir / "analytics.duckdb"

    def _init_duckdb_schema(self):
        """Initializes stdp_synapses table in DuckDB."""
        if not HAS_DUCKDB:
            return
        try:
            con = duckdb.connect(str(self.duckdb_path))
            con.execute(
                """CREATE TABLE IF NOT EXISTS stdp_synapses (
                    source_file VARCHAR NOT NULL,
                    target_file VARCHAR NOT NULL,
                    weight DOUBLE NOT NULL,
                    sample_count BIGINT NOT NULL DEFAULT 1,
                    last_updated TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                    PRIMARY KEY(source_file, target_file)
                );"""
            )
            con.close()
        except Exception:
            pass

    @classmethod
    def calculate_stdp_delta(cls, dt_seconds: float) -> float:
        """Calculates synaptic weight adjustment Delta_w based on time interval Delta_t = t_j - t_i."""
        if dt_seconds > 0:
            # Pre-synaptic spike precedes post-synaptic spike -> LTP
            return cls.A_PLUS * math.exp(-dt_seconds / cls.TAU_PLUS)
        else:
            # Post-synaptic spike precedes pre-synaptic or simultaneous -> LTD
            return -cls.A_MINUS * math.exp(dt_seconds / cls.TAU_MINUS)

    def record_synapse_spike(
        self,
        source_file: str,
        target_file: str,
        dt_seconds: float,
    ) -> float:
        """Records an observed pair transition and updates the synaptic weight in DuckDB."""
        src = normalize_rel_path(source_file)
        tgt = normalize_rel_path(target_file)
        if src == tgt or not HAS_DUCKDB:
            return 0.0

        delta_w = self.calculate_stdp_delta(dt_seconds)

        try:
            con = duckdb.connect(str(self.duckdb_path))
            cur = con.cursor()
            cur.execute(
                "SELECT weight, sample_count FROM stdp_synapses WHERE source_file = ? AND target_file = ?",
                [src, tgt],
            )
            row = cur.fetchone()

            if row:
                old_w, count = row[0], row[1]
                new_w = max(0.01, min(1.0, old_w + self.LEARNING_RATE * delta_w))
                cur.execute(
                    """UPDATE stdp_synapses
                       SET weight = ?, sample_count = sample_count + 1, last_updated = CURRENT_TIMESTAMP
                       WHERE source_file = ? AND target_file = ?""",
                    [new_w, src, tgt],
                )
            else:
                initial_w = max(0.05, min(1.0, 0.50 + self.LEARNING_RATE * delta_w))
                cur.execute(
                    """INSERT INTO stdp_synapses (source_file, target_file, weight, sample_count)
                       VALUES (?, ?, ?, 1)""",
                    [src, tgt, initial_w],
                )
                new_w = initial_w

            con.close()
            return new_w
        except Exception:
            return 0.0

    def process_event_trace(self, trace_events: List[Dict[str, Any]]) -> Dict[str, Any]:
        """Processes an ordered trace of file touch events and updates STDP synapses."""
        if len(trace_events) < 2 or not HAS_DUCKDB:
            return {"synapses_updated": 0}

        # Sort events by timestamp
        sorted_events = sorted(trace_events, key=lambda e: float(e.get("timestamp", 0.0)))
        updated_count = 0

        for i in range(len(sorted_events) - 1):
            e1 = sorted_events[i]
            e2 = sorted_events[i + 1]
            f1 = e1.get("file_path", "")
            f2 = e2.get("file_path", "")
            if not f1 or not f2 or f1 == f2:
                continue

            dt = float(e2.get("timestamp", 0.0)) - float(e1.get("timestamp", 0.0))
            if abs(dt) <= self.TAU_PLUS * 2:  # within correlation horizon
                self.record_synapse_spike(f1, f2, dt)
                updated_count += 1

        return {"synapses_updated": updated_count}

    def get_strongest_synapses_for_file(
        self,
        file_path: str,
        min_weight: float = 0.50,
        limit: int = 5,
    ) -> List[Dict[str, Any]]:
        """Retrieves downstream files with strongest synaptic coupling."""
        src = normalize_rel_path(file_path)
        if not HAS_DUCKDB:
            return []

        try:
            con = duckdb.connect(str(self.duckdb_path))
            cur = con.cursor()
            cur.execute(
                """SELECT target_file, weight, sample_count
                   FROM stdp_synapses
                   WHERE source_file = ? AND weight >= ?
                   ORDER BY weight DESC, sample_count DESC
                   LIMIT ?""",
                [src, min_weight, limit],
            )
            rows = cur.fetchall()
            con.close()

            return [
                {
                    "source_file": src,
                    "target_file": r[0],
                    "weight": round(float(r[1]), 3),
                    "sample_count": int(r[2]),
                }
                for r in rows
            ]
        except Exception:
            return []
