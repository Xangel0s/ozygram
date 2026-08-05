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


def _cluster_and_dedupe_memories(memories: list[str]) -> list[dict[str, Any]]:
    """Groups memories by similarity topics and removes duplicates."""
    seen_keys: set[str] = set()
    clustered: list[dict[str, Any]] = []

    for idx, mem in enumerate(memories):
        clean = mem.strip()
        lower = clean.lower()
        # Extract topic key heuristic
        words = [w for w in lower.replace(":", " ").replace("-", " ").split() if len(w) >= 4]
        topic_key = "_".join(words[:3]) if words else f"topic_{idx}"

        if lower not in seen_keys:
            seen_keys.add(lower)
            clustered.append({
                "rank": len(clustered) + 1,
                "title": clean,
                "topic_key": topic_key,
                "confidence": max(0.60, round(0.95 - (len(clustered) * 0.05), 2)),
            })

    return clustered


def recall_deep(payload: dict[str, Any]) -> BrainResponse:
    """Combines observations, lessons, graph summary, and files into a prioritized context recall."""
    raw_memories = _combined_memory_titles(payload, limit=15)
    clustered = _cluster_and_dedupe_memories(raw_memories)

    plan_steps = (
        ["Use memories in priority order:"] + [f"- [Rank {c['rank']}] {c['title']}" for c in clustered]
        if clustered
        else ["No memories supplied; refresh Ozy context first."]
    )

    return BrainResponse(
        action="recall_deep",
        summary=_base_summary("deep recall", payload),
        plan=plan_steps,
        risks=["Memory can be stale; verify against active code and runtime evidence before applying."],
        recommendations=[
            "Pair Ozy recall with graph/file checks before implementation.",
            "Use ozy_memory action=search for targeted topic queries if context is missing.",
        ],
        memory_updates=[],
        confidence=0.85 if clustered else 0.45,
        suggested_mcp_calls=_safe_mcp_calls(payload),
        execution_policy=_execution_policy(payload, autonomy="recall_only"),
        brain_context_pack=_brain_context_pack(payload),
    )


def rank_memories(payload: dict[str, Any]) -> BrainResponse:
    """Ranks and clusters raw memories based on relevance and topic stability."""
    raw_memories = _combined_memory_titles(payload, limit=20)
    clustered = _cluster_and_dedupe_memories(raw_memories)

    plan_steps = (
        [f"Priority {c['rank']} (topic: {c['topic_key']}, score: {c['confidence']}): {c['title']}" for c in clustered]
        if clustered
        else ["No memories supplied for ranking."]
    )

    return BrainResponse(
        action="rank_memories",
        summary=_base_summary("memory ranking", payload),
        plan=plan_steps,
        risks=["High-ranked memory still requires verification against current codebase state."],
        recommendations=["Use ranking order for retrieval priority during agent context loading."],
        memory_updates=[],
        confidence=0.80 if clustered else 0.50,
    )
