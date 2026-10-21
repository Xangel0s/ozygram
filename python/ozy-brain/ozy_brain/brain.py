from __future__ import annotations

from typing import Any, Callable

from ozy_brain.agents.risk_critic import compute_blast_radius
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


def _dream_rsi_handler(payload: dict[str, Any]) -> BrainResponse:
    from ozy_brain.dream.optimizer import DreamRsiOptimizer
    from ozy_brain.dream.policy import MctsExplorationPolicy

    db_path = payload.get("db_path")
    trajectories = payload.get("trajectories") or []
    candidate_cfg = payload.get("candidate_policy")

    candidate = None
    if isinstance(candidate_cfg, dict):
        candidate = MctsExplorationPolicy(
            version=candidate_cfg.get("version", "v1.1.0"),
            exploration_constant=float(candidate_cfg.get("exploration_constant", 1.4142)),
            prune_threshold=float(candidate_cfg.get("prune_threshold", -3.0)),
        )

    optimizer = DreamRsiOptimizer(db_path=db_path)
    res = optimizer.run_optimization_round(trajectories=trajectories, candidate_policy=candidate)

    return BrainResponse(
        action="dream_rsi",
        summary=res.get("message", "Dream-RSI optimization cycle complete."),
        plan=[f"Promoted: {res.get('promoted', False)}", f"Score: {res.get('active_score', 0.0)} -> {res.get('candidate_score', 0.0)}"],
        risks=[f"Pruning errors: {res.get('total_pruning_errors', 0)}"],
        recommendations=res.get("bottlenecks", []),
        memory_updates=[f"Active policy: {res.get('active_version', 'v1.0.0')}"],
        confidence=0.90 if res.get("promoted") else 0.70,
        structured_plan=res,
    )


