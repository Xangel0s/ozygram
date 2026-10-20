from __future__ import annotations

import gc
import os
import sqlite3
import tempfile
import unittest
from pathlib import Path

from ozy_brain.amygdala import AmygdalaReflexEngine, normalize_rel_path
from ozy_brain.glial import GlialHomeostasisEngine
from ozy_brain.vector_store import VectorMemoryStore


class TestAmygdalaReflexEngine(unittest.TestCase):
    def setUp(self):
        self.temp_dir = tempfile.TemporaryDirectory(ignore_cleanup_errors=True)
        self.db_path = Path(self.temp_dir.name) / "test_memory.db"
        self.engine = AmygdalaReflexEngine(
            db_path=self.db_path,
            tenant_id="test_tenant",
            workspace_root=self.temp_dir.name,
        )

        # Initialize SQLite table with Rust schema
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
        conn.execute("CREATE INDEX idx_amygdala_file ON amygdala_reflexes(file_path, tenant_id);")
        conn.commit()
        conn.close()

    def tearDown(self):
        gc.collect()
        try:
            self.temp_dir.cleanup()
        except Exception:
            pass

    def test_path_normalization(self):
        self.assertEqual(normalize_rel_path("src\\api\\routes.rs"), "src/api/routes.rs")
        self.assertEqual(normalize_rel_path("./crates\\core/lib.rs"), "crates/core/lib.rs")
        self.assertEqual(normalize_rel_path("  models/user.py  "), "models/user.py")

    def test_calculate_threat_scars_and_sqlite_upsert(self):
        signals = [
            {
                "file_path": "src\\services\\order.rs",
                "symbol_name": "checkout",
                "commit_hash": "abc1234",
                "commit_msg": "fix: resolve deadlock in checkout transaction",
            },
            {
                "file_path": "src/services/order.rs",
                "symbol_name": "checkout",
                "commit_hash": "def5678",
                "commit_msg": "fix: unreleased mutex on checkout failure",
            },
        ]

        scars = self.engine.calculate_threat_scars(signals)
        # Expect 1 file-level scar and 1 symbol-level scar
        self.assertEqual(len(scars), 2)
        sym_scar = next(s for s in scars if s["symbol_name"] == "checkout")
        self.assertEqual(sym_scar["file_path"], "src/services/order.rs")
        self.assertEqual(sym_scar["occurrences"], 2)
        self.assertGreaterEqual(sym_scar["threat_score"], 0.70)

        # Write to SQLite
        written = self.engine.write_reflexes_to_sqlite(scars)
        self.assertEqual(written, 2)

        # Verify in SQLite directly
        conn = sqlite3.connect(str(self.db_path))
        cursor = conn.cursor()
        cursor.execute("SELECT file_path, symbol_name, threat_score, occurrences FROM amygdala_reflexes ORDER BY threat_score DESC")
        rows = cursor.fetchall()
        conn.close()

        self.assertEqual(len(rows), 2)
        self.assertEqual(rows[0][0], "src/services/order.rs")
        self.assertEqual(rows[0][3], 2)

        # Test Upsert: bumping threat score on next failure
        additional_error = [{
            "file_path": "src/services/order.rs",
            "symbol_name": "checkout",
            "error_message": "revert: rollback checkout timeout",
        }]
        updated_scars = self.engine.calculate_threat_scars([], direct_errors=additional_error)
        self.engine.write_reflexes_to_sqlite(updated_scars)

        conn = sqlite3.connect(str(self.db_path))
        cursor = conn.cursor()
        cursor.execute("SELECT occurrences, threat_score FROM amygdala_reflexes WHERE symbol_name = 'checkout'")
        row = cursor.fetchone()
        conn.close()

        # Occurrences must have incremented and threat score bumped
        self.assertEqual(row[0], 3)
        self.assertGreaterEqual(row[1], 0.75)


