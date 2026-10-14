from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from ozy_brain.outbox_consumer import (
    DiagnosticPrescription,
    OutboxStatus,
    ProactiveDiagnosticConsumer,
    UniversalOutbox,
)


class TestProactiveDiagnosticConsumer(unittest.TestCase):
    def setUp(self):
        self.temp_dir = tempfile.TemporaryDirectory(ignore_cleanup_errors=True)
        self.db_path = Path(self.temp_dir.name) / "test_outbox.db"
        self.outbox = UniversalOutbox(db_path=self.db_path, dialect="sqlite")
        self.consumer = ProactiveDiagnosticConsumer(self.outbox)

    def tearDown(self):
        try:
            self.temp_dir.cleanup()
        except Exception:
            pass

    def test_missing_import_python_prescription(self):
        payload = {
            "file_path": "crmnew/api-geofal-crm/app/routers/sig.py",
            "line": 42,
            "message": "NameError: name 'CotizacionService' is not defined",
            "code": "F821",
            "source": "ruff",
        }
        rx = self.consumer.analyze_diagnostic(payload, event_id=1, aggregate_id="diag_py_1")

        self.assertEqual(rx.category, "MISSING_IMPORT")
        self.assertIn("CotizacionService", rx.diagnosis)
        self.assertIn("CotizacionService", rx.suggested_action)
        self.assertEqual(rx.code_fix, "from ... import CotizacionService")
        self.assertGreaterEqual(rx.confidence, 0.90)
        self.assertLess(rx.latency_ms, 1000.0, "Must be computed in < 1 second")

    def test_missing_import_typescript_prescription(self):
        payload = {
            "file_path": "crmnew/crm-geofal/src/components/OrdenForm.tsx",
            "line": 88,
            "message": "Cannot find name 'useRouter'",
            "code": "TS2304",
            "source": "tsc",
        }
        rx = self.consumer.analyze_diagnostic(payload, event_id=2, aggregate_id="diag_ts_1")

        self.assertEqual(rx.category, "MISSING_IMPORT")
        self.assertIn("useRouter", rx.suggested_action)
        self.assertIn("import { useRouter }", rx.code_fix)
        self.assertLess(rx.latency_ms, 1000.0)

    def test_async_await_mismatch_prescription(self):
        payload = {
            "file_path": "app/services/outbox.py",
            "line": 15,
            "message": "RuntimeWarning: coroutine 'publish_event' was never awaited",
            "code": "ASYNC_MISMATCH",
        }
        rx = self.consumer.analyze_diagnostic(payload, event_id=3, aggregate_id="diag_async_1")

        self.assertEqual(rx.category, "ASYNC_AWAIT_MISMATCH")
        self.assertIn("await", rx.suggested_action.lower())
        self.assertGreaterEqual(rx.confidence, 0.90)

    def test_rust_borrow_checker_prescription(self):
        payload = {
            "file_path": "crates/ozymem-core/src/graph_backend/memory_graph.rs",
            "line": 780,
            "message": "cannot borrow `*inner` as mutable more than once at a time",
            "code": "E0502",
            "source": "rustc",
        }
        rx = self.consumer.analyze_diagnostic(payload, event_id=4, aggregate_id="diag_rs_1")

        self.assertEqual(rx.category, "MUTABILITY_OR_BORROW")
        self.assertIn("borrow", rx.suggested_action.lower())
        self.assertGreaterEqual(rx.confidence, 0.85)

    def test_postmortem_invariant_violation_prescription(self):
        payload = {
            "file_path": "crmnew/api-geofal-crm/app/routers/control_probetas.py",
            "line": 120,
            "message": "INVARIANT[control_probetas.costo_servicio]: NEVER OVERWRITE WITH COTIZACION_TOTAL",
            "code": "INVARIANT_VETO",
        }
        rx = self.consumer.analyze_diagnostic(payload, event_id=5, aggregate_id="diag_inv_1")

        self.assertEqual(rx.category, "INVARIANT_VIOLATION")
        self.assertIn("[CRITICAL_VETO", rx.suggested_action)
        self.assertEqual(rx.confidence, 0.99)

    def test_end_to_end_outbox_drain_and_reactive_prescription(self):
        # 1. Publish 2 DIAGNOSTIC_ERROR events to the outbox
        self.outbox.publish(
            event_type="DIAGNOSTIC_ERROR",
            aggregate_id="diag_err_101",
            payload={
                "file_path": "crmnew/api-geofal-crm/app/main.py",
                "line": 10,
                "message": "SyntaxError: invalid syntax",
                "code": "SYNTAX",
            },
        )
        self.outbox.publish(
            event_type="DIAGNOSTIC_ERROR",
            aggregate_id="diag_err_102",
            payload={
                "file_path": "crmnew/crm-geofal/src/app/page.tsx",
                "line": 55,
                "message": "Type 'string' is not assignable to type 'number'",
                "code": "TS2322",
            },
        )

        # 2. Drain proactively
        prescriptions = self.consumer.drain_proactive_diagnostics(limit=10, publish_prescriptions=True)

        self.assertEqual(len(prescriptions), 2)
        rx1 = prescriptions[0]
        self.assertEqual(rx1["category"], "SYNTAX_ERROR")
        self.assertEqual(rx1["file_path"], "crmnew/api-geofal-crm/app/main.py")
        self.assertLess(rx1["latency_ms"], 1000.0)

        rx2 = prescriptions[1]
        self.assertEqual(rx2["category"], "TYPE_MISMATCH")
        self.assertEqual(rx2["file_path"], "crmnew/crm-geofal/src/app/page.tsx")

        # 3. Verify original DIAGNOSTIC_ERROR events are now marked PROCESSED
        pending_after = self.outbox.fetch_pending(limit=10)
        error_events = [e for e in pending_after if e.event_type == "DIAGNOSTIC_ERROR"]
        self.assertEqual(len(error_events), 0, "All diagnostic error events must be PROCESSED")

        # 4. Verify proactive DIAGNOSTIC_PRESCRIPTION events were emitted into the outbox
        rx_events = [e for e in pending_after if e.event_type == "DIAGNOSTIC_PRESCRIPTION"]
        self.assertEqual(len(rx_events), 2, "Proactive prescriptions must be published to outbox")
        self.assertEqual(rx_events[0].aggregate_id, "diag_err_101")
        self.assertEqual(rx_events[0].payload["category"], "SYNTAX_ERROR")


if __name__ == "__main__":
    unittest.main()
