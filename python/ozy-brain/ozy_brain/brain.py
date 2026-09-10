from __future__ import annotations

from typing import Any, Callable

from ozy_brain.agents.supervisor import SupervisorAgent
from ozy_brain.memory import consolidate_engrams, rank_memories, recall_deep
from ozy_brain.patterns import detect_patterns, suggest_next_steps
from ozy_brain.planner import plan
from ozy_brain.reflector import reflect
from ozy_brain.risk import risk_review
from ozy_brain.schemas import BrainResponse
from ozy_brain.summaries import build_mental_model, compress_session, summarize_project
from ozy_brain.vector_store import VectorMemoryStore


def _audit_critic_handler(payload: dict[str, Any]) -> BrainResponse:
    supervisor = SupervisorAgent(project_path=payload.get("project"))
    return supervisor.audit_with_critic(payload)


def _hotspots_handler(payload: dict[str, Any]) -> BrainResponse:
    supervisor = SupervisorAgent(project_path=payload.get("project"))
    return supervisor.get_repository_hotspots(payload)


def _consolidate_memory_handler(payload: dict[str, Any]) -> BrainResponse:
    supervisor = SupervisorAgent(project_path=payload.get("project"))
    return supervisor.consolidate_memory(payload)


def _deep_semantic_search_handler(payload: dict[str, Any]) -> BrainResponse:
    query = str(payload.get("query") or payload.get("goal") or "")
    limit = int(payload.get("limit", 5))
    store = VectorMemoryStore(persist_dir=payload.get("chroma_dir"))

    # 1. Retrieve dense vectors from ChromaDB
    similar = store.search_similar(query, limit=limit * 2)

    # 2. Merge candidates passed from SQLite/FTS5 (hybrid search)
    extra_candidates = payload.get("candidates") or []
    for c in extra_candidates:
        if isinstance(c, dict):
            similar.append(c)
        elif isinstance(c, str):
            similar.append({"content": c, "similarity_score": 0.6, "metadata": {}})

    # 3. Apply neural cross-relevance re-ranker
    reranked = store.rerank(query, similar, top_k=limit)

    plan_steps = [
        f"[Rank {i+1} | Score: {item.get('rerank_score', 0.5):.2f}] {item.get('content')}"
        for i, item in enumerate(reranked)
    ] or ["No relevant semantic memories found for this query."]

    return BrainResponse(
        action="deep_semantic_search",
        summary=f"Deep semantic search for '{query}': retrieved {len(reranked)} verified memories via ChromaDB + Re-ranker.",
        plan=plan_steps,
        risks=[],
        recommendations=["Apply highest-ranked architectural conventions before code changes."],
        memory_updates=[f"Semantic hits: {len(reranked)}"],
        confidence=round(reranked[0].get("rerank_score", 0.5), 2) if reranked else 0.3,
        structured_plan={"query": query, "results": reranked},
    )


def _upsert_vector_memory_handler(payload: dict[str, Any]) -> BrainResponse:
    store = VectorMemoryStore(persist_dir=payload.get("chroma_dir"))
    mem_id = str(payload.get("id") or payload.get("title") or "mem_0")
    content = str(payload.get("content") or payload.get("text") or "")
    metadata = payload.get("metadata") or {}

    ok = store.upsert_lesson(mem_id, content, metadata=metadata)
    return BrainResponse(
        action="upsert_vector_memory",
        summary=f"Vector memory upsert {'succeeded' if ok else 'failed'} for ID {mem_id}.",
        plan=[f"Upserted memory {mem_id} into ChromaDB collection 'ozy_lessons'"],
        risks=[],
        recommendations=[],
        memory_updates=[f"upserted: {mem_id}"],
        confidence=0.95 if ok else 0.20,
    )


ACTIONS: dict[str, Callable[[dict[str, Any]], BrainResponse]] = {
    "plan": plan,
    "reflect": reflect,
    "analyze_failure": reflect,
    "recall_deep": recall_deep,
    "rank_memories": rank_memories,
    "consolidate_engrams": consolidate_engrams,
    "consolidate_memory": _consolidate_memory_handler,
    "risk_review": risk_review,
    "audit_changes_with_critic": _audit_critic_handler,
    "get_repository_hotspots": _hotspots_handler,
    "build_mental_model": build_mental_model,
    "summarize_project": summarize_project,
    "compress_session": compress_session,
    "detect_patterns": detect_patterns,
    "suggest_next_steps": suggest_next_steps,
    "deep_semantic_search": _deep_semantic_search_handler,
    "upsert_vector_memory": _upsert_vector_memory_handler,
}


def run(action: str, payload: dict[str, Any]) -> dict[str, Any]:
    handler = ACTIONS.get(action, plan)
    response = handler(payload).to_dict()
    response["engine"] = "ozy-brain-python"
    response["safe_mode"] = True
    return response
