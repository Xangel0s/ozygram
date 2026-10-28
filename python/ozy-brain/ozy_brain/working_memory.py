"""
Working Memory Compactor and Recursive Distillation Engine.
Inspired by MemGPT (Packer et al.) and CoALA Working Memory (Princeton).
Compactifies agent conversation and tool execution history when context grows large,
retaining high-value invariants, exact symbol signatures, and line windows while
discarding noisy terminal stdout and repetitive status messages.
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field
from typing import Any, Dict, List, Optional


@dataclass
class WorkingMemoryDigest:
    """Compact structured representation of active working memory."""
    goal: str
    active_files: List[str]
    symbol_contracts: List[Dict[str, Any]]
    active_invariants: List[str]
    recent_failures_count: int
    compressed_summary: str
    estimated_tokens_saved: int

    def to_dict(self) -> Dict[str, Any]:
        return {
            "goal": self.goal,
            "active_files": self.active_files,
            "symbol_contracts": self.symbol_contracts,
            "active_invariants": self.active_invariants,
            "recent_failures_count": self.recent_failures_count,
            "compressed_summary": self.compressed_summary,
            "estimated_tokens_saved": self.estimated_tokens_saved,
        }


class WorkingMemoryCompactor:
    """
    Distills verbose agent interaction logs into high-density working memory.
    """

    def __init__(self, token_threshold: int = 4000):
        self.token_threshold = token_threshold

    def estimate_tokens(self, text_or_list: str | List[Any]) -> int:
        """Heuristic token estimator (~4 chars per token)."""
        if isinstance(text_or_list, list):
            raw_len = sum(len(str(item)) for item in text_or_list)
        else:
            raw_len = len(str(text_or_list))
        return max(1, raw_len // 4)

    def compactify(
        self,
        goal: str,
        history: List[Dict[str, Any]] | str,
        active_files: Optional[List[str]] = None,
        symbols: Optional[List[Dict[str, Any]]] = None,
    ) -> WorkingMemoryDigest:
        """
        Extracts core contracts and discards terminal fluff.
        """
        raw_text = history if isinstance(history, str) else str(history)
        initial_tokens = self.estimate_tokens(raw_text)

        files: List[str] = list(active_files or [])
        # Extract file paths from raw text if not explicitly given
        if not files:
            path_matches = re.findall(r"[\w\-\./]+\.(?:py|rs|ts|tsx|js|sql|json)", raw_text)
            for p in path_matches:
                if len(p) > 3 and p not in files:
                    files.append(p)
            files = files[:8]

        # Extract symbol contracts
        contracts: List[Dict[str, Any]] = list(symbols or [])
        if not contracts:
            fn_matches = re.findall(r"(?:def|fn|function|class)\s+([a-zA-Z_][a-zA-Z0-9_]*)", raw_text)
            for fn in fn_matches:
                if not any(c.get("name") == fn for c in contracts):
                    contracts.append({"name": fn, "kind": "inferred_symbol"})
            contracts = contracts[:6]

        # Extract invariants & gotchas
        invariants: List[str] = []
        for line in raw_text.splitlines():
            clean = line.strip()
            if any(k in clean.lower() for k in ["[alert:", "[invariant", "cannot", "must not", "error:", "failed:"]):
                if len(clean) > 10 and clean not in invariants:
                    invariants.append(clean[:120])
        invariants = invariants[:5]

        failures_count = len([line for line in raw_text.splitlines() if "error" in line.lower() or "fail" in line.lower()])

        summary = (
            f"Active Goal: '{goal[:80]}'. "
            f"Focused on {len(files)} files ({', '.join(files[:3])}). "
            f"{len(contracts)} symbol contracts tracked. "
            f"{len(invariants)} critical invariants enforced."
        )

        final_tokens = self.estimate_tokens(summary) + self.estimate_tokens(str(contracts)) + self.estimate_tokens(str(invariants))
        tokens_saved = max(0, initial_tokens - final_tokens)

        return WorkingMemoryDigest(
            goal=goal,
            active_files=files,
            symbol_contracts=contracts,
            active_invariants=invariants,
            recent_failures_count=failures_count,
            compressed_summary=summary,
            estimated_tokens_saved=tokens_saved,
        )

