from __future__ import annotations

import sqlite3
import tempfile
import unittest
from pathlib import Path

from ozy_brain.outbox_consumer import OutboxEvent, OutboxStatus, UniversalOutbox


class TestUniversalOutbox(unittest.TestCase):
    def setUp(self):
        self.temp_dir = tempfile.TemporaryDirectory(ignore_cleanup_errors=True)
        self.db_path = Path(self.temp_dir.name) / "test_outbox.db"
        self.outbox = UniversalOutbox(db_path=self.db_path, dialect="sqlite")

    def tearDown(self):
        try:
            self.temp_dir.cleanup()
        except Exception:
            pass

    def test_schema_initialization_sqlite(self):
        conn = sqlite3.connect(str(self.db_path))
        cursor = conn.cursor()
        cursor.execute("SELECT name FROM sqlite_master WHERE type='table' AND name='outbox_events'")
        table = cursor.fetchone()
        self.assertIsNotNone(table)
        self.assertEqual(table[0], "outbox_events")
        conn.close()

    def test_postgres_ddl_and_dialect(self):
        pg_outbox = UniversalOutbox(dialect="postgres")
        ddl = pg_outbox.get_ddl()
        self.assertIn("BIGSERIAL PRIMARY KEY", ddl)
        self.assertIn("JSONB NOT NULL", ddl)
        self.assertIn("TIMESTAMPTZ NOT NULL DEFAULT NOW()", ddl)

    def test_publish_and_fetch_pending(self):
        event_id = self.outbox.publish(
            event_type="USER_REGISTERED",
            aggregate_id="usr_123",
            payload={"email": "dev@geofal.com", "name": "Dev User"},
        )
        self.assertGreaterEqual(event_id, 1)

        pending = self.outbox.fetch_pending(limit=10)
        self.assertEqual(len(pending), 1)
        evt = pending[0]
        self.assertEqual(evt.id, event_id)
        self.assertEqual(evt.event_type, "USER_REGISTERED")
        self.assertEqual(evt.aggregate_id, "usr_123")
        self.assertEqual(evt.payload["email"], "dev@geofal.com")
        self.assertEqual(evt.status, OutboxStatus.PENDING)
        self.assertEqual(evt.retry_count, 0)

    def test_publish_batch(self):
        events = [
            {"event_type": "QUOTE_CREATED", "aggregate_id": "q_1", "payload": {"amount": 100}},
            {"event_type": "QUOTE_CREATED", "aggregate_id": "q_2", "payload": {"amount": 250}},
            {"event_type": "QUOTE_CREATED", "aggregate_id": "q_3", "payload": {"amount": 500}},
        ]
        ids = self.outbox.publish_batch(events)
        self.assertEqual(len(ids), 3)

        pending = self.outbox.fetch_pending(limit=10)
        self.assertEqual(len(pending), 3)

    def test_drain_success_marks_processed(self):
        self.outbox.publish("INVOICE_SENT", "inv_99", {"total": 1200})

        dispatched = []

        def sample_handler(event: OutboxEvent) -> bool:
            dispatched.append(event.aggregate_id)
            return True

        stats = self.outbox.drain(handler=sample_handler)
        self.assertEqual(stats["processed"], 1)
        self.assertEqual(stats["failed"], 0)
        self.assertEqual(len(dispatched), 1)
        self.assertEqual(dispatched[0], "inv_99")

        # Verify status in database
        conn = sqlite3.connect(str(self.db_path))
        conn.row_factory = sqlite3.Row
        row = conn.execute("SELECT status, processed_at FROM outbox_events WHERE aggregate_id = 'inv_99'").fetchone()
        self.assertEqual(row["status"], OutboxStatus.PROCESSED)
        self.assertIsNotNone(row["processed_at"])
        conn.close()

    def test_drain_failure_increments_retry_count_and_retries(self):
        self.outbox.publish("WEBHOOK_DISPATCH", "wh_1", {"target": "https://api.thirdparty.com"})

        def failing_handler(event: OutboxEvent) -> bool:
            raise ConnectionError("Endpoint timed out")

        stats = self.outbox.drain(handler=failing_handler, max_retries=3)
        self.assertEqual(stats["retried"], 1)
        self.assertEqual(stats["processed"], 0)

        conn = sqlite3.connect(str(self.db_path))
        conn.row_factory = sqlite3.Row
        row = conn.execute("SELECT status, retry_count, error_message FROM outbox_events WHERE aggregate_id = 'wh_1'").fetchone()
        self.assertEqual(row["status"], OutboxStatus.PENDING)
        self.assertEqual(row["retry_count"], 1)
        self.assertIn("Endpoint timed out", row["error_message"])
        conn.close()

    def test_drain_max_retries_marks_failed(self):
        event_id = self.outbox.publish("CRITICAL_ALERT", "alert_5", {"msg": "Server high CPU"})

        # Manually set retry_count to 2 so next failure hits max_retries (3)
        conn = sqlite3.connect(str(self.db_path))
        conn.execute("UPDATE outbox_events SET retry_count = 2 WHERE id = ?", (event_id,))
        conn.commit()
        conn.close()

        def failing_handler(event: OutboxEvent) -> bool:
            return False

        stats = self.outbox.drain(handler=failing_handler, max_retries=3, enforce_backoff=False)
        self.assertEqual(stats["failed"], 1)

        conn = sqlite3.connect(str(self.db_path))
        conn.row_factory = sqlite3.Row
        row = conn.execute("SELECT status, retry_count FROM outbox_events WHERE id = ?", (event_id,)).fetchone()
        self.assertEqual(row["status"], OutboxStatus.FAILED)
        self.assertEqual(row["retry_count"], 3)
        conn.close()

    def test_exponential_backoff_skips_when_delay_not_met(self):
        event_id = self.outbox.publish("WEBHOOK", "wh_delayed", {"data": 1})

        # Set retry_count=2, delay should be 2^2 = 4 seconds
        conn = sqlite3.connect(str(self.db_path))
        conn.execute("UPDATE outbox_events SET retry_count = 2 WHERE id = ?", (event_id,))
        conn.commit()
        conn.close()

        called = []
        def handler(event: OutboxEvent) -> bool:
            called.append(event.id)
            return True

        # With enforce_backoff=True (default), it should skip the event because 4 seconds have not elapsed
        stats = self.outbox.drain(handler=handler, enforce_backoff=True)
        self.assertEqual(len(called), 0, "Event should be skipped due to exponential backoff")
        self.assertEqual(stats["processed"], 0)

    def test_transactional_atomicity_with_external_connection(self):
        conn = sqlite3.connect(str(self.db_path))
        try:
            # Publish inside transaction
            self.outbox.publish(
                "TEST_TX",
                "tx_1",
                {"step": 1},
                conn=conn,
            )
            # Explicit rollback
            conn.rollback()
        finally:
            conn.close()

        pending = self.outbox.fetch_pending()
        self.assertEqual(len(pending), 0, "Rolled back transaction must not persist outbox event")
