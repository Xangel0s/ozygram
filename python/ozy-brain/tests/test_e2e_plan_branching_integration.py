"""
End-to-End integration tests for Warm-Started Plan Branching MCTS (Fase 4).
Standard: Zero-Emoji, SOLID, SOTA in Tree Search for Agents (LATS ICML 2024 / RAP NeurIPS).
"""

import json
import sqlite3
import tempfile
from pathlib import Path
import pytest

from ozy_brain.planner import plan
from ozy_brain.dream.plan_branching import (
    PlanExecutionState,
    PlanBranchingMctsOptimizer,
    persist_mitigation_vaccine,
)
from ozy_brain.dream.orchestrator import DreamOrchestrator


def test_e2e_plan_with_intermediate_phase():
    payload = {
        "goal": "Modernize probetas endpoint while guaranteeing zero downtime and unbroken frontend views",
        "completed_phases": ["context", "leaf_contracts"],
        "current_phase": "domain_services",
        "files": ["api-geofal-crm/app/api/endpoints/probetas.py"],
        "working_edits": [
            {"file": "api-geofal-crm/app/schemas/probetas.py", "action": "add_v2_pydantic_schema"}
        ],
    }

    res = plan(payload)
    assert res.action == "plan"
    assert res.structured_plan is not None

    struct = res.structured_plan
    assert struct["current_phase"] == "domain_services"
    assert "context" in struct["completed_phases"]
    assert "leaf_contracts" in struct["completed_phases"]

    # MCTS Branch Optimization verification
    dream_opt = struct.get("dream_mcts_optimization", {})
    assert dream_opt.get("best_strategy") == "DualActiveContract"
    assert dream_opt.get("badge") == "[OPTIMIZED_BY_DREAM_RSI]"
    assert dream_opt.get("best_score", 0.0) >= 0.85
    assert len(dream_opt.get("evaluated_trajectories", [])) >= 2

    # Verification of plan steps and recommendations
    plan_text = "\n".join(res.plan)
    assert "[DREAM_RSI]" in plan_text
    assert "DualActiveContract" in plan_text
    assert "[OPTIMIZED_BY_DREAM_RSI]" in plan_text

    recs_text = "\n".join(res.recommendations)
    assert "[OPTIMIZED_BY_DREAM_RSI]" in recs_text
    assert "DualActiveContract" in recs_text


def test_e2e_persist_mitigation_vaccine_markdown(tmp_path: Path):
    state = PlanExecutionState(
        completed_phases=["context", "leaf_contracts"],
        active_phase="domain_services",
    )
    cands = ["api-geofal-crm/app/api/endpoints/probetas.py"]

    # Run optimizer to get result
    opt_res = PlanBranchingMctsOptimizer.optimize_plan(state=state, candidate_files=cands)
    assert opt_res.best_strategy == "DualActiveContract"

    # Export gotcha vaccine to Markdown
    base_dir = tmp_path / ".ozy" / "memories"
    gotcha_path = persist_mitigation_vaccine(
        optimization_result=opt_res,
        target_files=cands,
        base_dir=base_dir,
    )
    assert gotcha_path is not None
    assert gotcha_path.exists()

    content = gotcha_path.read_text(encoding="utf-8")
    assert "kind: gotcha" in content
    assert "topic_key: contract_drift_prevention"
    assert "DualActiveContract" in content
    assert "[OPTIMIZED_BY_DREAM_RSI]" in content
    assert "api-geofal-crm/app/api/endpoints/probetas.py" in content
    assert "# Verbal Self-Reflection (Centinela Engine)" in content


def test_e2e_orchestrator_dream_cycle_plan_branching(tmp_path: Path):
    workspace_root = tmp_path / "workspace"
    workspace_root.mkdir()
    db_path = workspace_root / ".ozymem" / "memory.db"
    db_path.parent.mkdir(parents=True, exist_ok=True)

    # Initialize SQLite schema with a fragile file record
    conn = sqlite3.connect(str(db_path))
    conn.execute(
        """CREATE TABLE IF NOT EXISTS amygdala_reflexes (
            id TEXT PRIMARY KEY,
            tenant_id TEXT,
            file_path TEXT,
            symbol_name TEXT,
            threat_score REAL,
            occurrences INTEGER,
            pain_trigger TEXT,
            antidote_directive TEXT,
            created_at REAL
        )"""
    )
    conn.execute(
        """CREATE TABLE IF NOT EXISTS procedural_habits (
            id TEXT PRIMARY KEY,
            tenant_id TEXT,
            trigger_file TEXT,
            choreography_json TEXT,
            confidence REAL,
            occurrences INTEGER,
            created_at REAL
        )"""
    )
    conn.execute(
        """INSERT INTO amygdala_reflexes (id, tenant_id, file_path, symbol_name, threat_score, occurrences, pain_trigger, antidote_directive, created_at)
           VALUES ('rf1', 'test_tenant', 'api-geofal-crm/app/api/endpoints/probetas.py', 'get_probetas', 0.85, 3, 'Signature mismatch', 'Use DualActiveContract', 1000.0)"""
    )
    conn.commit()
    conn.close()

    # Create the fragile file on disk so Glial Homeostasis phagocytosis does not purge it
    target_file = workspace_root / "api-geofal-crm" / "app" / "api" / "endpoints" / "probetas.py"
    target_file.parent.mkdir(parents=True, exist_ok=True)
    target_file.write_text("def get_probetas():\n    return []\n", encoding="utf-8")

    orchestrator = DreamOrchestrator(
        workspace_root=workspace_root,
        db_path=db_path,
        tenant_id="test_tenant",
    )

    consolidated = orchestrator.run_dream_cycle(max_commits=10)
    assert consolidated["status"] == "complete"
    assert "plan_branch_optimizations" in consolidated
    assert len(consolidated["plan_branch_optimizations"]) >= 1

    branch_opt = consolidated["plan_branch_optimizations"][0]
    assert branch_opt["best_strategy"] == "DualActiveContract"
    assert branch_opt["badge"] == "[OPTIMIZED_BY_DREAM_RSI]"

    # Verifies persisted insights file
    insights_file = workspace_root / ".ozymem" / "dream_latest_insights.json"
    assert insights_file.exists()
    insights_data = json.loads(insights_file.read_text(encoding="utf-8"))
    assert "plan_branch_optimizations" in insights_data
    assert len(insights_data["plan_branch_optimizations"]) >= 1
