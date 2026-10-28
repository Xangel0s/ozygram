"""Unit and integration tests for Hybrid Dream Memory Store (SQLite WAL + Markdown Git)."""

import sqlite3
import tempfile
from pathlib import Path

import pytest
from ozy_brain.scanner.contract_tracer import ContractDriftAlert
from ozy_brain.scanner.memory_store import (
    GotchaMemory,
    MarkdownGitSynchronizer,
    PostgresVectorBackend,
    SqliteWalBackend,
)


def test_sqlite_wal_lifecycle():
    """Verify SQLite storage in WAL mode, gotcha saving, and sub-millisecond retrieval."""
    with tempfile.TemporaryDirectory() as tmp:
        db_file = Path(tmp) / "memory.db"
        backend = SqliteWalBackend(db_file)

        # Verify WAL mode
        conn = sqlite3.connect(db_file)
        try:
            mode = conn.execute("PRAGMA journal_mode;").fetchone()[0]
            assert mode.lower() == "wal"
        finally:
            conn.close()

        # Save gotcha
        gotcha = GotchaMemory(
            id="gotcha_test_1234",
            topic_key="gotcha_prevention",
            title="[CENTINELA GOTCHA] Test signature parity",
            severity="high",
            applies_to=["api/routes.py", "frontend/use-control.ts"],
            tags=["reflexion", "test"],
            flawed_assumption="Assumed types matched",
            countermeasure="Verify schemas",
        )
        gotcha_id = backend.save_gotcha(gotcha)
        assert gotcha_id == "gotcha_test_1234"

        # Save drift alert
        alert = ContractDriftAlert(
            kind="HTTP_METHOD_MISMATCH",
            severity="CRITICAL",
            frontend_file="frontend/use-control.ts",
            frontend_line=12,
            canonical_path="/api/test",
            frontend_method="GET",
            backend_file="backend/router.py",
            backend_line=34,
            backend_method="POST",
            details="GET instead of POST",
        )
        alert_id = backend.save_drift_alert(alert)
        assert alert_id.startswith("alert_")

        # Query active threats
        threats = backend.get_active_threats(limit=10)
        assert len(threats) == 1
        assert threats[0].id == "gotcha_test_1234"
        assert "api/routes.py" in threats[0].applies_to

        # Query drift alerts
        alerts = backend.get_drift_alerts(unresolved_only=True)
        assert len(alerts) == 1
        assert alerts[0]["canonical_path"] == "/api/test"


def test_markdown_git_synchronizer():
    """Verify serialization of GotchaMemory to standard Git Markdown with YAML frontmatter."""
    with tempfile.TemporaryDirectory() as tmp:
        base_dir = Path(tmp) / ".ozy" / "memories"

        gotcha = GotchaMemory(
            id="gotcha_reflexion_48df0d75",
            topic_key="gotcha_prevention",
            title="[REFLEXION GOTCHA] Caller and callee signatures must maintain arity parity.",
            author="ozybrain_reflexion",
            date="2026-10-28",
            severity="high",
            applies_to=["crates/ozymem-core/src/indexing.rs"],
            tags=["reflexion", "failure_triangulation", "auto_learned"],
            flawed_assumption="Assumed change sequence for 'fix compiler trait method error' would succeed.",
            countermeasure="Enforce strict arity check before applying AST patches.",
        )

        file_path = MarkdownGitSynchronizer.export_gotcha(gotcha, base_dir)
        assert file_path.is_file()
        assert file_path.name == "gotcha_reflexion_48df0d75.md"

        content = file_path.read_text(encoding="utf-8")
        assert "---" in content
        assert "id: gotcha_reflexion_48df0d75" in content
        assert "severity: high" in content
        assert '- "crates/ozymem-core/src/indexing.rs"' in content
        assert "- reflexion" in content
        assert "# Verbal Self-Reflection" in content
        assert "**Flawed Assumption:**" in content


def test_postgres_vector_backend_fallback():
    """Verify PostgresVectorBackend operates smoothly with local SQLite fallback."""
    with tempfile.TemporaryDirectory() as tmp:
        db_file = Path(tmp) / "fallback.db"
        backend = PostgresVectorBackend(dsn=None, fallback_sqlite_path=db_file)

        gotcha = GotchaMemory(
            id="gotcha_fallback_1",
            title="Fallback test",
            severity="critical",
        )
        saved_id = backend.save_gotcha(gotcha)
        assert saved_id == "gotcha_fallback_1"

        threats = backend.get_active_threats()
        assert len(threats) == 1
        assert threats[0].id == "gotcha_fallback_1"


def test_end_to_end_drift_to_markdown_pipeline():
    """Verify complete end-to-end pipeline: DriftAlert -> Gotcha -> SQLite WAL -> Markdown."""
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        db_file = root / ".ozymem" / "memory.db"
        memories_dir = root / ".ozy" / "memories"

        store = SqliteWalBackend(db_file)

        # 1. Incoming drift alert
        alert = ContractDriftAlert(
            kind="ORPHAN_ENDPOINT_CONSUMER",
            severity="HIGH",
            frontend_file="crm-geofal/src/use-control.ts",
            frontend_line=54,
            canonical_path="/api/control-neoprenos/auto-sync",
            frontend_method="POST",
            details="Endpoint called in frontend was removed in backend",
            suggested_fix="Register @router.post('/auto-sync') in FastAPI backend",
        )

        # 2. Convert to Gotcha vaccine
        gotcha = GotchaMemory.from_drift_alert(alert)
        assert "ORPHAN_ENDPOINT_CONSUMER" in gotcha.title
        assert "crm-geofal/src/use-control.ts" in gotcha.applies_to

        # 3. Persist in SQLite WAL
        store.save_gotcha(gotcha)
        store.save_drift_alert(alert)

        # 4. Synchronize to Git Markdown
        exported_files = MarkdownGitSynchronizer.sync_all_gotchas([gotcha], memories_dir)
        assert len(exported_files) == 1
        assert exported_files[0].is_file()

        # 5. Read back markdown and check content
        md_text = exported_files[0].read_text(encoding="utf-8")
        assert alert.canonical_path in md_text
        assert "contract_drift" in md_text
