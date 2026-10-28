"""
Amygdala Reflex Generator & Bipartite Neuro-Cognitive Substrate for OzyBrain.
Architecture:
- Basolateral Amygdala (BLA): Associative Pavlovian threat conditioning (commits, compile errors).
- Central Amygdala (CeA): Autonomic reflex vetoes and emergency motor gating.
- vmPFC Extinction Gater: Prefrontal inhibitory learning preventing artificial PTSD on stabilized code.
Zero-emoji compliant.
"""

from __future__ import annotations

import math
import os
import re
import sqlite3
import subprocess
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Dict, List, Optional


def normalize_rel_path(path: str) -> str:
    """Normalizes file path across Windows and POSIX separators."""
    normalized = path.strip().replace("\\", "/")
    while normalized.startswith("./"):
        normalized = normalized[2:]
    return normalized


def now_iso() -> str:
    """Returns current UTC timestamp in ISO 8601 format."""
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


class VmPFCExtinctionGater:
    """
    Ventromedial Prefrontal Cortex (vmPFC) Extinction Engine.
    Implements active inhibitory gating over the Amygdala.
    Extinction is NOT forgetting: it learns an active safety signal (I_ext)
    that suppresses fear responses after consecutive successful validations.
    """

    @staticmethod
    def calculate_inhibition(clean_runs: int, k: float = 0.8, theta: float = 3.0) -> float:
        """
        Sigmoidal extinction inhibition curve:
        I_ext = 1 / (1 + exp(-k * (clean_runs - theta)))
        """
        if clean_runs <= 0:
            return 0.0
        exp_val = math.exp(-k * (float(clean_runs) - theta))
        inhibition = 1.0 / (1.0 + exp_val)
        return max(0.0, min(1.0, round(inhibition, 4)))

    @classmethod
    def record_clean_run(cls, db_path: Path, file_path: str, tenant_id: str = "local") -> float:
        """
        Increments clean_runs for all reflexes matching file_path and updates extinction_score.
        Returns the new extinction inhibition factor (0.0 to 1.0).
        """
        if not db_path.exists():
            return 0.0

        norm_path = normalize_rel_path(file_path)
        conn = sqlite3.connect(str(db_path), timeout=5.0)
        try:
            conn.execute("PRAGMA journal_mode=WAL;")
            # Fetch current clean_runs
            cursor = conn.execute(
                "SELECT COALESCE(MAX(clean_runs), 0) FROM amygdala_reflexes WHERE file_path = ? AND tenant_id = ?;",
                (norm_path, tenant_id),
            )
            row = cursor.fetchone()
            current_runs = row[0] if row else 0
            new_runs = current_runs + 1
            inhibition = cls.calculate_inhibition(new_runs)

            conn.execute(
                """UPDATE amygdala_reflexes
                   SET clean_runs = ?, extinction_score = ?
                   WHERE file_path = ? AND tenant_id = ?;""",
                (new_runs, inhibition, norm_path, tenant_id),
            )
            conn.commit()
            return inhibition
        except Exception:
            return 0.0
        finally:
            conn.close()

    @classmethod
    def record_relapse(cls, db_path: Path, file_path: str, tenant_id: str = "local") -> None:
        """
        Reinstates fear on new error: resets clean_runs and extinction inhibition to 0.
        """
        if not db_path.exists():
            return

        norm_path = normalize_rel_path(file_path)
        conn = sqlite3.connect(str(db_path), timeout=5.0)
        try:
            conn.execute("PRAGMA journal_mode=WAL;")
            conn.execute(
                """UPDATE amygdala_reflexes
                   SET clean_runs = 0, extinction_score = 0.0
                   WHERE file_path = ? AND tenant_id = ?;""",
                (norm_path, tenant_id),
            )
            conn.commit()
        except Exception:
            pass
        finally:
            conn.close()


