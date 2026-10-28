"""
Generative MCTS (Language Agent Tree Search - LATS) Engine for Dream-RSI.
Provides forward tree search, active candidate expansion, multi-dimensional reward
evaluation, verbal reflection on failure, and postmortem vaccine synthesis.
"""

from __future__ import annotations

import json
import math
import uuid
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Callable, Dict, List, Optional


@dataclass
class MctsNode:
    """Represents a state node in the Generative MCTS tree."""
    node_id: str = field(default_factory=lambda: str(uuid.uuid4())[:8])
    state: Dict[str, Any] = field(default_factory=dict)
    parent: Optional[MctsNode] = None
    children: List[MctsNode] = field(default_factory=list)
    action: Optional[Dict[str, Any]] = None
    visits: int = 0
    value_sum: float = 0.0
    reward: float = 0.0
    depth: int = 0
    reflection: Optional[str] = None
    is_terminal: bool = False
    is_solution: bool = False

    @property
    def q_value(self) -> float:
        if self.visits == 0:
            return 0.0
        return self.value_sum / self.visits


class GenerativeMctsSearcher:
    """
    Language Agent Tree Search (LATS) engine for code exploration.
    Navigates code refactoring and invariant discovery through UCT selection,
    symbolic mutation expansion, multi-dimensional reward rollouts, and
    postmortem vaccine synthesis at $0 LLM cost.
    """

    def __init__(
        self,
        c_puct: float = math.sqrt(2.0),
        beta_threat: float = 0.35,
        max_depth: int = 6,
        simulations: int = 15,
        workspace_root: Optional[str | Path] = None,
    ):
        self.c_puct = c_puct
        self.beta_threat = beta_threat
        self.max_depth = max_depth
        self.simulations = simulations
        self.workspace_root = Path(workspace_root).resolve() if workspace_root else Path.cwd()

    def compute_uct(self, node: MctsNode, parent_visits: int, threat_scar: float = 0.0) -> float:
        """Computes UCT score with Amygdala threat scar aversion."""
        if node.visits == 0:
            return float("inf")

        q_val = node.q_value
        exploration = self.c_puct * math.sqrt(math.log(max(1, parent_visits)) / node.visits)
        penalty = self.beta_threat * threat_scar
        bonus = 2.0 if node.is_solution else 0.0

        return q_val + exploration - penalty + bonus

    def select(self, root: MctsNode, threat_scar: float = 0.0) -> MctsNode:
        """Traverses the tree to find the most promising leaf node."""
        current = root
        while current.children and not current.is_terminal:
            unvisited = [c for c in current.children if c.visits == 0]
            if unvisited:
                return unvisited[0]

            best_score = -float("inf")
            best_child = current.children[0]
            for child in current.children:
                score = self.compute_uct(child, current.visits, threat_scar)
                if score > best_score:
                    best_score = score
                    best_child = child
            current = best_child
        return current

    def expand(self, leaf: MctsNode, candidate_actions: Optional[List[Dict[str, Any]]] = None) -> List[MctsNode]:
        """Expands candidate mutation branches from the leaf node."""
        if leaf.is_terminal or leaf.depth >= self.max_depth:
            leaf.is_terminal = True
            return [leaf]

        actions = candidate_actions or self._generate_default_mutations(leaf.state)
        for act in actions:
            new_state = dict(leaf.state)
            new_state.update(act.get("state_patch", {}))
            child = MctsNode(
                state=new_state,
                parent=leaf,
                action=act,
                depth=leaf.depth + 1,
            )
            leaf.children.append(child)

        return leaf.children

    def _generate_default_mutations(self, state: Dict[str, Any]) -> List[Dict[str, Any]]:
        """Generates domain-specific symbolic code refactoring mutations."""
        symbol = str(state.get("symbol") or "target_fn")
        file_path = str(state.get("file_path") or "unknown.py")

        return [
            {
                "type": "add_default_param",
                "description": f"Add optional default parameter to signature of {symbol}",
                "state_patch": {"param_added": True, "backward_compatible": True},
            },
            {
                "type": "wrap_result_guard",
                "description": f"Wrap return of {symbol} in error/null safety check",
                "state_patch": {"guard_applied": True, "syntax_valid": True},
            },
            {
                "type": "isolate_boundary_contract",
                "description": f"Decouple {symbol} from direct caller dependencies",
                "state_patch": {"isolated": True, "blast_ratio": 0.1},
            },
        ]

    def evaluate_rollout(self, node: MctsNode) -> float:
        """
        Evaluates the node state with multi-dimensional deterministic fitness:
        R(s) = 0.35 * S_ast + 0.35 * S_contract + 0.30 * S_build - 0.20 * BlastRatio
        """
        state = node.state
        s_ast = 1.0 if state.get("syntax_valid", True) else 0.0
        s_contract = 1.0 if state.get("backward_compatible", True) else 0.0
        s_build = 1.0 if state.get("guard_applied", False) or state.get("isolated", False) else 0.5
        blast_ratio = float(state.get("blast_ratio", 0.0))

        reward = (0.35 * s_ast) + (0.35 * s_contract) + (0.30 * s_build) - (0.20 * blast_ratio)
        reward = max(-1.0, min(1.0, reward))

        node.reward = reward
        if reward >= 0.75:
            node.is_solution = True
            node.is_terminal = True
        elif reward < 0.20:
            node.reflection = f"Refactoring for {state.get('symbol')} failed safety or arity contract."
        return reward

    def backpropagate(self, node: MctsNode, reward: float) -> None:
        """Propagates reward and visit count updates up the tree."""
        current: Optional[MctsNode] = node
        while current is not None:
            current.visits += 1
            current.value_sum += reward
            current = current.parent

    def search(
        self,
        root_state: Dict[str, Any],
        threat_scar: float = 0.0,
    ) -> Dict[str, Any]:
        """Runs the complete LATS MCTS search cycle."""
        root = MctsNode(state=root_state, depth=0)
        solutions: List[MctsNode] = []

        for _ in range(self.simulations):
            leaf = self.select(root, threat_scar)
            if not leaf.is_terminal:
                children = self.expand(leaf)
                node_to_eval = children[0] if children else leaf
            else:
                node_to_eval = leaf

            reward = self.evaluate_rollout(node_to_eval)
            if node_to_eval.is_solution:
                solutions.append(node_to_eval)

            self.backpropagate(node_to_eval, reward)

        best_child = root.children[0] if root.children else root
        for c in root.children:
            if c.q_value > best_child.q_value:
                best_child = c

        vaccine_report = None
        if solutions:
            vaccine_report = self.synthesize_vaccine(solutions[0])

        return {
            "root_visits": root.visits,
            "best_q_value": round(best_child.q_value, 4),
            "best_action": best_child.action,
            "solutions_found": len(solutions),
            "vaccine_generated": vaccine_report is not None,
            "vaccine_report": vaccine_report,
        }

    def synthesize_vaccine(self, node: MctsNode) -> Dict[str, Any]:
        """Saves a postmortem vaccine memory when a solution is found."""
        now_iso = datetime.now(timezone.utc).isoformat()
        vaccine_id = f"vaccine_{node.node_id}"
        symbol = node.state.get("symbol", "unknown_symbol")
        file_path = node.state.get("file_path", "unknown_file")
        desc = node.action.get("description", "Automated contract invariant") if node.action else "Safe pattern"

        vaccine_data = {
            "id": vaccine_id,
            "created_at": now_iso,
            "file_path": file_path,
            "symbol": symbol,
            "reward_score": round(node.reward, 4),
            "antidote": desc,
            "invariant": f"Preserve backward-compatible arity for {symbol}",
        }

        # 1. Save to .ozymem/postmortem_vaccines.json
        vaccines_dir = self.workspace_root / ".ozymem"
        vaccines_dir.mkdir(parents=True, exist_ok=True)
        vaccines_file = vaccines_dir / "postmortem_vaccines.json"

        existing = []
        if vaccines_file.exists():
            try:
                with open(vaccines_file, "r", encoding="utf-8") as f:
                    existing = json.load(f)
            except Exception:
                existing = []

        existing.append(vaccine_data)
        # Keep last 50 vaccines
        existing = existing[-50:]
        try:
            with open(vaccines_file, "w", encoding="utf-8") as f:
                json.dump(existing, f, indent=2, ensure_ascii=False)
        except Exception:
            pass

        # 2. Materialize atomic Git Markdown memory in .ozy/memories/pattern/
        mem_dir = self.workspace_root / ".ozy" / "memories" / "pattern"
        mem_dir.mkdir(parents=True, exist_ok=True)
        md_file = mem_dir / f"{vaccine_id}.md"

        md_content = f"""---
id: {vaccine_id}
kind: pattern
topic_key: gotcha_prevention
title: "[MCTS VACCINE] {desc}"
author: dream_rsi_mcts
date: "{now_iso[:10]}"
severity: high
applies_to:
  - "{file_path}"
tags:
  - mcts_vaccine
  - contract_invariant
  - dream_rsi
---

# MCTS Postmortem Vaccine: {symbol}

- **Target File:** `{file_path}`
- **Symbol:** `{symbol}`
- **Reward Score:** `{node.reward}`
- **Action Applied:** {desc}

### Invariant Contract:
When modifying `{symbol}`, maintain arity safety and backward compatibility.
"""
        try:
            with open(md_file, "w", encoding="utf-8") as f:
                f.write(md_content)
        except Exception:
            pass

        return vaccine_data

