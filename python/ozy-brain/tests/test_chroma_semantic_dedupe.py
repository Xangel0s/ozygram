from __future__ import annotations

import tempfile
from pathlib import Path

from ozy_brain.dream.orchestrator import DreamOrchestrator
from ozy_brain.scanner.chroma_bridge import ChromaVectorBridge


def test_chroma_semantic_deduplication_lifecycle():
    with tempfile.TemporaryDirectory(ignore_cleanup_errors=True) as tmp_dir:
        bridge = ChromaVectorBridge(persist_dir=tmp_dir)
        if not bridge.is_available:
            return  # Skip gracefully if ChromaDB not installed

        coll_name = ChromaVectorBridge.COLLECTION_HEURISTICS

        # Insert 3 documents: doc1 and doc2 are near duplicates (high similarity), doc3 is distinct
        doc1 = "Use DualActiveContract when migrating endpoints to avoid breaking frontend clients."
        doc2 = "When migrating endpoints, use DualActiveContract to prevent breaking frontend clients."
        doc3 = "PostgreSQL migration idempotency requires IF NOT EXISTS guards on all tables."

        bridge.upsert_document(coll_name, "heur_1", doc1, metadata={"kind": "convention"})
        bridge.upsert_document(coll_name, "heur_2", doc2, metadata={"kind": "convention"})
        bridge.upsert_document(coll_name, "heur_3", doc3, metadata={"kind": "convention"})

        # Deduplicate with 0.85 threshold
        res = bridge.deduplicate_collection(coll_name, similarity_threshold=0.85)

        assert res["status"] == "ok"
        assert res["scanned_count"] == 3
        assert res["pruned_count"] >= 1

        # Verify only unique documents remain
        coll = bridge.get_collection(coll_name)
        data = coll.get()
        assert len(data["ids"]) < 3
        assert "heur_1" in data["ids"]

        bridge.close()


def test_dream_orchestrator_includes_chroma_dedupe():
    with tempfile.TemporaryDirectory(ignore_cleanup_errors=True) as tmp_dir:
        root = Path(tmp_dir)
        (root / ".git").mkdir()

        orchestrator = DreamOrchestrator(
            workspace_root=root,
            db_path=root / "memory.db",
            chroma_dir=root / "chroma",
        )

        report = orchestrator.run_dream_cycle(max_commits=5)
        assert "chroma_dedupe" in report
        assert "pruned_count" in report["chroma_dedupe"]
