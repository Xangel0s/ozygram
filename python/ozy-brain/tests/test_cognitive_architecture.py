from __future__ import annotations

import time
from typing import Any
import pytest

from ozy_brain.cognitive_router import CognitiveSystemTier, route_cognitive_system
from ozy_brain.agents.supervisor import SupervisorAgent
from ozy_brain.brain import run


class TestCognitiveRouter:
    """Tests for Task 8.1: System 1 (Fast reflex) vs System 2 (Deliberative) router."""

    def test_system_1_low_blast_radius_and_mechanical(self):
        start = time.perf_counter()
        decision = route_cognitive_system({
            "file_path": "app/utils/formatter.py",
            "blast_radius": 1,
            "goal": "Add missing type annotations to helper",
        })
        elapsed_ms = (time.perf_counter() - start) * 1000

        assert decision.tier == CognitiveSystemTier.SYSTEM_1_REFLEX
        assert decision.zero_token_guarantee
        assert decision.latency_budget_ms <= 150
        assert not decision.requires_committee
        assert elapsed_ms < 10, "Router classification must complete in < 10ms"

    def test_system_2_high_blast_radius(self):
        decision = route_cognitive_system({
            "file_path": "app/services/billing.py",
            "blast_radius": 5,
            "affected_files": ["f1.py", "f2.py", "f3.py", "f4.py", "f5.py"],
        })
        assert decision.tier == CognitiveSystemTier.SYSTEM_2_DELIBERATIVE
        assert not decision.zero_token_guarantee
        assert decision.requires_committee
        assert "High blast radius" in decision.reason

    def test_system_2_critical_schema_and_migration(self):
        decision = route_cognitive_system({
            "file_path": "migrations/versions/001_initial_schema.sql",
            "blast_radius": 1,
        })
        assert decision.tier == CognitiveSystemTier.SYSTEM_2_DELIBERATIVE
        assert decision.requires_committee
        assert "schema" in decision.reason.lower()

    def test_system_2_destructive_sql(self):
        decision = route_cognitive_system({
            "file_path": "app/cleanup.py",
            "diff": "TRUNCATE TABLE session_logs;",
            "blast_radius": 1,
        })
        assert decision.tier == CognitiveSystemTier.SYSTEM_2_DELIBERATIVE
        assert "destructive sql" in decision.reason.lower()

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
        assert decision.tier == CognitiveSystemTier.SYSTEM_2_DELIBERATIVE
        assert "ghost patch" in decision.reason.lower()


class TestDialecticCommittee:
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

        assert resp.action == "dialectic_committee"
        assert "[DIALECTIC VERDICT: APPROVED]" in resp.summary
        assert resp.structured_plan["approved"]
        assert resp.confidence >= 0.90
        # Verify both Implementer and Critic are represented
        assert "implementer_plan" in resp.structured_plan
        assert "critic_audit" in resp.structured_plan

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

        assert "[DIALECTIC VERDICT: REJECTED: CRITICAL_VETO]" in resp.summary
        assert not resp.structured_plan["approved"]
        assert any("[CRITIC VETO]" in p for p in resp.plan)
        assert any("Execution halted" in p for p in resp.plan)

    def test_end_to_end_via_brain_run(self):
        res_route = run("cognitive_route", {
            "file_path": "app/test.py",
            "blast_radius": 1,
        })
        assert res_route["action"] == "cognitive_route"
        assert res_route["engine"] == "ozy-brain-python"
        assert "SYSTEM_1_REFLEX" in res_route["summary"]

        res_comm = run("dialectic_committee", {
            "file_path": "app/auth/tokens.py",
            "diff": "def verify(): pass",
        })
        assert res_comm["action"] == "dialectic_committee"
        assert "Dialectic Committee Review" in res_comm["summary"]
