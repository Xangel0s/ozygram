from __future__ import annotations

from typing import Any

from ozy_brain.schemas import (
    BrainResponse,
    _base_summary,
    _brain_context_pack,
    _combined_memory_titles,
    _execution_policy,
    _safe_mcp_calls,
)


def recall_deep(payload: dict[str, Any]) -> BrainResponse:
    memories = _combined_memory_titles(payload, limit=10)
    return BrainResponse(
        action="recall_deep",
        summary=_base_summary("deep recall", payload),
        plan=["Use memories in priority order:"] + [f"- {m}" for m in memories] if memories else ["No memories supplied; refresh Ozy context first."],
        risks=["Memory can be stale; verify against current code and runtime evidence."],
        recommendations=["Pair Ozy recall with graph/file checks before implementation."],
        memory_updates=[],
        confidence=0.82 if memories else 0.45,
        suggested_mcp_calls=_safe_mcp_calls(payload),
        execution_policy=_execution_policy(payload, autonomy="recall_only"),
        brain_context_pack=_brain_context_pack(payload),
    )


def rank_memories(payload: dict[str, Any]) -> BrainResponse:
    memories = _combined_memory_titles(payload, limit=10)
    return BrainResponse(
        action="rank_memories",
        summary=_base_summary("memory ranking", payload),
        plan=[f"priority {idx + 1}: {memory}" for idx, memory in enumerate(memories)] or ["No memories supplied."],
        risks=["High-ranked memory still needs current-state verification."],
        recommendations=["Use ranking as retrieval order, not as proof of current truth."],
        memory_updates=[],
        confidence=0.75,
    )
