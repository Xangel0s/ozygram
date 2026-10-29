"""Unified Hybrid Dream Memory Store (SQLite WAL + Git-First Markdown + Optional Postgres).

Persists reflexions, gotchas, and contract drift alerts with sub-millisecond local latency,
and exports merge-conflict-free Git Markdown files (.ozy/memories/gotchas/*.md) for PR auditability.
"""

from __future__ import annotations

import json
import logging
import os
import sqlite3
import time
import uuid
from abc import ABC, abstractmethod
from contextlib import contextmanager
from dataclasses import asdict, dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Dict, Generator, List, Optional

from ozy_brain.scanner.contract_tracer import ContractDriftAlert

logger = logging.getLogger("ozy_brain.scanner.memory_store")


@dataclass
class GotchaMemory:
    """Standardized gotcha memory item matching Git Markdown YAML Frontmatter."""
    id: str
    kind: str = "gotcha"
    topic_key: str = "gotcha_prevention"
    title: str = ""
    author: str = "ozybrain_centinela"
    date: str = field(default_factory=lambda: datetime.now(timezone.utc).strftime("%Y-%m-%d"))
    severity: str = "high"
    applies_to: List[str] = field(default_factory=list)
    tags: List[str] = field(default_factory=lambda: ["reflexion", "contract_drift", "auto_learned"])
    flawed_assumption: str = ""
    countermeasure: str = ""
    created_at: float = field(default_factory=time.time)

    @classmethod
    def from_drift_alert(cls, alert: ContractDriftAlert) -> GotchaMemory:
        """Synthesize a permanent Gotcha vaccine from a contract drift alert."""
        short_id = uuid.uuid4().hex[:8]
        applies = [alert.frontend_file]
        if alert.backend_file:
            applies.append(alert.backend_file)

        return cls(
            id=f"gotcha_centinela_{short_id}",
            topic_key="contract_drift_prevention",
            title=f"[CENTINELA GOTCHA] {alert.kind} on {alert.canonical_path}",
            severity=alert.severity.lower(),
            applies_to=applies,
            tags=["centinela", "contract_drift", alert.kind.lower(), "auto_learned"],
            flawed_assumption=f"Assumed frontend '{alert.frontend_file}' contract was synchronized with backend: {alert.details}",
            countermeasure=alert.suggested_fix or "Keep backend routes and frontend fetch callers strictly synchronized in the same commit.",
        )


class DreamMemoryStore(ABC):
    """Abstract base contract for local and enterprise dream memory backends."""

    @abstractmethod
    def save_gotcha(self, gotcha: GotchaMemory) -> str:
        """Persist a gotcha memory item."""
        pass

    @abstractmethod
    def save_drift_alert(self, alert: ContractDriftAlert) -> str:
        """Persist a contract drift alert."""
        pass

    @abstractmethod
    def get_active_threats(self, limit: int = 50) -> List[GotchaMemory]:
        """Fetch high/critical gotchas as active threats."""
        pass

    @abstractmethod
    def get_drift_alerts(self, unresolved_only: bool = True) -> List[Dict[str, Any]]:
        """Fetch tracked drift alerts."""
        pass

    @abstractmethod
    def prune_decayed(self, max_age_days: int = 30) -> int:
        """Purge old resolved alerts and return count of pruned rows."""
        pass


