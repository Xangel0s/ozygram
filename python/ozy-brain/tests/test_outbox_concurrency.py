from __future__ import annotations

import concurrent.futures
import sqlite3
import tempfile
import threading
import time
import unittest
from pathlib import Path
from typing import Any

from ozy_brain.outbox_consumer import OutboxEvent, OutboxStatus, UniversalOutbox


class TestOutboxConcurrencyAndIdempotency(unittest.TestCase):
    def setUp(self):
        self.temp_dir = tempfile.TemporaryDirectory(ignore_cleanup_errors=True)
        self.db_path = Path(self.temp_dir.name) / "concurrency_outbox.db"
        self.outbox = UniversalOutbox(db_path=self.db_path, dialect="sqlite")

    def tearDown(self):
        try:
            self.temp_dir.cleanup()
        except Exception:
            pass

    def test_concurrent_drain_no_duplicate_processing(self):
        """Simulates multiple concurrent worker threads draining the same queue."""
        num_events = 40
        for i in range(num_events):
            self.outbox.publish("TELEMETRY", f"node_{i}", {"sensor_val": i * 10})

        processed_ids = set()
        lock = threading.Lock()
        duplicate_executions = []

        def worker_handler(event: OutboxEvent) -> bool:
            with lock:
                if event.id in processed_ids:
                    duplicate_executions.append(event.id)
                processed_ids.add(event.id)
            # Simulate small I/O work
            time.sleep(0.005)
            return True

        # Run 4 concurrent worker threads draining the outbox
        def drain_task():
            worker_outbox = UniversalOutbox(db_path=self.db_path, dialect="sqlite")
            for _ in range(5):
                worker_outbox.drain(handler=worker_handler, limit=10, enforce_backoff=False)
                time.sleep(0.01)

        with concurrent.futures.ThreadPoolExecutor(max_workers=4) as executor:
            futures = [executor.submit(drain_task) for _ in range(4)]
            concurrent.futures.wait(futures)

        # Verify all events are processed and no duplicates occurred
        self.assertEqual(len(processed_ids), num_events)
        self.assertEqual(len(duplicate_executions), 0, f"Duplicates detected: {duplicate_executions}")

        conn = sqlite3.connect(str(self.db_path))
        cursor = conn.cursor()
        cursor.execute("SELECT COUNT(*) FROM outbox_events WHERE status = 'PROCESSED'")
        processed_count = cursor.fetchone()[0]
        cursor.execute("SELECT COUNT(*) FROM outbox_events WHERE status != 'PROCESSED'")
        unprocessed_count = cursor.fetchone()[0]
        conn.close()

        self.assertEqual(processed_count, num_events)
        self.assertEqual(unprocessed_count, 0)

    def test_abrupt_interruption_zero_event_loss(self):
        """Simulates worker crash/abrupt failure midway, followed by complete recovery."""
        total_events = 25
        for i in range(total_events):
            self.outbox.publish("PAYMENT_CONFIRM", f"pay_{i}", {"amount": 50 + i})

        attempts = {}
        lock = threading.Lock()

        def crashing_handler(event: OutboxEvent) -> bool:
            with lock:
                count = attempts.get(event.id, 0) + 1
                attempts[event.id] = count

            # Abrupt crash simulation on first 2 attempts for even events
            if count <= 2 and int(str(event.aggregate_id).split("_")[1]) % 2 == 0:
                raise RuntimeError("[ALERT: SIMULATED_PROCESS_CRASH]")

            return True

        # Phase 1: Failing run - crashes on certain events
        self.outbox.drain(handler=crashing_handler, limit=50, max_retries=5, enforce_backoff=False)

        # Phase 2: Recovery run - worker restarts and continues draining until all succeed
        for _ in range(4):
            self.outbox.drain(handler=crashing_handler, limit=50, max_retries=5, enforce_backoff=False)

        # Verify zero event loss: all events must be PROCESSED
        conn = sqlite3.connect(str(self.db_path))
        cursor = conn.cursor()
        cursor.execute("SELECT COUNT(*) FROM outbox_events WHERE status = 'PROCESSED'")
        completed = cursor.fetchone()[0]
        cursor.execute("SELECT COUNT(*) FROM outbox_events WHERE status != 'PROCESSED'")
        pending_or_failed = cursor.fetchone()[0]
        conn.close()

        self.assertEqual(completed, total_events, f"Expected {total_events} processed, got {completed}")
        self.assertEqual(pending_or_failed, 0, "Zero events should be lost or left unfinished")

    def test_idempotent_receiver_deduplication(self):
        """Validates that a receiver can achieve idempotency using aggregate_id tracking."""
        processed_aggregates: set[str] = set()
        side_effects: list[str] = []

        def idempotent_consumer(payload: dict[str, Any], aggregate_id: str) -> bool:
            # Idempotency guard: ignore already-handled aggregates
            if aggregate_id in processed_aggregates:
                return True
            processed_aggregates.add(aggregate_id)
            side_effects.append(aggregate_id)
            return True

        def handler(event: OutboxEvent) -> bool:
            return idempotent_consumer(event.payload, event.aggregate_id)

        # Publish initial event
        self.outbox.publish("USER_SYNC", "user_abc", {"email": "u@test.com"})
        self.outbox.drain(handler=handler)

        self.assertEqual(len(side_effects), 1)

        # Simulate redelivery or duplicate event with same aggregate_id
        self.outbox.publish("USER_SYNC", "user_abc", {"email": "u@test.com"})
        self.outbox.drain(handler=handler)

        # Side effects should still be exactly 1 despite second delivery
        self.assertEqual(len(side_effects), 1)
