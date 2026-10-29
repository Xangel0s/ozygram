"""
Unit and integration tests for Hippocampal STDP Sleep Replay and offline vaccine consolidation.
Standard: Zero-Emoji, SOLID, High Concurrency.
"""

import json
import sqlite3
import tempfile
import time
from pathlib import Path
import pytest

from ozy_brain.dream.orchestrator import DreamOrchestrator


def test_sleep_replay_skipped_when_no_db(tmp_path):
    """Verify replay returns skipped status gracefully if db does not exist."""
    orch = DreamOrchestrator(workspace_root=tmp_path, db_path=tmp_path / "nonexistent.db")
    res = orch.replay_synaptic_trajectories()
    assert res["status"] == "skipped"
    assert res["replayed_count"] == 0


def test_sleep_replay_synaptic_consolidation_and_gotcha_synthesis(tmp_path):
    """Verify sleep replay runs MCTS rollouts over fragile nodes and writes durable gotcha."""
    ozymem_dir = tmp_path / ".ozymem"
    ozymem_dir.mkdir(parents=True, exist_ok=True)
    db_path = ozymem_dir / "memory.db"

    conn = sqlite3.connect(str(db_path))
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
            'test_r1', 'app/services/payment.py', 'charge_card', 0.85,
            'Payment Gateway Timeout', 'Apply exponential retry with idempotency key',
            3, '2026-10-29T12:00:00Z', '2026-10-29T12:00:00Z', 'local', ?
        )""",
        (str(tmp_path),),
    )
    conn.commit()
    conn.close()

    orch = DreamOrchestrator(
        workspace_root=tmp_path,
        db_path=db_path,
        tenant_id="local",
    )

    res = orch.replay_synaptic_trajectories(max_trajectories=2)
    assert res["status"] == "complete"
    assert res["replayed_count"] >= 1
    assert res["vaccines_synthesized"] >= 1
    assert len(res["strengthened_synapses"]) >= 1

    # Verify atomic Gotcha Markdown file was generated in .ozy/memories/gotchas/
    gotchas_dir = tmp_path / ".ozy" / "memories" / "gotchas"
    assert gotchas_dir.exists()
    md_files = list(gotchas_dir.glob("gotcha_sleep_replay_*.md"))
    assert len(md_files) >= 1
    content = md_files[0].read_text(encoding="utf-8")
    assert "kind: gotcha" in content
    assert "app/services/payment.py" in content
    assert "sleep_replay_consolidation" in content
    assert "[SLEEP_REPLAY_VACCINE]" in content


def test_run_dream_cycle_integrates_sleep_replay(tmp_path):
    """Verify run_dream_cycle executes sleep replay and includes results in insights."""
    ozymem_dir = tmp_path / ".ozymem"
    ozymem_dir.mkdir(parents=True, exist_ok=True)
    db_path = ozymem_dir / "memory.db"

    conn = sqlite3.connect(str(db_path))
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
    conn.commit()
    conn.close()

    orch = DreamOrchestrator(
        workspace_root=tmp_path,
        db_path=db_path,
        tenant_id="local",
    )

    report = orch.run_dream_cycle(max_commits=5)
    assert "sleep_replay" in report
    assert report["sleep_replay"]["status"] in ("complete", "skipped")

    insights_file = ozymem_dir / "dream_latest_insights.json"
    assert insights_file.exists()
    insights_data = json.loads(insights_file.read_text(encoding="utf-8"))
    assert "sleep_replay" in insights_data