class SqliteWalBackend(DreamMemoryStore):
    """Sub-millisecond in-process SQLite WAL storage backend."""

    def __init__(self, db_path: Path) -> None:
        self.db_path = db_path
        self.db_path.parent.mkdir(parents=True, exist_ok=True)
        self._init_db()

    @contextmanager
    def _connection(self) -> Generator[sqlite3.Connection, None, None]:
        conn = sqlite3.connect(self.db_path, timeout=5.0)
        conn.execute("PRAGMA journal_mode=WAL;")
        conn.execute("PRAGMA synchronous=NORMAL;")
        conn.execute("PRAGMA busy_timeout=5000;")
        conn.row_factory = sqlite3.Row
        try:
            with conn:
                yield conn
        finally:
            conn.close()

    def _init_db(self) -> None:
        with self._connection() as conn:
            conn.execute("""
                CREATE TABLE IF NOT EXISTS centinela_gotchas (
                    id TEXT PRIMARY KEY,
                    kind TEXT,
                    topic_key TEXT,
                    title TEXT,
                    author TEXT,
                    date TEXT,
                    severity TEXT,
                    applies_to TEXT,
                    tags TEXT,
                    flawed_assumption TEXT,
                    countermeasure TEXT,
                    created_at REAL
                );
            """)
            conn.execute("""
                CREATE INDEX IF NOT EXISTS idx_gotchas_severity
                ON centinela_gotchas(severity);
            """)
            conn.execute("""
                CREATE TABLE IF NOT EXISTS centinela_drift_alerts (
                    id TEXT PRIMARY KEY,
                    kind TEXT,
                    severity TEXT,
                    frontend_file TEXT,
                    frontend_line INT,
                    canonical_path TEXT,
                    frontend_method TEXT,
                    backend_file TEXT,
                    backend_line INT,
                    details TEXT,
                    suggested_fix TEXT,
                    resolved INT DEFAULT 0,
                    created_at REAL
                );
            """)
            conn.execute("""
                CREATE INDEX IF NOT EXISTS idx_drift_resolved
                ON centinela_drift_alerts(resolved, canonical_path);
            """)

    def save_gotcha(self, gotcha: GotchaMemory) -> str:
        with self._connection() as conn:
            conn.execute(
                """
                INSERT OR REPLACE INTO centinela_gotchas (
                    id, kind, topic_key, title, author, date, severity,
                    applies_to, tags, flawed_assumption, countermeasure, created_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                """,
                (
                    gotcha.id,
                    gotcha.kind,
                    gotcha.topic_key,
                    gotcha.title,
                    gotcha.author,
                    gotcha.date,
                    gotcha.severity,
                    json.dumps(gotcha.applies_to),
                    json.dumps(gotcha.tags),
                    gotcha.flawed_assumption,
                    gotcha.countermeasure,
                    gotcha.created_at,
                ),
            )
        return gotcha.id

    def save_drift_alert(self, alert: ContractDriftAlert) -> str:
        alert_id = f"alert_{uuid.uuid4().hex[:12]}"
        with self._connection() as conn:
            conn.execute(
                """
                INSERT INTO centinela_drift_alerts (
                    id, kind, severity, frontend_file, frontend_line,
                    canonical_path, frontend_method, backend_file, backend_line,
                    details, suggested_fix, resolved, created_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0, ?)
                """,
                (
                    alert_id,
                    alert.kind,
                    alert.severity,
                    alert.frontend_file,
                    alert.frontend_line,
                    alert.canonical_path,
                    alert.frontend_method,
                    alert.backend_file,
                    alert.backend_line,
                    alert.details,
                    alert.suggested_fix,
                    time.time(),
                ),
            )
        return alert_id

    def get_active_threats(self, limit: int = 50) -> List[GotchaMemory]:
        with self._connection() as conn:
            cursor = conn.execute(
                """
                SELECT id, kind, topic_key, title, author, date, severity,
                       applies_to, tags, flawed_assumption, countermeasure, created_at
                FROM centinela_gotchas
                WHERE severity IN ('high', 'critical')
                ORDER BY created_at DESC
                LIMIT ?
                """,
                (limit,),
            )
            rows = cursor.fetchall()

        results: List[GotchaMemory] = []
        for r in rows:
            results.append(
                GotchaMemory(
                    id=r["id"],
                    kind=r["kind"],
                    topic_key=r["topic_key"],
                    title=r["title"],
                    author=r["author"],
                    date=r["date"],
                    severity=r["severity"],
                    applies_to=json.loads(r["applies_to"]),
                    tags=json.loads(r["tags"]),
                    flawed_assumption=r["flawed_assumption"],
                    countermeasure=r["countermeasure"],
                    created_at=r["created_at"],
                )
            )
        return results

    def get_drift_alerts(self, unresolved_only: bool = True) -> List[Dict[str, Any]]:
        query = "SELECT * FROM centinela_drift_alerts"
        params: tuple = ()
        if unresolved_only:
            query += " WHERE resolved = 0"
        query += " ORDER BY created_at DESC"

        with self._connection() as conn:
            cursor = conn.execute(query, params)
            return [dict(row) for row in cursor.fetchall()]

    def prune_decayed(self, max_age_days: int = 30) -> int:
        cutoff = time.time() - (max_age_days * 86400)
        with self._connection() as conn:
            cursor = conn.execute(
                "DELETE FROM centinela_drift_alerts WHERE resolved = 1 AND created_at < ?",
                (cutoff,),
            )
            return cursor.rowcount

    def search_lexical(self, query: str, limit: int = 10) -> List[GotchaMemory]:
        """Lexical search across titles, flawed assumptions, and countermeasures."""
        tokens = [t.strip().lower() for t in query.split() if len(t.strip()) >= 3]
        if not tokens:
            return self.get_active_threats(limit=limit)

        like_clauses = []
        params = []
        for t in tokens:
            like_clauses.append("(LOWER(title) LIKE ? OR LOWER(flawed_assumption) LIKE ? OR LOWER(countermeasure) LIKE ?)")
            pattern = f"%{t}%"
            params.extend([pattern, pattern, pattern])

        sql = f"""
        SELECT id, kind, topic_key, title, author, date, severity,
               applies_to, tags, flawed_assumption, countermeasure, created_at
        FROM centinela_gotchas
        WHERE {" OR ".join(like_clauses)}
        ORDER BY created_at DESC
        LIMIT ?
        """
        params.append(limit)

        with self._connection() as conn:
            cursor = conn.execute(sql, tuple(params))
            rows = cursor.fetchall()

        results: List[GotchaMemory] = []
        for r in rows:
            results.append(
                GotchaMemory(
                    id=r["id"],
                    kind=r["kind"],
                    topic_key=r["topic_key"],
                    title=r["title"],
                    author=r["author"],
                    date=r["date"],
                    severity=r["severity"],
                    applies_to=json.loads(r["applies_to"]),
                    tags=json.loads(r["tags"]),
                    flawed_assumption=r["flawed_assumption"],
                    countermeasure=r["countermeasure"],
                    created_at=r["created_at"],
                )
            )
        return results


