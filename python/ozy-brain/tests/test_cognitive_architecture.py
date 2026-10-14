from __future__ import annotations

import time
import unittest
from typing import Any

from ozy_brain.cognitive_router import CognitiveSystemTier, route_cognitive_system
from ozy_brain.agents.supervisor import SupervisorAgent
from ozy_brain.brain import run


class TestCognitiveRouter(unittest.TestCase):
    """Tests for Task 8.1: System 1 (Fast reflex) vs System 2 (Deliberative) router."""

    def test_system_1_low_blast_radius_and_mechanical(self):
        start = time.perf_counter()
        decision = route_cognitive_system({
            "file_path": "app/utils/formatter.py",
            "blast_radius": 1,
            "goal": "Add missing type annotations to helper",
        })
        elapsed_ms = (time.perf_counter() - start) * 1000

        self.assertEqual(decision.tier, CognitiveSystemTier.SYSTEM_1_REFLEX)
        self.assertTrue(decision.zero_token_guarantee)
        self.assertLessEqual(decision.latency_budget_ms, 150)
        self.assertFalse(decision.requires_committee)
        self.assertLess(elapsed_ms, 10, "Router classification must complete in < 10ms")

    def test_system_2_high_blast_radius(self):
        decision = route_cognitive_system({
            "file_path": "app/services/billing.py",
            "blast_radius": 5,
            "affected_files": ["f1.py", "f2.py", "f3.py", "f4.py", "f5.py"],
        })
        self.assertEqual(decision.tier, CognitiveSystemTier.SYSTEM_2_DELIBERATIVE)
        self.assertFalse(decision.zero_token_guarantee)
        self.assertTrue(decision.requires_committee)
        self.assertIn("High blast radius", decision.reason)

    def test_system_2_critical_schema_and_migration(self):
        decision = route_cognitive_system({
            "file_path": "migrations/versions/001_initial_schema.sql",
            "blast_radius": 1,
        })
        self.assertEqual(decision.tier, CognitiveSystemTier.SYSTEM_2_DELIBERATIVE)
        self.assertTrue(decision.requires_committee)
        self.assertIn("schema", decision.reason.lower())

    def test_system_2_destructive_sql(self):
        decision = route_cognitive_system({
            "file_path": "app/cleanup.py",
            "diff": "TRUNCATE TABLE session_logs;",
            "blast_radius": 1,
        })
        self.assertEqual(decision.tier, CognitiveSystemTier.SYSTEM_2_DELIBERATIVE)
        self.assertIn("destructive sql", decision.reason.lower())

    def test_system_2_ghost_patch_contract_breakage(self):
        decision = route_cognitive_system({
            "file_path": "app/calc.py",
            "blast_radius": 1,
            "simulation_result": {
                "target_symbol": "calculate_tax",
                "breaks_dependents": True,
                "breakages": [{"caller_file": "app/order.py", "line_number": 20}],
            },
        })
        self.assertEqual(decision.tier, CognitiveSystemTier.SYSTEM_2_DELIBERATIVE)
        self.assertIn("ghost patch", decision.reason.lower())


class TestDialecticCommittee(unittest.TestCase):
    """Tests for Task 8.2: 3-agent dialectic committee in ozy-brain."""

    def test_committee_approves_safe_proposal(self):
        supervisor = SupervisorAgent()
        payload = {
            "file_path": "app/helpers/string_utils.py",
            "goal": "Refactor slugify helper",
            "affected_files": ["app/helpers/string_utils.py"],
            "diff": "def slugify(text: str) -> str:\n    return text.lower().strip()",
        }
        resp = supervisor.deliberate_committee(payload)

        self.assertEqual(resp.action, "dialectic_committee")
        self.assertIn("[DIALECTIC VERDICT: APPROVED]", resp.summary)
        self.assertTrue(resp.structured_plan["approved"])
        self.assertGreaterEqual(resp.confidence, 0.90)
        # Verify both Implementer and Critic are represented
        self.assertIn("implementer_plan", resp.structured_plan)
        self.assertIn("critic_audit", resp.structured_plan)

    def test_committee_vetoes_breaking_proposal(self):
        supervisor = SupervisorAgent()
        payload = {
            "file_path": "app/core/calculations.py",
            "symbol": "compute_rate",
            "goal": "Change compute_rate signature requiring new argument",
            "simulation_result": {
                "target_symbol": "compute_rate",
                "breaks_dependents": True,
                "breakages": [
                    {
                        "caller_file": "app/services/invoice.py",
                        "caller_symbol": "generate_invoice",
                        "line_number": 88,
                        "reason": "Caller passes 1 argument(s), but updated signature requires at least 2",
                    }
                ],
            },
        }
        resp = supervisor.deliberate_committee(payload)

        self.assertIn("[DIALECTIC VERDICT: REJECTED: CRITICAL_VETO]", resp.summary)
        self.assertFalse(resp.structured_plan["approved"])
        self.assertTrue(any("[CRITIC VETO]" in p for p in resp.plan))
        self.assertTrue(any("Execution halted" in p for p in resp.plan))

    def test_end_to_end_via_brain_run(self):
        res_route = run("cognitive_route", {
            "file_path": "app/test.py",
            "blast_radius": 1,
        })
        self.assertEqual(res_route["action"], "cognitive_route")
        self.assertEqual(res_route["engine"], "ozy-brain-python")
        self.assertIn("SYSTEM_1_REFLEX", res_route["summary"])

        res_comm = run("dialectic_committee", {
            "file_path": "app/auth/tokens.py",
            "diff": "def verify(): pass",
        })
        self.assertEqual(res_comm["action"], "dialectic_committee")
        self.assertIn("Dialectic Committee Review", res_comm["summary"])


if __name__ == "__main__":
    unittest.main()
