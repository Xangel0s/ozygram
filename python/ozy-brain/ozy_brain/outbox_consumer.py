from __future__ import annotations

import json
import sqlite3
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Callable

from ozy_brain.vector_store import VectorMemoryStore


class OutboxStatus:
    PENDING = "PENDING"
    PROCESSING = "PROCESSING"
    PROCESSED = "PROCESSED"
    FAILED = "FAILED"


@dataclass
class OutboxEvent:
    id: int | str
    event_type: str
    aggregate_id: str
    payload: dict[str, Any]
    status: str = OutboxStatus.PENDING
    retry_count: int = 0
    created_at: str = ""
    processed_at: str | None = None
    error_message: str | None = None

    def to_dict(self) -> dict[str, Any]:
        return {
            "id": self.id,
            "event_type": self.event_type,
            "aggregate_id": self.aggregate_id,
            "payload": self.payload,
            "status": self.status,
            "retry_count": self.retry_count,
            "created_at": self.created_at,
            "processed_at": self.processed_at,
            "error_message": self.error_message,
        }


class UniversalOutbox:
    """Generic transactional Outbox Pattern supporting SQLite and PostgreSQL.

    Guarantees at-least-once delivery, decoupled asynchronous dispatching,
    and exponential backoff retry management for microservices and CRM backends.
    """

    SQLITE_DDL = """
    CREATE TABLE IF NOT EXISTS outbox_events (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        event_type TEXT NOT NULL,
        aggregate_id TEXT NOT NULL,
        payload TEXT NOT NULL,
        status TEXT NOT NULL DEFAULT 'PENDING',
        retry_count INTEGER NOT NULL DEFAULT 0,
        created_at TEXT NOT NULL,
        processed_at TEXT NULL,
        error_message TEXT NULL
    );
    CREATE INDEX IF NOT EXISTS idx_outbox_events_status ON outbox_events(status, retry_count);
    """

    POSTGRES_DDL = """
    CREATE TABLE IF NOT EXISTS outbox_events (
        id BIGSERIAL PRIMARY KEY,
        event_type VARCHAR(255) NOT NULL,
        aggregate_id VARCHAR(255) NOT NULL,
        payload JSONB NOT NULL,
        status VARCHAR(50) NOT NULL DEFAULT 'PENDING',
        retry_count INTEGER NOT NULL DEFAULT 0,
        created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
        processed_at TIMESTAMPTZ NULL,
        error_message TEXT NULL
    );
    CREATE INDEX IF NOT EXISTS idx_outbox_events_status ON outbox_events(status, retry_count);
    """

    def __init__(
        self,
        db_path: str | Path | None = None,
        connection: Any = None,
        dialect: str = "sqlite",
    ):
        self.dialect = dialect.lower()
        self.connection = connection
        self.db_path = Path(db_path) if db_path is not None else None
        if self.connection is None and self.db_path is not None and self.dialect == "sqlite":
            self.init_schema()

    def get_ddl(self) -> str:
        return self.POSTGRES_DDL if self.dialect == "postgres" else self.SQLITE_DDL

    def _get_conn(self, external_conn: Any = None):
        if external_conn is not None:
            return external_conn, False
        if self.connection is not None:
            return self.connection, False
        if self.db_path is not None and self.dialect == "sqlite":
            conn = sqlite3.connect(str(self.db_path), timeout=10.0)
            conn.row_factory = sqlite3.Row
            return conn, True
        raise ValueError("[ERROR: OUTBOX_NO_CONNECTION] No database connection or path configured.")

    def init_schema(self, conn: Any = None) -> None:
        """Applies idempotent DDL to create outbox_events table and index."""
        active_conn, should_close = self._get_conn(conn)
        try:
            cursor = active_conn.cursor()
            if self.dialect == "sqlite":
                cursor.executescript(self.SQLITE_DDL)
            else:
                cursor.execute(self.POSTGRES_DDL)
            if hasattr(active_conn, "commit"):
                active_conn.commit()
        finally:
            if should_close:
                active_conn.close()

    def publish(
        self,
        event_type: str,
        aggregate_id: str,
        payload: dict[str, Any] | str,
        conn: Any = None,
    ) -> int | str:
        """Publishes an event transactionally into outbox_events."""
        payload_str = json.dumps(payload) if isinstance(payload, dict) else str(payload)
        now_iso = datetime.now(timezone.utc).isoformat()

        active_conn, should_close = self._get_conn(conn)
        try:
            cursor = active_conn.cursor()
            if self.dialect == "sqlite":
                cursor.execute(
                    """
                    INSERT INTO outbox_events (event_type, aggregate_id, payload, status, retry_count, created_at)
                    VALUES (?, ?, ?, 'PENDING', 0, ?)
                    """,
                    (event_type, str(aggregate_id), payload_str, now_iso),
                )
                event_id = cursor.lastrowid
            else:
                cursor.execute(
                    """
                    INSERT INTO outbox_events (event_type, aggregate_id, payload, status, retry_count, created_at)
                    VALUES (%s, %s, %s, 'PENDING', 0, NOW())
                    RETURNING id
                    """,
                    (event_type, str(aggregate_id), payload_str),
                )
                row = cursor.fetchone()
                event_id = row[0] if row else 0

            if hasattr(active_conn, "commit") and should_close:
                active_conn.commit()
            return event_id
        finally:
            if should_close:
                active_conn.close()

    def publish_batch(
        self,
        events: list[dict[str, Any]],
        conn: Any = None,
    ) -> list[int | str]:
        """Publishes multiple events in a single transaction."""
        published_ids = []
        active_conn, should_close = self._get_conn(conn)
        try:
            cursor = active_conn.cursor()
            now_iso = datetime.now(timezone.utc).isoformat()
            for evt in events:
                event_type = evt.get("event_type", "GENERIC")
                aggregate_id = str(evt.get("aggregate_id", ""))
                raw_payload = evt.get("payload", {})
                payload_str = json.dumps(raw_payload) if isinstance(raw_payload, dict) else str(raw_payload)

                if self.dialect == "sqlite":
                    cursor.execute(
                        """
                        INSERT INTO outbox_events (event_type, aggregate_id, payload, status, retry_count, created_at)
                        VALUES (?, ?, ?, 'PENDING', 0, ?)
                        """,
                        (event_type, aggregate_id, payload_str, now_iso),
                    )
                    published_ids.append(cursor.lastrowid)
                else:
                    cursor.execute(
                        """
                        INSERT INTO outbox_events (event_type, aggregate_id, payload, status, retry_count, created_at)
                        VALUES (%s, %s, %s, 'PENDING', 0, NOW())
                        RETURNING id
                        """,
                        (event_type, aggregate_id, payload_str),
                    )
                    row = cursor.fetchone()
                    published_ids.append(row[0] if row else 0)

            if hasattr(active_conn, "commit") and should_close:
                active_conn.commit()
            return published_ids
        finally:
            if should_close:
                active_conn.close()

    def fetch_pending(
        self,
        limit: int = 100,
        max_retries: int = 5,
        conn: Any = None,
    ) -> list[OutboxEvent]:
        """Fetches pending or retryable failed events."""
        active_conn, should_close = self._get_conn(conn)
        try:
            cursor = active_conn.cursor()
            query_sqlite = """
                SELECT id, event_type, aggregate_id, payload, status, retry_count, created_at, processed_at, error_message
                FROM outbox_events
                WHERE (status = 'PENDING' OR (status = 'FAILED' AND retry_count < ?))
                ORDER BY id ASC
                LIMIT ?
            """
            query_pg = """
                SELECT id, event_type, aggregate_id, payload, status, retry_count, created_at, processed_at, error_message
                FROM outbox_events
                WHERE (status = 'PENDING' OR (status = 'FAILED' AND retry_count < %s))
                ORDER BY id ASC
                LIMIT %s
            """
            if self.dialect == "sqlite":
                cursor.execute(query_sqlite, (max_retries, limit))
            else:
                cursor.execute(query_pg, (max_retries, limit))

            rows = cursor.fetchall()
            events = []
            for r in rows:
                raw_payload = r[3] if isinstance(r, (tuple, list)) else r["payload"]
                try:
                    payload = json.loads(raw_payload) if isinstance(raw_payload, str) else raw_payload
                except Exception:
                    payload = {}

                events.append(
                    OutboxEvent(
                        id=r[0] if isinstance(r, (tuple, list)) else r["id"],
                        event_type=r[1] if isinstance(r, (tuple, list)) else r["event_type"],
                        aggregate_id=r[2] if isinstance(r, (tuple, list)) else r["aggregate_id"],
                        payload=payload,
                        status=r[4] if isinstance(r, (tuple, list)) else r["status"],
                        retry_count=r[5] if isinstance(r, (tuple, list)) else r["retry_count"],
                        created_at=str(r[6] if isinstance(r, (tuple, list)) else r["created_at"]),
                        processed_at=str(r[7]) if (r[7] if isinstance(r, (tuple, list)) else r["processed_at"]) else None,
                        error_message=r[8] if isinstance(r, (tuple, list)) else r["error_message"],
                    )
                )
            return events
        finally:
            if should_close:
                active_conn.close()

    def drain(
        self,
        handler: Callable[[OutboxEvent], bool | None],
        limit: int = 100,
        max_retries: int = 5,
        backoff_base: float = 2.0,
        enforce_backoff: bool = True,
        conn: Any = None,
    ) -> dict[str, int]:
        """Drains pending events using exponential backoff.

        handler returns True (or None) on success, False or raises Exception on failure.
        Backoff formula: delay = backoff_base ** retry_count (seconds).
        """
        stats = {"processed": 0, "failed": 0, "retried": 0, "total": 0}
        events = self.fetch_pending(limit=limit, max_retries=max_retries, conn=conn)
        if not events:
            return stats

        active_conn, should_close = self._get_conn(conn)
        try:
            cursor = active_conn.cursor()
            now_dt = datetime.now(timezone.utc)
            now_iso = now_dt.isoformat()

            for event in events:
                stats["total"] += 1
                if enforce_backoff and event.retry_count > 0 and event.created_at:
                    try:
                        created_clean = event.created_at.replace("Z", "+00:00")
                        event_dt = datetime.fromisoformat(created_clean)
                        if event_dt.tzinfo is None:
                            event_dt = event_dt.replace(tzinfo=timezone.utc)
                        elapsed_secs = (now_dt - event_dt).total_seconds()
                        backoff_delay = backoff_base ** event.retry_count
                        if elapsed_secs < backoff_delay:
                            continue
                    except Exception:
                        pass

                success = False
                err_msg = None
                try:
                    res = handler(event)
                    success = res is not False
                except Exception as exc:
                    success = False
                    err_msg = str(exc)

                if success:
                    if self.dialect == "sqlite":
                        cursor.execute(
                            "UPDATE outbox_events SET status = 'PROCESSED', processed_at = ?, error_message = NULL WHERE id = ?",
                            (now_iso, event.id),
                        )
                    else:
                        cursor.execute(
                            "UPDATE outbox_events SET status = 'PROCESSED', processed_at = NOW(), error_message = NULL WHERE id = %s",
                            (event.id,),
                        )
                    stats["processed"] += 1
                else:
                    new_retry = event.retry_count + 1
                    err_text = err_msg or "Handler returned False"
                    if new_retry >= max_retries:
                        if self.dialect == "sqlite":
                            cursor.execute(
                                "UPDATE outbox_events SET status = 'FAILED', retry_count = ?, error_message = ? WHERE id = ?",
                                (new_retry, err_text, event.id),
                            )
                        else:
                            cursor.execute(
                                "UPDATE outbox_events SET status = 'FAILED', retry_count = %s, error_message = %s WHERE id = %s",
                                (new_retry, err_text, event.id),
                            )
                        stats["failed"] += 1
                    else:
                        if self.dialect == "sqlite":
                            cursor.execute(
                                "UPDATE outbox_events SET status = 'PENDING', retry_count = ?, error_message = ? WHERE id = ?",
                                (new_retry, err_text, event.id),
                            )
                        else:
                            cursor.execute(
                                "UPDATE outbox_events SET status = 'PENDING', retry_count = %s, error_message = %s WHERE id = %s",
                                (new_retry, err_text, event.id),
                            )
                        stats["retried"] += 1

            if hasattr(active_conn, "commit"):
                active_conn.commit()
            return stats
        finally:
            if should_close:
                active_conn.close()


