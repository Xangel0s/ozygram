"""
Spike-Timing-Dependent Plasticity (STDP) Synapse Engine for OzyBrain.
Calculates directional synaptic weight updates between code modules based on temporal causality:
LTP (Long-Term Potentiation): Delta_w = A_+ * exp(-Delta_t / tau_+) if Delta_t > 0
LTD (Long-Term Depression):   Delta_w = -A_- * exp(Delta_t / tau_-) if Delta_t <= 0
Persists synaptic matrices in analytics.duckdb and exports strong virtual Hebbian
edges into SQLite memory_edges (edge_type='COUPLED_WITH') for sub-millisecond graph traversal.
Zero-emoji compliant.
"""

from __future__ import annotations

import math
import os
import sqlite3
import subprocess
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
        db_path: Optional[str | Path] = None,
        tenant_id: str = "local",
    ):
        self.workspace_root = Path(workspace_root or os.getcwd()).resolve()
        self.duckdb_path = Path(duckdb_path) if duckdb_path else self._resolve_duckdb_path()
        self.db_path = Path(db_path) if db_path else self._resolve_db_path()
        self.tenant_id = tenant_id
        self._init_duckdb_schema()

    def _resolve_duckdb_path(self) -> Path:
        local_dir = self.workspace_root / ".ozymem"
        local_dir.mkdir(parents=True, exist_ok=True)
        return local_dir / "analytics.duckdb"

    def _resolve_db_path(self) -> Path:
        local_db = self.workspace_root / ".ozymem" / "memory.db"
        if local_db.exists():
            return local_db
        local_db_sqlite = self.workspace_root / ".ozymem" / "db.sqlite"
        if local_db_sqlite.exists():
            return local_db_sqlite
        return self.workspace_root / ".ozymem" / "memory.db"

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
                    tenant_id VARCHAR NOT NULL DEFAULT 'local',
                    PRIMARY KEY(source_file, target_file, tenant_id)
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
        tenant_id: Optional[str] = None,
        con: Optional[Any] = None,
    ) -> float:
        """Records an observed pair transition and updates the synaptic weight in DuckDB."""
        src = normalize_rel_path(source_file)
        tgt = normalize_rel_path(target_file)
        tenant = tenant_id or self.tenant_id
        if src == tgt or not HAS_DUCKDB:
            return 0.0

        delta_w = self.calculate_stdp_delta(dt_seconds)
        close_con = False

        try:
            if con is None:
                con = duckdb.connect(str(self.duckdb_path))
                close_con = True
            cur = con.cursor()
            cur.execute(
                "SELECT weight, sample_count FROM stdp_synapses WHERE source_file = ? AND target_file = ? AND tenant_id = ?",
                [src, tgt, tenant],
            )
            row = cur.fetchone()

            if row:
                old_w, count = row[0], row[1]
                new_w = max(0.01, min(1.0, old_w + self.LEARNING_RATE * delta_w))
                cur.execute(
                    """UPDATE stdp_synapses
                       SET weight = ?, sample_count = sample_count + 1, last_updated = CURRENT_TIMESTAMP
                       WHERE source_file = ? AND target_file = ? AND tenant_id = ?""",
                    [new_w, src, tgt, tenant],
                )
            else:
                initial_w = max(0.05, min(1.0, 0.50 + self.LEARNING_RATE * delta_w))
                cur.execute(
                    """INSERT INTO stdp_synapses (source_file, target_file, weight, sample_count, tenant_id)
                       VALUES (?, ?, ?, 1, ?)""",
                    [src, tgt, initial_w, tenant],
                )
                new_w = initial_w

            if close_con:
                con.close()
            return new_w
        except Exception:
            if close_con and con is not None:
                try:
                    con.close()
                except Exception:
                    pass
            return 0.0

    def process_event_trace(self, trace_events: List[Dict[str, Any]]) -> Dict[str, Any]:
        """Processes an ordered trace of file touch events and updates STDP synapses."""
        if len(trace_events) < 2 or not HAS_DUCKDB:
            return {"synapses_updated": 0}

        sorted_events = sorted(trace_events, key=lambda e: float(e.get("timestamp", 0.0)))
        updated_count = 0

        try:
            con = duckdb.connect(str(self.duckdb_path))
            for i in range(len(sorted_events) - 1):
                e1 = sorted_events[i]
                e2 = sorted_events[i + 1]
                f1 = e1.get("file_path", "")
                f2 = e2.get("file_path", "")
                if not f1 or not f2 or f1 == f2:
                    continue

                dt = float(e2.get("timestamp", 0.0)) - float(e1.get("timestamp", 0.0))
                if abs(dt) <= self.TAU_PLUS * 2:  # within correlation horizon
                    self.record_synapse_spike(f1, f2, dt, con=con)
                    updated_count += 1
            con.close()
        except Exception:
            pass

        return {"synapses_updated": updated_count}

    def get_strongest_synapses_for_file(
        self,
        file_path: str,
        min_weight: float = 0.50,
        limit: int = 5,
        tenant_id: Optional[str] = None,
    ) -> List[Dict[str, Any]]:
        """Retrieves downstream files with strongest synaptic coupling."""
        src = normalize_rel_path(file_path)
        tenant = tenant_id or self.tenant_id
        if not HAS_DUCKDB:
            return []

        try:
            con = duckdb.connect(str(self.duckdb_path))
            cur = con.cursor()
            cur.execute(
                """SELECT target_file, weight, sample_count
                   FROM stdp_synapses
                   WHERE source_file = ? AND weight >= ? AND tenant_id = ?
                   ORDER BY weight DESC, sample_count DESC
                   LIMIT ?""",
                [src, min_weight, tenant, limit],
            )
            rows = cur.fetchall()
            con.close()

            return [
                {
                    "source_file": src,
                    "target_file": r[0],
                    "weight": round(float(r[1]), 3),
                    "sample_count": int(r[2]),
                    "tenant_id": tenant,
                }
                for r in rows
            ]
        except Exception:
            return []

    def get_strongest_synapses(
        self,
        min_weight: float = 0.50,
        limit: int = 10,
        tenant_id: Optional[str] = None,
    ) -> List[Dict[str, Any]]:
        """Retrieves global strongest synaptic couplings across all files."""
        tenant = tenant_id or self.tenant_id
        if not HAS_DUCKDB:
            return []

        try:
            con = duckdb.connect(str(self.duckdb_path))
            cur = con.cursor()
            cur.execute(
                """SELECT source_file, target_file, weight, sample_count
                   FROM stdp_synapses
                   WHERE weight >= ? AND tenant_id = ?
                   ORDER BY weight DESC, sample_count DESC
                   LIMIT ?""",
                [min_weight, tenant, limit],
            )
            rows = cur.fetchall()
            con.close()

            return [
                {
                    "source_file": r[0],
                    "target_file": r[1],
                    "weight": round(float(r[2]), 3),
                    "sample_count": int(r[3]),
                    "tenant_id": tenant,
                }
                for r in rows
            ]
        except Exception:
            return []


    def sample_coupled_files_thompson(
        self, file_path: str, limit: int = 5
    ) -> List[Dict[str, Any]]:
        """
        Bayesian Thompson Sampling for STDP Synapses:
        Draws theta_sample ~ Beta(alpha, beta) for each coupled candidate,
        balancing exploitation of high-confidence links with exploration of emerging links.
        """
        import random
        synapses = self.get_strongest_synapses_for_file(file_path, limit=limit * 2)
        if not synapses:
            return []

        sampled = []
        for syn in synapses:
            w = max(0.01, min(0.99, float(syn.get("weight", 0.5))))
            alpha = 1.0 + 10.0 * w
            beta = 1.0 + 10.0 * (1.0 - w)
            sample_val = random.betavariate(alpha, beta)
            sampled.append({
                **syn,
                "thompson_sample": round(sample_val, 4),
            })

        sampled.sort(key=lambda s: s["thompson_sample"], reverse=True)
        return sampled[:limit]

    def export_hebbian_edges_to_sqlite(self, min_weight: float = 0.65) -> int:
        """Exports strong Hebbian synapses (weight >= min_weight) from DuckDB to SQLite memory_edges table.

        Materializes virtual edges as edge_type='COUPLED_WITH' so the Rust fast-lane and ozy_graph
        can navigate causal dependencies at sub-millisecond speeds.
        """
        if not HAS_DUCKDB or not self.db_path.parent.exists():
            return 0

        # 1. Fetch strong synapses from DuckDB
        try:
            con = duckdb.connect(str(self.duckdb_path))
            cur = con.cursor()
            cur.execute(
                """SELECT source_file, target_file, weight
                   FROM stdp_synapses
                   WHERE weight >= ? AND tenant_id = ?
                   ORDER BY weight DESC""",
                [min_weight, self.tenant_id],
            )
            strong_synapses = cur.fetchall()
            con.close()
        except Exception:
            return 0

        if not strong_synapses:
            return 0

        # 2. Write to SQLite memory_edges
        exported = 0
        now_iso = datetime.now(timezone.utc).isoformat()
        try:
            conn = sqlite3.connect(str(self.db_path), timeout=5.0)
            conn.execute("PRAGMA journal_mode=WAL;")
            conn.execute("PRAGMA busy_timeout=5000;")
            conn.execute(
                """CREATE TABLE IF NOT EXISTS memory_edges (
                    source_type TEXT NOT NULL,
                    source_id TEXT NOT NULL,
                    target_type TEXT NOT NULL,
                    target_id TEXT NOT NULL,
                    edge_type TEXT NOT NULL,
                    weight REAL NOT NULL DEFAULT 1.0,
                    created_at TEXT NOT NULL,
                    tenant_id TEXT NOT NULL,
                    workspace_root TEXT NOT NULL DEFAULT '',
                    PRIMARY KEY (source_type, source_id, target_type, target_id, edge_type, tenant_id)
                );"""
            )
            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_memory_edges_src ON memory_edges(source_type, source_id, tenant_id);"
            )

            upsert_edge_sql = """
            INSERT INTO memory_edges (
                source_type, source_id, target_type, target_id, edge_type,
                weight, created_at, tenant_id, workspace_root
            ) VALUES ('file', ?, 'file', ?, 'COUPLED_WITH', ?, ?, ?, ?)
            ON CONFLICT(source_type, source_id, target_type, target_id, edge_type, tenant_id)
            DO UPDATE SET
                weight = excluded.weight,
                created_at = excluded.created_at;
            """

            for src, tgt, w in strong_synapses:
                conn.execute(
                    upsert_edge_sql,
                    (
                        src,
                        tgt,
                        round(float(w), 3),
                        now_iso,
                        self.tenant_id,
                        str(self.workspace_root),
                    ),
                )
                exported += 1

            conn.commit()
            conn.close()
            return exported
        except Exception:
            return 0

    def mine_git_history_stdp(self, max_commits: int = 100) -> Dict[str, Any]:
        """Extracts chronological commit file transitions from Git log, applies STDP, and exports to SQLite."""
        if not HAS_DUCKDB:
            return {"status": "duckdb_unavailable", "synapses_recorded": 0, "edges_exported": 0}

        cmd = [
            "git",
            "log",
            f"-n{max_commits}",
            "--name-only",
            "--pretty=format:COMMIT_MARKER|%H|%at",
            "--reverse",  # Chronological order
        ]

        try:
            res = subprocess.run(
                cmd,
                cwd=str(self.workspace_root),
                capture_output=True,
                text=True,
                encoding="utf-8",
                errors="replace",
                timeout=10,
                check=False,
            )
            if res.returncode != 0 or not res.stdout.strip():
                return {"status": "no_git_commits", "synapses_recorded": 0, "edges_exported": 0}

            commits_raw = res.stdout.split("COMMIT_MARKER|")
            commit_events: List[Dict[str, Any]] = []

            for chunk in commits_raw:
                chunk = chunk.strip()
                if not chunk:
                    continue
                lines = chunk.splitlines()
                header = lines[0].strip()
                files = [
                    normalize_rel_path(f.strip())
                    for f in lines[1:]
                    if f.strip() and not f.strip().endswith((".png", ".jpg", ".md", ".json", ".lock"))
                ]
                parts = header.split("|")
                epoch_sec = float(parts[1]) if len(parts) > 1 and parts[1].isdigit() else 0.0

                for f in files:
                    commit_events.append({"file_path": f, "timestamp": epoch_sec})

            trace_res = self.process_event_trace(commit_events)
            exported = self.export_hebbian_edges_to_sqlite(min_weight=0.60)

            return {
                "status": "ok",
                "events_processed": len(commit_events),
                "synapses_recorded": trace_res.get("synapses_updated", 0),
                "hebbian_edges_exported": exported,
            }
        except Exception as e:
            return {"status": "error", "error": str(e), "synapses_recorded": 0, "edges_exported": 0}
