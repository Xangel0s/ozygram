from __future__ import annotations

import json
import sqlite3
from pathlib import Path
from typing import Any

from ozy_brain.vector_store import VectorMemoryStore


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
