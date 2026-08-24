from __future__ import annotations

import json
from dataclasses import dataclass
from typing import Any

from ozy_brain.config import BrainConfig, LLMProvider, call_llm
from ozy_brain.data_engine import DataEngine


@dataclass
class RiskAuditResult:
    risk_level: str
    blocked: bool
    summary: str
    reasons: list[str]
    regression_vectors: list[str]
    hotspots_detected: list[dict[str, Any]]
    mitigations: list[str]
    critic_engine: str

    def to_dict(self) -> dict[str, Any]:
        return {
            "risk_level": self.risk_level,
            "blocked": self.blocked,
            "summary": self.summary,
            "reasons": self.reasons,
            "regression_vectors": self.regression_vectors,
            "hotspots_detected": self.hotspots_detected,
            "mitigations": self.mitigations,
            "critic_engine": self.critic_engine,
        }


class RiskCriticAgent:
    """Adversarial Critic Agent that challenges proposed changes, simulates regressions,

    and cross-checks architectural hotspots using DuckDB/Polars telemetry.
    """

    def __init__(self, project_path: str | None = None, config: BrainConfig | None = None):
        self.config = config or BrainConfig.load()
        self.data_engine = DataEngine(project_path=project_path)

    def audit(self, payload: dict[str, Any]) -> RiskAuditResult:
        files = payload.get("files") or payload.get("affected_files") or []
        diff_text = payload.get("diff") or payload.get("patch") or ""
        plan_steps = payload.get("plan") or []
        project = payload.get("project") or ""

        # 1. Telemetry & Hotspot extraction from DuckDB
        detected_hotspots: list[dict[str, Any]] = []
        for f in files:
            telemetry = self.data_engine.get_file_telemetry(str(f))
            if telemetry and telemetry.get("risk_level") in ("HIGH", "CRITICAL"):
                detected_hotspots.append(telemetry)

        # 2. Try LLM Adversarial Review if OpenRouter/Ollama is configured
        if self.config.provider != LLMProvider.OFFLINE_FALLBACK:
            llm_result = self._audit_with_llm(files, diff_text, plan_steps, detected_hotspots)
            if llm_result:
                return llm_result

        # 3. Deterministic Adversarial Heuristic Fallback ($0 cost)
        return self._audit_offline(files, diff_text, plan_steps, detected_hotspots)

    def _audit_with_llm(
        self,
        files: list[str],
        diff_text: str,
        plan_steps: list[str],
        hotspots: list[dict[str, Any]],
    ) -> RiskAuditResult | None:
        system_prompt = (
            "You are Ozygram's Adversarial Critic Agent. Your job is to rigorously audit code changes, "
            "simulate regression vectors, challenge design assumptions, and prevent bugs. "
            "Output valid JSON only with keys: risk_level (LOW, MEDIUM, HIGH, CRITICAL), "
            "blocked (boolean), summary (string), reasons (list of strings), "
            "regression_vectors (list of strings), mitigations (list of strings)."
        )
        user_prompt = f"""Audit the following proposed operation:
Files affected: {files}
Plan steps: {plan_steps}
Repository Hotspot Telemetry: {hotspots}
Diff/Context snippet:
{diff_text[:2000]}

Evaluate potential architectural breaks, concurrency hazards, data loss risks, or missing test coverage.
"""
        response_text = call_llm(user_prompt, system_prompt=system_prompt, config=self.config)
        if not response_text:
            return None

        try:
            # Extract JSON block if surrounded by markdown fences
            clean_json = response_text.strip()
            if "```json" in clean_json:
                clean_json = clean_json.split("```json", 1)[1].split("```", 1)[0].strip()
            elif "```" in clean_json:
                clean_json = clean_json.split("```", 1)[1].split("```", 1)[0].strip()

            parsed = json.loads(clean_json)
            return RiskAuditResult(
                risk_level=parsed.get("risk_level", "MEDIUM"),
                blocked=bool(parsed.get("blocked", False)),
                summary=parsed.get("summary", "Adversarial audit completed by LLM critic."),
                reasons=parsed.get("reasons", []),
                regression_vectors=parsed.get("regression_vectors", []),
                hotspots_detected=hotspots,
                mitigations=parsed.get("mitigations", []),
                critic_engine=f"{self.config.provider.value}:{self.config.model}",
            )
        except Exception:
            return None

    def _audit_offline(
        self,
        files: list[str],
        diff_text: str,
        plan_steps: list[str],
        hotspots: list[dict[str, Any]],
    ) -> RiskAuditResult:
        reasons: list[str] = []
        regression_vectors: list[str] = []
        mitigations: list[str] = []
        is_blocked = False

        # Hotspot risk checks
        if hotspots:
            reasons.append(
                f"Modifying {len(hotspots)} high-churn/critical hotspot files: "
                + ", ".join(h["file_path"] for h in hotspots[:3])
            )
            regression_vectors.append("Regression in historic bug-prone hotspot area.")
            mitigations.append("Run full regression tests before and after modifications on hotspot files.")

        # Large blast radius check
        if len(files) > 8:
            reasons.append(f"High blast radius: {len(files)} files modified concurrently.")
            regression_vectors.append("Unintended side-effects across distant modules.")
            mitigations.append("Split changes into smaller atomic task groups.")

        # SQL / Schema migration checks
        sql_keywords = ("DROP TABLE", "DELETE FROM", "ALTER TABLE", "TRUNCATE", "DROP COLUMN")
        if any(kw in diff_text.upper() for kw in sql_keywords):
            reasons.append("Destructive DDL/DML detected in diff.")
            regression_vectors.append("Irreversible schema or data loss in database.")
            mitigations.append("Verify non-destructive migration scripts with rollback safeguards.")
            is_blocked = True

        # Determine level
        if is_blocked:
            level = "CRITICAL"
        elif len(reasons) >= 2 or any(h.get("risk_level") == "CRITICAL" for h in hotspots):
            level = "HIGH"
        elif reasons:
            level = "MEDIUM"
        else:
            level = "LOW"
            reasons.append("No critical architectural regressions or hotspot conflicts detected.")

        summary = f"Adversarial risk audit completed. Verdict: {level} (Blocked: {is_blocked})."

        return RiskAuditResult(
            risk_level=level,
            blocked=is_blocked,
            summary=summary,
            reasons=reasons,
            regression_vectors=regression_vectors,
            hotspots_detected=hotspots,
            mitigations=mitigations,
            critic_engine="ozy-brain:offline-adversarial-critic",
        )


def audit_risk_with_critic(payload: dict[str, Any]) -> dict[str, Any]:
    """Convenience functional dispatcher for risk critic."""
    agent = RiskCriticAgent(project_path=payload.get("project"))
    return agent.audit(payload).to_dict()
