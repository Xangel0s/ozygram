from __future__ import annotations

import json
from dataclasses import dataclass, field
from typing import Any

from ozy_brain.config import BrainConfig, LLMProvider, call_llm
from ozy_brain.data_engine import DataEngine

# Keywords that identify modules as architecturally critical.
# A change touching a symbol imported by 5+ of these modules triggers
# an obligatory [ALERT: HIGH_BLAST_RADIUS: SNAPSHOT REQUIRED].
_CRITICAL_MODULE_KEYWORDS: tuple[str, ...] = (
    "schema",
    "auth",
    "migration",
    "core",
    "token",
    "secret",
    "config",
    "backend",
    "database",
    "registry",
    "api",
    "router",
    "middleware",
    "gateway",
)

# Threshold: references from N or more critical modules → mandatory snapshot.
_HIGH_BLAST_RADIUS_THRESHOLD: int = 5


@dataclass
class BlastRadiusReport:
    """Structured result of blast radius analysis for a single symbol change."""

    symbol: str
    total_references: int
    critical_module_hits: int
    critical_modules: list[str] = field(default_factory=list)
    alert_required: bool = False
    alert_text: str = ""

    def to_dict(self) -> dict[str, Any]:
        return {
            "symbol": self.symbol,
            "total_references": self.total_references,
            "critical_module_hits": self.critical_module_hits,
            "critical_modules": self.critical_modules,
            "alert_required": self.alert_required,
            "alert_text": self.alert_text,
        }


def compute_blast_radius(symbol: str, impact_references: list[dict[str, Any]]) -> BlastRadiusReport:
    """Analyse the blast radius of a symbol change from `ozy_find_references` output.

    Args:
        symbol: Name of the symbol being modified.
        impact_references: List of reference dicts produced by `ozy_find_references`.
            Each dict must contain at least a ``file_path`` key.

    Returns:
        BlastRadiusReport with alert flag set when critical module threshold is exceeded.
    """
    critical_modules: list[str] = []
    seen: set[str] = set()

    for ref in impact_references:
        file_path = str(ref.get("file_path") or ref.get("file") or "").lower()
        if not file_path or file_path in seen:
            continue
        seen.add(file_path)
        if any(kw in file_path for kw in _CRITICAL_MODULE_KEYWORDS):
            critical_modules.append(file_path)

    total = len(seen)
    hits = len(critical_modules)
    alert_required = hits >= _HIGH_BLAST_RADIUS_THRESHOLD
    alert_text = (
        "[ALERT: HIGH_BLAST_RADIUS: SNAPSHOT REQUIRED] "
        f"Symbol '{symbol}' is referenced by {hits} critical modules "
        f"(threshold: {_HIGH_BLAST_RADIUS_THRESHOLD}). "
        "Create an ozy_exploration snapshot before applying changes."
        if alert_required
        else ""
    )
    return BlastRadiusReport(
        symbol=symbol,
        total_references=total,
        critical_module_hits=hits,
        critical_modules=critical_modules[:10],
        alert_required=alert_required,
        alert_text=alert_text,
    )


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
    blast_radius: BlastRadiusReport | None = None

    def to_dict(self) -> dict[str, Any]:
        result: dict[str, Any] = {
            "risk_level": self.risk_level,
            "blocked": self.blocked,
            "summary": self.summary,
            "reasons": self.reasons,
            "regression_vectors": self.regression_vectors,
            "hotspots_detected": self.hotspots_detected,
            "mitigations": self.mitigations,
            "critic_engine": self.critic_engine,
        }
        if self.blast_radius is not None:
            result["blast_radius"] = self.blast_radius.to_dict()
        return result


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

        # Blast Radius: extract symbol references from ozy_find_references output.
        symbol = str(payload.get("symbol") or payload.get("target_symbol") or "")
        impact_references: list[dict[str, Any]] = list(
            payload.get("impact_references") or payload.get("references") or []
        )
        blast_report = compute_blast_radius(symbol, impact_references) if symbol or impact_references else None

        # 1. Telemetry & Hotspot extraction from DuckDB
        detected_hotspots: list[dict[str, Any]] = []
        for f in files:
            telemetry = self.data_engine.get_file_telemetry(str(f))
            if telemetry and telemetry.get("risk_level") in ("HIGH", "CRITICAL"):
                detected_hotspots.append(telemetry)

        # 2. Try LLM Adversarial Review if OpenRouter/Ollama is configured
        if self.config.provider != LLMProvider.OFFLINE_FALLBACK:
            llm_result = self._audit_with_llm(
                files, diff_text, plan_steps, detected_hotspots, blast_report=blast_report
            )
            if llm_result:
                return llm_result

        # 3. Deterministic Adversarial Heuristic Fallback ($0 cost)
        return self._audit_offline(files, diff_text, plan_steps, detected_hotspots, blast_report=blast_report)

    def _audit_with_llm(
        self,
        files: list[str],
        diff_text: str,
        plan_steps: list[str],
        hotspots: list[dict[str, Any]],
        blast_report: BlastRadiusReport | None = None,
    ) -> RiskAuditResult | None:
        blast_context = ""
        if blast_report and blast_report.alert_required:
            blast_context = (
                f"\n[ALERT: HIGH_BLAST_RADIUS] Symbol '{blast_report.symbol}' has "
                f"{blast_report.critical_module_hits} critical module references "
                f"(threshold: {_HIGH_BLAST_RADIUS_THRESHOLD}). "
                "A snapshot is mandatory before changes."
            )

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
Repository Hotspot Telemetry: {hotspots}{blast_context}
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
                blast_radius=blast_report,
            )
        except Exception:
            return None

    def _audit_offline(
        self,
        files: list[str],
        diff_text: str,
        plan_steps: list[str],
        hotspots: list[dict[str, Any]],
        blast_report: BlastRadiusReport | None = None,
    ) -> RiskAuditResult:
        reasons: list[str] = []
        regression_vectors: list[str] = []
        mitigations: list[str] = []
        is_blocked = False

        # --- Blast Radius check (primary new gate) ---
        if blast_report and blast_report.alert_required:
            reasons.append(blast_report.alert_text)
            regression_vectors.append(
                f"Symbol '{blast_report.symbol}' imported by {blast_report.critical_module_hits} "
                "critical modules — cascading regression risk across architecture."
            )
            mitigations.append(
                "Execute ozy_exploration snapshot (rollback_snapshot) before applying the change. "
                "Run integration test suite on all referencing modules after the edit."
            )
            is_blocked = True

        # Hotspot risk checks
        if hotspots:
            reasons.append(
                f"Modifying {len(hotspots)} high-churn/critical hotspot files: "
                + ", ".join(h["file_path"] for h in hotspots[:3])
            )
            regression_vectors.append("Regression in historic bug-prone hotspot area.")
            mitigations.append("Run full regression tests before and after modifications on hotspot files.")

        # Large file-count blast radius check
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
            blast_radius=blast_report,
        )


def audit_risk_with_critic(payload: dict[str, Any]) -> dict[str, Any]:
    """Convenience functional dispatcher for risk critic."""
    agent = RiskCriticAgent(project_path=payload.get("project"))
    return agent.audit(payload).to_dict()
