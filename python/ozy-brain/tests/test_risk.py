"""tests/test_risk.py
Adversarial test suite for Task 2.3: Blast Radius integration in RiskCriticAgent.

Coverage targets:
    - compute_blast_radius: threshold detection, deduplication, empty inputs
    - RiskCriticAgent._audit_offline: blast radius gate blocks change (CRITICAL)
    - RiskCriticAgent.audit: payload wiring (symbol + impact_references)
    - brain._simulate_action_handler: end-to-end via ozy_brain.brain.run
    - Edge cases: zero refs, exactly-at-threshold, non-dict refs, missing symbol
"""
from __future__ import annotations

from typing import Any
from unittest.mock import MagicMock, patch

import pytest

from ozy_brain.agents.risk_critic import (
    BlastRadiusReport,
    RiskCriticAgent,
    RiskAuditResult,
    _HIGH_BLAST_RADIUS_THRESHOLD,
    compute_blast_radius,
)


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def _make_refs(file_paths: list[str]) -> list[dict[str, Any]]:
    """Build minimal reference dicts as ozy_find_references would return."""
    return [{"file_path": p, "line": 1, "snippet": "import X"} for p in file_paths]


def _critical_refs(n: int, suffix: str = "") -> list[dict[str, Any]]:
    """Return N references each pointing to a distinct critical module path."""
    return _make_refs([f"/app/auth/module_{i}{suffix}.py" for i in range(n)])


def _benign_refs(n: int) -> list[dict[str, Any]]:
    """Return N references pointing to non-critical modules."""
    return _make_refs([f"/app/utils/helper_{i}.py" for i in range(n)])


# ---------------------------------------------------------------------------
# compute_blast_radius unit tests
# ---------------------------------------------------------------------------

class TestComputeBlastRadius:
    """Unit tests for the pure blast-radius computation function."""

    def test_below_threshold_no_alert(self):
        """Less than threshold critical refs must not trigger alert."""
        refs = _critical_refs(_HIGH_BLAST_RADIUS_THRESHOLD - 1)
        report = compute_blast_radius("my_func", refs)

        assert not report.alert_required
        assert report.alert_text == ""
        assert report.critical_module_hits == _HIGH_BLAST_RADIUS_THRESHOLD - 1

    def test_at_threshold_triggers_alert(self):
        """Exactly N critical refs must trigger the obligatory alert."""
        refs = _critical_refs(_HIGH_BLAST_RADIUS_THRESHOLD)
        report = compute_blast_radius("validate_token", refs)

        assert report.alert_required
        assert "[ALERT: HIGH_BLAST_RADIUS: SNAPSHOT REQUIRED]" in report.alert_text
        assert "validate_token" in report.alert_text
        assert str(_HIGH_BLAST_RADIUS_THRESHOLD) in report.alert_text

    def test_above_threshold_triggers_alert(self):
        """More than N critical refs must also trigger the alert."""
        refs = _critical_refs(_HIGH_BLAST_RADIUS_THRESHOLD + 3)
        report = compute_blast_radius("BaseSchema", refs)

        assert report.alert_required
        assert report.critical_module_hits == _HIGH_BLAST_RADIUS_THRESHOLD + 3

    def test_deduplication_of_file_paths(self):
        """Duplicate file paths must be counted only once."""
        path = "/app/auth/dependency.py"
        refs = _make_refs([path] * 20)  # same file repeated 20 times
        report = compute_blast_radius("some_func", refs)

        assert report.total_references == 1
        assert report.critical_module_hits == 1
        assert not report.alert_required  # only 1 unique file

    def test_mixed_critical_and_benign_refs(self):
        """Only critical module paths count toward the threshold."""
        refs = _critical_refs(3) + _benign_refs(10)
        report = compute_blast_radius("process_data", refs)

        assert report.critical_module_hits == 3
        assert report.total_references == 13
        assert not report.alert_required

    def test_empty_references_no_alert(self):
        """Empty reference list must return a clean, no-alert report."""
        report = compute_blast_radius("orphan_func", [])

        assert report.total_references == 0
        assert report.critical_module_hits == 0
        assert not report.alert_required
        assert report.alert_text == ""

    def test_empty_symbol_name_is_tolerated(self):
        """Empty symbol with references must still compute correctly."""
        refs = _critical_refs(_HIGH_BLAST_RADIUS_THRESHOLD)
        report = compute_blast_radius("", refs)

        assert report.alert_required
        assert report.symbol == ""

    def test_critical_modules_capped_at_ten(self):
        """critical_modules list must be capped at 10 items."""
        refs = _critical_refs(20)
        report = compute_blast_radius("bulk_func", refs)

        assert len(report.critical_modules) <= 10
        assert report.critical_module_hits == 20

    def test_to_dict_contains_all_keys(self):
        """to_dict must expose all documented keys."""
        report = compute_blast_radius("f", _critical_refs(3))
        d = report.to_dict()
        expected_keys = {
            "symbol", "total_references", "critical_module_hits",
            "critical_modules", "alert_required", "alert_text",
        }
        assert expected_keys.issubset(d.keys())

    def test_file_key_fallback(self):
        """References with 'file' key instead of 'file_path' must be parsed."""
        # Use 5 distinct schema files so deduplication does not collapse them.
        refs = [
            {"file": f"/app/schema/base_{i}.py", "line": i}
            for i in range(_HIGH_BLAST_RADIUS_THRESHOLD)
        ]
        report = compute_blast_radius("Base", refs)

        assert report.alert_required
        assert report.critical_module_hits == _HIGH_BLAST_RADIUS_THRESHOLD


