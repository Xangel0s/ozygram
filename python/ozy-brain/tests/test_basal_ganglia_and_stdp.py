from __future__ import annotations

import gc
import json
import sqlite3
import tempfile
import unittest
from pathlib import Path

from ozy_brain.procedural_habits import BasalGangliaEngine
from ozy_brain.stdp_synapse import StdpSynapseEngine


class TestBasalGangliaEngine(unittest.TestCase):
    def setUp(self):
        self.temp_dir = tempfile.TemporaryDirectory(ignore_cleanup_errors=True)
        self.workspace = Path(self.temp_dir.name)
        self.db_path = self.workspace / ".ozymem" / "db.sqlite"
        self.db_path.parent.mkdir(parents=True, exist_ok=True)

        self.engine = BasalGangliaEngine(
            workspace_root=self.workspace,
            db_path=self.db_path,
            tenant_id="test_tenant",
        )

    def tearDown(self):
        gc.collect()
        try:
            self.temp_dir.cleanup()
        except Exception:
            pass

    def test_mine_procedural_playbooks_and_sqlite_write(self):
        # Synthetic sessions showing consistent choreography:
        # Whenever dispatch.rs is modified, schemas.rs and server.rs follow
        sessions = [
            {
                "commit_hash": "c1",
                "timestamp": 1700000000,
                "files": ["crates/server/dispatch.rs", "crates/server/schemas.rs", "crates/server/server.rs"],
            },
            {
                "commit_hash": "c2",
                "timestamp": 1700000600,
                "files": ["crates/server/dispatch.rs", "crates/server/schemas.rs", "crates/server/server.rs"],
            },
            {
                "commit_hash": "c3",
                "timestamp": 1700001200,
                "files": ["crates/server/dispatch.rs", "crates/server/schemas.rs"],
            },
            {
                "commit_hash": "c4",
                "timestamp": 1700001800,
                "files": ["other/docs.md", "other/readme.md"],
            },
        ]

        habits = self.engine.mine_procedural_playbooks(sessions, min_confidence=0.50, min_co_occurrences=2)
        self.assertGreaterEqual(len(habits), 1)

        dispatch_habit = next((h for h in habits if h["trigger_file"] == "crates/server/dispatch.rs"), None)
        self.assertIsNotNone(dispatch_habit, "Must mine habit triggered by dispatch.rs")
        self.assertIn("crates/server/schemas.rs", dispatch_habit["subsequent_files"])
        self.assertIn("crates/server/server.rs", dispatch_habit["subsequent_files"])
        self.assertGreaterEqual(dispatch_habit["confidence"], 0.60)

        # Write to SQLite
        written = self.engine.write_habits_to_sqlite(habits)
        self.assertGreaterEqual(written, 1)

        # Verify in SQLite directly
        conn = sqlite3.connect(str(self.db_path))
        cur = conn.cursor()
        cur.execute("SELECT trigger_file, choreography_json, confidence, occurrences FROM procedural_habits WHERE trigger_file = 'crates/server/dispatch.rs'")
        row = cur.fetchone()
        conn.close()

        self.assertIsNotNone(row)
        self.assertEqual(row[0], "crates/server/dispatch.rs")
        subsequent = json.loads(row[1])
        self.assertIn("crates/server/schemas.rs", subsequent)
        self.assertGreaterEqual(row[2], 0.60)


class TestStdpSynapseEngine(unittest.TestCase):
    def setUp(self):
        self.temp_dir = tempfile.TemporaryDirectory(ignore_cleanup_errors=True)
        self.workspace = Path(self.temp_dir.name)
        self.duckdb_path = self.workspace / "analytics.duckdb"

        self.engine = StdpSynapseEngine(
            workspace_root=self.workspace,
            duckdb_path=self.duckdb_path,
        )

    def tearDown(self):
        gc.collect()
        try:
            self.temp_dir.cleanup()
        except Exception:
            pass

    def test_stdp_mathematical_delta(self):
        # 1. Causal transition Delta_t > 0 -> LTP (positive delta)
        delta_causal_fast = self.engine.calculate_stdp_delta(10.0)
        self.assertGreater(delta_causal_fast, 0.0)

        delta_causal_slow = self.engine.calculate_stdp_delta(300.0)
        self.assertGreater(delta_causal_slow, 0.0)
        self.assertGreater(delta_causal_fast, delta_causal_slow, "Faster transition must yield higher LTP")

        # 2. Anti-causal or simultaneous transition Delta_t <= 0 -> LTD (negative delta)
        delta_anti = self.engine.calculate_stdp_delta(-10.0)
        self.assertLess(delta_anti, 0.0)

    def test_record_synapse_and_query_strongest(self):
        # Simulate causal workflow: backend migration -> frontend DTO (t_2 > t_1)
        w1 = self.engine.record_synapse_spike(
            source_file="backend/migrations/001_user.sql",
            target_file="frontend/models/user.ts",
            dt_seconds=15.0,
        )
        self.assertGreater(w1, 0.50, "Causal spike must increase synaptic weight above baseline")

        # Repeat causal transition to reinforce Hebbian bond
        w2 = self.engine.record_synapse_spike(
            source_file="backend/migrations/001_user.sql",
            target_file="frontend/models/user.ts",
            dt_seconds=5.0,
        )
        self.assertGreater(w2, w1, "Subsequent causal spike must further potentiate weight")

        # Query strongest synapses
        strongest = self.engine.get_strongest_synapses_for_file("backend/migrations/001_user.sql", min_weight=0.50)
        self.assertGreaterEqual(len(strongest), 1)
        self.assertEqual(strongest[0]["target_file"], "frontend/models/user.ts")
        self.assertAlmostEqual(strongest[0]["weight"], w2, places=2)

    def test_process_event_trace(self):
        trace = [
            {"file_path": "api/routes.py", "timestamp": 100.0},
            {"file_path": "api/controllers.py", "timestamp": 115.0},
            {"file_path": "frontend/client.ts", "timestamp": 140.0},
        ]
        res = self.engine.process_event_trace(trace)
        self.assertGreaterEqual(res["synapses_updated"], 2)


if __name__ == "__main__":
    unittest.main()