def _simulate_action_handler(payload: dict[str, Any]) -> BrainResponse:
    target_file = str(payload.get("file_path") or payload.get("file") or payload.get("target_file") or "")
    impact = payload.get("impact") or []
    blast_radius = int(payload.get("blast_radius") or len(impact))

    # --- Semantic blast radius via ozy_find_references ---
    symbol = str(payload.get("symbol") or payload.get("target_symbol") or "")
    impact_references: list[dict[str, Any]] = list(
        payload.get("impact_references") or payload.get("references") or []
    )
    blast_report = compute_blast_radius(symbol, impact_references) if (symbol or impact_references) else None

    # Merge semantic blast radius into the file-count blast radius
    if blast_report:
        blast_radius = max(blast_radius, blast_report.total_references)

    # --- Speculative Ghost Patch simulation check ---
    ghost_sim: dict[str, Any] = payload.get("simulation_result") or payload.get("ghost_patch_report") or {}

    critical_keywords = ["schema", "auth", "migration", "core", "token", "secret", "config", "backend"]
    is_critical = any(kw in target_file.lower() for kw in critical_keywords) or any(
        isinstance(item, dict) and any(kw in str(item.get("file_path", "")).lower() for kw in critical_keywords)
        for item in impact
    )
    # A high-blast-radius symbol alert or contract breakage also makes the change critical.
    if (blast_report and blast_report.alert_required) or (ghost_sim and ghost_sim.get("breaks_dependents")):
        is_critical = True

    snapshot_required = blast_radius >= 3 or is_critical

    plan_steps = [
        f"[SIMULATION: ACTION PREVIEW] Simulacion de accion sobre '{target_file or 'workspace'}'.",
        f"[BLAST RADIUS] Radio de impacto calculado: {blast_radius} archivo(s) dependiente(s).",
    ]
    if ghost_sim and ghost_sim.get("breaks_dependents"):
        target_sym = ghost_sim.get("target_symbol") or symbol or "unknown"
        breakages_count = len(ghost_sim.get("breakages", []))
        plan_steps.insert(
            0,
            f"[CRITICAL_VETO: CONTRACT_BREAKAGE] La simulacion en RAM detecto que modificar '{target_sym}' "
            f"rompe el contrato de {breakages_count} llamador(es) dependiente(s).",
        )
    if blast_report and blast_report.alert_required:
        plan_steps.append(blast_report.alert_text)
    elif snapshot_required:
        plan_steps.append(
            "[ALERT: HIGH_BLAST_RADIUS: SNAPSHOT REQUIRED] Se requiere snapshot previo en ozy_exploration para rollback seguro."
        )
    else:
        plan_steps.append(
            "[INFO: LOW_BLAST_RADIUS: SAFE TO EDIT] Cambio con impacto acotado en dependencias."
        )

    resume_node = payload.get("recommended_resume_node")
    resume_node_id = None
    if isinstance(resume_node, dict) and resume_node.get("id"):
        resume_node_id = resume_node.get("id")
        plan_steps.append(
            f"[RESUME RECOMMENDATION] En caso de fallo o poda, reanudar desde nodo '{resume_node_id}'."
        )

    risks = []
    if snapshot_required:
        risks.append(f"Riesgo de regresion en cascada sobre {blast_radius} dependientes.")
    if is_critical:
        risks.append("Modificacion directa sobre modulos criticos o contratos del sistema.")
    if blast_report and blast_report.alert_required:
        risks.append(
            f"Simbolo '{symbol}' referenciado por {blast_report.critical_module_hits} "
            "modulos criticos: riesgo de regresion arquitectural."
        )

    recommendations = [
        "Registrar nodo de exploracion con rollback_snapshot antes de aplicar diffs."
        if snapshot_required
        else "Aplicar cambios de manera incremental.",
        "Verificar suite de pruebas sobre componentes dependientes.",
    ]

    # Build semantic blast radius payload for structured_plan
    blast_radius_analysis: dict[str, Any] = {
        "total_blast_radius": blast_radius,
        "risk_tier": "critical" if (blast_radius > 6 or is_critical) else ("high" if blast_radius >= 3 else "low"),
        "high_severity_files": [i.get("file_path") for i in impact if isinstance(i, dict)][:5],
    }
    if blast_report:
        blast_radius_analysis["symbol_blast_radius"] = blast_report.to_dict()

    return BrainResponse(
        action="simulate_action",
        summary=f"Simulacion de accion: blast radius {blast_radius} | Snapshot {'REQUERIDO' if snapshot_required else 'OPCIONAL'}.",
        plan=plan_steps,
        risks=risks or ["Riesgo minimo bajo alcance actual."],
        recommendations=recommendations,
        memory_updates=[f"Blast radius: {blast_radius}", f"Snapshot required: {snapshot_required}"],
        confidence=0.88,
        structured_plan={
            "target_file": target_file,
            "blast_radius_analysis": blast_radius_analysis,
            "exploration_guidance": {
                "snapshot_required": snapshot_required,
                "snapshot_alert": "[ALERT: HIGH_BLAST_RADIUS: SNAPSHOT REQUIRED]" if snapshot_required else "[INFO: LOW_BLAST_RADIUS: SAFE TO EDIT]",
                "recommended_resume_node_id": resume_node_id,
                "backtracking_advice": f"Usar rollback_to_parent hacia '{resume_node_id}' si la prueba falla." if resume_node_id else "No hay nodo de rescate previo.",
            },
        },
    )


def _route_handler(payload: dict[str, Any]) -> BrainResponse:
    from ozy_brain.cognitive_router import route_cognitive_system

    decision = route_cognitive_system(payload)
    return BrainResponse(
        action="cognitive_route",
        summary=f"Cognitive Routing: {decision.tier.value} | Budget: {decision.latency_budget_ms}ms (Zero-Token: {decision.zero_token_guarantee}).",
        plan=[f"Route decision: {decision.tier.value} - {decision.reason}"],
        risks=[],
        recommendations=[
            "Dispatch immediately via System 1 Reflex (< 150ms)"
            if decision.zero_token_guarantee
            else "Escalate to System 2 Dialectic Committee for adversarial review."
        ],
        memory_updates=[f"Cognitive tier: {decision.tier.value}"],
        confidence=0.98,
        structured_plan=decision.to_dict(),
    )


def _dialectic_committee_handler(payload: dict[str, Any]) -> BrainResponse:
    supervisor = SupervisorAgent(project_path=payload.get("project"))
    return supervisor.deliberate_committee(payload)


