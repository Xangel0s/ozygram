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
            conn.execute("""
                CREATE TABLE IF NOT EXISTS raw_noise_telemetry (
                    id VARCHAR PRIMARY KEY,
                    timestamp TIMESTAMP,
                    noise_type VARCHAR,
                    reason VARCHAR,
                    raw_content VARCHAR,
                    project VARCHAR
                );
            """)
        except Exception:
            pass

    def record_noise_telemetry(
        self,
        noise_id: str,
        noise_type: str,
        reason: str,
        raw_content: str,
        project: str = "",
    ) -> bool:
        """Persists filtered noisy/garbage terminal outputs and logs into DuckDB for telemetry."""
        conn = self._get_connection()
        if conn is None:
            return False
        try:
            conn.execute(
                """
                INSERT OR REPLACE INTO raw_noise_telemetry (id, timestamp, noise_type, reason, raw_content, project)
                VALUES (?, ?, ?, ?, ?, ?)
                """,
                [noise_id, datetime.now(timezone.utc), noise_type, reason, str(raw_content)[:4000], project],
            )
            return True
        except Exception:
            return False

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

    @staticmethod
    def audit_sql_content(sql_content: str, file_path: str = "") -> list[dict[str, Any]]:
        """Static AST-like heuristic analyzer for SQL migration idempotency and safety.
        
        Rules:
          1. DDL without IF NOT EXISTS / IF EXISTS -> [ALERT: NON_IDEMPOTENT_SQL]
          2. Destructive DDL without '-- OZYMEM_ALLOW_DESTRUCTIVE' -> [ALERT: DESTRUCTIVE_UNGUARDED]
        """
        non_idempotent_verbs = [
            "CREATE TABLE", "CREATE INDEX", "CREATE UNIQUE INDEX", "CREATE VIEW",
            "CREATE SEQUENCE", "CREATE TYPE", "CREATE SCHEMA",
            "DROP TABLE", "DROP INDEX", "DROP VIEW", "DROP SEQUENCE", "DROP TYPE", "DROP SCHEMA",
        ]
        destructive_verbs = ["DROP TABLE", "DROP SCHEMA", "TRUNCATE"]
        override_token = "OZYMEM_ALLOW_DESTRUCTIVE"

        lines = sql_content.splitlines()
        findings: list[dict[str, Any]] = []
        stmt_tokens: list[str] = []
        stmt_first_line = 1
        statements: list[tuple[int, str, str]] = []

        for idx, raw in enumerate(lines):
            line_no = idx + 1
            comment_idx = raw.find("--")
            effective = raw[:comment_idx].strip() if comment_idx != -1 else raw.strip()
            if not effective:
                continue
            has_semi = ";" in effective
            token = effective.rstrip(";").strip()
            if not stmt_tokens and token:
                stmt_first_line = line_no
            if token:
                stmt_tokens.append(token)
            if has_semi and stmt_tokens:
                raw_joined = " ".join(stmt_tokens)
                upper = raw_joined.upper()
                preview = raw_joined[:120]
                statements.append((stmt_first_line, upper, preview))
                stmt_tokens.clear()

        if stmt_tokens:
            raw_joined = " ".join(stmt_tokens)
            upper = raw_joined.upper()
            preview = raw_joined[:120]
            statements.append((stmt_first_line, upper, preview))

        for first_line, upper, preview in statements:
            for verb in non_idempotent_verbs:
                if verb in upper:
                    guarded = "IF NOT EXISTS" in upper or "IF EXISTS" in upper
                    if not guarded:
                        findings.append({
                            "file_path": file_path,
                            "line_number": first_line,
                            "statement_preview": preview,
                            "severity": "NON_IDEMPOTENT_SQL",
                            "badge": "[ALERT: NON_IDEMPOTENT_SQL]",
                            "suggestion": "Add IF NOT EXISTS / IF EXISTS guard to make this statement safe to re-run.",
                        })
                        break

            for verb in destructive_verbs:
                if verb in upper:
                    lookback_start = max(0, first_line - 4)
                    lookback_end = min(len(lines), first_line)
                    has_override = any(override_token in lines[i] for i in range(lookback_start, lookback_end))
                    if not has_override:
                        findings.append({
                            "file_path": file_path,
                            "line_number": first_line,
                            "statement_preview": preview,
                            "severity": "DESTRUCTIVE_UNGUARDED",
                            "badge": "[ALERT: DESTRUCTIVE_UNGUARDED]",
                            "suggestion": f"Destructive DDL requires '-- {override_token}' on the preceding line.",
                        })
                    break

        return findings

    def audit_migrations(self, migrations_dir: str | Path | None = None) -> dict[str, Any]:
        """Audits SQL migration files in the project or given directory."""
        target_dir = Path(migrations_dir) if migrations_dir else self.project_path
        if not target_dir.exists():
            return {
                "status": "[STATUS: CLEAN]",
                "files_scanned": 0,
                "findings": [],
                "report": "[STATUS: IDEMPOTENT] No files scanned.",
            }

        sql_files: list[Path] = []
        for p in target_dir.rglob("*.sql"):
            parts = p.parts
            if any(part.startswith(".") or part in ("node_modules", "target", "__pycache__") for part in parts):
                continue
            sql_files.append(p)

        all_findings: list[dict[str, Any]] = []
        for sql_file in sql_files:
            try:
                content = sql_file.read_text(encoding="utf-8", errors="replace")
                fp = str(sql_file).replace("\\", "/")
                all_findings.extend(self.audit_sql_content(content, fp))
            except Exception:
                continue

        non_idempotent = [f for f in all_findings if f["severity"] == "NON_IDEMPOTENT_SQL"]
        destructive = [f for f in all_findings if f["severity"] == "DESTRUCTIVE_UNGUARDED"]

        lines = [
            "# [AUDIT: SQL_MIGRATIONS]",
            "",
            "Mode: preview_safe (no files modified)",
            f"Files scanned: {len(sql_files)}",
            "",
        ]
        if not all_findings:
            lines.append("[STATUS: IDEMPOTENT] All scanned SQL migration files are idempotent and contain no unguarded destructive statements.")
        else:
            lines.append(f"[STATS] non_idempotent={len(non_idempotent)} destructive_unguarded={len(destructive)}")
            lines.append("")
            if non_idempotent:
                lines.append("## [ALERT: NON_IDEMPOTENT_SQL] Missing IF NOT EXISTS / IF EXISTS\n")
                for f in non_idempotent:
                    lines.append(f"- {f['file_path']}:L{f['line_number']}")
                    lines.append(f"  Statement: {f['statement_preview']}")
                    lines.append(f"  Suggestion: {f['suggestion']}\n")
            if destructive:
                lines.append("## [ALERT: DESTRUCTIVE_UNGUARDED] Destructive Statements Without Override\n")
                for f in destructive:
                    lines.append(f"- {f['file_path']}:L{f['line_number']}")
                    lines.append(f"  Statement: {f['statement_preview']}")
                    lines.append(f"  Suggestion: {f['suggestion']}")
                    lines.append(f"  Override: Add '-- OZYMEM_ALLOW_DESTRUCTIVE' on the preceding line.\n")

        return {
            "status": "[STATUS: ALERT]" if all_findings else "[STATUS: IDEMPOTENT]",
            "files_scanned": len(sql_files),
            "findings": all_findings,
            "report": "\n".join(lines),
        }
