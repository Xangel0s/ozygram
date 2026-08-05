from __future__ import annotations

from typing import Any

from ozy_brain.planner import _structured_plan
from ozy_brain.schemas import (
    BrainResponse,
    _base_risks,
    _base_summary,
    _brain_context_pack,
    _candidate_file_scores,
    _execution_policy,
    _git_status_files,
    _goal,
    _items,
    _project,
    _safe_mcp_calls,
)


def _build_risk_assessment(payload: dict[str, Any]) -> dict[str, Any]:
    goal = _goal(payload).lower()
    project = _project(payload)
    dirty_files = _git_status_files(payload)
    candidate_scores = _candidate_file_scores(payload)

    risk_categories: list[str] = []
    critical_paths: list[str] = []
    risk_level = "low"
    requires_confirmation = False

    if any(w in goal for w in ["drop", "truncate", "delete", "remove", "rm", "destroy"]):
        risk_categories.append("data_loss")
        risk_level = "critical"
        requires_confirmation = True
    elif any(w in goal for w in ["auth", "security", "token", "password", "secret", "login", "credential"]):
        risk_categories.append("auth_security")
        risk_level = "high"
        requires_confirmation = True
    elif any(w in goal for w in ["migration", "database", "schema", "table", "sql"]):
        risk_categories.append("database_migration")
        risk_level = "high"
        requires_confirmation = True

    if any(w in goal for w in ["refactor", "architecture", "monorepo", "cross"]):
        risk_categories.append("architectural_refactor")
        if risk_level not in ["high", "critical"]:
            risk_level = "medium"

    if len(dirty_files) >= 5:
        risk_categories.append("dirty_workspace")
        if risk_level == "low":
            risk_level = "medium"

    high_risk_keywords = ["auth", "config", "schema", "migration", "registry", "secret", "env", "core", "backend"]
    for path_item in candidate_scores:
        path = str(path_item.get("path", "")).lower()
        if any(kw in path for kw in high_risk_keywords):
            critical_paths.append(path_item["path"])

    verification_checklist = [
        "Verify git status baseline before starting edits.",
        "Check graph neighbors and impacted dependents using ozy_graph or ozy_context.",
    ]
    if "data_loss" in risk_categories or "database_migration" in risk_categories:
        verification_checklist.append("Create database backup or ensure transaction dry-run rollback capability.")
    if "auth_security" in risk_categories:
        verification_checklist.append("Ensure no hardcoded tokens/secrets in proposed code edits.")
    verification_checklist.append("Run targeted unit tests followed by integration test suite.")

    return {
        "project": project,
        "goal": _goal(payload),
        "risk_level": risk_level,
        "risk_categories": risk_categories or ["standard_maintenance"],
        "critical_paths": critical_paths[:10],
        "dirty_workspace_count": len(dirty_files),
        "requires_user_confirmation": requires_confirmation,
        "verification_checklist": verification_checklist,
    }


def risk_review(payload: dict[str, Any]) -> BrainResponse:
    assessment = _build_risk_assessment(payload)
    risks = _base_risks(payload)

    if assessment["requires_user_confirmation"]:
        risks.insert(0, f"CRITICAL: User confirmation required before proceeding due to {', '.join(assessment['risk_categories'])} risk.")

    plan_steps = [
        "Classify change scope: file, module, cross-module, database, security, deployment.",
        f"Target risk level: {assessment['risk_level'].upper()} ({', '.join(assessment['risk_categories'])}).",
        "Inspect critical paths and graph dependencies prior to making changes.",
        "Execute mandatory verification checklist before declaring task complete.",
    ]

    recommendations = [
        "Require explicit user confirmation for any destructive filesystem, DB, or credential operation.",
        "Prefer dry-run, preview, or diff output before modifying core contracts.",
    ]

    return BrainResponse(
        action="risk_review",
        summary=_base_summary("risk review", payload),
        plan=plan_steps,
        risks=risks,
        recommendations=recommendations,
        memory_updates=["Record confirmed risks as project gotchas when they repeat."],
        confidence=0.88 if assessment["risk_level"] in ["high", "critical"] else 0.80,
        suggested_mcp_calls=_safe_mcp_calls(payload),
        structured_plan=_structured_plan(payload, autonomy="risk_review_only"),
        execution_policy=_execution_policy(payload, autonomy="risk_review_only"),
        brain_context_pack=_brain_context_pack(payload),
        risk_assessment=assessment,
    )
