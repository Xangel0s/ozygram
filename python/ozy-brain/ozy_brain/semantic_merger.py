"""
Distributed Semantic Merger & Deduplication Engine for OzyBrain.
Calculates semantic similarity between incoming peer heuristics and local catalog.
If similarity cos >= 0.88:
  - Fuses complementary rules without string duplication.
  - Increments peer_confirmations.
  - Boosts synaptic confidence weight: w_new = min(1.0, w_old + 0.08).
Zero-emoji compliant.
"""

from __future__ import annotations

import math
import re
from collections import Counter
from typing import Any, Dict, List, Optional, Tuple


def tokenize_text(text: str) -> List[str]:
    """Extracts lower-case alpha-numeric word tokens."""
    return [t for t in re.findall(r"[a-z0-9_]+", text.lower()) if len(t) > 1]


def compute_token_cosine_similarity(text_a: str, text_b: str) -> float:
    """Computes cosine similarity between token frequency vectors of two texts."""
    tokens_a = tokenize_text(text_a)
    tokens_b = tokenize_text(text_b)

    if not tokens_a or not tokens_b:
        return 0.0

    counts_a = Counter(tokens_a)
    counts_b = Counter(tokens_b)

    all_tokens = set(counts_a.keys()) | set(counts_b.keys())
    dot_product = sum(counts_a[t] * counts_b[t] for t in all_tokens)
    norm_a = math.sqrt(sum(c * c for c in counts_a.values()))
    norm_b = math.sqrt(sum(c * c for c in counts_b.values()))

    if norm_a == 0.0 or norm_b == 0.0:
        return 0.0

    return float(dot_product / (norm_a * norm_b))


class SemanticHeuristicMerger:
    """Merges incoming peer heuristics into local catalog with semantic deduplication."""

    SIMILARITY_THRESHOLD = 0.88
    CONFIDENCE_BOOST_STEP = 0.08

    def __init__(self, similarity_threshold: float = SIMILARITY_THRESHOLD):
        self.similarity_threshold = similarity_threshold

    @staticmethod
    def _heuristic_representation(pattern: Dict[str, Any]) -> str:
        """Constructs full text representation of a heuristic pattern for similarity computation."""
        parts = [
            str(pattern.get("title", "")),
            str(pattern.get("description", "")),
            " ".join(str(r) for r in pattern.get("abstract_rules", [])),
        ]
        return " ".join(parts).strip()

    def find_most_similar_pattern(
        self,
        incoming: Dict[str, Any],
        candidates: List[Dict[str, Any]],
    ) -> Tuple[Optional[Dict[str, Any]], float]:
        """Finds candidate with highest semantic similarity in the same category."""
        target_cat = incoming.get("category", "")
        incoming_rep = self._heuristic_representation(incoming)
        if not incoming_rep:
            return None, 0.0

        best_match: Optional[Dict[str, Any]] = None
        best_sim = 0.0

        for cand in candidates:
            # Prefer matching within same or compatible category
            if cand.get("category") and target_cat and cand.get("category") != target_cat:
                continue

            cand_rep = self._heuristic_representation(cand)
            sim = compute_token_cosine_similarity(incoming_rep, cand_rep)
            if sim > best_sim:
                best_sim = sim
                best_match = cand

        return best_match, best_sim

    def fuse_duplicate_patterns(
        self,
        canonical: Dict[str, Any],
        incoming: Dict[str, Any],
    ) -> Dict[str, Any]:
        """Fuses complementary rules from incoming pattern into canonical pattern."""
        from ozy_brain.bbb_immune import ImmunologicalBBB

        fused = dict(canonical)
        existing_rules = set(fused.get("abstract_rules", []))
        incoming_rules = incoming.get("abstract_rules", [])

        # Add non-duplicate rules
        for rule in incoming_rules:
            rule_str = str(rule).strip()
            if not rule_str:
                continue
            is_dup = any(
                compute_token_cosine_similarity(rule_str, str(er)) >= 0.80
                for er in existing_rules
            )
            if not is_dup:
                existing_rules.add(rule_str)

        fused["abstract_rules"] = sorted(list(existing_rules))

        # Bump peer confirmations and synaptic confidence
        current_confirmations = fused.get("peer_confirmations", 0)
        fused["peer_confirmations"] = current_confirmations + 1 + incoming.get("peer_confirmations", 0)

        current_conf = float(fused.get("confidence", 0.70))
        fused["confidence"] = min(1.0, round(current_conf + self.CONFIDENCE_BOOST_STEP, 3))

        # Re-sign the fused canonical pattern
        fused["signature_sha256"] = ImmunologicalBBB.compute_pattern_signature(fused)

        return fused