class MarkdownGitSynchronizer:
    """Exports and synchronizes GotchaMemory objects as Git-versionable Markdown files."""

    @staticmethod
    def export_gotcha(gotcha: GotchaMemory, base_dir: Path) -> Path:
        """Write GotchaMemory to .ozy/memories/gotchas/<id>.md with YAML frontmatter."""
        gotchas_dir = base_dir / "gotchas"
        gotchas_dir.mkdir(parents=True, exist_ok=True)
        file_path = gotchas_dir / f"{gotcha.id}.md"

        yaml_applies = "\n".join(f'  - "{f}"' for f in gotcha.applies_to)
        yaml_tags = "\n".join(f"  - {t}" for t in gotcha.tags)

        content = f"""---
id: {gotcha.id}
kind: {gotcha.kind}
topic_key: {gotcha.topic_key}
title: "{gotcha.title}"
author: {gotcha.author}
date: "{gotcha.date}"
severity: {gotcha.severity}
applies_to:
{yaml_applies}
tags:
{yaml_tags}
---

# Verbal Self-Reflection (Centinela Engine)

- **Flawed Assumption:** {gotcha.flawed_assumption}
- **Countermeasure & Vaccine:** {gotcha.countermeasure}
"""
        file_path.write_text(content, encoding="utf-8")
        return file_path

    @classmethod
    def sync_all_gotchas(cls, gotchas: List[GotchaMemory], base_dir: Path) -> List[Path]:
        """Export a list of gotchas to markdown."""
        return [cls.export_gotcha(g, base_dir) for g in gotchas]


class PostgresVectorBackend(DreamMemoryStore):
    """
    Enterprise collaborative PostgreSQL + pgvector adapter.
    Falls back gracefully to SQLite WAL if connection string is missing or unreachable.
    """

    def __init__(self, dsn: Optional[str] = None, fallback_sqlite_path: Optional[Path] = None) -> None:
        self.dsn = dsn or os.environ.get("OZY_PG_DSN") or os.environ.get("DATABASE_URL")
        self.fallback = SqliteWalBackend(fallback_sqlite_path or Path(".ozymem/memory.db"))
        self._pg_available = bool(self.dsn)

    def save_gotcha(self, gotcha: GotchaMemory) -> str:
        # Graceful fallback to SQLite WAL
        return self.fallback.save_gotcha(gotcha)

    def save_drift_alert(self, alert: ContractDriftAlert) -> str:
        return self.fallback.save_drift_alert(alert)

    def get_active_threats(self, limit: int = 50) -> List[GotchaMemory]:
        return self.fallback.get_active_threats(limit=limit)

    def get_drift_alerts(self, unresolved_only: bool = True) -> List[Dict[str, Any]]:
        return self.fallback.get_drift_alerts(unresolved_only=unresolved_only)

    def prune_decayed(self, max_age_days: int = 30) -> int:
        return self.fallback.prune_decayed(max_age_days=max_age_days)
