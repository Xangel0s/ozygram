from __future__ import annotations

import os
from pathlib import Path
import pytest

from ozy_brain.agents.memory_agent import MemoryConsolidationAgent, consolidate_and_decay_memories
from ozy_brain.agents.risk_critic import RiskCriticAgent, audit_risk_with_critic
from ozy_brain.agents.supervisor import SupervisorAgent
from ozy_brain.brain import run
from ozy_brain.config import BrainConfig, LLMProvider
from ozy_brain.data_engine import DataEngine


def test_brain_config_openrouter_detection(monkeypatch):
    monkeypatch.setenv("OPENROUTER_API_KEY", "sk-or-v1-test-key")
    monkeypatch.delenv("OLLAMA_HOST", raising=False)
    monkeypatch.delenv("GEMINI_API_KEY", raising=False)
    
    cfg = BrainConfig.load()
    assert cfg.provider == LLMProvider.OPENROUTER
    assert cfg.api_key == "sk-or-v1-test-key"
    assert "openrouter" in cfg.model


def test_brain_config_offline_fallback(monkeypatch):
    monkeypatch.setattr("ozy_brain.config._get_env_var", lambda k: None)
    monkeypatch.setattr("ozy_brain.agents.risk_critic.call_llm", lambda *a, **kw: None)

    cfg = BrainConfig.load()
    assert cfg.provider == LLMProvider.OFFLINE_FALLBACK
    assert cfg.api_key is None


def test_data_engine_in_memory():
    engine = DataEngine(db_path=":memory:")
    hotspots = engine.get_top_hotspots(limit=5)
    assert isinstance(hotspots, list)


def test_risk_critic_offline_audit():
    offline_cfg = BrainConfig(
        provider=LLMProvider.OFFLINE_FALLBACK,
        model="offline",
        api_key=None,
        api_base=None,
    )
    agent = RiskCriticAgent(config=offline_cfg)
    payload = {
        "files": ["src/payment_gateway.py", "src/auth.py"],
        "diff": "ALTER TABLE users DROP COLUMN password_hash;",
        "plan": ["Drop legacy column"],
    }
    result = agent.audit(payload)
    assert result.blocked is True
    assert result.risk_level == "CRITICAL"
    assert any("Destructive DDL/DML" in r for r in result.reasons)


def test_memory_consolidation_and_decay():
    agent = MemoryConsolidationAgent(half_life_days=30.0)
    payload = {
        "memories": [
            {"summary": "Avoid raw SQL queries in auth module", "topic": "auth", "timestamp": "2026-01-01T00:00:00Z"},
            {"summary": "Use parameterized queries in auth", "topic": "auth", "timestamp": "2026-08-20T00:00:00Z"},
            {"summary": "Cache user sessions with Redis", "topic": "caching", "timestamp": "2026-08-23T00:00:00Z"},
        ]
    }
    res = agent.consolidate(payload)
    assert res["status"] == "success"
    assert res["clusters_count"] >= 2
    assert len(res["consolidated"]) >= 2


def test_supervisor_dispatcher():
    # Test supervisor via brain.run
    res_critic = run("audit_changes_with_critic", {"files": ["src/main.py"], "diff": "+ print('hello')"})
    assert res_critic["action"] == "audit_changes_with_critic"
    assert "summary" in res_critic

    res_hotspots = run("get_repository_hotspots", {"limit": 5})
    assert res_hotspots["action"] == "get_repository_hotspots"
    assert "hotspots" in res_hotspots.get("structured_plan", {})

    res_mem = run("consolidate_memory", {"lessons": [{"summary": "rule 1", "topic": "db"}]})
    assert res_mem["action"] == "consolidate_memory"
