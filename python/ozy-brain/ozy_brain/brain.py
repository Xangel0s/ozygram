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
from ozy_brain.outbox_consumer import OutboxConsumer
from ozy_brain.vector_store import VectorMemoryStore, reciprocal_rank_fusion


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

    # 1. Optionally drain outbox if db_path provided
    db_path = payload.get("db_path")
    if db_path:
        try:
            consumer = OutboxConsumer(db_path=db_path, chroma_dir=payload.get("chroma_dir"))
            consumer.drain(limit=100)
        except Exception:
            pass

    # 2. Retrieve dense vectors from ChromaDB
    similar = store.search_similar(query, limit=limit * 3)

    # 3. Extract and normalize lexical candidates (from SQLite FTS5)
    raw_candidates = payload.get("candidates") or []
    bm25_candidates: list[dict[str, Any]] = []
    for idx, c in enumerate(raw_candidates):
        if isinstance(c, dict):
            item = dict(c)
            if "id" not in item:
                item["id"] = f"fts_{idx}"
            bm25_candidates.append(item)
        elif isinstance(c, str):
            bm25_candidates.append({"id": f"fts_{idx}", "content": c, "similarity_score": 0.6})

    # 4. Apply Reciprocal Rank Fusion (RRF) between lexical FTS5 and dense vectors
    fused = reciprocal_rank_fusion(bm25_candidates, similar, k=60, top_k=limit * 2)

    # 5. Apply neural cross-relevance re-ranker
    reranked = store.rerank(query, fused if fused else similar, top_k=limit)

    plan_steps = [
        f"[Rank {i+1} | Score: {item.get('rerank_score', 0.5):.2f}] {item.get('content')}"
        for i, item in enumerate(reranked)
    ] or ["No relevant semantic memories found for this query."]

    return BrainResponse(
        action="deep_semantic_search",
        summary=f"Deep semantic search for '{query}': retrieved {len(reranked)} verified memories via RRF + Re-ranker.",
        plan=plan_steps,
        risks=[],
        recommendations=["Apply highest-ranked architectural conventions before code changes."],
        memory_updates=[f"Semantic hits: {len(reranked)}"],
        confidence=round(reranked[0].get("rerank_score", 0.5), 2) if reranked else 0.3,
        structured_plan={"query": query, "results": reranked},
    )


def _sync_outbox_handler(payload: dict[str, Any]) -> BrainResponse:
    consumer = OutboxConsumer(db_path=payload.get("db_path"), chroma_dir=payload.get("chroma_dir"))
    stats = consumer.drain(limit=int(payload.get("limit", 100)))
    return BrainResponse(
        action="sync_outbox",
        summary=f"Outbox sync complete: {stats['upserted']} upserted, {stats['deleted']} deleted (total: {stats['total']}).",
        plan=[f"Processed {stats['total']} outbox events"],
        risks=[],
        recommendations=[],
        memory_updates=[f"upserted: {stats['upserted']}", f"deleted: {stats['deleted']}"],
        confidence=1.0,
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
    "sync_outbox": _sync_outbox_handler,
}


def run(action: str, payload: dict[str, Any]) -> dict[str, Any]:
    handler = ACTIONS.get(action, plan)
    response = handler(payload).to_dict()
    response["engine"] = "ozy-brain-python"
    response["safe_mode"] = True
    return response
