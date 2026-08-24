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


def _audit_critic_handler(payload: dict[str, Any]) -> BrainResponse:
    supervisor = SupervisorAgent(project_path=payload.get("project"))
    return supervisor.audit_with_critic(payload)


def _hotspots_handler(payload: dict[str, Any]) -> BrainResponse:
    supervisor = SupervisorAgent(project_path=payload.get("project"))
    return supervisor.get_repository_hotspots(payload)


def _consolidate_memory_handler(payload: dict[str, Any]) -> BrainResponse:
    supervisor = SupervisorAgent(project_path=payload.get("project"))
    return supervisor.consolidate_memory(payload)


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
}


def run(action: str, payload: dict[str, Any]) -> dict[str, Any]:
    handler = ACTIONS.get(action, plan)
    response = handler(payload).to_dict()
    response["engine"] = "ozy-brain-python"
    response["safe_mode"] = True
    return response