# ---------------------------------------------------------------------------
# RiskCriticAgent offline audit tests
# ---------------------------------------------------------------------------

class TestRiskCriticOfflineBlastRadius:
    """Integration tests for the offline adversarial critic with blast radius."""

    def _make_agent(self) -> RiskCriticAgent:
        """Return agent with mocked DataEngine to avoid filesystem hits."""
        agent = RiskCriticAgent.__new__(RiskCriticAgent)
        agent.config = MagicMock()
        agent.config.provider.value = "mock"
        agent.config.model = "mock-model"
        # Simulate OFFLINE_FALLBACK so LLM path is skipped
        from ozy_brain.config import LLMProvider
        agent.config.provider = LLMProvider.OFFLINE_FALLBACK
        agent.data_engine = MagicMock()
        agent.data_engine.get_file_telemetry.return_value = None
        return agent

    def test_high_blast_radius_blocks_change(self):
        """Audit with 5+ critical module refs must block change and set CRITICAL level."""
        agent = self._make_agent()
        payload = {
            "symbol": "authenticate_user",
            "impact_references": _critical_refs(_HIGH_BLAST_RADIUS_THRESHOLD),
            "files": ["app/views.py"],
        }
        result = agent.audit(payload)

        assert result.blocked
        assert result.risk_level == "CRITICAL"
        assert any("[ALERT: HIGH_BLAST_RADIUS: SNAPSHOT REQUIRED]" in r for r in result.reasons)
        assert result.blast_radius is not None
        assert result.blast_radius.alert_required

    def test_low_blast_radius_does_not_block(self):
        """Audit with fewer than threshold critical refs must not block."""
        agent = self._make_agent()
        payload = {
            "symbol": "format_date",
            "impact_references": _critical_refs(_HIGH_BLAST_RADIUS_THRESHOLD - 1),
            "files": ["utils/date.py"],
        }
        result = agent.audit(payload)

        assert not result.blocked
        assert result.risk_level != "CRITICAL"

    def test_blast_radius_included_in_to_dict(self):
        """to_dict must include blast_radius when present."""
        agent = self._make_agent()
        payload = {
            "symbol": "get_user_token",
            "impact_references": _critical_refs(_HIGH_BLAST_RADIUS_THRESHOLD),
        }
        d = agent.audit(payload).to_dict()
        assert "blast_radius" in d
        assert d["blast_radius"]["alert_required"]

    def test_no_refs_no_blast_radius_key(self):
        """Audit with no symbol and no refs must not include blast_radius key."""
        agent = self._make_agent()
        payload = {"files": ["utils/helper.py"]}
        d = agent.audit(payload).to_dict()
        assert "blast_radius" not in d

    def test_blast_radius_combined_with_hotspot(self):
        """Both hotspot and blast radius must both appear in reasons when present."""
        agent = self._make_agent()
        agent.data_engine.get_file_telemetry.return_value = {
            "file_path": "app/core/db.py",
            "risk_level": "HIGH",
        }
        payload = {
            "symbol": "run_migration",
            "impact_references": _critical_refs(_HIGH_BLAST_RADIUS_THRESHOLD),
            "files": ["app/core/db.py"],
        }
        result = agent.audit(payload)
        alert_in_reasons = any("[ALERT: HIGH_BLAST_RADIUS" in r for r in result.reasons)
        hotspot_in_reasons = any("hotspot" in r.lower() for r in result.reasons)
        assert alert_in_reasons
        assert hotspot_in_reasons

    def test_mitigation_recommends_snapshot(self):
        """Mitigations must mention ozy_exploration snapshot on high blast radius."""
        agent = self._make_agent()
        payload = {
            "symbol": "BaseModel",
            "impact_references": _critical_refs(_HIGH_BLAST_RADIUS_THRESHOLD),
        }
        result = agent.audit(payload)
        assert any("rollback_snapshot" in m.lower() or "snapshot" in m.lower() for m in result.mitigations)


