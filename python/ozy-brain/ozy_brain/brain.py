from __future__ import annotations

from typing import Any, Callable

from ozy_brain.memory import rank_memories, recall_deep
from ozy_brain.patterns import detect_patterns, suggest_next_steps
from ozy_brain.planner import plan
from ozy_brain.reflector import reflect
from ozy_brain.risk import risk_review
from ozy_brain.schemas import BrainResponse
from ozy_brain.summaries import build_mental_model, compress_session, summarize_project

ACTIONS: dict[str, Callable[[dict[str, Any]], BrainResponse]] = {
    "plan": plan,
    "reflect": reflect,
    "analyze_failure": reflect,
    "recall_deep": recall_deep,
    "rank_memories": rank_memories,
    "risk_review": risk_review,
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