class TestGlialHomeostasisEngine(unittest.TestCase):
    def setUp(self):
        self.temp_dir = tempfile.TemporaryDirectory(ignore_cleanup_errors=True)
        self.workspace = Path(self.temp_dir.name)
        self.db_path = self.workspace / ".ozymem" / "db.sqlite"
        self.db_path.parent.mkdir(parents=True, exist_ok=True)

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
            """CREATE TABLE lessons (
                id INTEGER PRIMARY KEY,
                file_path TEXT NOT NULL,
                error_context TEXT,
                solution TEXT,
                stale INTEGER DEFAULT 0,
                stale_reason TEXT
            );"""
        )
        conn.execute(
            """CREATE TABLE files (
                path TEXT PRIMARY KEY
            );"""
        )

        # Insert a real file and an orphaned deleted file
        real_file = self.workspace / "src" / "valid.rs"
        real_file.parent.mkdir(parents=True, exist_ok=True)
        real_file.write_text("fn main() {}\n")

        conn.execute("INSERT INTO files (path) VALUES ('src/valid.rs'), ('src/deleted_legacy.rs');")
        conn.execute("INSERT INTO lessons (file_path, error_context, solution) VALUES ('src/valid.rs', 'err', 'sol'), ('src/deleted_legacy.rs', 'err_old', 'sol_old');")
        conn.execute("INSERT INTO amygdala_reflexes (id, file_path, threat_score, pain_trigger, antidote_directive, last_triggered, created_at) VALUES ('1', 'src/deleted_legacy.rs', 0.8, 'deadlock', 'directive', 'now', 'now');")
        conn.commit()
        conn.close()

        self.glial = GlialHomeostasisEngine(
            workspace_root=self.workspace,
            db_path=self.db_path,
            chroma_dir=self.workspace / ".ozymem" / "chroma",
        )

    def tearDown(self):
        self.glial.vector_store.close()
        gc.collect()
        try:
            self.temp_dir.cleanup()
        except Exception:
            pass

    def test_detect_and_prune_orphaned_files(self):
        orphans = self.glial.detect_orphaned_files()
        self.assertIn("src/deleted_legacy.rs", orphans)
        self.assertNotIn("src/valid.rs", orphans)

        stats = self.glial.prune_sqlite_orphans(orphans)
        self.assertEqual(stats["reflexes_pruned"], 1)
        self.assertEqual(stats["lessons_marked_stale"], 1)
        self.assertEqual(stats["files_pruned"], 1)

        # Verify lessons was marked stale, not deleted
        conn = sqlite3.connect(str(self.db_path))
        cursor = conn.cursor()
        cursor.execute("SELECT stale, stale_reason FROM lessons WHERE file_path = 'src/deleted_legacy.rs'")
        row = cursor.fetchone()
        conn.close()
        self.assertEqual(row[0], 1)
        self.assertEqual(row[1], "file_deleted_in_git")


class TestVectorStoreCollections(unittest.TestCase):
    def setUp(self):
        self.temp_dir = tempfile.TemporaryDirectory(ignore_cleanup_errors=True)
        self.store = VectorMemoryStore(persist_dir=self.temp_dir.name)

    def tearDown(self):
        self.store.close()
        gc.collect()
        try:
            self.temp_dir.cleanup()
        except Exception:
            pass

    def test_four_specialized_collections(self):
        if not self.store.is_available():
            self.skipTest("ChromaDB is not available in environment")

        # 1. episodic_raw
        ok1 = self.store.ingest_episodic_event(
            event_id="ep_001",
            content="Error: connection timeout after 3000ms in dispatch.rs",
            metadata={"source": "mcp_call", "latency_ms": 3000},
        )
        self.assertTrue(ok1)

        # 2. code_graph_ast
        ok2 = self.store.upsert_code_ast(
            symbol_id="ast_001",
            code_content="pub fn acquire_connection() -> Result<Conn>",
            metadata={"pagerank": 0.12, "blast_radius": 5, "file_path": "src/pool.rs"},
        )
        self.assertTrue(ok2)

        # 3. semantic_heuristics
        ok3 = self.store.upsert_heuristic(
            rule_id="rule_001",
            content="Always configure connection pool timeout to 500ms under high concurrency",
            metadata={"kind": "convention", "confidence": 0.95},
        )
        self.assertTrue(ok3)

        # 4. dream_insights
        ok4 = self.store.upsert_insight(
            insight_id="ins_001",
            content="Identified 3 cyclic calls between dispatch.rs and unified.rs during dream simulation",
            metadata={"severity": "warning", "phase": "dream_rsi"},
        )
        self.assertTrue(ok4)

        # Search each collection independently
        hits_ep = self.store.search_similar("connection timeout", collection_name="episodic_raw", limit=1)
        self.assertEqual(len(hits_ep), 1)
        self.assertIn("dispatch.rs", hits_ep[0]["content"])

        hits_heur = self.store.search_similar("connection pool timeout", collection_name="semantic_heuristics", limit=1)
        self.assertEqual(len(hits_heur), 1)
        self.assertIn("500ms", hits_heur[0]["content"])

        hits_insight = self.store.search_similar("cyclic calls", collection_name="dream_insights", limit=1)
        self.assertEqual(len(hits_insight), 1)
        self.assertIn("unified.rs", hits_insight[0]["content"])


if __name__ == "__main__":
    unittest.main()
