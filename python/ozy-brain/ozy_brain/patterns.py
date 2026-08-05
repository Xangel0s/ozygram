from __future__ import annotations

from typing import Any

from ozy_brain.schemas import (
    BrainResponse,
    _base_risks,
    _base_summary,
    _combined_memory_titles,
)


def detect_patterns(payload: dict[str, Any]) -> BrainResponse:
    memories = _combined_memory_titles(payload, limit=20)
    patterns: list[str] = []
    joined = " ".join(memories).lower()

    if "test" in joined or "validation" in joined:
        patterns.append("User/project values validation evidence before reporting completion.")
    if "push" in joined or "stage" in joined:
        patterns.append("Git operations should remain scoped and avoid staging unrelated files.")
    if "solid" in joined or "dry" in joined or "kiss" in joined:
        patterns.append("Follow clean code principles: SOLID, DRY, and KISS.")
    if "windows" in joined or "cmd" in joined or "powershell" in joined:
        patterns.append("Ensure Windows shell compatibility (cmd /c, escape paths).")

    if not patterns:
        patterns.append("Insufficient repeated evidence; keep observing before creating hard rules.")

    return BrainResponse(
        action="detect_patterns",
        summary=_base_summary("pattern detection", payload),
        plan=patterns,
        risks=["Do not overfit one-off behavior into a permanent rule."],
        recommendations=["Promote repeated patterns into project conventions only after confirmation."],
        memory_updates=patterns,
        confidence=0.72,
    )


def suggest_next_steps(payload: dict[str, Any]) -> BrainResponse:
    return BrainResponse(
        action="suggest_next_steps",
        summary=_base_summary("next steps", payload),
        plan=[
            "Pick the smallest next safe milestone.",
            "Validate with focused tests or runtime checks.",
            "Update memory/context if the milestone teaches a durable rule.",
        ],
        risks=_base_risks(payload),
        recommendations=["Ask for confirmation only when assumptions are risky or irreversible."],
        memory_updates=[],
        confidence=0.80,
    )