class CentralAmygdala:
    """
    Central Nucleus of the Amygdala (CeA).
    Computes autonomic motor gating and critical veto signals
    by combining raw BLA threat with vmPFC extinction inhibition:
    T_effective = T_BLA * (1 - I_ext).
    """

    @staticmethod
    def evaluate_reflex(
        raw_threat: float,
        extinction_inhibition: float,
        file_path: str = "",
    ) -> Dict[str, Any]:
        raw_clamped = max(0.0, min(1.0, float(raw_threat)))
        ext_clamped = max(0.0, min(1.0, float(extinction_inhibition)))
        effective_threat = round(raw_clamped * (1.0 - ext_clamped), 4)

        if effective_threat >= 0.75:
            action = "CRITICAL_VETO"
            badge = "[CRITICAL_VETO: HIGH_THREAT_AMYGDALA]"
            directive = f"Block autonomous modifications on '{file_path}'; requires deliberative System 2 validation."
        elif effective_threat >= 0.40:
            action = "CAUTION_ALERT"
            badge = "[ALERT: AMYGDALA_REFLEX - HIGH_FRAGILITY]"
            directive = f"Fragile file '{file_path}': verify AST signatures and execute isolated unit tests."
        else:
            action = "EXTINGUISHED_SAFE"
            badge = "[STATUS: EXTINCTION_GATED_SAFE]"
            directive = f"Historical vulnerability on '{file_path}' successfully mitigated by consecutive clean validations."

        return {
            "file_path": file_path,
            "raw_threat": raw_clamped,
            "extinction_inhibition": ext_clamped,
            "effective_threat": effective_threat,
            "autonomic_action": action,
            "badge": badge,
            "directive": directive,
        }


class BasolateralAmygdala:
    """
    Basolateral Amygdala (BLA).
    Extracts recurring failure patterns, commits, and compiler traces
    into associative threat memories.
    """

    @staticmethod
    def compute_threat_score(occurrences: int, base: float = 0.40, increment: float = 0.15) -> float:
        return min(1.0, round(base + increment * occurrences, 2))


