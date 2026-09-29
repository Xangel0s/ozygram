"""Unit tests for Cognitive Auto-Wiring and Contradiction Arbitration in Dream-RSI."""

from __future__ import annotations

import unittest

from ozy_brain.dream.auto_wire import RiskCriticArbitrator, auto_wire_memory_embeddings


class TestDreamAutoWire(unittest.TestCase):
    def test_auto_wire_reinforces_clustering(self):
        memories = [
            {
                "id": "mem_1",
                "title": "FastAPI router dependency injection",
                "content": "Use Depends(get_db) for database sessions",
                "solution": "Pass db session via Depends",
                "embedding": [0.9, 0.8, 0.1, 0.2],
                "created_at": "2026-09-29T10:00:00Z",
            },
            {
                "id": "mem_2",
                "title": "FastAPI session dependency management",
                "content": "Inject database session with Depends(get_db)",
                "solution": "Ensure db session injection using Depends",
                "embedding": [0.89, 0.82, 0.09, 0.21],
                "created_at": "2026-09-29T11:00:00Z",
            },
        ]

        result = auto_wire_memory_embeddings(memories, similarity_threshold=0.85)
        self.assertEqual(result["edges_created"], 1)
        self.assertEqual(result["reinforces_count"], 1)
        self.assertEqual(result["supersedes_count"], 0)
        self.assertEqual(result["edges"][0]["edge_type"], "reinforces")
        self.assertGreater(result["edges"][0]["weight"], 0.95)

    def test_auto_wire_supersedes_arbitration(self):
        memories = [
            {
                "id": "mem_active",
                "title": "JWT cookie security settings",
                "content": "Set Secure, HttpOnly, and SameSite=Lax on auth cookies",
                "solution": "Configure cookies with Secure flag",
                "embedding": [0.9, 0.1, 0.0, 0.5],
                "created_at": "2026-09-29T12:00:00Z",
            },
            {
                "id": "mem_legacy",
                "title": "Old auth cookie config",
                "content": "Deprecated legacy auth cookie config no usar",
                "solution": "Deprecated setting do not use",
                "embedding": [0.88, 0.12, 0.01, 0.49],
                "created_at": "2026-09-28T10:00:00Z",
            },
        ]

        result = auto_wire_memory_embeddings(memories, similarity_threshold=0.85)
        self.assertEqual(result["edges_created"], 1)
        self.assertEqual(result["supersedes_count"], 1)
        self.assertEqual(result["edges"][0]["edge_type"], "supersedes")
        self.assertEqual(result["edges"][0]["source_id"], "mem_active")
        self.assertEqual(result["edges"][0]["target_id"], "mem_legacy")

    def test_temporal_conflict_arbitration(self):
        arbitrator = RiskCriticArbitrator()
        mem_old = {
            "id": "mem_old",
            "symbol_name": "build_cache_key",
            "solution": "Return md5 hex digest of parameters",
            "created_at": "2026-09-20T10:00:00Z",
        }
        mem_new = {
            "id": "mem_new",
            "symbol_name": "build_cache_key",
            "solution": "Return sha256 hex digest for security compliance",
            "created_at": "2026-09-29T10:00:00Z",
        }

        is_conflict, resolution = arbitrator.detect_conflict(mem_old, mem_new)
        self.assertTrue(is_conflict)
        self.assertEqual(resolution, "mem_new SUPERSEDES mem_old")

    def test_zero_emojis(self):
        memories = [
            {"id": "m1", "title": "test", "content": "test", "solution": "test"},
            {"id": "m2", "title": "test", "content": "test", "solution": "test"},
        ]
        result = auto_wire_memory_embeddings(memories, similarity_threshold=0.5)
        full_text = " ".join(result["details"])
        for emoji in ["\u274C", "\u1F3C6", "\u1F535", "\u26A0", "\u1F6A8", "\u2705", "\u1F525", "\u1F3AF"]:
            self.assertNotIn(emoji, full_text)


if __name__ == "__main__":
    unittest.main()
