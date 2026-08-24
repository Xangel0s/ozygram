from __future__ import annotations

from typing import Any

from ozy_brain.agents.memory_agent import MemoryConsolidationAgent
from ozy_brain.agents.risk_critic import RiskCriticAgent
from ozy_brain.config import BrainConfig, LLMProvider, call_llm
from ozy_brain.data_engine import DataEngine
from ozy_brain.schemas import BrainResponse


class SupervisorAgent:
    """Supervisor Agent that routes high-level cognitive requests, coordinates

    the Risk Critic, Memory Consolidator, and Analytical Data Engine.
    """

    def __init__(self, project_path: str | None = None, config: BrainConfig | None = None):
        self.config = config or BrainConfig.load()
        self.project_path = project_path
        self.critic = RiskCriticAgent(project_path=project_path, config=self.config)
        self.memory_agent = MemoryConsolidationAgent()
        self.data_engine = DataEngine(project_path=project_path)

    def audit_with_critic(self, payload: dict[str, Any]) -> BrainResponse:
        audit_res = self.critic.audit(payload)
        return BrainResponse(
            action="audit_changes_with_critic",
            summary=audit_res.summary,
            plan=[f"Apply mitigation: {m}" for m in audit_res.mitigations],
            risks=audit_res.reasons,
            recommendations=audit_res.mitigations,
            memory_updates=[f"Risk level: {audit_res.risk_level} (Blocked: {audit_res.blocked})"],
            confidence=0.92,
            risk_assessment=audit_res.to_dict(),
        )

    def get_repository_hotspots(self, payload: dict[str, Any]) -> BrainResponse:
        limit = int(payload.get("limit", 10))
        hotspots = self.data_engine.get_top_hotspots(limit=limit)
        critical_count = sum(1 for h in hotspots if h.get("risk_level") in ("HIGH", "CRITICAL"))

        summary = (
            f"Repository analytics: identified {len(hotspots)} hotspot files "
            f"({critical_count} critical/high churn)."
        )
        plan = [
            f"Prioritize testing and refactoring for top hotspot: {hotspots[0]['file_path']}"
            if hotspots
            else "Repository is uniform with low churn."
        ]
        risks = [
            f"Hotspot file {h['file_path']} has {h['churn_score']} lines churned across {h['commit_count']} commits"
            for h in hotspots[:3]
        ]
        recs = [
            "Break down large modules with high churn into cohesive submodules.",
            "Add automated integration tests for critical hotspot files.",
        ]

        return BrainResponse(
            action="get_repository_hotspots",
            summary=summary,
            plan=plan,
            risks=risks,
            recommendations=recs,
            memory_updates=[f"Top hotspot: {hotspots[0]['file_path']}" if hotspots else "No hotspots"],
            confidence=0.95,
            structured_plan={"hotspots": hotspots},
        )

    def consolidate_memory(self, payload: dict[str, Any]) -> BrainResponse:
        result = self.memory_agent.consolidate(payload)
        summary = (
            f"Memory consolidation: synthesized {result.get('total_processed', 0)} engrams "
            f"into {result.get('clusters_count', 0)} clusters. "
            f"Found {len(result.get('stale_candidates', []))} stale candidates."
        )
        plan = [
            "Prune stale memories with high temporal decay score.",
            "Persist consolidated cluster rules to active memory.",
        ]
        risks = ["Loss of niche edge-case context if over-pruned."]
        recs = ["Review stale candidate memories before deletion."]

        return BrainResponse(
            action="consolidate_memory",
            summary=summary,
            plan=plan,
            risks=risks,
            recommendations=recs,
            memory_updates=[f"Synthesized {result.get('clusters_count', 0)} topic clusters"],
            confidence=0.90,
            structured_plan=result,
        )
