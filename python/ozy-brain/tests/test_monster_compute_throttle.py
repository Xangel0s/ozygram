"""Unit and stress tests for Monster Compute Config, Reverse Blast Radius, and Counterfactual MCTS."""

import os
import tempfile
import time
from pathlib import Path

import pytest
from ozy_brain.scanner.blast_radius import BlastRadiusReport, ReverseBlastRadiusEngine
from ozy_brain.scanner.compute_config import ComputeMode, MonsterComputeConfig
from ozy_brain.scanner.contract_tracer import ContractDriftAlert, FrontendConsumer
from ozy_brain.scanner.counterfactual_solver import (
    CandidateAction,
    CounterfactualMctsSolver,
    CounterfactualResolution,
)


def test_monster_compute_config_modes(monkeypatch):
    """Verify switching between Monster Compute and Lightweight mode via environment variables."""
    # 1. Monster Compute Mode
    monkeypatch.setenv("OZY_MAX_COMPUTE", "1")
    monkeypatch.delenv("OZY_LIGHTWEIGHT_MODE", raising=False)
    cfg_monster = MonsterComputeConfig.from_env()
    assert cfg_monster.mode == ComputeMode.MONSTER
    assert cfg_monster.mcts_simulations == 60
    assert cfg_monster.throttle_sleep_ms == 0
    assert "MonsterCompute" in cfg_monster.badge

    # 2. Lightweight Mode
    monkeypatch.delenv("OZY_MAX_COMPUTE", raising=False)
    monkeypatch.setenv("OZY_LIGHTWEIGHT_MODE", "1")
    cfg_light = MonsterComputeConfig.from_env()
    assert cfg_light.mode == ComputeMode.LIGHTWEIGHT
    assert cfg_light.mcts_simulations == 10
    assert cfg_light.throttle_sleep_ms == 30
    assert "Lightweight" in cfg_light.badge

    # 3. Yield CPU throttle verification
    t0 = time.time()
    cfg_light.yield_cpu_if_needed()
    elapsed = time.time() - t0
    assert elapsed >= 0.025  # Slept ~30ms to yield CPU


def test_reverse_blast_radius_engine():
    """Verify reverse AST traversal discovering direct callers and indirect views."""
    with tempfile.TemporaryDirectory() as tmp:
        fe_root = Path(tmp) / "crm-geofal"
        fe_root.mkdir()

        # 1. Leaf hook calling the endpoint
        hook_path = fe_root / "use-control.ts"
        hook_path.write_text("export function useControl() {}\n", encoding="utf-8")

        # 2. Table importing the hook
        table_path = fe_root / "neoprenos-table.tsx"
        table_path.write_text("import { useControl } from './use-control';\nexport function Table() {}\n", encoding="utf-8")

        # 3. Page importing the table
        page_path = fe_root / "page.tsx"
        page_path.write_text("import { Table } from './neoprenos-table';\nexport default function Page() {}\n", encoding="utf-8")

        # Inverted graph
        graph = ReverseBlastRadiusEngine.build_import_graph(fe_root)
        assert "use-control.ts" in graph
        assert "neoprenos-table.tsx" in graph["use-control.ts"]
        assert "neoprenos-table.tsx" in graph
        assert "page.tsx" in graph["neoprenos-table.tsx"]

        # Calculate blast radius
        consumers = [
            FrontendConsumer(
                raw_endpoint="/api/control-neoprenos/auto-sync",
                canonical_path="/api/control-neoprenos/auto-sync",
                http_method="POST",
                file_path="crm-geofal/use-control.ts",
                line_number=1,
            )
        ]

        report = ReverseBlastRadiusEngine.calculate_blast_radius(
            target_path="/api/control-neoprenos/auto-sync",
            frontend_consumers=consumers,
            inverted_graph=graph,
            config=MonsterComputeConfig.from_env(),
        )

        assert report.total_affected == 3
        assert "crm-geofal/use-control.ts" in report.direct_consumers
        assert "neoprenos-table.tsx" in report.indirect_dependents
        assert "page.tsx" in report.indirect_dependents
        assert len(report.affected_views) >= 2
        assert report.severity == "MEDIUM"
        assert "[ALERT: BLAST_RADIUS_RISK" in report.badge


def test_counterfactual_mcts_solver():
    """Verify counterfactual MCTS resolution generation with Q-value optimization."""
    drift_alert = ContractDriftAlert(
        kind="HTTP_METHOD_MISMATCH",
        severity="CRITICAL",
        frontend_file="crm-geofal/use-control.ts",
        frontend_line=54,
        canonical_path="/api/control-neoprenos/auto-sync",
        frontend_method="GET",
        backend_file="api-geofal-crm/router.py",
        backend_line=70,
        backend_method="POST",
        details="Method mismatch: GET vs POST",
    )

    # 1. Test resolution with small blast radius (favors FrontendCallerSync or DualActive)
    resolution = CounterfactualMctsSolver.resolve_drift(
        drift_alert, blast_radius_count=1
    )
    assert isinstance(resolution, CounterfactualResolution)
    assert resolution.q_value > 0.80
    assert resolution.best_action in (
        CandidateAction.FRONTEND_CALLER_SYNC,
        CandidateAction.DUAL_ACTIVE_CONTRACT,
    )
    assert len(resolution.suggested_patch_preview) > 0
    assert "[STATUS: COUNTERFACTUAL_MCTS_SOLVED]" in resolution.badge

    # 2. Test resolution with large blast radius (favors BackendFallbackAdapter or DualActive)
    res_large = CounterfactualMctsSolver.resolve_drift(
        drift_alert, blast_radius_count=12
    )
    assert res_large.q_value > 0.70
    assert res_large.best_action in (
        CandidateAction.BACKEND_FALLBACK_ADAPTER,
        CandidateAction.DUAL_ACTIVE_CONTRACT,
    )


def test_real_crmnew_blast_radius():
    """Verify import graph and blast radius calculation against real crmnew workspace."""
    crmnew_path = Path("crmnew").resolve()
    if not crmnew_path.is_dir():
        crmnew_path = Path("../../crmnew").resolve()

    fe_path = crmnew_path / "crm-geofal"
    if fe_path.is_dir():
        graph = ReverseBlastRadiusEngine.build_import_graph(fe_path)
        assert len(graph) > 0

        consumers = [
            FrontendConsumer(
                raw_endpoint="${API_URL}/api/control-neoprenos/auto-sync",
                canonical_path="/api/control-neoprenos/auto-sync",
                http_method="POST",
                file_path="src/components/dashboard/control-neoprenos/use-control-neoprenos.ts",
                line_number=54,
            )
        ]

        report = ReverseBlastRadiusEngine.calculate_blast_radius(
            target_path="/api/control-neoprenos/auto-sync",
            frontend_consumers=consumers,
            inverted_graph=graph,
        )
        assert report.total_affected >= 1