# ---------------------------------------------------------------------------
# brain.run simulate_action end-to-end
# ---------------------------------------------------------------------------

class TestSimulateActionBlastRadius:
    """End-to-end tests via ozy_brain.brain.run for simulate_action."""

    def _run(self, payload: dict[str, Any]) -> dict[str, Any]:
        from ozy_brain.brain import run
        return run("simulate_action", payload)

    def test_high_blast_radius_via_run(self):
        """simulate_action with 5+ critical refs must include SNAPSHOT REQUIRED in plan."""
        result = self._run({
            "file_path": "app/users.py",
            "symbol": "get_auth_token",
            "impact_references": _critical_refs(_HIGH_BLAST_RADIUS_THRESHOLD),
        })
        plan_text = " ".join(result.get("plan", []))
        assert "[ALERT: HIGH_BLAST_RADIUS: SNAPSHOT REQUIRED]" in plan_text

    def test_symbol_blast_radius_in_structured_plan(self):
        """structured_plan must expose symbol_blast_radius sub-dict."""
        result = self._run({
            "symbol": "validate_schema",
            "impact_references": _critical_refs(_HIGH_BLAST_RADIUS_THRESHOLD),
        })
        br = result.get("structured_plan", {}).get("blast_radius_analysis", {})
        assert "symbol_blast_radius" in br
        assert br["symbol_blast_radius"]["alert_required"]

    def test_benign_symbol_no_symbol_alert_in_plan(self):
        """simulate_action with only benign refs must NOT include the symbol-specific
        SNAPSHOT REQUIRED alert text (which includes the symbol name).
        The generic file-count alert may still fire since blast_radius>=3.
        """
        symbol = "format_currency"
        result = self._run({
            "file_path": "app/formatters.py",
            "symbol": symbol,
            "impact_references": _benign_refs(20),
        })
        plan_text = " ".join(result.get("plan", []))
        # The symbol-specific ALERT includes the symbol name; verify it is absent.
        assert symbol not in plan_text or "ALERT: HIGH_BLAST_RADIUS" not in plan_text
        # The blast_radius report must NOT have alert_required
        br = result.get("structured_plan", {}).get("blast_radius_analysis", {}).get("symbol_blast_radius")
        if br is not None:
            assert not br["alert_required"]

    def test_blast_radius_merges_with_file_impact(self):
        """total_blast_radius must be max of file-count and semantic reference count."""
        # 8 files in impact list, but 10 critical refs semantically
        refs = _critical_refs(_HIGH_BLAST_RADIUS_THRESHOLD + 5)
        result = self._run({
            "impact": [{"file_path": f"f{i}.py"} for i in range(8)],
            "symbol": "core_func",
            "impact_references": refs,
        })
        total = result["structured_plan"]["blast_radius_analysis"]["total_blast_radius"]
        # semantic count (10) > file list (8), so must be 10
        assert total >= _HIGH_BLAST_RADIUS_THRESHOLD + 5

    def test_engine_tag_present(self):
        """Response must always carry the engine tag."""
        result = self._run({"symbol": "noop"})
        assert result.get("engine") == "ozy-brain-python"
