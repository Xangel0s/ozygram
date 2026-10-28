from __future__ import annotations

import gc
import sqlite3
import tempfile
import unittest
from pathlib import Path

from ozy_brain.amygdala import (
    AmygdalaReflexEngine,
    BasolateralAmygdala,
    CentralAmygdala,
    VmPFCExtinctionGater,
    normalize_rel_path,
)


class TestAmygdalaBipartiteAndExtinction(unittest.TestCase):
    def setUp(self):
        self.temp_dir = tempfile.TemporaryDirectory(ignore_cleanup_errors=True)
        self.db_path = Path(self.temp_dir.name) / "test_memory.db"
        self.engine = AmygdalaReflexEngine(
            db_path=self.db_path,
            tenant_id="test_tenant",
            workspace_root=self.temp_dir.name,
        )

        # Initialize SQLite database
        conn = sqlite3.connect(str(self.db_path))
        conn.execute(
            """CREATE TABLE amygdala_reflexes (
                id TEXT PRIMARY KEY,
                file_path TEXT NOT NULL,
                symbol_name TEXT NOT NULL DEFAULT '',
                threat_score REAL NOT NULL,
                pain_trigger TEXT NOT NULL,
                antidote_directive TEXT NOT NULL,
                occurrences INTEGER NOT NULL DEFAULT 1,
                last_triggered TEXT NOT NULL,
                created_at TEXT NOT NULL,
                tenant_id TEXT NOT NULL DEFAULT 'local',
                workspace_root TEXT NOT NULL DEFAULT '',
                clean_runs INTEGER NOT NULL DEFAULT 0,
                extinction_score REAL NOT NULL DEFAULT 0.0
            );"""
        )
        conn.execute("CREATE INDEX idx_amygdala_file ON amygdala_reflexes(file_path, tenant_id);")
        conn.commit()
        conn.close()

    def tearDown(self):
        gc.collect()
        try:
            self.temp_dir.cleanup()
        except Exception:
            pass

    def test_vmpfc_extinction_curve(self):
        """Validates sigmoidal inhibition curve mathematically."""
        # Clean runs = 0 -> No extinction (inhibition = 0.0)
        self.assertEqual(VmPFCExtinctionGater.calculate_inhibition(0), 0.0)

        # Clean runs = 3 -> Around inflection point ~0.50
        inhibition_3 = VmPFCExtinctionGater.calculate_inhibition(3)
        self.assertAlmostEqual(inhibition_3, 0.50, delta=0.05)

        # Clean runs = 6 -> Strong extinction > 0.90
        inhibition_6 = VmPFCExtinctionGater.calculate_inhibition(6)
        self.assertGreater(inhibition_6, 0.90)

    def test_central_amygdala_autonomic_gating(self):
        """Validates Central Amygdala (CeA) response thresholds."""
        target_file = "src/services/payment.ts"

        # 1. Uninhibited high threat -> CRITICAL_VETO
        res_veto = CentralAmygdala.evaluate_reflex(raw_threat=0.90, extinction_inhibition=0.0, file_path=target_file)
        self.assertEqual(res_veto["autonomic_action"], "CRITICAL_VETO")
        self.assertEqual(res_veto["effective_threat"], 0.90)
        self.assertIn("Block autonomous modifications", res_veto["directive"])

        # 2. Moderate threat -> CAUTION_ALERT
        res_alert = CentralAmygdala.evaluate_reflex(raw_threat=0.60, extinction_inhibition=0.10, file_path=target_file)
        self.assertEqual(res_alert["autonomic_action"], "CAUTION_ALERT")
        self.assertAlmostEqual(res_alert["effective_threat"], 0.54, delta=0.02)

        # 3. Extinguished threat -> EXTINGUISHED_SAFE
        res_safe = CentralAmygdala.evaluate_reflex(raw_threat=0.85, extinction_inhibition=0.80, file_path=target_file)
        self.assertEqual(res_safe["autonomic_action"], "EXTINGUISHED_SAFE")
        self.assertLess(res_safe["effective_threat"], 0.40)
        self.assertIn("mitigated by consecutive clean validations", res_safe["directive"])

    def test_clean_run_extinction_progression_and_relapse(self):
        """End-to-end test: trauma acquisition -> clean runs extinction -> relapse reinstatement."""
        test_file = "crates/ozymem-core/src/indexing.rs"

        # Step 1: Simulate trauma / regression entry written to SQLite
        self.engine.write_reflexes_to_sqlite([
            {
                "id": f"test_tenant::{test_file}::",
                "file_path": test_file,
                "symbol_name": "",
                "threat_score": 0.85,
                "pain_trigger": "fix: resolve compiler trait error",
                "antidote_directive": "Verify caller arity before modifying",
                "occurrences": 3,
                "last_triggered": "2026-10-28T00:00:00Z",
                "created_at": "2026-10-28T00:00:00Z",
                "clean_runs": 0,
                "extinction_score": 0.0,
            }
        ])

        # Initial state: Full threat
        initial_eval = self.engine.evaluate_file_threat(test_file)
        self.assertEqual(initial_eval["autonomic_action"], "CRITICAL_VETO")
        self.assertEqual(initial_eval["effective_threat"], 0.85)

        # Step 2: Simulate 5 consecutive successful validations
        for _ in range(5):
            self.engine.record_clean_run(test_file)

        # Post-extinction state: Threat should be extinguished and downgraded to EXTINGUISHED_SAFE
        post_eval = self.engine.evaluate_file_threat(test_file)
        self.assertEqual(post_eval["autonomic_action"], "EXTINGUISHED_SAFE")
        self.assertGreater(post_eval["extinction_inhibition"], 0.80)
        self.assertLess(post_eval["effective_threat"], 0.20)

        # Step 3: Relapse event (e.g. test failure on the file)
        self.engine.record_relapse(test_file)

        # Post-relapse state: Threat immediately reinstated (zero-latency fear conditioning)
        relapse_eval = self.engine.evaluate_file_threat(test_file)
        self.assertEqual(relapse_eval["autonomic_action"], "CRITICAL_VETO")
        self.assertEqual(relapse_eval["extinction_inhibition"], 0.0)
        self.assertEqual(relapse_eval["effective_threat"], 0.85)


if __name__ == "__main__":
    unittest.main()