class OutboxConsumer:
    """Drains pending SQLite memory_outbox events and synchronizes them with ChromaDB."""

    def __init__(
        self,
        db_path: str | Path | None = None,
        chroma_dir: str | Path | None = None,
    ):
        if db_path is not None:
            self.db_path = Path(db_path)
        else:
            home_db = Path.home() / ".ozymem" / "memory.db"
            self.db_path = home_db if home_db.exists() else Path(".ozymem/memory.db")

        self.vector_store = VectorMemoryStore(persist_dir=chroma_dir)

    def drain(self, limit: int = 100) -> dict[str, int]:
        """Polls and drains unprocessed outbox events from SQLite into ChromaDB."""
        stats = {"upserted": 0, "deleted": 0, "errors": 0, "total": 0}
        if not self.db_path.exists():
            return stats

        if not self.vector_store.is_available():
            return stats

        try:
            conn = sqlite3.connect(str(self.db_path), timeout=5.0)
            conn.row_factory = sqlite3.Row
            cursor = conn.cursor()

            cursor.execute(
                """
                SELECT id, entity_type, entity_id, operation, payload, created_at
                FROM memory_outbox
                WHERE processed_at IS NULL
                ORDER BY id ASC
                LIMIT ?
                """,
                (limit,),
            )
            rows = cursor.fetchall()
            if not rows:
                conn.close()
                return stats

            processed_ids = []
            for row in rows:
                event_id = row["id"]
                entity_type = row["entity_type"]
                entity_id = row["entity_id"]
                operation = row["operation"]
                raw_payload = row["payload"]

                try:
                    payload = json.loads(raw_payload) if isinstance(raw_payload, str) else raw_payload
                except Exception:
                    payload = {}

                vector_id = f"{entity_type}:{entity_id}"

                if operation == "DELETE":
                    ok = self.vector_store.delete_item(vector_id)
                    self.vector_store.delete_item(str(entity_id))
                    if ok:
                        stats["deleted"] += 1
                elif operation == "UPSERT":
                    if entity_type == "lesson":
                        ctx = payload.get("error_context", "")
                        sol = payload.get("solution", "")
                        sym = payload.get("symbol_name", "")
                        content = f"Symbol: {sym}\nError: {ctx}\nSolution: {sol}".strip()
                    else:
                        title = payload.get("title", "")
                        body = payload.get("content", "")
                        content = f"{title}\n{body}".strip()

                    metadata = {
                        "entity_type": str(entity_type),
                        "entity_id": str(entity_id),
                        "kind": str(payload.get("kind", entity_type)),
                        "project": str(payload.get("project", "")),
                        "scope": str(payload.get("scope", "project")),
                        "file_path": str(payload.get("file_path", "")),
                        "created_at": str(row["created_at"]),
                    }

                    ok = self.vector_store.upsert_lesson(vector_id, content, metadata=metadata)
                    if ok:
                        stats["upserted"] += 1
                    else:
                        stats["errors"] += 1

                processed_ids.append(event_id)

            if processed_ids:
                placeholders = ",".join("?" for _ in processed_ids)
                cursor.execute(
                    f"UPDATE memory_outbox SET processed_at = datetime('now') WHERE id IN ({placeholders})",
                    processed_ids,
                )
                conn.commit()

            stats["total"] = len(processed_ids)
            conn.close()
            return stats

        except Exception:
            stats["errors"] += 1
            return stats
