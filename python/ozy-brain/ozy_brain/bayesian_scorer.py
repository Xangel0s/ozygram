"""
Bayesian Evidence Scorer for Supervisor Agent & Dialectic Committee.
Replaces arbitrary confidence constants (0.95, 0.82, 0.50) with calibrated probabilistic
scoring based on blast radius, git churn, test coverage, and cerebellar dysmetria (D_KL).
"""

from __future__ import annotations

import math
from dataclasses import dataclass
from typing import Any, Dict, Optional


@dataclass
class BayesianConfidenceResult:
    """Represents a mathematically calibrated confidence score with credibility bounds."""
    probability_success: float
    confidence: float
    credible_interval_low: float
    credible_interval_high: float
    risk_level: str
    decision: str
    evidence_factors: Dict[str, float]

    def to_dict(self) -> Dict[str, Any]:
        return {
            "probability_success": round(self.probability_success, 4),
            "confidence": round(self.confidence, 4),
            "credible_interval": [
                round(self.credible_interval_low, 4),
                round(self.credible_interval_high, 4),
            ],
            "risk_level": self.risk_level,
            "decision": self.decision,
            "evidence_factors": {k: round(v, 4) for k, v in self.evidence_factors.items()},
        }


class BayesianEvidenceScorer:
    """
    Computes calibrated posterior confidence for code changes and architectural plans.
    Uses logistic evidence aggregation:
    logit(P) = w0 - w1*BlastRadius - w2*Churn + w3*Coverage - w4*D_KL
    """

    def __init__(
        self,
        w_intercept: float = 2.4,
        w_blast: float = 0.35,
        w_churn: float = 0.002,
        w_coverage: float = 1.8,
        w_dkl: float = 0.8,
    ):
        self.w_intercept = w_intercept
        self.w_blast = w_blast
        self.w_churn = w_churn
        self.w_coverage = w_coverage
        self.w_dkl = w_dkl

    def calculate_confidence(
        self,
        blast_radius: int = 1,
        churn_lines: int = 0,
        test_coverage: float = 0.80,
        d_kl_dysmetria: float = 0.0,
        has_critical_keywords: bool = False,
        is_blocked: bool = False,
    ) -> BayesianConfidenceResult:
        """
        Evaluates Bayesian evidence and returns calibrated confidence and decision.
        """
        if is_blocked:
            return BayesianConfidenceResult(
                probability_success=0.15,
                confidence=0.50,
                credible_interval_low=0.08,
                credible_interval_high=0.25,
                risk_level="CRITICAL",
                decision="REJECTED_CRITICAL_VETO",
                evidence_factors={"blocked_veto": 1.0},
            )

        # Safe defaults if None passed
        b_radius = int(blast_radius if blast_radius is not None else 1)
        c_lines = int(churn_lines if churn_lines is not None else 0)
        t_cov = float(test_coverage if test_coverage is not None else 0.80)
        d_kl = float(d_kl_dysmetria if d_kl_dysmetria is not None else 0.0)

        # Compute logit
        logit = self.w_intercept
        logit -= self.w_blast * min(15, b_radius)
        logit -= self.w_churn * min(1500, c_lines)
        logit += self.w_coverage * max(0.0, min(1.0, test_coverage))
        logit -= self.w_dkl * max(0.0, min(3.0, d_kl_dysmetria))

        if has_critical_keywords:
            logit -= 1.1

        # Sigmoid probability
        prob_success = 1.0 / (1.0 + math.exp(-max(-6.0, min(6.0, logit))))

        # Credibility interval (Laplace approximation around posterior)
        variance = prob_success * (1.0 - prob_success) / 10.0
        margin = 1.96 * math.sqrt(variance)
        ci_low = max(0.01, prob_success - margin)
        ci_high = min(0.99, prob_success + margin)

        # Risk Tier and Decision
        if prob_success >= 0.80:
            risk_level = "LOW"
            decision = "APPROVED"
            confidence = prob_success
        elif prob_success >= 0.55:
            risk_level = "MEDIUM"
            decision = "CONDITIONAL_APPROVAL"
            confidence = prob_success
        elif prob_success >= 0.35:
            risk_level = "HIGH"
            decision = "CONDITIONAL_APPROVAL_WITH_SAFEGUARDS"
            confidence = prob_success
        else:
            risk_level = "CRITICAL"
            decision = "VETOED"
            confidence = prob_success

        return BayesianConfidenceResult(
            probability_success=prob_success,
            confidence=confidence,
            credible_interval_low=ci_low,
            credible_interval_high=ci_high,
            risk_level=risk_level,
            decision=decision,
            evidence_factors={
                "blast_radius": float(b_radius),
                "churn_lines": float(c_lines),
                "test_coverage": float(test_coverage),
                "d_kl_dysmetria": float(d_kl_dysmetria),
                "has_critical_keywords": 1.0 if has_critical_keywords else 0.0,
            },
        )

