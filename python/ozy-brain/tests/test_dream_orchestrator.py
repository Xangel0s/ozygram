from __future__ import annotations

import gc
import json
import sqlite3
import tempfile
import unittest
from pathlib import Path

from ozy_brain.brain import run
from ozy_brain.dream.orchestrator import DreamOrchestrator


class TestDreamOrchestrator(unittest.TestCase):
    def setUp(self):
        self.temp_dir = tempfile.TemporaryDirectory(ignore_cleanup_errors=True)
        self.workspace = Path(self.temp_dir.name)
        self.ozymem_dir = self.workspace / ".ozymem"
        self.ozymem_dir.mkdir(parents=True, exist_ok=True)
        self.db_path = self.ozymem_dir / "db.sqlite"

        # Create physical test files so Glial phagocytosis doesn't prune them
        core_dir = self.workspace / "core"
        core_dir.mkdir(parents=True, exist_ok=True)
        (core_dir / "dispatch.rs").write_text("// dummy dispatch", encoding="utf-8")
        (core_dir / "schemas.rs").write_text("// dummy schemas", encoding="utf-8")
        (core_dir / "server.rs").write_text("// dummy server", encoding="utf-8")

        # Pre-seed SQLite with schema and sample records
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
                workspace_root TEXT NOT NULL DEFAULT ''
            );"""
        )
        conn.execute(
            """INSERT INTO amygdala_reflexes (
                id, file_path, symbol_name, threat_score, pain_trigger,
                antidote_directive, occurrences, last_triggered, created_at, tenant_id, workspace_root
            ) VALUES (
                'test::core/dispatch.rs::', 'core/dispatch.rs', '', 0.85,
                'Multiple regressions in router dispatch',
                'Run unit test suite before modifying dispatch router',
                4, '2026-10-20T12:00:00Z', '2026-10-20T12:00:00Z', 'test_tenant', ?
            )""",
            (str(self.workspace),),
        )

        conn.execute(
            """CREATE TABLE procedural_habits (
                id TEXT PRIMARY KEY,
                trigger_file TEXT NOT NULL,
                choreography_json TEXT NOT NULL,
                confidence REAL NOT NULL,
                occurrences INTEGER NOT NULL DEFAULT 1,
                updated_at TEXT NOT NULL,
                tenant_id TEXT NOT NULL DEFAULT 'local',
                workspace_root TEXT NOT NULL DEFAULT ''
            );"""
        )
        conn.execute(
            """INSERT INTO procedural_habits (
                id, trigger_file, choreography_json, confidence, occurrences,
                updated_at, tenant_id, workspace_root
            ) VALUES (
                'test::core/dispatch.rs', 'core/dispatch.rs', '["core/schemas.rs", "core/server.rs"]',
                0.90, 5, '2026-10-20T12:00:00Z', 'test_tenant', ?
            )""",
            (str(self.workspace),),
        )
        conn.commit()
        conn.close()

        self.orchestrator = DreamOrchestrator(
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

    def test_run_dream_cycle_generates_insights_json(self):
        result = self.orchestrator.run_dream_cycle(max_commits=10)

        self.assertEqual(result["status"], "complete")
        self.assertEqual(result["cycle_type"], "offline_dreaming")
        self.assertGreater(result["duration_ms"], 0)

        insights_path = self.ozymem_dir / "dream_latest_insights.json"
        self.assertTrue(insights_path.exists(), "Must create .ozymem/dream_latest_insights.json")

        with open(insights_path, "r", encoding="utf-8") as f:
            data = json.load(f)

        self.assertEqual(data["status"], "complete")
        self.assertIn("top_insights", data)
        self.assertGreaterEqual(len(data["top_insights"]), 1)

        # Check that Amygdala high-fragility alert was generated
        has_fragility = any("HIGH_FRAGILITY" in ins for ins in data["top_insights"])
        self.assertTrue(has_fragility, "Top insights must highlight high fragility for core/dispatch.rs")

        # Check that Basal Ganglia playbook was recognized
        has_playbook = any("PLAYBOOK_READY" in ins for ins in data["top_insights"])
        self.assertTrue(has_playbook, "Top insights must highlight playbook for core/dispatch.rs")

        # Verify structured reports
        self.assertEqual(len(data["amygdala_reflexes"]["top_fragile_files"]), 1)
        self.assertEqual(data["amygdala_reflexes"]["top_fragile_files"][0]["file_path"], "core/dispatch.rs")
        self.assertEqual(len(data["basal_ganglia"]["top_playbooks"]), 1)
        self.assertEqual(data["basal_ganglia"]["top_playbooks"][0]["trigger_file"], "core/dispatch.rs")

    def test_brain_dispatch_dream_offline_cycle(self):
        payload = {
            "workspace_root": str(self.workspace),
            "db_path": str(self.db_path),
            "tenant_id": "test_tenant",
            "max_commits": 10,
        }
        res = run("dream_offline_cycle", payload)

        self.assertEqual(res["action"], "dream_offline_cycle")
        self.assertEqual(res["engine"], "ozy-brain-python")
        self.assertIn("Offline dream consolidation complete", res["summary"])
        self.assertGreaterEqual(len(res["plan"]), 1)
