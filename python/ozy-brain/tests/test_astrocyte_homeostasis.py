from __future__ import annotations

import sqlite3
import tempfile
import unittest
from pathlib import Path

from ozy_brain.glial import AstrocyteMetabolicEngine


class TestAstrocyteHomeostasis(unittest.TestCase):
    def setUp(self):
        self.temp_dir = tempfile.TemporaryDirectory(ignore_cleanup_errors=True)
        self.db_path = Path(self.temp_dir.name) / "test_memory.db"

        # Create dummy SQLite DB in WAL mode
        conn = sqlite3.connect(str(self.db_path))
        conn.execute("PRAGMA journal_mode=WAL;")
        conn.execute("CREATE TABLE dummy (id INTEGER PRIMARY KEY, val TEXT);")
        for i in range(100):
            conn.execute("INSERT INTO dummy (val) VALUES (?);", (f"data_{i}",))
        conn.commit()
        conn.close()

        self.engine = AstrocyteMetabolicEngine(
            workspace_root=self.temp_dir.name,
            db_path=self.db_path,
            a_critical=50.0,
        )

    def tearDown(self):
        try:
            self.temp_dir.cleanup()
        except Exception:
            pass

    def test_adenosine_accumulation(self):
        self.assertEqual(self.engine.adenosine_level, 0.0)
        self.assertEqual(self.engine.sleep_pressure, 0.0)

        # 1. Low tax queries
        self.engine.register_metabolic_activity("query", weight=2.0)
        self.assertEqual(self.engine.adenosine_level, 1.0)

        # 2. Medium tax AST patch
        self.engine.register_metabolic_activity("ast_patch", weight=2.0)
        self.assertEqual(self.engine.adenosine_level, 6.0)

        # 3. High tax compile errors
        self.engine.register_metabolic_activity("compile_error", weight=4.0)
        self.assertEqual(self.engine.adenosine_level, 26.0)

        self.assertFalse(self.engine.requires_sleep_consolidation)

    def test_sleep_pressure_and_micro_nap(self):
        # Accumulate until exceeding threshold of 50.0
        for _ in range(11):
            self.engine.register_metabolic_activity("compile_error", weight=1.0)  # +5.0 each

        self.assertGreater(self.engine.adenosine_level, 50.0)
        self.assertGreaterEqual(self.engine.sleep_pressure, 1.0)
        self.assertTrue(self.engine.requires_sleep_consolidation)

        # Execute micro-nap consolidation
        report = self.engine.execute_micro_nap_consolidation()

        self.assertTrue(report["micro_nap_executed"])
        self.assertTrue(report["wal_checkpoint_truncate"])
        self.assertEqual(report["current_adenosine"], 0.0)
        self.assertEqual(report["sleep_pressure"], 0.0)
        self.assertFalse(self.engine.requires_sleep_consolidation)
        self.assertEqual(report["badge"], "[STATUS: ASTROCYTE_HOMEOSTASIS_RESTORED]")


if __name__ == "__main__":
    unittest.main()

