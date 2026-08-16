from __future__ import annotations

from typing import Any

from ozy_brain.schemas import (
    BrainResponse,
    _base_summary,
    _brain_context_pack,
    _combined_memory_titles,
    _execution_policy,
    _extract_provenance,
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
        provenance=_extract_provenance(payload),
    )


def extract_knowledge_triads(text: str) -> list[dict[str, str]]:
    """Extracts [Subject -> Relation/Rule -> Object/Solution] knowledge triads from text."""
    triads: list[dict[str, str]] = []
    lines = [line.strip() for line in text.replace(";", "\n").splitlines() if line.strip()]

    for line in lines:
        # Pattern 1: Arrow notation A -> B -> C or A -> B
        if "->" in line:
            parts = [p.strip() for p in line.split("->") if p.strip()]
            if len(parts) >= 3:
                triads.append({"subject": parts[0], "relation": parts[1], "object": parts[2]})
            elif len(parts) == 2:
                triads.append({"subject": parts[0], "relation": "requires", "object": parts[1]})
            continue

        # Pattern 2: Colon notation Subject: Rule / Solution
        if ":" in line:
            parts = line.split(":", 1)
            subj = parts[0].strip()
            rest = parts[1].strip()
            relation = "must" if any(w in rest.lower() for w in ["never", "always", "must", "should"]) else "relates_to"
            triads.append({"subject": subj, "relation": relation, "object": rest})
            continue

        # Pattern 3: Heuristic keyword extraction
        words = line.split()
        if len(words) >= 4:
            triads.append({
                "subject": words[0],
                "relation": "standard",
                "object": " ".join(words[1:]),
            })

    return triads


def classify_memory_tier(text: str, trigger_event: str | None = None) -> dict[str, Any]:
    """Classifies memory into Working Cache (Layer 1) vs Engram Store (Layer 2)."""
    is_consolidation_trigger = bool(
        trigger_event and any(t in trigger_event.lower() for t in ["commit", "test_pass", "bugfix", "release", "milestone"])
    )

    has_architectural_rule = any(
        kw in text.lower() for kw in ["always", "never", "standard", "convention", "schema", "architecture", "security", "jwt", "dto"]
    )

    if is_consolidation_trigger or has_architectural_rule:
        tier = "Engram Store (Layer 2 - Long-Term)"
        action_needed = "consolidate_to_sqlite"
        retention = "permanent"
    else:
        tier = "Working Cache (Layer 1 - Ephemeral)"
        action_needed = "keep_in_memory"
        retention = "session_only"

    return {
        "tier": tier,
        "action_needed": action_needed,
        "retention": retention,
        "is_consolidated": action_needed == "consolidate_to_sqlite",
    }


def consolidate_engrams(payload: dict[str, Any]) -> BrainResponse:
    """Processes working memories into structured [Subject -> Relation -> Object] Engram Store entries."""
    raw_memories = _combined_memory_titles(payload, limit=20)
    trigger = payload.get("trigger_event") or payload.get("event")

    triads: list[dict[str, str]] = []
    consolidated_entries: list[dict[str, Any]] = []

    for mem in raw_memories:
        extracted = extract_knowledge_triads(mem)
        triads.extend(extracted)
        tier_info = classify_memory_tier(mem, trigger)
        if tier_info["is_consolidated"]:
            consolidated_entries.append({
                "source": mem,
                "tier": tier_info["tier"],
                "triads": extracted,
            })

    plan_steps = [
        f"Consolidation trigger: {trigger or 'manual_eval'}",
        f"Total knowledge triads extracted: {len(triads)}",
        f"Promoted to Engram Store (L2): {len(consolidated_entries)} of {len(raw_memories)}",
    ]
    for c in consolidated_entries[:5]:
        plan_steps.append(f"- [Engram L2] {c['source'][:75]}...")

    return BrainResponse(
        action="consolidate_engrams",
        summary=_base_summary("2-layer memory consolidation", payload),
        plan=plan_steps,
        risks=["Ensure consolidated rules reflect verified architectural requirements, not transient bugs."],
        recommendations=[
            "Use ozymem record_convention for architectural invariants.",
            "Use ozymem record_lesson for bugfix troubleshooting patterns.",
        ],
        memory_updates=[c["source"] for c in consolidated_entries],
        confidence=0.88 if consolidated_entries else 0.50,
        suggested_mcp_calls=_safe_mcp_calls(payload),
        execution_policy=_execution_policy(payload, autonomy="consolidation"),
        brain_context_pack=_brain_context_pack(payload),
        provenance=_extract_provenance(payload),
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
        execution_policy=_execution_policy(payload, autonomy="ranking_only"),
        brain_context_pack=_brain_context_pack(payload),
        provenance=_extract_provenance(payload),
    )

