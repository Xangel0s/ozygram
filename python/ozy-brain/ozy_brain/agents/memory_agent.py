from __future__ import annotations

import math
from dataclasses import dataclass
from datetime import datetime, timezone
from typing import Any


@dataclass
class ConsolidatedLesson:
    topic: str
    summary: str
    source_count: int
    confidence: float
    decay_score: float
    is_stale: bool
    affected_files: list[str]

    def to_dict(self) -> dict[str, Any]:
        return {
            "topic": self.topic,
            "summary": self.summary,
            "source_count": self.source_count,
            "confidence": round(self.confidence, 2),
            "decay_score": round(self.decay_score, 2),
            "is_stale": self.is_stale,
            "affected_files": self.affected_files,
        }


class MemoryConsolidationAgent:
    """Agent responsible for memory synthesis, redundancy clustering,

    and temporal decay calculations on project lessons/engrams.
    """

    def __init__(self, half_life_days: float = 60.0):
        self.half_life_days = half_life_days

    def calculate_decay(self, timestamp_str: str | None, base_confidence: float = 1.0) -> float:
        """Calculates exponential temporal decay score: S = C * exp(-lambda * delta_days)."""
        if not timestamp_str:
            return base_confidence

        try:
            # Parse ISO or Unix timestamp
            if timestamp_str.isdigit():
                dt = datetime.fromtimestamp(int(timestamp_str), tz=timezone.utc)
            else:
                dt = datetime.fromisoformat(timestamp_str.replace("Z", "+00:00"))
            
            now = datetime.now(timezone.utc)
            delta_days = max(0.0, (now - dt).total_seconds() / 86400.0)
            decay_constant = math.log(2) / self.half_life_days
            return float(base_confidence * math.exp(-decay_constant * delta_days))
        except Exception:
            return base_confidence

    def consolidate(self, payload: dict[str, Any]) -> dict[str, Any]:
        raw_lessons = payload.get("lessons") or payload.get("memories") or []
        if not raw_lessons:
            return {
                "status": "empty",
                "consolidated": [],
                "stale_candidates": [],
                "total_processed": 0,
            }

        # Cluster by category / module / keyword
        clusters: dict[str, list[dict[str, Any]]] = {}
        for item in raw_lessons:
            if isinstance(item, str):
                key = "general"
                entry = {"summary": item, "topic": "general", "timestamp": None, "confidence": 1.0}
            elif isinstance(item, dict):
                key = item.get("topic") or item.get("category") or item.get("module") or "general"
                entry = item
            else:
                continue

            clusters.setdefault(key, []).append(entry)

        consolidated_results: list[ConsolidatedLesson] = []
        stale_candidates: list[dict[str, Any]] = []

        for topic, entries in clusters.items():
            files_set: set[str] = set()
            summaries: list[str] = []
            decay_scores: list[float] = []

            for e in entries:
                text = e.get("summary") or e.get("content") or str(e)
                summaries.append(text.strip())
                f = e.get("file") or e.get("file_path")
                if f:
                    files_set.add(f)

                decay = self.calculate_decay(
                    e.get("timestamp") or e.get("created_at"),
                    float(e.get("confidence", 1.0)),
                )
                decay_scores.append(decay)
                if decay < 0.35:
                    stale_candidates.append({
                        "topic": topic,
                        "summary": text[:100],
                        "decay_score": round(decay, 2),
                    })

            avg_decay = sum(decay_scores) / len(decay_scores) if decay_scores else 1.0
            is_stale = avg_decay < 0.4

            # Synthesize combined summary
            merged_summary = " | ".join(sorted(set(summaries))[:5])

            consolidated_results.append(ConsolidatedLesson(
                topic=topic,
                summary=merged_summary,
                source_count=len(entries),
                confidence=min(1.0, 0.7 + (0.1 * min(len(entries), 3))),
                decay_score=avg_decay,
                is_stale=is_stale,
                affected_files=sorted(files_set),
            ))

        return {
            "status": "success",
            "total_processed": len(raw_lessons),
            "clusters_count": len(consolidated_results),
            "consolidated": [c.to_dict() for c in consolidated_results],
            "stale_candidates": stale_candidates,
        }


def consolidate_and_decay_memories(payload: dict[str, Any]) -> dict[str, Any]:
    agent = MemoryConsolidationAgent()
    return agent.consolidate(payload)
