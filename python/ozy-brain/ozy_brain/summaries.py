from __future__ import annotations

from typing import Any

from ozy_brain.planner import _structured_plan
from ozy_brain.schemas import (
    BrainResponse,
    _base_risks,
    _base_summary,
    _brain_context_pack,
    _candidate_file_scores,
    _combined_memory_titles,
    _execution_policy,
    _goal,
    _items,
    _project,
    _safe_mcp_calls,
    _validation_commands,
)


def _build_mental_model_dict(payload: dict[str, Any]) -> dict[str, Any]:
    project = _project(payload)
    files = _items(payload, "files")
    graph = payload.get("graph_summary") if isinstance(payload.get("graph_summary"), dict) else {}
    memories = _combined_memory_titles(payload, limit=10)

    modules: list[str] = []
    for f in files:
        path = str(f).replace("\\", "/")
        parts = path.split("/")
        if len(parts) > 1 and parts[0] not in modules:
            modules.append(parts[0])

    where_to_look_first: list[str] = []
    candidate_scores = _candidate_file_scores(payload, limit=5)
    for cs in candidate_scores:
        where_to_look_first.append(f"{cs['path']} (score: {cs['score']})")

    return {
        "project": project,
        "purpose": f"High-performance memory, context, and reasoning architecture for {project}.",
        "architecture_type": "Hybrid Monorepo (Rust Core / MCP Server + Python Advisory Brain Worker)",
        "indexed_file_count": len(files),
        "graph_nodes": graph.get("nodes") or graph.get("files_count") or len(files),
        "core_modules": modules[:10] if modules else ["crates", "python"],
        "critical_data_flows": [
            "MCP Client stdio -> Rust ozymem-server dispatcher",
            "Rust GraphBackend (SQLite memory.db & registry.db)",
            "Python ozy_brain worker via JSON stdio payload/response",
        ],
        "user_rules": [
            "SOLID, DRY, KISS principles",
            "Commit after each completed task with descriptive messages",
            "Show code diff/preview before applying changes",
            "Maintain test coverage and Windows compatibility",
        ],
        "validation_playbook": _validation_commands(payload),
        "where_to_look_first": where_to_look_first or ["crates/ozymem-server/src/main.rs", "python/ozy-brain/ozy_brain/main.py"],
        "known_memories": memories[:5],
    }


def build_mental_model(payload: dict[str, Any]) -> BrainResponse:
    project = _project(payload)
    model = _build_mental_model_dict(payload)

    plan_steps = [
        "Synthesize project purpose, core architecture, and module ownership.",
        "Map critical data flows (MCP -> Rust -> SQLite -> Python worker).",
        "Identify validation playbook and candidate entrypoint files.",
        "Persist mental model seed to ozy_memory for fast agent context retrieval.",
    ]

    recommendations = [
        "Refresh the mental model after major refactors or adding new modules.",
        "Use entrypoint guidance ('Where to look first') when onboarding subagents.",
    ]

    return BrainResponse(
        action="build_mental_model",
        summary=f"Mental model for {project}: {model['architecture_type']}, {model['indexed_file_count']} files indexed, {len(model['core_modules'])} core modules.",
        plan=plan_steps,
        risks=_base_risks(payload),
        recommendations=recommendations,
        memory_updates=["Store the final mental model under a stable project topic key (e.g. ozy_memory action=save)."],
        confidence=0.86,
        suggested_mcp_calls=_safe_mcp_calls(payload),
        structured_plan=_structured_plan(payload, autonomy="mental_model_only"),
        execution_policy=_execution_policy(payload, autonomy="mental_model_only"),
        brain_context_pack=_brain_context_pack(payload),
        mental_model=model,
    )


def summarize_project(payload: dict[str, Any]) -> BrainResponse:
    graph = payload.get("graph_summary") if isinstance(payload.get("graph_summary"), dict) else {}
    summary = _base_summary("project summary", payload)
    if graph:
        summary += " Graph: " + ", ".join(f"{k}={v}" for k, v in graph.items() if isinstance(v, (str, int, float)))[:300]
    return BrainResponse(
        action="summarize_project",
        summary=summary,
        plan=["Maintain a living project map: purpose, modules, risks, conventions, validation commands."],
        risks=_base_risks(payload),
        recommendations=["Rebuild this summary after major architecture changes."],
        memory_updates=["Save stable module rules and project conventions."],
        confidence=0.78,
    )


def compress_session(payload: dict[str, Any]) -> BrainResponse:
    return BrainResponse(
        action="compress_session",
        summary=_base_summary("session compression", payload),
        plan=[
            "Goal: summarize the intended outcome.",
            "Discoveries: keep non-obvious technical findings.",
            "Accomplished: list completed changes and validation evidence.",
            "Next Steps: preserve unresolved work.",
            "Relevant Files: include only files actually touched or verified.",
        ],
        risks=["Avoid losing validation evidence during compaction."],
        recommendations=["Persist the compressed summary as a project observation/session summary."],
        memory_updates=["Save compaction summaries before context resets."],
        confidence=0.84,
    )
