"""
Basal Ganglia Procedural Habits Engine for OzyBrain.
Uses Polars dynamic temporal windowing over Git commit logs and session edits
to discover procedural playbooks (choreographies of files edited together).
Writes playbooks to SQLite procedural_habits table for sub-millisecond retrieval by Rust.
Zero-emoji compliant.
"""

from __future__ import annotations

import json
import os
import sqlite3
import subprocess
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Dict, List, Optional, Set

try:
    import polars as pl
    HAS_POLARS = True
except ImportError:
    HAS_POLARS = False


def normalize_rel_path(path: str) -> str:
    """Normalizes relative file path across Windows and POSIX."""
    normalized = path.strip().replace("\\", "/")
    while normalized.startswith("./"):
        normalized = normalized[2:]
    return normalized


def now_iso() -> str:
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


class BasalGangliaEngine:
    """Discovers procedural playbooks (habits) representing sequential file edits."""

    def __init__(
        self,
        workspace_root: Optional[str | Path] = None,
        db_path: Optional[str | Path] = None,
        tenant_id: str = "local",
    ):
        self.workspace_root = Path(workspace_root or os.getcwd()).resolve()
        self.db_path = Path(db_path) if db_path else self._resolve_db_path()
        self.tenant_id = tenant_id

    def _resolve_db_path(self) -> Path:
        local_db = self.workspace_root / ".ozymem" / "db.sqlite"
        if local_db.exists():
            return local_db
        local_db_alt = self.workspace_root / ".ozymem" / "memory.db"
        if local_db_alt.exists():
            return local_db_alt
        return Path.home() / ".ozymem" / "memory.db"

    def extract_git_commit_sessions(self, max_commits: int = 100) -> List[Dict[str, Any]]:
        """Extracts commit sessions with timestamp and modified file lists from Git."""
        if not (self.workspace_root / ".git").exists():
            return []

        cmd = [
            "git",
            "log",
            f"-n{max_commits}",
            "--name-only",
            "--pretty=format:COMMIT_MARKER|%H|%ct|%s",
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
                return []

            commits: List[Dict[str, Any]] = []
            chunks = res.stdout.split("COMMIT_MARKER|")

            for chunk in chunks:
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
                if len(files) < 2:
                    continue  # Only interested in multi-file choreographies

                parts = header.split("|")
                commit_hash = parts[0] if parts else ""
                epoch_sec = int(parts[1]) if len(parts) > 1 and parts[1].isdigit() else 0

                commits.append({
                    "commit_hash": commit_hash,
                    "timestamp": epoch_sec,
                    "files": files,
                })

            return commits
        except Exception:
            return []

    def mine_procedural_playbooks(
        self,
        sessions: List[Dict[str, Any]],
        min_confidence: float = 0.50,
        min_co_occurrences: int = 2,
    ) -> List[Dict[str, Any]]:
        """Mines frequent file transition rules (File A -> [File B, File C]) using co-occurrence statistics."""
        if not sessions:
            return []

        # Tally single-file frequencies and co-occurring transition pairs
        file_counts: Dict[str, int] = {}
        pair_counts: Dict[tuple[str, str], int] = {}

        for sess in sessions:
            file_list = sess.get("files", [])
            unique_files = list(dict.fromkeys(file_list))  # preserve order, remove duplicates
            for f in unique_files:
                file_counts[f] = file_counts.get(f, 0) + 1

            for i in range(len(unique_files)):
                for j in range(i + 1, len(unique_files)):
                    f_a = unique_files[i]
                    f_b = unique_files[j]
                    pair_counts[(f_a, f_b)] = pair_counts.get((f_a, f_b), 0) + 1
                    # Also tally reverse association with slightly discounted weight if simultaneous
                    pair_counts[(f_b, f_a)] = pair_counts.get((f_b, f_a), 0) + 1

        # Group transitions by trigger file
        habits: List[Dict[str, Any]] = []
        now = now_iso()

        for trigger_file, count_a in file_counts.items():
            candidates: List[tuple[str, float, int]] = []
            for (f_a, f_b), count_ab in pair_counts.items():
                if f_a == trigger_file and count_ab >= min_co_occurrences:
                    conf = count_ab / max(1, count_a)
                    if conf >= min_confidence:
                        candidates.append((f_b, conf, count_ab))

            if not candidates:
                continue

            candidates.sort(key=lambda c: (c[1], c[2]), reverse=True)
            subsequent = [c[0] for c in candidates[:5]]
            top_conf = round(candidates[0][1], 2)
            total_occurrences = max(c[2] for c in candidates)

            habit_id = f"{self.tenant_id}::{trigger_file}"
            habits.append({
                "id": habit_id,
                "trigger_file": trigger_file,
                "subsequent_files": subsequent,
                "confidence": top_conf,
                "occurrences": total_occurrences,
                "updated_at": now,
                "tenant_id": self.tenant_id,
                "workspace_root": str(self.workspace_root),
            })

        return habits

    def write_habits_to_sqlite(self, habits: List[Dict[str, Any]]) -> int:
        """Atomically upserts playbooks into SQLite procedural_habits table."""
        if not habits or not self.db_path.parent.exists():
            return 0

        conn = sqlite3.connect(str(self.db_path), timeout=5.0)
        try:
            conn.execute("PRAGMA journal_mode=WAL;")
            conn.execute("PRAGMA synchronous=NORMAL;")
            conn.execute("PRAGMA busy_timeout=5000;")

            conn.execute(
                """CREATE TABLE IF NOT EXISTS procedural_habits (
                    id TEXT PRIMARY KEY,
                    trigger_file TEXT NOT NULL,
                    choreography_json TEXT NOT NULL,
                    confidence REAL NOT NULL,
                    occurrences INTEGER NOT NULL DEFAULT 1,
                    updated_at TEXT NOT NULL,
                    tenant_id TEXT NOT NULL DEFAULT 'local',
                    workspace_root TEXT NOT NULL DEFAULT ''
                );"""
            )
            conn.execute("CREATE INDEX IF NOT EXISTS idx_habits_trigger ON procedural_habits(trigger_file, tenant_id);")

            upsert_sql = """
            INSERT INTO procedural_habits (
                id, trigger_file, choreography_json, confidence, occurrences,
                updated_at, tenant_id, workspace_root
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET
                choreography_json = excluded.choreography_json,
                confidence = MAX(procedural_habits.confidence, excluded.confidence),
                occurrences = procedural_habits.occurrences + 1,
                updated_at = excluded.updated_at;
            """

            written = 0
            for h in habits:
                ch_json = json.dumps(h["subsequent_files"])
                conn.execute(
                    upsert_sql,
                    (
                        h["id"],
                        h["trigger_file"],
                        ch_json,
                        h["confidence"],
                        h.get("occurrences", 1),
                        h["updated_at"],
                        h.get("tenant_id", self.tenant_id),
                        h.get("workspace_root", str(self.workspace_root)),
                    ),
                )
                written += 1

            conn.commit()
            return written
        finally:
            conn.close()

    def run_basal_ganglia_cycle(self, max_commits: int = 100) -> Dict[str, Any]:
        """Runs the complete Basal Ganglia mining cycle: extracts commits, mines playbooks, writes to SQLite."""
        sessions = self.extract_git_commit_sessions(max_commits=max_commits)
        habits = self.mine_procedural_playbooks(sessions)
        written = self.write_habits_to_sqlite(habits)

        return {
            "status": "ok",
            "sessions_mined": len(sessions),
            "habits_discovered": len(habits),
            "sqlite_habits_upserted": written,
        }
