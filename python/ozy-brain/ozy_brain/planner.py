from __future__ import annotations

from typing import Any

from ozy_brain.schemas import (
    BrainResponse,
    _base_risks,
    _base_summary,
    _brain_context_pack,
    _candidate_file_scores,
    _candidate_files,
    _combined_memory_titles,
    _execution_policy,
    _goal,
    _safe_mcp_calls,
    _validation_commands,
)


def _structured_plan(payload: dict[str, Any], autonomy: str = "advisory") -> dict[str, Any]:
    goal = _goal(payload)
    return {
        "autonomy_level": autonomy,
        "goal": goal,
        "phases": [
            {"name": "context", "objective": "Collect current-state evidence before edits", "exit_condition": "Relevant files, memories, graph summary, and risks are known"},
            {"name": "design", "objective": "Choose the smallest safe approach", "exit_condition": "Plan has rollback, validations, and bounded file scope"},
            {"name": "implementation", "objective": "Apply scoped changes only", "exit_condition": "No unrelated files are modified"},
            {"name": "validation", "objective": "Prove behavior with focused and broad tests", "exit_condition": "Required validation commands pass or failures are documented"},
            {"name": "learning", "objective": "Persist durable project knowledge", "exit_condition": "Decisions, gotchas, or patterns are saved when applicable"},
        ],
        "candidate_files": [item["path"] for item in _candidate_file_scores(payload, limit=8)] or _candidate_files(payload),
        "suggested_commands": _validation_commands(payload),
        "validation_checks": [
            "Confirm active project and branch",
            "Inspect git status before and after changes",
            "Verify no destructive operation is needed",
            "Run tests relevant to touched code",
        ],
        "stop_conditions": [
            "Required context is missing or contradictory",
            "A destructive action would be needed without explicit approval",
            "Validation fails in a way unrelated to the intended change",
            "Touched file scope expands beyond the plan",
        ],
    }


def plan(payload: dict[str, Any]) -> BrainResponse:
    goal = _goal(payload)
    memories = _combined_memory_titles(payload, limit=5)
    steps = [
        "Validate active project, branch, and dirty files before editing.",
        "Gather targeted context: relevant memories, graph summary, and candidate files.",
        "Identify risks and define rollback/verification before code changes.",
        f"Implement the smallest safe change for: {goal}.",
        "Run focused tests first, then broader validation if the change is architectural.",
        "Record durable learnings and next steps after validation.",
    ]
    if memories:
        steps.insert(2, "Apply relevant project memories: " + "; ".join(memories))
    return BrainResponse(
        action="plan",
        summary=_base_summary("plan", payload),
        plan=steps,
        risks=_base_risks(payload),
        recommendations=[
            "Prefer scoped edits and scoped staging.",
            "Do not execute destructive operations without explicit confirmation.",
            "Keep Rust as authority; use Python output as advisory guidance.",
        ],
        memory_updates=["Save decisions, bugfixes, and non-obvious discoveries after validation."],
        confidence=0.82,
        suggested_mcp_calls=_safe_mcp_calls(payload),
        structured_plan=_structured_plan(payload),
        execution_policy=_execution_policy(payload),
        brain_context_pack=_brain_context_pack(payload),
    )
