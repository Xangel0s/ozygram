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



class ThetaGammaWorkingMemoryBuffer:
    """
    Hippocampal Theta-Gamma Phase Precession Working Memory Buffer (Lisman & Idiart).
    Prevents LLM attention dilution ('lost in the middle') by structuring working context
    into 4 strictly ordered Theta phase bins (leaves to entrypoints), each bounding
    Gamma item slots to extractive +-10 line windows (<= 300 tokens per slot).
    """

    THETA_PHASE_NAMES = {
        1: "leaf_contracts",
        2: "domain_services",
        3: "entrypoints",
        4: "validation",
    }

    def __init__(self, window_lines: int = 10, max_tokens_per_slot: int = 300):
        self.window_lines = window_lines
        self.max_tokens_per_slot = max_tokens_per_slot

    @classmethod
    def classify_file_phase(cls, file_path: str) -> tuple[str, int]:
        """Classifies a file path into its natural Theta phase bin (1 to 4)."""
        fp = file_path.lower().replace("\\", "/")

        if any(k in fp for k in ["test", "spec", "eval", "mock"]):
            return (cls.THETA_PHASE_NAMES[4], 4)
        if any(k in fp for k in ["type", "model", "schema", "contract", ".d.ts", "entity", "dto"]):
            return (cls.THETA_PHASE_NAMES[1], 1)
        if any(k in fp for k in ["route", "endpoint", "controller", "handler", "component", "page", "view", "main.", "server."]):
            return (cls.THETA_PHASE_NAMES[3], 3)
        # Default to domain services / helpers / mutations
        return (cls.THETA_PHASE_NAMES[2], 2)

    def extract_gamma_window(
        self,
        content: str,
        symbol_hint: Optional[str] = None,
    ) -> Dict[str, Any]:
        """
        Extracts a bounded +-10 line extractive window around the target symbol,
        preserving contract integrity while pruning surrounding noise.
        """
        lines = content.splitlines()
        total_lines = len(lines)
        if total_lines == 0:
            return {"snippet": "", "start_line": 0, "end_line": 0, "tokens": 0}

        target_line_idx = 0
        if symbol_hint:
            for idx, l in enumerate(lines):
                if symbol_hint in l:
                    target_line_idx = idx
                    break
        else:
            # Fall back to first export or definition
            for idx, l in enumerate(lines):
                if any(k in l for k in ["export ", "def ", "fn ", "pub fn ", "class ", "interface "]):
                    target_line_idx = idx
                    break

        start = max(0, target_line_idx - self.window_lines)
        end = min(total_lines, target_line_idx + self.window_lines + 1)
        snippet_lines = lines[start:end]
        snippet = "\n".join(snippet_lines)

        tokens = max(1, len(snippet) // 4)
        if tokens > self.max_tokens_per_slot:
            # Truncate to token limit
            char_limit = self.max_tokens_per_slot * 4
            snippet = snippet[:char_limit] + "\n... [TRUNCATED_GAMMA_SLOT]"
            tokens = self.max_tokens_per_slot

        return {
            "snippet": snippet,
            "start_line": start + 1,
            "end_line": end,
            "tokens": tokens,
        }

    def pack_context(
        self,
        files: List[str],
        symbol_hints: Optional[Dict[str, List[str]]] = None,
        file_contents: Optional[Dict[str, str]] = None,
    ) -> Dict[str, Any]:
        """
        Packs candidate files into Theta phase bins and extracts compact Gamma slots.
        Returns ThetaGammaContext dictionary compatible with BrainResponse.
        """
        hints = symbol_hints or {}
        contents = file_contents or {}

        bins: Dict[int, List[Dict[str, Any]]] = {1: [], 2: [], 3: [], 4: []}
        raw_tokens_total = 0
        gamma_tokens_total = 0

        for f in files:
            phase_name, phase_idx = self.classify_file_phase(f)
            file_body = contents.get(f, f"// Content placeholder for {f}")
            raw_tokens_total += max(1, len(file_body) // 4)

            syms = hints.get(f, [])
            primary_sym = syms[0] if syms else None
            window = self.extract_gamma_window(file_body, symbol_hint=primary_sym)

            gamma_tokens_total += window["tokens"]
            bins[phase_idx].append({
                "file_path": f.replace("\\", "/"),
                "symbol": primary_sym or "contract_interface",
                "start_line": window["start_line"],
                "end_line": window["end_line"],
                "snippet": window["snippet"],
                "tokens": window["tokens"],
            })

        phases_list = []
        for idx in range(1, 5):
            slots = bins[idx]
            p_tokens = sum(s["tokens"] for s in slots)
            phases_list.append({
                "phase_name": self.THETA_PHASE_NAMES[idx],
                "theta_index": idx,
                "gamma_slots": slots,
                "total_tokens": p_tokens,
            })

        tokens_saved = max(0, raw_tokens_total - gamma_tokens_total)
        ratio = round(tokens_saved / max(1, raw_tokens_total), 4)

        return {
            "phases": phases_list,
            "total_gamma_tokens": gamma_tokens_total,
            "tokens_saved_vs_raw": tokens_saved,
            "compression_ratio": ratio,
        }

    @classmethod
    def format_bounded_prompt(cls, context_dict: Dict[str, Any]) -> str:
        """Formats the Theta-Gamma context into bounded prefill text for LLMs."""
        lines = [
            "[THETA_GAMMA_WORKING_MEMORY - PHASE PRECESSION ACTIVE]",
            f"Compression: {context_dict.get('compression_ratio', 0.0) * 100:.1f}% saved "
            f"({context_dict.get('tokens_saved_vs_raw', 0)} tokens pruned, {context_dict.get('total_gamma_tokens', 0)} tokens retained)",
            "",
        ]
        for p in context_dict.get("phases", []):
            slots = p.get("gamma_slots", [])
            if not slots:
                continue
            lines.append(f"=== Theta-{p['theta_index']}: {p['phase_name']} ({p['total_tokens']} tokens) ===")
            for s in slots:
                lines.append(f"• {s['file_path']} [L{s['start_line']}-L{s['end_line']}] ({s['symbol']}):")
                for s_line in s["snippet"].splitlines()[:12]:
                    lines.append(f"    {s_line}")
                lines.append("")
        return "\n".join(lines)
