from __future__ import annotations

import gc
import json
import sqlite3
import tempfile
import unittest
from pathlib import Path

from ozy_brain.outbox_consumer import OutboxConsumer
from ozy_brain.vector_store import VectorMemoryStore, reciprocal_rank_fusion


class TestOutboxSync(unittest.TestCase):
    def setUp(self):
        self.temp_dir = tempfile.TemporaryDirectory(ignore_cleanup_errors=True)
        self.db_path = Path(self.temp_dir.name) / "test_memory.db"
        self.chroma_dir = Path(self.temp_dir.name) / "chroma"

        # Create schema for memory_outbox
        conn = sqlite3.connect(str(self.db_path))
        conn.execute(
            """
            CREATE TABLE memory_outbox (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                entity_type TEXT NOT NULL,
                entity_id TEXT NOT NULL,
                operation TEXT NOT NULL,
                payload TEXT NOT NULL,
                created_at TEXT NOT NULL,
                processed_at TEXT NULL
            )
            """
        )
        conn.commit()
        conn.close()

        self.consumer = OutboxConsumer(db_path=self.db_path, chroma_dir=self.chroma_dir)

    def tearDown(self):
        self.consumer.vector_store.close()
        gc.collect()
        try:
            self.temp_dir.cleanup()
        except Exception:
            pass

    def test_outbox_drain_upsert_and_delete(self):
        if not self.consumer.vector_store.is_available():
            self.skipTest("ChromaDB not available in this environment")

        # 1. Insert 2 pending events in memory_outbox
        conn = sqlite3.connect(str(self.db_path))
        conn.execute(
            """
            INSERT INTO memory_outbox (entity_type, entity_id, operation, payload, created_at)
            VALUES (?, ?, ?, ?, datetime('now'))
            """,
            (
                "lesson",
                "101",
                "UPSERT",
                json.dumps({
                    "symbol_name": "validate_jwt",
                    "error_context": "Invalid token expiry format",
                    "solution": "Parse ISO-8601 timestamps using chrono",
                    "kind": "convention",
                }),
            ),
        )
        conn.execute(
            """
            INSERT INTO memory_outbox (entity_type, entity_id, operation, payload, created_at)
            VALUES (?, ?, ?, ?, datetime('now'))
            """,
            (
                "observation",
                "202",
                "UPSERT",
                json.dumps({
                    "title": "Hexagonal Architecture Rule",
                    "content": "Ports and adapters must decouple storage from services",
                    "project": "ozygram",
                }),
            ),
        )
        conn.commit()
        conn.close()

        # 2. Drain outbox
        stats = self.consumer.drain(limit=10)
        self.assertEqual(stats["upserted"], 2)
        self.assertEqual(stats["total"], 2)

        # 3. Verify processed_at in SQLite
        conn = sqlite3.connect(str(self.db_path))
        cursor = conn.cursor()
        cursor.execute("SELECT COUNT(*) FROM memory_outbox WHERE processed_at IS NULL")
        unprocessed = cursor.fetchone()[0]
        self.assertEqual(unprocessed, 0, "All events should be marked as processed")
        conn.close()

        # 4. Verify items in ChromaDB
        hits = self.consumer.vector_store.search_similar("chrono timestamp ISO-8601", limit=3)
        self.assertGreaterEqual(len(hits), 1)
        self.assertIn("chrono", hits[0]["content"])

        # 5. Insert DELETE event
        conn = sqlite3.connect(str(self.db_path))
        conn.execute(
            """
            INSERT INTO memory_outbox (entity_type, entity_id, operation, payload, created_at)
            VALUES (?, ?, ?, ?, datetime('now'))
            """,
            ("lesson", "101", "DELETE", json.dumps({"id": 101})),
        )
        conn.commit()
        conn.close()

        # 6. Drain delete
        del_stats = self.consumer.drain(limit=10)
        self.assertEqual(del_stats["deleted"], 1)

    def test_reciprocal_rank_fusion(self):
        bm25_items = [
            {"id": "doc_1", "content": "Authentication token validation with RSA keys", "bm25_score": 10.5},
            {"id": "doc_2", "content": "Database pool connections with SQLite WAL mode", "bm25_score": 8.2},
            {"id": "doc_3", "content": "Tree-sitter AST symbol indexing", "bm25_score": 5.1},
        ]
        vector_items = [
            {"id": "doc_2", "content": "Database pool connections with SQLite WAL mode", "similarity_score": 0.89},
            {"id": "doc_4", "content": "Neural reranking with Cross-Encoder models", "similarity_score": 0.85},
            {"id": "doc_1", "content": "Authentication token validation with RSA keys", "similarity_score": 0.72},
        ]

        # In RRF, doc_2 is rank 2 in BM25 and rank 1 in vector -> high combined score
        fused = reciprocal_rank_fusion(bm25_items, vector_items, k=60, top_k=3)
        self.assertEqual(len(fused), 3)
        # Verify doc_2 and doc_1 are at the top because they appeared in both lists
        top_ids = [item["id"] for item in fused]
        self.assertIn("doc_2", top_ids[:2])
        self.assertIn("doc_1", top_ids[:2])
        self.assertTrue(all("rrf_score" in item for item in fused))
