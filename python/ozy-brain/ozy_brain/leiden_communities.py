"""
GraphRAG & Leiden/Louvain Community Detection for OzyBrain.
Partitions code dependency graphs (memory_edges & STDP synapses) into cohesive
architectural domains (e.g. Domain Billing, Auth & Security, Presentation UI).
Warns the planner when candidate changes cross domain boundaries (Cross-Domain Blast Radius).
"""

from __future__ import annotations

import collections
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Dict, List, Set, Tuple


@dataclass
class CodeCommunity:
    """Represents a macro-architectural domain or cluster of files."""
    community_id: int
    label: str
    files: List[str]
    internal_edge_weight: float = 0.0
    external_edge_weight: float = 0.0

    def to_dict(self) -> Dict[str, Any]:
        return {
            "community_id": self.community_id,
            "label": self.label,
            "files_count": len(self.files),
            "files": self.files,
            "internal_edge_weight": round(self.internal_edge_weight, 4),
            "external_edge_weight": round(self.external_edge_weight, 4),
        }


class LeidenCommunityDetector:
    """
    Greedy modularity community clustering for code dependency networks.
    """

    def __init__(self, min_community_size: int = 1):
        self.min_community_size = min_community_size

    def partition_graph(
        self,
        files: List[str],
        edges: List[Dict[str, Any]],
    ) -> List[CodeCommunity]:
        """
        Partitions vertices into communities maximizing modularity Q.
        edges: [{"source": "a.py", "target": "b.py", "weight": 0.8}, ...]
        """
        if not files:
            return []

        # Build adjacency graph
        adj: Dict[str, Dict[str, float]] = collections.defaultdict(dict)
        all_nodes = set(files)

        for edge in edges:
            u = str(edge.get("source") or edge.get("from") or "")
            v = str(edge.get("target") or edge.get("to") or "")
            w = float(edge.get("weight") or 1.0)
            if u and v and u != v:
                all_nodes.add(u)
                all_nodes.add(v)
                adj[u][v] = max(adj[u].get(v, 0.0), w)
                adj[v][u] = max(adj[v].get(u, 0.0), w)

        nodes = list(all_nodes)
        if len(nodes) <= 1:
            return [CodeCommunity(community_id=0, label="Core Monolith", files=nodes)]

        # Initial assignment: each node in its own cluster
        community_map: Dict[str, int] = {node: idx for idx, node in enumerate(nodes)}

        # Modularity local moving passes (up to 4 iterations)
        improved = True
        iterations = 0
        while improved and iterations < 4:
            improved = False
            iterations += 1
            for u in nodes:
                current_comm = community_map[u]
                best_comm = current_comm
                best_weight = 0.0

                # Sum weights to each neighboring community
                comm_weights: Dict[int, float] = collections.defaultdict(float)
                for neighbor, weight in adj[u].items():
                    comm_weights[community_map[neighbor]] += weight

                for comm_id, weight in comm_weights.items():
                    if weight > best_weight:
                        best_weight = weight
                        best_comm = comm_id

                if best_comm != current_comm and best_weight > 0.3:
                    community_map[u] = best_comm
                    improved = True

        # Group nodes by community
        groups: Dict[int, List[str]] = collections.defaultdict(list)
        for node, c_id in community_map.items():
            groups[c_id].append(node)

        # Label each community based on directory roots and file keywords
        communities: List[CodeCommunity] = []
        for c_id, comm_files in groups.items():
            label = self._infer_community_label(comm_files)
            # Calculate internal and external edge weights
            comm_set = set(comm_files)
            internal_w = 0.0
            external_w = 0.0
            for u in comm_files:
                for v, w in adj[u].items():
                    if v in comm_set:
                        internal_w += w
                    else:
                        external_w += w

            communities.append(
                CodeCommunity(
                    community_id=c_id,
                    label=label,
                    files=sorted(comm_files),
                    internal_edge_weight=internal_w / 2.0,
                    external_edge_weight=external_w,
                )
            )

        communities.sort(key=lambda c: len(c.files), reverse=True)
        return communities

    def _infer_community_label(self, files: List[str]) -> str:
        """Heuristically labels a functional domain from file paths."""
        words = collections.Counter()
        for f in files:
            parts = Path(f).parts
            for p in parts:
                stem = Path(p).stem.lower()
                for token in stem.split("_"):
                    if len(token) > 3 and token not in ("service", "helper", "index", "utils", "router", "test"):
                        words[token] += 1

        top_words = [w for w, _ in words.most_common(2)]
        if top_words:
            return f"Domain: {' / '.join(w.capitalize() for w in top_words)}"
        return "Domain: Core Common"

    def detect_cross_domain_blast(
        self,
        candidate_files: List[str],
        communities: List[CodeCommunity],
    ) -> Dict[str, Any]:
        """Detects whether proposed candidate files cross domain boundaries."""
        touched_domains: Dict[int, str] = {}
        for f in candidate_files:
            for comm in communities:
                if f in comm.files:
                    touched_domains[comm.community_id] = comm.label
                    break

        cross_domain = len(touched_domains) > 1
        return {
            "cross_domain_detected": cross_domain,
            "domains_count": len(touched_domains),
            "domains": list(touched_domains.values()),
            "warning": (
                f"[CROSS_DOMAIN_ALERT] Proposed edit crosses {len(touched_domains)} distinct domains: {', '.join(touched_domains.values())}"
                if cross_domain
                else None
            ),
        }

