from __future__ import annotations

from typing import Any

from ozy_brain.schemas import (
    BrainResponse,
    _base_risks,
    _base_summary,
    _combined_memory_titles,
    _goal,
    _project,
)


def detect_patterns(payload: dict[str, Any]) -> BrainResponse:
    """Detects recurring user preferences, domain rules, and platform constraints."""
    memories = _combined_memory_titles(payload, limit=25)
    project = _project(payload).lower()
    goal = _goal(payload).lower()
    patterns: list[str] = []
    joined = " ".join(memories).lower() + " " + goal + " " + project

    # Domain & User Pattern Heuristics
    if "crm" in joined or "comercial" in joined or "gerencia" in joined:
        patterns.append("Domain Rule (CRM): Keep Comercial and Gerencia modules decoupled; do not mix business logic across boundaries.")

    if "evidence" in joined or "test" in joined or "validation" in joined:
        patterns.append("User Rule: Provide concrete empirical validation evidence before reporting completion.")

    if "push" in joined or "stage" in joined or "commit" in joined:
        patterns.append("Git Rule: Never push without validation; stage changes incrementally and avoid staging unrelated files.")

    if "powershell" in joined or "cmd" in joined or "windows" in joined:
        patterns.append("Platform Rule (Windows): Avoid slow or blocking terminal commands; use cmd /c for script invocations.")

    if "solid" in joined or "dry" in joined or "kiss" in joined:
        patterns.append("Code Rule: Follow SOLID, DRY, and KISS principles across all code modifications.")

    if not patterns:
        patterns.append("Insufficient repeated evidence; keep observing before formalizing hard rules.")

    return BrainResponse(
        action="detect_patterns",
        summary=_base_summary("pattern detection", payload),
        plan=patterns,
        risks=["Do not overfit temporary one-off user preferences into permanent project conventions."],
        recommendations=[
            "Promote confirmed user patterns into AGENTS.md or ozy_memory conventions.",
            "Enforce domain separation rules during code review and planning phases.",
        ],
        memory_updates=patterns,
        confidence=0.82 if len(patterns) > 1 else 0.65,
    )


def suggest_next_steps(payload: dict[str, Any]) -> BrainResponse:
    """Suggests sequential next steps tailored to current task status and patterns."""
    return BrainResponse(
        action="suggest_next_steps",
        summary=_base_summary("next steps", payload),
        plan=[
            "1. Confirm current branch and git status baseline.",
            "2. Pick the smallest safe milestone for the active goal.",
            "3. Validate changes with focused unit tests first, then integration suite.",
            "4. Persist non-obvious learnings and gotchas to ozy_memory.",
        ],
        risks=_base_risks(payload),
        recommendations=["Ask for user confirmation only when assumptions carry irreversible risks."],
        memory_updates=[],
        confidence=0.82,
    )