class AmygdalaReflexEngine:
    """
    Efferent generator that coordinates BLA associative conditioning,
    vmPFC extinction gating, and CeA autonomic motor vetoes.
    Writes atomic pain reflexes to SQLite amygdala_reflexes table.
    """

    def __init__(
        self,
        db_path: Optional[str | Path] = None,
        tenant_id: str = "local",
        workspace_root: str = "",
    ):
        self.tenant_id = tenant_id
        self.workspace_root = workspace_root or os.getcwd()
        self.db_path = Path(db_path) if db_path else self._resolve_default_db_path()
        self.bla = BasolateralAmygdala()
        self.cea = CentralAmygdala()
        self.vmpfc = VmPFCExtinctionGater()

    def _resolve_default_db_path(self) -> Path:
        """Finds project or global database path."""
        local_db = Path(self.workspace_root) / ".ozymem" / "db.sqlite"
        if local_db.exists():
            return local_db
        local_db_alt = Path(self.workspace_root) / ".ozymem" / "memory.db"
        if local_db_alt.exists():
            return local_db_alt
        home_db = Path.home() / ".ozymem" / "memory.db"
        home_db.parent.mkdir(parents=True, exist_ok=True)
        return home_db

    def scan_git_pain_signals(
        self,
        repo_path: Optional[str | Path] = None,
        max_commits: int = 50,
    ) -> List[Dict[str, Any]]:
        """Scans Git history for fix, revert, bug, and hotfix commits to identify scarred files."""
        target_repo = Path(repo_path or self.workspace_root)
        if not (target_repo / ".git").exists():
            return []

        cmd = [
            "git",
            "log",
            f"-n{max_commits}",
            "--grep=fix:",
            "--grep=revert:",
            "--grep=bug:",
            "--grep=hotfix:",
            "--grep=panic:",
            "--name-only",
            "--pretty=format:COMMIT_SEP|%H|%s",
        ]

        try:
            result = subprocess.run(
                cmd,
                cwd=str(target_repo),
                capture_output=True,
                text=True,
                encoding="utf-8",
                errors="replace",
                timeout=10,
                check=False,
            )
            if result.returncode != 0 or not result.stdout.strip():
                return []

            commits_raw = result.stdout.split("COMMIT_SEP|")
            signals: List[Dict[str, Any]] = []

            for chunk in commits_raw:
                chunk = chunk.strip()
                if not chunk:
                    continue
                lines = chunk.splitlines()
                header = lines[0].strip()
                files = [f.strip() for f in lines[1:] if f.strip()]
                parts = header.split("|", 1)
                commit_hash = parts[0] if parts else ""
                commit_msg = parts[1] if len(parts) > 1 else ""

                for file_path in files:
                    norm_path = normalize_rel_path(file_path)
                    signals.append({
                        "file_path": norm_path,
                        "symbol_name": "",
                        "commit_hash": commit_hash,
                        "commit_msg": commit_msg,
                    })

            return signals
        except Exception:
            return []

    def calculate_threat_scars(
        self,
        signals: List[Dict[str, Any]],
        direct_errors: Optional[List[Dict[str, Any]]] = None,
    ) -> List[Dict[str, Any]]:
        """Aggregates failure signals into prioritized Amygdala reflex entries."""
        file_stats: Dict[str, Dict[str, Any]] = {}

        for sig in signals:
            path = normalize_rel_path(sig.get("file_path", ""))
            if not path or path.endswith((".png", ".jpg", ".md", ".json", ".lock")):
                continue
            if path not in file_stats:
                file_stats[path] = {
                    "occurrences": 0,
                    "triggers": [],
                    "symbols": set(),
                }
            file_stats[path]["occurrences"] += 1
            if sig.get("commit_msg") and sig["commit_msg"] not in file_stats[path]["triggers"]:
                file_stats[path]["triggers"].append(sig["commit_msg"])
            if sig.get("symbol_name"):
                file_stats[path]["symbols"].add(sig["symbol_name"])

        if direct_errors:
            for err in direct_errors:
                path = normalize_rel_path(err.get("file_path", ""))
                if not path:
                    continue
                if path not in file_stats:
                    file_stats[path] = {
                        "occurrences": 0,
                        "triggers": [],
                        "symbols": set(),
                    }
                file_stats[path]["occurrences"] += 1
                msg = err.get("error_message") or err.get("pain_trigger")
                if msg and msg not in file_stats[path]["triggers"]:
                    file_stats[path]["triggers"].append(msg)
                if err.get("symbol_name"):
                    file_stats[path]["symbols"].add(err["symbol_name"])

        reflexes: List[Dict[str, Any]] = []
        now = now_iso()

        for path, data in file_stats.items():
            count = data["occurrences"]
            threat_score = self.bla.compute_threat_score(count)
            top_triggers = data["triggers"][:3]
            pain_trigger = "; ".join(top_triggers) if top_triggers else "Recurring regression observed"
            antidote_directive = (
                f"Verify invariants and run targeted tests on '{path}' before and after applying modifications."
            )

            reflexes.append({
                "id": f"{self.tenant_id}::{path}::",
                "file_path": path,
                "symbol_name": "",
                "threat_score": threat_score,
                "pain_trigger": pain_trigger,
                "antidote_directive": antidote_directive,
                "occurrences": count,
                "last_triggered": now,
                "created_at": now,
                "tenant_id": self.tenant_id,
                "workspace_root": self.workspace_root,
                "clean_runs": 0,
                "extinction_score": 0.0,
            })

            for sym in data["symbols"]:
                sym_clean = sym.strip()
                if sym_clean:
                    reflexes.append({
                        "id": f"{self.tenant_id}::{path}::{sym_clean}",
                        "file_path": path,
                        "symbol_name": sym_clean,
                        "threat_score": min(1.0, round(threat_score + 0.05, 2)),
                        "pain_trigger": f"Symbol '{sym_clean}': {pain_trigger}",
                        "antidote_directive": f"Check caller arity and contract for '{sym_clean}' before modifying.",
                        "occurrences": count,
                        "last_triggered": now,
                        "created_at": now,
                        "tenant_id": str(self.tenant_id or "local"),
                        "workspace_root": str(self.workspace_root or ""),
                        "clean_runs": 0,
                        "extinction_score": 0.0,
                    })

        return reflexes

    def write_reflexes_to_sqlite(self, reflexes: List[Dict[str, Any]]) -> int:
        """Atomically upserts reflexes into SQLite amygdala_reflexes table using WAL mode."""
        if not reflexes or not self.db_path.exists():
            return 0

        conn = sqlite3.connect(str(self.db_path), timeout=5.0)
        try:
            conn.execute("PRAGMA journal_mode=WAL;")
            conn.execute("PRAGMA synchronous=NORMAL;")
            conn.execute("PRAGMA busy_timeout=5000;")

            conn.execute(
                """CREATE TABLE IF NOT EXISTS amygdala_reflexes (
                    id TEXT PRIMARY KEY,
                    file_path TEXT NOT NULL,
                    symbol_name TEXT NOT NULL DEFAULT '',
                    threat_score REAL NOT NULL,
                    pain_trigger TEXT NOT NULL,
                    antidote_directive TEXT NOT NULL,
                    occurrences INTEGER NOT NULL DEFAULT 1,
                    last_triggered TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    tenant_id TEXT NOT NULL DEFAULT 'local',
                    workspace_root TEXT NOT NULL DEFAULT '',
                    clean_runs INTEGER NOT NULL DEFAULT 0,
                    extinction_score REAL NOT NULL DEFAULT 0.0
                );"""
            )
            # Idempotent column migrations
            try:
                conn.execute("ALTER TABLE amygdala_reflexes ADD COLUMN clean_runs INTEGER NOT NULL DEFAULT 0;")
            except Exception:
                pass
            try:
                conn.execute("ALTER TABLE amygdala_reflexes ADD COLUMN extinction_score REAL NOT NULL DEFAULT 0.0;")
            except Exception:
                pass

            conn.execute("CREATE INDEX IF NOT EXISTS idx_amygdala_file ON amygdala_reflexes(file_path, tenant_id);")

            upsert_query = """
            INSERT INTO amygdala_reflexes (
                id, file_path, symbol_name, threat_score, pain_trigger,
                antidote_directive, occurrences, last_triggered, created_at,
                tenant_id, workspace_root, clean_runs, extinction_score
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET
                occurrences = amygdala_reflexes.occurrences + 1,
                threat_score = MIN(1.0, MAX(amygdala_reflexes.threat_score, excluded.threat_score) + 0.05),
                pain_trigger = excluded.pain_trigger,
                antidote_directive = excluded.antidote_directive,
                last_triggered = excluded.last_triggered,
                clean_runs = 0,
                extinction_score = 0.0;
            """

            count = 0
            for r in reflexes:
                conn.execute(
                    upsert_query,
                    (
                        r["id"],
                        r["file_path"],
                        r.get("symbol_name", ""),
                        r["threat_score"],
                        r["pain_trigger"],
                        r["antidote_directive"],
                        r.get("occurrences", 1),
                        r["last_triggered"],
                        r["created_at"],
                        str(r.get("tenant_id", self.tenant_id) or "local"),
                        str(r.get("workspace_root", self.workspace_root) or ""),
                        int(r.get("clean_runs", 0)),
                        float(r.get("extinction_score", 0.0)),
                    ),
                )
                count += 1

            conn.commit()
            return count
        finally:
            conn.close()

    def record_clean_run(self, file_path: str) -> float:
        """Records a successful clean execution, advancing vmPFC extinction."""
        return self.vmpfc.record_clean_run(self.db_path, file_path, self.tenant_id)

    def record_relapse(self, file_path: str) -> None:
        """Reinstates threat after a regression or error."""
        self.vmpfc.record_relapse(self.db_path, file_path, self.tenant_id)

    def evaluate_file_threat(self, file_path: str) -> Dict[str, Any]:
        """Queries SQLite and computes the effective gated threat via Central Amygdala."""
        norm_path = normalize_rel_path(file_path)
        if not self.db_path.exists():
            return self.cea.evaluate_reflex(0.0, 0.0, norm_path)

        conn = sqlite3.connect(str(self.db_path), timeout=5.0)
        try:
            cursor = conn.execute(
                """SELECT threat_score, clean_runs, extinction_score
                   FROM amygdala_reflexes
                   WHERE file_path = ? AND tenant_id = ?
                   ORDER BY threat_score DESC LIMIT 1;""",
                (norm_path, self.tenant_id),
            )
            row = cursor.fetchone()
            if not row:
                return self.cea.evaluate_reflex(0.0, 0.0, norm_path)
            raw_threat, clean_runs, extinction_score = row
            return self.cea.evaluate_reflex(raw_threat, extinction_score, norm_path)
        finally:
            conn.close()

    def run_efferent_cycle(
        self,
        direct_errors: Optional[List[Dict[str, Any]]] = None,
        max_commits: int = 100,
    ) -> Dict[str, Any]:
        """Runs the complete Amygdala efferent cycle: scans signals, computes threat scores, writes to SQLite."""
        signals = self.scan_git_pain_signals(max_commits=max_commits)
        reflexes = self.calculate_threat_scars(signals, direct_errors=direct_errors)
        written = self.write_reflexes_to_sqlite(reflexes)

        return {
            "status": "ok",
            "git_signals_detected": len(signals),
            "scars_computed": len(reflexes),
            "sqlite_rows_upserted": written,
            "db_path": str(self.db_path),
        }

    run_amygdala_cycle = run_efferent_cycle
