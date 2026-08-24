from __future__ import annotations

import os
import sys
import unittest
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from ozy_brain.agents.memory_agent import MemoryConsolidationAgent, consolidate_and_decay_memories
from ozy_brain.agents.risk_critic import RiskCriticAgent, audit_risk_with_critic
from ozy_brain.agents.supervisor import SupervisorAgent
from ozy_brain.brain import run
from ozy_brain.config import BrainConfig, LLMProvider
from ozy_brain.data_engine import DataEngine


class TestMultiAgent(unittest.TestCase):
    def test_brain_config_openrouter_detection(self):
        with patch.dict(
            os.environ,
            {"OPENROUTER_API_KEY": "sk-or-v1-test-key"},
            clear=True,
        ):
            cfg = BrainConfig.load()
            self.assertEqual(cfg.provider, LLMProvider.OPENROUTER)
            self.assertEqual(cfg.api_key, "sk-or-v1-test-key")
            self.assertIn("openrouter", cfg.model)

    def test_brain_config_offline_fallback(self):
        with patch("ozy_brain.config._get_env_var", return_value=None), \
             patch("ozy_brain.agents.risk_critic.call_llm", return_value=None):
            cfg = BrainConfig.load()
            self.assertEqual(cfg.provider, LLMProvider.OFFLINE_FALLBACK)
            self.assertIsNone(cfg.api_key)

    def test_data_engine_in_memory(self):
        engine = DataEngine(db_path=":memory:")
        hotspots = engine.get_top_hotspots(limit=5)
        self.assertIsInstance(hotspots, list)

    def test_risk_critic_offline_audit(self):
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
        self.assertTrue(result.blocked)
        self.assertEqual(result.risk_level, "CRITICAL")
        self.assertTrue(any("Destructive DDL/DML" in r for r in result.reasons))

    def test_memory_consolidation_and_decay(self):
        agent = MemoryConsolidationAgent(half_life_days=30.0)
        payload = {
            "memories": [
                {"summary": "Avoid raw SQL queries in auth module", "topic": "auth", "timestamp": "2026-01-01T00:00:00Z"},
                {"summary": "Use parameterized queries in auth", "topic": "auth", "timestamp": "2026-08-20T00:00:00Z"},
                {"summary": "Cache user sessions with Redis", "topic": "caching", "timestamp": "2026-08-23T00:00:00Z"},
            ]
        }
        res = agent.consolidate(payload)
        self.assertEqual(res["status"], "success")
        self.assertGreaterEqual(res["clusters_count"], 2)
        self.assertGreaterEqual(len(res["consolidated"]), 2)

    def test_supervisor_dispatcher(self):
        with patch("ozy_brain.agents.risk_critic.call_llm", return_value=None):
            res_critic = run("audit_changes_with_critic", {"files": ["src/main.py"], "diff": "+ print('hello')"})
            self.assertEqual(res_critic["action"], "audit_changes_with_critic")
            self.assertIn("summary", res_critic)

            res_hotspots = run("get_repository_hotspots", {"limit": 5})
            self.assertEqual(res_hotspots["action"], "get_repository_hotspots")
            self.assertIn("hotspots", res_hotspots.get("structured_plan", {}))

            res_mem = run("consolidate_memory", {"lessons": [{"summary": "rule 1", "topic": "db"}]})
            self.assertEqual(res_mem["action"], "consolidate_memory")


if __name__ == "__main__":
    unittest.main()
