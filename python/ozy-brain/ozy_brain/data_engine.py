from __future__ import annotations

import os
import re
import subprocess
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

try:
    import duckdb
except ImportError:
    duckdb = None

try:
    import polars as pl
except ImportError:
    pl = None


@dataclass
class FileHotspot:
    file_path: str
    churn_score: int
    commit_count: int
    authors_count: int
    fix_commits: int
    hotspot_score: float
    risk_level: str

    def to_dict(self) -> dict[str, Any]:
        return {
            "file_path": self.file_path,
            "churn_score": self.churn_score,
            "commit_count": self.commit_count,
            "authors_count": self.authors_count,
            "fix_commits": self.fix_commits,
            "hotspot_score": round(self.hotspot_score, 2),
            "risk_level": self.risk_level,
        }


class DataEngine:
    """Analytical OLAP Engine for Ozygram repository telemetry, Git churn, and hotspots.

    Uses Polars for fast DataFrame transformation and DuckDB for embedded OLAP persistence.
    """

    def __init__(self, project_path: str | Path | None = None, db_path: str | Path | None = None):
        self.project_path = Path(project_path) if project_path else Path.cwd()
        if db_path:
            self.db_path = str(db_path)
        else:
            ozymem_dir = self.project_path / ".ozymem"
            ozymem_dir.mkdir(parents=True, exist_ok=True)
            self.db_path = str(ozymem_dir / "analytics.duckdb")
        
        self._persistent_conn = None
        if duckdb is not None:
            try:
                self._persistent_conn = duckdb.connect(self.db_path)
            except Exception:
                try:
                    self._persistent_conn = duckdb.connect(self.db_path, read_only=True)
                except Exception:
                    self._persistent_conn = duckdb.connect(":memory:")
        self._init_db()

    def _get_connection(self):
        return self._persistent_conn

    def _init_db(self) -> None:
        conn = self._get_connection()
        if conn is None:
            return
        try:
            conn.execute("""
                CREATE TABLE IF NOT EXISTS file_churn (
                    file_path VARCHAR PRIMARY KEY,
                    churn_score BIGINT,
                    insertions BIGINT,
                    deletions BIGINT,
                    commit_count INTEGER,
                    authors_count INTEGER,
                    fix_commits INTEGER,
                    last_modified TIMESTAMP,
                    hotspot_score DOUBLE,
                    risk_level VARCHAR
                );
            """)
            conn.execute("""
                CREATE TABLE IF NOT EXISTS git_commit_history (
                    commit_hash VARCHAR,
                    author VARCHAR,
                    timestamp TIMESTAMP,
                    subject VARCHAR,
                    is_fix BOOLEAN
                );
            """)
            conn.execute("""
                CREATE TABLE IF NOT EXISTS co_changes (
                    file_a VARCHAR,
                    file_b VARCHAR,
                    co_change_count INTEGER,
                    PRIMARY KEY(file_a, file_b)
                );
            """)
        except Exception:
            pass

    def extract_git_log(self, max_commits: int = 500) -> list[dict[str, Any]]:
        """Extracts structured commit history and numstat diffs using Git CLI."""
        if not (self.project_path / ".git").exists():
            return []

        cmd = [
            "git",
            "-C",
            str(self.project_path),
            "log",
            f"-n{max_commits}",
            "--pretty=format:COMMIT_SEP%n%H|%an|%ad|%s",
            "--date=iso-strict",
            "--numstat",
        ]
        try:
            proc = subprocess.run(
                cmd,
                capture_output=True,
                text=True,
                encoding="utf-8",
                errors="replace",
                check=False,
            )
            if proc.returncode != 0 or not proc.stdout:
                return []
            return self._parse_git_numstat(proc.stdout)
        except Exception:
            return []

    def _parse_git_numstat(self, raw_output: str) -> list[dict[str, Any]]:
        entries: list[dict[str, Any]] = []
        commits = raw_output.split("COMMIT_SEP\n")

        for chunk in commits:
            lines = chunk.strip().split("\n")
            if not lines or not lines[0]:
                continue
            meta = lines[0].split("|", 3)
            if len(meta) < 4:
                continue
            commit_hash, author, date_str, subject = meta[0].strip(), meta[1].strip(), meta[2].strip(), meta[3].strip()
            is_fix = bool(re.search(r"\b(fix|bug|patch|revert|hotfix|error|issue)\b", subject, re.I))

            for line in lines[1:]:
                parts = line.strip().split("\t")
                if len(parts) != 3:
                    continue
                ins_str, del_str, file_path = parts
                if ins_str == "-" or del_str == "-":
                    continue  # Binary file
                try:
                    ins = int(ins_str)
                    dels = int(del_str)
                except ValueError:
                    continue

                entries.append({
                    "commit_hash": commit_hash,
                    "author": author,
                    "timestamp": date_str,
                    "subject": subject,
                    "is_fix": is_fix,
                    "file_path": file_path.replace("\\", "/"),
                    "insertions": ins,
                    "deletions": dels,
                    "total_churn": ins + dels,
                })
        return entries

    def ingest_and_analyze(self, max_commits: int = 500) -> dict[str, Any]:
        """Ingests git history and calculates hotspot & churn analytics into DuckDB."""
        entries = self.extract_git_log(max_commits=max_commits)
        if not entries:
            return {"status": "no_git_data", "total_records": 0, "hotspots": []}

        if pl is not None:
            df = pl.DataFrame(entries)
            # Aggregate per file
            aggregated = (
                df.group_by("file_path")
                .agg([
                    pl.col("total_churn").sum().alias("churn_score"),
                    pl.col("insertions").sum().alias("insertions"),
                    pl.col("deletions").sum().alias("deletions"),
                    pl.col("commit_hash").n_unique().alias("commit_count"),
                    pl.col("author").n_unique().alias("authors_count"),
                    pl.col("is_fix").cast(pl.Int32).sum().alias("fix_commits"),
                    pl.col("timestamp").max().alias("last_modified"),
                ])
                .with_columns(
                    # Hotspot score: churn * commit_frequency * (1 + fix_commits*0.5)
                    (
                        pl.col("churn_score")
                        * pl.col("commit_count")
                        * (1.0 + pl.col("fix_commits") * 0.5)
                    ).alias("hotspot_score")
                )
                .sort("hotspot_score", descending=True)
            )
            records = aggregated.to_dicts()
        else:
            # Fallback pure-Python aggregation if polars is not present
            temp_dict: dict[str, dict[str, Any]] = {}
            for e in entries:
                fp = e["file_path"]
                if fp not in temp_dict:
                    temp_dict[fp] = {
                        "file_path": fp,
                        "churn_score": 0,
                        "insertions": 0,
                        "deletions": 0,
                        "commits": set(),
                        "authors": set(),
                        "fix_commits": 0,
                        "last_modified": e["timestamp"],
                    }
                rec = temp_dict[fp]
                rec["churn_score"] += e["total_churn"]
                rec["insertions"] += e["insertions"]
                rec["deletions"] += e["deletions"]
                rec["commits"].add(e["commit_hash"])
                rec["authors"].add(e["author"])
                if e["is_fix"]:
                    rec["fix_commits"] += 1
                if e["timestamp"] > rec["last_modified"]:
                    rec["last_modified"] = e["timestamp"]

            records = []
            for fp, r in temp_dict.items():
                commit_count = len(r["commits"])
                authors_count = len(r["authors"])
                fix_count = r["fix_commits"]
                score = r["churn_score"] * commit_count * (1.0 + fix_count * 0.5)
                records.append({
                    "file_path": fp,
                    "churn_score": r["churn_score"],
                    "insertions": r["insertions"],
                    "deletions": r["deletions"],
                    "commit_count": commit_count,
                    "authors_count": authors_count,
                    "fix_commits": fix_count,
                    "last_modified": r["last_modified"],
                    "hotspot_score": score,
                })
            records.sort(key=lambda x: x["hotspot_score"], reverse=True)

        # Classify risk levels
        hotspots: list[FileHotspot] = []
        for r in records:
            score = float(r["hotspot_score"])
            if score > 1000 or r["fix_commits"] >= 5:
                risk = "CRITICAL"
            elif score > 250 or r["fix_commits"] >= 2:
                risk = "HIGH"
            elif score > 50:
                risk = "MEDIUM"
            else:
                risk = "LOW"

            hotspots.append(FileHotspot(
                file_path=r["file_path"],
                churn_score=int(r["churn_score"]),
                commit_count=int(r["commit_count"]),
                authors_count=int(r["authors_count"]),
                fix_commits=int(r["fix_commits"]),
                hotspot_score=score,
                risk_level=risk,
            ))

        # Store to DuckDB
        conn = self._get_connection()
        if conn is not None:
            conn.execute("DELETE FROM file_churn;")
            for h in hotspots:
                conn.execute("""
                    INSERT OR REPLACE INTO file_churn VALUES (
                        ?, ?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP, ?, ?
                    );
                """, [
                    h.file_path,
                    h.churn_score,
                    r.get("insertions", 0),
                    r.get("deletions", 0),
                    h.commit_count,
                    h.authors_count,
                    h.fix_commits,
                    h.hotspot_score,
                    h.risk_level,
                ])

        return {
            "status": "success",
            "total_files": len(hotspots),
            "top_hotspots": [h.to_dict() for h in hotspots[:15]],
        }

    def get_top_hotspots(self, limit: int = 10) -> list[dict[str, Any]]:
        """Returns top repository hotspots from DuckDB or calculates them on the fly."""
        conn = self._get_connection()
        if conn is not None:
            rows = conn.execute("""
                SELECT file_path, churn_score, commit_count, authors_count, fix_commits, hotspot_score, risk_level
                FROM file_churn
                ORDER BY hotspot_score DESC
                LIMIT ?;
            """, [limit]).fetchall()
            if rows:
                return [
                    {
                        "file_path": row[0],
                        "churn_score": row[1],
                        "commit_count": row[2],
                        "authors_count": row[3],
                        "fix_commits": row[4],
                        "hotspot_score": round(row[5], 2),
                        "risk_level": row[6],
                    }
                    for row in rows
                ]

        res = self.ingest_and_analyze()
        return res.get("top_hotspots", [])[:limit]

    def get_file_telemetry(self, target_file: str) -> dict[str, Any] | None:
        """Queries churn and stability metrics for a single specific file."""
        norm_target = target_file.replace("\\", "/").strip("/")
        conn = self._get_connection()
        if conn is not None:
            row = conn.execute("""
                SELECT file_path, churn_score, commit_count, authors_count, fix_commits, hotspot_score, risk_level
                FROM file_churn
                WHERE file_path = ? OR file_path LIKE ?;
            """, [norm_target, f"%/{norm_target}"]).fetchone()
            if row:
                return {
                    "file_path": row[0],
                    "churn_score": row[1],
                    "commit_count": row[2],
                    "authors_count": row[3],
                    "fix_commits": row[4],
                    "hotspot_score": round(row[5], 2),
                    "risk_level": row[6],
                }
        return None
