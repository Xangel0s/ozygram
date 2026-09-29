"""
Cognitive Auto-Wiring and Contradiction Arbitration Engine for Memory Graph.
Performs offline semantic clustering, REINFORCES relationship discovery,
and RiskCritic contradiction arbitration (SUPERSEDES).
Zero-emoji compliant.
"""

from __future__ import annotations

import math
from typing import Any, Dict, List, Optional, Tuple


def _cosine_similarity(vec_a: List[float], vec_b: List[float]) -> float:
    """Calcula similitud coseno entre dos vectores numéricos."""
    if not vec_a or not vec_b or len(vec_a) != len(vec_b):
        return 0.0
    dot = sum(a * b for a, b in zip(vec_a, vec_b))
    norm_a = math.sqrt(sum(a * a for a in vec_a))
    norm_b = math.sqrt(sum(b * b for b in vec_b))
    if norm_a == 0.0 or norm_b == 0.0:
        return 0.0
    return max(0.0, min(1.0, dot / (norm_a * norm_b)))


def _jaccard_similarity(text_a: str, text_b: str) -> float:
    """Calcula similitud léxica Jaccard como fallback sin embeddings."""
    tokens_a = {w.lower() for w in text_a.split() if len(w) > 3}
    tokens_b = {w.lower() for w in text_b.split() if len(w) > 3}
    if not tokens_a or not tokens_b:
        return 0.0
    intersection = tokens_a.intersection(tokens_b)
    union = tokens_a.union(tokens_b)
    return len(intersection) / len(union) if union else 0.0


class RiskCriticArbitrator:
    """
    Arbitra contradicciones entre directivas o reglas de memoria.
    Evalúa timestamps, directivas obsoletas ('deprecated', 'no usar') y estado vivo.
    """

    CONTRADICTION_KEYWORDS = {
        "deprecated",
        "obsolete",
        "obsoleto",
        "no usar",
        "antiguo",
        "reemplazado",
        "legacy",
        "avoid",
    }

    def detect_conflict(self, mem_a: Dict[str, Any], mem_b: Dict[str, Any]) -> Tuple[bool, Optional[str]]:
        """Determina si dos memorias con alta similitud entran en conflicto."""
        text_a = f"{mem_a.get('title', '')} {mem_a.get('content', '')} {mem_a.get('solution', '')}".lower()
        text_b = f"{mem_b.get('title', '')} {mem_b.get('content', '')} {mem_b.get('solution', '')}".lower()

        a_has_obsolete = any(k in text_a for k in self.CONTRADICTION_KEYWORDS)
        b_has_obsolete = any(k in text_b for k in self.CONTRADICTION_KEYWORDS)

        if a_has_obsolete != b_has_obsolete:
            # Una de las dos declara la directiva como obsoleta
            superseding_id = mem_b["id"] if a_has_obsolete else mem_a["id"]
            superseded_id = mem_a["id"] if a_has_obsolete else mem_b["id"]
            return True, f"{superseding_id} SUPERSEDES {superseded_id}"

        # Conflicto temporal si ambas declaran directivas sobre el mismo símbolo con soluciones distintas
        sym_a = mem_a.get("symbol_name") or ""
        sym_b = mem_b.get("symbol_name") or ""
        if sym_a and sym_a == sym_b:
            sol_a = mem_a.get("solution", "").strip().lower()
            sol_b = mem_b.get("solution", "").strip().lower()
            if sol_a != sol_b and len(sol_a) > 5 and len(sol_b) > 5:
                # La memoria más reciente prevalece
                date_a = mem_a.get("created_at", "")
                date_b = mem_b.get("created_at", "")
                if date_b > date_a:
                    return True, f"{mem_b['id']} SUPERSEDES {mem_a['id']}"
                elif date_a > date_b:
                    return True, f"{mem_a['id']} SUPERSEDES {mem_b['id']}"

        return False, None


def auto_wire_memory_embeddings(
    memories: List[Dict[str, Any]],
    similarity_threshold: float = 0.80,
    arbitrator: Optional[RiskCriticArbitrator] = None,
) -> Dict[str, Any]:
    """
    Analiza una lista de memorias y produce las aristas recomendadas
    REINFORCES o SUPERSEDES.
    """
    if len(memories) < 2:
        return {
            "edges_created": 0,
            "reinforces_count": 0,
            "supersedes_count": 0,
            "edges": [],
            "details": ["[INFO] Menos de 2 memorias disponibles para auto-wiring."],
        }

    arbitrator = arbitrator or RiskCriticArbitrator()
    threshold = max(0.2, min(0.99, similarity_threshold))
    edges: List[Dict[str, Any]] = []
    details: List[str] = []
    reinforces_count = 0
    supersedes_count = 0

    for i in range(len(memories)):
        for j in range(i + 1, len(memories)):
            m1 = memories[i]
            m2 = memories[j]

            vec_a = m1.get("embedding")
            vec_b = m2.get("embedding")

            if vec_a and vec_b:
                sim = _cosine_similarity(vec_a, vec_b)
            else:
                text_a = f"{m1.get('title', '')} {m1.get('content', '')} {m1.get('solution', '')}"
                text_b = f"{m2.get('title', '')} {m2.get('content', '')} {m2.get('solution', '')}"
                sim = _jaccard_similarity(text_a, text_b)

            if sim >= threshold:
                is_conflict, resolution = arbitrator.detect_conflict(m1, m2)
                if is_conflict and resolution:
                    parts = resolution.split(" SUPERSEDES ")
                    src_id, tgt_id = parts[0], parts[1]
                    edges.append({
                        "source_id": src_id,
                        "target_id": tgt_id,
                        "edge_type": "supersedes",
                        "weight": 1.0,
                    })
                    supersedes_count += 1
                    details.append(f"[SUPERSEDES] {src_id} -> {tgt_id} (sim: {sim:.2f})")
                else:
                    edges.append({
                        "source_id": m1["id"],
                        "target_id": m2["id"],
                        "edge_type": "reinforces",
                        "weight": sim,
                    })
                    reinforces_count += 1
                    details.append(f"[REINFORCES] {m1['id']} <-> {m2['id']} (sim: {sim:.2f})")

    return {
        "edges_created": len(edges),
        "reinforces_count": reinforces_count,
        "supersedes_count": supersedes_count,
        "edges": edges,
        "details": details,
    }
