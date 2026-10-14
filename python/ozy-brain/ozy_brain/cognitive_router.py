from __future__ import annotations

from dataclasses import dataclass
from enum import Enum
from typing import Any


class CognitiveSystemTier(str, Enum):
    SYSTEM_1_REFLEX = "SYSTEM_1_REFLEX"
    SYSTEM_2_DELIBERATIVE = "SYSTEM_2_DELIBERATIVE"


@dataclass
class CognitiveRouteDecision:
    tier: CognitiveSystemTier
    reason: str
    latency_budget_ms: int
    zero_token_guarantee: bool
    requires_committee: bool

    def to_dict(self) -> dict[str, Any]:
        return {
            "tier": self.tier.value,
            "reason": self.reason,
            "latency_budget_ms": self.latency_budget_ms,
            "zero_token_guarantee": self.zero_token_guarantee,
            "requires_committee": self.requires_committee,
        }


# Keywords that immediately trigger System 2 deliberative committee
_SYSTEM_2_CRITICAL_KEYWORDS: tuple[str, ...] = (
    "schema",
    "migration",
    "auth",
    "database",
    "core",
    "token",
    "secret",
    "drop table",
    "alter table",
    "truncate",
    "delete from",
)


def route_cognitive_system(payload: dict[str, Any]) -> CognitiveRouteDecision:
    """Classifies incoming requests into System 1 (Reflex) or System 2 (Deliberative).

    System 1 (Reflex, < 150ms, 0 extra tokens):
        - Mechanical tasks (formatting, imports, single-function AST extraction).
        - Low blast radius (<= 2 non-critical files).
        - Clean AST diagnostics and no contract breakage.

    System 2 (Deliberative Committee, Multi-Agent Dialectic):
        - High blast radius (>= 3 files or high-severity dependents).
        - Sensitive modules (schema, database migrations, auth, tokens).
        - Speculative contract breakage detected in ghost patch simulation.
        - Destructive SQL DDL/DML.
    """
    # Explicit override in payload
    mode = str(payload.get("mode") or "").lower()
    system_override = payload.get("system")
    if mode == "reflex" or system_override == 1 or system_override == "system_1":
        return CognitiveRouteDecision(
            tier=CognitiveSystemTier.SYSTEM_1_REFLEX,
            reason="Explicit System 1 reflex override requested.",
            latency_budget_ms=150,
            zero_token_guarantee=True,
            requires_committee=False,
        )
    if mode == "deliberative" or system_override == 2 or system_override == "system_2":
        return CognitiveRouteDecision(
            tier=CognitiveSystemTier.SYSTEM_2_DELIBERATIVE,
            reason="Explicit System 2 deliberative override requested.",
            latency_budget_ms=3000,
            zero_token_guarantee=False,
            requires_committee=True,
        )

    # 1. Speculative Ghost Patch check
    ghost_sim = payload.get("simulation_result") or payload.get("ghost_patch_report") or {}
    if ghost_sim and ghost_sim.get("breaks_dependents"):
        return CognitiveRouteDecision(
            tier=CognitiveSystemTier.SYSTEM_2_DELIBERATIVE,
            reason="Speculative ghost patch broke dependent callers; requires dialectic review.",
            latency_budget_ms=2500,
            zero_token_guarantee=False,
            requires_committee=True,
        )

    # 2. File scope & Blast radius
    files = payload.get("files") or payload.get("affected_files") or []
    impact = payload.get("impact") or payload.get("graph_impact") or []
    blast_radius = int(payload.get("blast_radius") or (len(files) + len(impact)))

    if blast_radius >= 3:
        return CognitiveRouteDecision(
            tier=CognitiveSystemTier.SYSTEM_2_DELIBERATIVE,
            reason=f"High blast radius ({blast_radius} files touched/impacted); requires dialectic review.",
            latency_budget_ms=3000,
            zero_token_guarantee=False,
            requires_committee=True,
        )

    # 3. Critical module paths & SQL keywords
    target_file = str(payload.get("file_path") or payload.get("file") or "").lower()
    diff_text = str(payload.get("diff") or payload.get("patch") or "").lower()

    if any(kw in target_file for kw in _SYSTEM_2_CRITICAL_KEYWORDS):
        return CognitiveRouteDecision(
            tier=CognitiveSystemTier.SYSTEM_2_DELIBERATIVE,
            reason=f"Target file '{target_file}' touches critical architectural core/schema.",
            latency_budget_ms=2500,
            zero_token_guarantee=False,
            requires_committee=True,
        )

    if any(kw in diff_text for kw in ("drop table", "alter table", "truncate", "delete from", "drop column")):
        return CognitiveRouteDecision(
            tier=CognitiveSystemTier.SYSTEM_2_DELIBERATIVE,
            reason="Potentially destructive SQL statements detected in proposed diff.",
            latency_budget_ms=2000,
            zero_token_guarantee=False,
            requires_committee=True,
        )

    # Otherwise, mechanical change: System 1 fast reflex
    return CognitiveRouteDecision(
        tier=CognitiveSystemTier.SYSTEM_1_REFLEX,
        reason=f"Low blast radius ({blast_radius} files) and non-critical module; resolved via System 1 fast reflex.",
        latency_budget_ms=150,
        zero_token_guarantee=True,
        requires_committee=False,
    )
