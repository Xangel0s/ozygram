from __future__ import annotations

import gc
import tempfile
import unittest

from ozy_brain import brain
from ozy_brain.vector_store import VectorMemoryStore


class TestVectorStore(unittest.TestCase):
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

    def test_chroma_store_upsert_and_search(self):
        if not self.store.is_available():
            self.skipTest("ChromaDB is not available in environment")

        ok = self.store.upsert_lesson(
            lesson_id="mem_test_1",
            content="Normalizar siempre fechas de laboratorio a YYYY/MM/DD en api-geofal-crm",
            metadata={"kind": "architecture", "module": "recepcion"},
        )
        self.assertTrue(ok)

        hits = self.store.search_similar("fechas de laboratorio YYYY/MM/DD", limit=3)
        self.assertGreaterEqual(len(hits), 1)
        self.assertIn("YYYY/MM/DD", hits[0]["content"])
        self.assertGreater(hits[0]["similarity_score"], 0.4)

    def test_reranker_filters_noise_and_boosts_relevant_rule(self):
        candidates = [
            {
                "id": "noise_1",
                "content": "Traceback (most recent call last): File main.py line 10 at Object.<anonymous>",
                "similarity_score": 0.70,
                "metadata": {"kind": "log"},
            },
            {
                "id": "table_1",
                "content": "| col1 | col2 | col3 | col4 | col5 | 123 |",
                "similarity_score": 0.65,
                "metadata": {"kind": "dump"},
            },
            {
                "id": "rule_1",
                "content": "Normalizar siempre fechas a formato YYYY/MM/DD en backend y recepcion",
                "similarity_score": 0.75,
                "metadata": {"kind": "architecture"},
            },
        ]

        reranked = self.store.rerank(
            query="formato de fechas YYYY/MM/DD en backend",
            candidates=candidates,
            top_k=2,
            min_score=0.30,
        )

        self.assertGreaterEqual(len(reranked), 1)
        # The clean architectural rule must rank #1
        self.assertEqual(reranked[0]["id"], "rule_1")
        self.assertGreater(reranked[0]["rerank_score"], 0.6)

    def test_brain_deep_semantic_search_action(self):
        payload = {
            "query": "autocompletado en tablas dinamicas",
            "chroma_dir": self.temp_dir.name,
            "candidates": [
                {
                    "content": "Usar onMouseDown preventDefault para evitar blur en autocompletado de tablas",
                    "similarity_score": 0.80,
                    "metadata": {"kind": "convention"},
                },
                {
                    "content": "Unrelated payment gateway notes",
                    "similarity_score": 0.20,
                    "metadata": {},
                },
            ],
            "limit": 3,
        }

        res = brain.run("deep_semantic_search", payload)
        self.assertEqual(res["action"], "deep_semantic_search")
        self.assertEqual(res["engine"], "ozy-brain-python")
        self.assertIn("autocompletado", res["summary"])
        results = res.get("structured_plan", {}).get("results", [])
        self.assertGreaterEqual(len(results), 1)
        self.assertIn("onMouseDown", results[0]["content"])


if __name__ == "__main__":
    unittest.main()