def _dream_offline_cycle_handler(payload: dict[str, Any]) -> BrainResponse:
    from ozy_brain.dream.orchestrator import DreamOrchestrator

    workspace_root = payload.get("workspace_root") or payload.get("project") or "."
    db_path = payload.get("db_path")
    tenant_id = str(payload.get("tenant_id") or "local")
    chroma_dir = payload.get("chroma_dir")
    max_commits = int(payload.get("max_commits", 100))

    orchestrator = DreamOrchestrator(
        workspace_root=workspace_root,
        db_path=db_path,
        tenant_id=tenant_id,
        chroma_dir=chroma_dir,
    )
    res = orchestrator.run_dream_cycle(max_commits=max_commits)

    return BrainResponse(
        action="dream_offline_cycle",
        summary=f"Offline dream consolidation complete in {res.get('duration_ms', 0)}ms. Status: {res.get('status')}.",
        plan=res.get("top_insights", []),
        risks=[],
        recommendations=[
            f"Fragile files tracked: {len(res.get('amygdala_reflexes', {}).get('top_fragile_files', []))}",
            f"Procedural playbooks active: {len(res.get('basal_ganglia', {}).get('top_playbooks', []))}",
        ],
        memory_updates=[
            f"Orphaned files purged: {res.get('glial_homeostasis', {}).get('orphaned_deleted_files_purged', 0)}",
            f"Reflexes updated: {res.get('amygdala_reflexes', {}).get('reflexes_written', 0)}",
            f"Playbooks mined: {res.get('basal_ganglia', {}).get('sqlite_habits_upserted', 0)}",
        ],
        confidence=0.99,
        structured_plan=res,
    )


def _neuromodulator_handler(payload: dict[str, Any]) -> BrainResponse:
    from ozy_brain.neuromodulators import NeuromodulatorEngine

    workspace_root = payload.get("workspace_root") or payload.get("project") or "."
    engine = NeuromodulatorEngine(workspace_root=workspace_root)

    sub_action = payload.get("sub_action") or payload.get("event") or "get_state"
    if sub_action == "reward":
        reward = float(payload.get("reward", 1.0))
        expected = float(payload.get("expected", 0.5))
        res = engine.record_reward_event(reward=reward, expected_reward=expected)
    elif sub_action in ("pain", "stress", "error"):
        penalty = float(payload.get("penalty", 0.5))
        res = engine.record_pain_event(penalty=penalty, error_context=str(payload.get("error_context", "")))
    elif sub_action == "novelty":
        is_new = bool(payload.get("is_new_subsystem", False))
        visits = int(payload.get("visit_count", 0))
        res = engine.record_code_novelty(is_new_subsystem=is_new, visit_count=visits)
    elif sub_action == "reset":
        engine.reset_to_homeostasis()
        res = {"status": "reset_to_homeostasis"}
    else:
        res = {}

    hyperparams = engine.get_effective_hyperparameters()

    return BrainResponse(
        action="neuromodulators",
        summary=f"Neuromodulator balance: {hyperparams['badge']}",
        plan=[hyperparams["badge"]],
        risks=[
            "Noradrenaline high: Amygdala threshold lowered to sensitive state"
            if hyperparams["neurochemical_vector"]["noradrenaline"] >= 0.60
            else "Chemical baseline stable."
        ],
        recommendations=[
            "One-shot flashbulb consolidation active (Dopamine >= 0.75)"
            if hyperparams["flashbulb_consolidation"]
            else "Standard multi-hop memory consolidation."
        ],
        memory_updates=[
            f"D: {hyperparams['neurochemical_vector']['dopamine']:.2f}",
            f"NE: {hyperparams['neurochemical_vector']['noradrenaline']:.2f}",
            f"ACh: {hyperparams['neurochemical_vector']['acetylcholine']:.2f}",
            f"5HT: {hyperparams['neurochemical_vector']['serotonin']:.2f}",
        ],
        confidence=0.99,
        structured_plan={**hyperparams, "event_result": res},
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
    "dream_rsi": _dream_rsi_handler,
    "dream_offline_cycle": _dream_offline_cycle_handler,
    "offline_dreaming": _dream_offline_cycle_handler,
    "neuromodulators": _neuromodulator_handler,
    "neuro_state": _neuromodulator_handler,
    "simulate_action": _simulate_action_handler,
    "critique_hypothesis": _simulate_action_handler,
    "cognitive_route": _route_handler,
    "route": _route_handler,
    "dialectic_committee": _dialectic_committee_handler,
    "deliberate_committee": _dialectic_committee_handler,
}


def run(action: str, payload: dict[str, Any]) -> dict[str, Any]:
    handler = ACTIONS.get(action, plan)
    response = handler(payload).to_dict()
    response["engine"] = "ozy-brain-python"
    response["safe_mode"] = True
    return response

