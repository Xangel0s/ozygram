from __future__ import annotations

from typing import Any

from ozy_brain.planner import _structured_plan
from ozy_brain.schemas import (
    BrainResponse,
    _base_risks,
    _base_summary,
    _brain_context_pack,
    _candidate_files,
    _execution_policy,
    _extract_provenance,
    _goal,
    _items,
    _project,
    _safe_mcp_calls,
)


def _build_reflection_report(payload: dict[str, Any]) -> dict[str, Any]:
    failures = _items(payload, "failures")
    changes = _items(payload, "changes")
    goal = _goal(payload)
    project = _project(payload)

    root_causes: list[str] = []
    for fail in failures:
        fail_str = str(fail)
        if "timeout" in fail_str.lower():
            root_causes.append("Operation timed out — potential deadlock, infinite loop, or slow subprocess execution.")
        elif "permission" in fail_str.lower() or "access denied" in fail_str.lower():
            root_causes.append("Permission or access failure — verify file/directory permissions or process privileges.")
        elif "missing" in fail_str.lower() or "not found" in fail_str.lower() or "null" in fail_str.lower():
            root_causes.append("Missing dependency, uninitialized reference, or broken file path contract.")
        elif "syntax" in fail_str.lower() or "parse" in fail_str.lower():
            root_causes.append("Syntax error or invalid format specification.")
        else:
            root_causes.append(f"Execution failure: {fail_str[:120]}")

    if not root_causes:
        root_causes.append("No runtime errors logged; reflection focused on change scope and architectural alignment.")

    candidate_paths = set(_candidate_files(payload, limit=15))
    out_of_scope: list[str] = []
    for change in changes:
        ch_str = str(change)
        if ch_str not in candidate_paths and not any(ch_str in p for p in candidate_paths):
            out_of_scope.append(ch_str)

    extracted_gotchas: list[str] = []
    if failures:
        extracted_gotchas.append(f"Gotcha for {project}: Verify precondition before running {goal[:50]}. Root cause: {root_causes[0]}")
    if out_of_scope:
        extracted_gotchas.append(f"Scope Gotcha: Editing {out_of_scope[0]} was outside initial candidate file bounds.")

    return {
        "project": project,
        "goal": goal,
        "total_failures": len(failures),
        "total_changes": len(changes),
        "root_causes": root_causes,
        "scope_creep_detected": len(out_of_scope) > 0,
        "out_of_scope_files": out_of_scope,
        "extracted_gotchas": extracted_gotchas,
        "recommended_memory_actions": [
            f"Record gotcha via ozy_memory action=passive in ## Key Learnings section." if extracted_gotchas else "No new gotchas required.",
            "Summarize clean resolution into session observations." if not failures else "Resolve root causes before session compaction.",
        ],
    }


def reflect(payload: dict[str, Any]) -> BrainResponse:
    failures = _items(payload, "failures")
    changes = _items(payload, "changes")
    report = _build_reflection_report(payload)

    plan_steps = [
        "Compare intended goal with actual changed files and test output.",
        "Extract root causes from failures, distinguishing symptoms from underlying defects.",
        "Verify if changed files remained within bounded scope.",
        "Convert verified learnings into durable project gotchas and conventions.",
    ]

    risks = _base_risks(payload)
    if failures:
        risks.append(f"Repeated failures ({len(failures)}) detected; escalate context depth before re-executing fixes.")
    if report["scope_creep_detected"]:
        risks.append(f"Scope creep detected: {len(report['out_of_scope_files'])} file(s) modified outside initial candidate scope.")

    recommendations = [
        f"Review {len(changes)} changed item(s) for compliance with SOLID/DRY principles.",
        "Persist lessons using ozy_memory action=passive when tests pass cleanly.",
    ]

    memory_updates = report["extracted_gotchas"] or ["Capture reusable gotchas, conventions, and validation commands."]

    return BrainResponse(
        action="reflect",
        summary=_base_summary("reflection", payload),
        plan=plan_steps,
        risks=risks,
        recommendations=recommendations,
        memory_updates=memory_updates,
        confidence=0.85 if not failures else 0.72,
        suggested_mcp_calls=_safe_mcp_calls(payload, include_graph=False),
        structured_plan=_structured_plan(payload, autonomy="reflection_only"),
        execution_policy=_execution_policy(payload, autonomy="reflection_only"),
        brain_context_pack=_brain_context_pack(payload),
        reflection_report=report,
        provenance=_extract_provenance(payload),
    )
