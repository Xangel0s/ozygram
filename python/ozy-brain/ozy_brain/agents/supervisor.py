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

    def deliberate_committee(self, payload: dict[str, Any]) -> BrainResponse:
        """3-Agent Dialectic Committee:
        1. Implementer Agent: Formulates structural plan & topological refactor sequence.
        2. Devil's Advocate Critic: Challenges assumptions, audits blast radius, regressions & contract breaks.
        3. Executive Judge: Evaluates arguments, applies vetoes or conditional approvals, synthesizing a hardened plan.
        """
        from ozy_brain.planner import plan as implementer_plan

        # Agent 1: Implementer Proposal
        imp_resp = implementer_plan(payload)

        # Agent 2: Risk Critic Audit
        critic_res = self.critic.audit(payload)

        # Agent 3: Executive Judge Evaluation (Calibrated Bayesian Evidence)
        from ozy_brain.bayesian_scorer import BayesianEvidenceScorer
        scorer = BayesianEvidenceScorer()
        b_res = scorer.calculate_confidence(
            blast_radius=getattr(critic_res, "blast_radius", 1),
            churn_lines=int(payload.get("churn_lines", 0)),
            test_coverage=float(payload.get("test_coverage", 0.80)),
            d_kl_dysmetria=float(payload.get("d_kl_dysmetria", 0.0)),
            has_critical_keywords=any(
                kw in str(critic_res.reasons).lower()
                for kw in ["schema", "auth", "secret", "core", "migration", "token"]
            ),
            is_blocked=critic_res.blocked,
        )

        if critic_res.blocked or b_res.decision in ("REJECTED_CRITICAL_VETO", "VETOED"):
            verdict = f"[DIALECTIC VERDICT: REJECTED: CRITICAL_VETO] (Confidence: {b_res.confidence:.2%})"
            approved = False
        elif b_res.risk_level in ("HIGH", "CRITICAL"):
            verdict = f"[DIALECTIC VERDICT: CONDITIONAL_APPROVAL] (Confidence: {b_res.confidence:.2%})"
            approved = True
        else:
            verdict = f"[DIALECTIC VERDICT: APPROVED] (Confidence: {b_res.confidence:.2%})"
            approved = True
        confidence = b_res.confidence

        combined_plan = [
            f"[DIALECTIC COMMITTEE] Executive Judge Verdict: {verdict}",
            f"[IMPLEMENTER PROPOSAL] Goal: {imp_resp.summary}",
        ]
        if critic_res.blocked:
            combined_plan.append("[CRITIC VETO] Proposed change was vetoed due to critical regression risks:")
            for reason in critic_res.reasons:
                combined_plan.append(f"  • {reason}")
            combined_plan.append("[EXECUTIVE JUDGE DIRECTIVE] Execution halted. Apply required mitigations before re-attempting.")
        else:
            combined_plan.extend(imp_resp.plan)
            if critic_res.mitigations:
                combined_plan.append("[CRITIC REQUIRED SAFEGUARDS]")
                for m in critic_res.mitigations:
                    combined_plan.append(f"  • {m}")

        committee_summary = (
            f"Dialectic Committee Review: {verdict} | Risk: {critic_res.risk_level} | "
            f"Approved: {approved}."
        )

        return BrainResponse(
            action="dialectic_committee",
            summary=committee_summary,
            plan=combined_plan,
            risks=critic_res.reasons + imp_resp.risks,
            recommendations=critic_res.mitigations + imp_resp.recommendations,
            memory_updates=[
                f"Committee verdict: {verdict}",
                f"Risk tier: {critic_res.risk_level}",
            ],
            confidence=confidence,
            structured_plan={
                "verdict": verdict,
                "approved": approved,
                "implementer_plan": imp_resp.structured_plan,
                "critic_audit": critic_res.to_dict(),
                "bayesian_evidence": b_res.to_dict(),
            },
            risk_assessment=critic_res.to_dict(),
        )
