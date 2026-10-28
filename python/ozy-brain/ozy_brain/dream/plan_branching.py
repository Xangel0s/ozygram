"""
Warm-Started Branching MCTS Plan Optimizer for OzyBrain / Dream-RSI.
Standard: Zero-Emoji, SOLID, SOTA in Tree Search for Agents (LATS ICML 2024 / RAP NeurIPS).

Allows an LLM agent to warm-start an MCTS tree search directly from an intermediate
completed phase (e.g., Phase 2 'leaf_contracts' done in api-geofal-crm), exploring
only the remaining pending phases without wasting compute on completed prefixes.
"""

from __future__ import annotations

import math
import time
import uuid
from dataclasses import dataclass, field
from typing import Any, Optional

from ozy_brain.schemas import (
    _completed_phases,
    _current_phase,
    _working_edits,
)


STANDARD_PHASES: list[str] = [
    "context",
    "leaf_contracts",
    "domain_services",
    "entrypoints",
    "validation",
    "learning",
]


@dataclass
class PlanExecutionState:
    """
    Captures the intermediate execution state of a planning trajectory.
    Stores completed phases, active phase, working edits in RAM, modified files,
    active synaptic tags (STC), and validation checkpoints.
    """
    completed_phases: list[str] = field(default_factory=list)
    active_phase: str = "context"
    working_edits: list[dict[str, Any]] = field(default_factory=list)
    modified_files: list[str] = field(default_factory=list)
    synaptic_tags: list[dict[str, Any]] = field(default_factory=list)
    validation_history: list[dict[str, Any]] = field(default_factory=list)
    metadata: dict[str, Any] = field(default_factory=dict)

    @classmethod
    def from_payload(cls, payload: dict[str, Any]) -> PlanExecutionState:
        """Constructs a PlanExecutionState from a standard BrainRequest payload."""
        completed = _completed_phases(payload)
        current = _current_phase(payload)
        edits = _working_edits(payload)

        # Extract touched files from payload or edits
        touched: set[str] = set()
        for f in payload.get("modified_files") or payload.get("dirty_files") or []:
            if isinstance(f, str):
                touched.add(f)
            elif isinstance(f, dict) and "path" in f:
                touched.add(f["path"])
        for edit in edits:
            if "file" in edit:
                touched.add(str(edit["file"]))
            elif "path" in edit:
                touched.add(str(edit["path"]))

        # Extract synaptic tags if present
        raw_tags = payload.get("synaptic_tags") or payload.get("active_tags") or []
        tags = [t for t in raw_tags if isinstance(t, dict)]

        # Extract validation history
        val_hist = payload.get("validation_history") or []
        history = [v for v in val_hist if isinstance(v, dict)]

        return cls(
            completed_phases=completed,
            active_phase=current,
            working_edits=edits,
            modified_files=sorted(touched),
            synaptic_tags=tags,
            validation_history=history,
            metadata=dict(payload.get("metadata") or {}),
        )

    def is_phase_completed(self, phase_name: str) -> bool:
        """Checks if a given phase has already been finalized."""
        return phase_name in self.completed_phases

    def remaining_phases(self, phase_order: list[str] | None = None) -> list[str]:
        """Returns the list of phases that have not yet been completed."""
        order = phase_order or STANDARD_PHASES
        completed_set = set(self.completed_phases)
        return [p for p in order if p not in completed_set]

    def advance_phase(self, next_phase: str) -> None:
        """Marks the current active phase as completed and advances to next_phase."""
        if self.active_phase and self.active_phase not in self.completed_phases:
            self.completed_phases.append(self.active_phase)
        self.active_phase = next_phase

    def record_edit(self, file_path: str, diff_or_action: str, phase: str | None = None) -> None:
        """Appends an in-flight working edit to the state without touching disk."""
        if file_path not in self.modified_files:
            self.modified_files.append(file_path)
            self.modified_files.sort()
        self.working_edits.append({
            "file": file_path,
            "action": diff_or_action,
            "phase": phase or self.active_phase,
            "timestamp": time.time(),
        })

    def add_synaptic_tag(self, node_id: str, file_path: str, weight: float = 1.0) -> None:
        """Registers a Synaptic Tag (STC) marking an active modification locus."""
        self.synaptic_tags.append({
            "node_id": node_id,
            "file_path": file_path,
            "weight": weight,
            "timestamp": time.time(),
        })

    def get_active_tags(self, now: float | None = None, max_age_seconds: float = 1800.0) -> list[dict[str, Any]]:
        """Returns non-decayed synaptic tags within the biological half-life window (tau = 1800s)."""
        current_time = now if now is not None else time.time()
        active: list[dict[str, Any]] = []
        for tag in self.synaptic_tags:
            age = current_time - float(tag.get("timestamp", current_time))
            if age <= max_age_seconds:
                # Biological exponential decay
                decayed_weight = float(tag.get("weight", 1.0)) * math.exp(-age / max_age_seconds)
                active.append({**tag, "effective_weight": round(decayed_weight, 4)})
        return active

    def to_dict(self) -> dict[str, Any]:
        """Serializes the state to a JSON-compatible dictionary."""
        return {
            "completed_phases": list(self.completed_phases),
            "active_phase": self.active_phase,
            "working_edits_count": len(self.working_edits),
            "working_edits": self.working_edits,
            "modified_files": list(self.modified_files),
            "synaptic_tags_count": len(self.synaptic_tags),
            "synaptic_tags": self.synaptic_tags,
            "validation_history": self.validation_history,
            "remaining_phases": self.remaining_phases(),
            "metadata": self.metadata,
        }


@dataclass
class WarmStartNode:
    """
    Monte Carlo Tree Search node warm-started from an intermediate phase.
    Carries the full PlanExecutionState and UCT search statistics.
    """
    node_id: str = field(default_factory=lambda: str(uuid.uuid4())[:8])
    state: PlanExecutionState = field(default_factory=PlanExecutionState)
    phase: str = "context"
    action: str = "WARM_START_ROOT"
    visits: int = 0
    value_sum: float = 0.0
    prior_score: float = 1.0
    children: list[WarmStartNode] = field(default_factory=list)
    parent: Optional[WarmStartNode] = None
    metadata: dict[str, Any] = field(default_factory=dict)

    @property
    def q_value(self) -> float:
        """Computes mean value estimate Q(s, a)."""
        if self.visits <= 0:
            return 0.0
        return self.value_sum / self.visits

    @property
    def is_leaf(self) -> bool:
        """Returns True if the node has not yet been expanded."""
        return len(self.children) == 0

    def add_child(self, child: WarmStartNode) -> None:
        """Links a child node to this parent."""
        child.parent = self
        self.children.append(child)

    def best_child(self, c_puct: float = 1.414) -> WarmStartNode:
        """
        Selects child with highest Upper Confidence Bound for Trees (UCT / PUCT):
        PUCT(s, a) = Q(s, a) + c_puct * P(s, a) * sqrt(N(s)) / (1 + N(s, a))
        """
        if not self.children:
            raise ValueError("Cannot select best child from a leaf node without children.")

        total_visits = sum(child.visits for child in self.children)
        sqrt_total = math.sqrt(max(1, total_visits))

        best: Optional[WarmStartNode] = None
        best_score = -float("inf")

        for child in self.children:
            u_score = c_puct * child.prior_score * (sqrt_total / (1 + child.visits))
            score = child.q_value + u_score
            if score > best_score:
                best_score = score
                best = child

        return best if best is not None else self.children[0]

    def to_dict(self, depth: int = 0, max_depth: int = 2) -> dict[str, Any]:
        """Serializes node information with recursive child summary."""
        res: dict[str, Any] = {
            "node_id": self.node_id,
            "phase": self.phase,
            "action": self.action,
            "visits": self.visits,
            "q_value": round(self.q_value, 4),
            "prior_score": round(self.prior_score, 4),
            "children_count": len(self.children),
            "metadata": self.metadata,
        }
        if depth < max_depth and self.children:
            res["children"] = [c.to_dict(depth=depth + 1, max_depth=max_depth) for c in self.children]
        return res


class WarmStartRootNodeBuilder:
    """
    Constructs the initial MCTS root node ($s_0$) warm-started from already completed phases.
    Zero disk I/O, in-memory initialization executing in under 1 millisecond.
    """

    @classmethod
    def build_root(
        cls,
        state: PlanExecutionState,
        candidate_files: list[str] | None = None,
        phase_order: list[str] | None = None,
    ) -> WarmStartNode:
        """
        Builds the root node s0 for branching MCTS.
        - Inherits completed phases and working edits.
        - Focuses root phase on state.active_phase or the first uncompleted phase.
        - Dynamically computes prior score based on active STC synaptic tags.
        """
        order = phase_order or STANDARD_PHASES
        remaining = state.remaining_phases(order)

        # Active phase resolution
        root_phase = state.active_phase
        if root_phase in state.completed_phases:
            root_phase = remaining[0] if remaining else "validation"

        # Synaptic tag attention weighting
        active_tags = state.get_active_tags()
        tagged_paths = {t.get("file_path", "") for t in active_tags if t.get("file_path")}

        # Check candidate files intersection with active synaptic tags
        cands = candidate_files or []
        overlap_count = sum(1 for c in cands if c in tagged_paths or any(t in c for t in tagged_paths))

        # Base prior is 1.0, boosted if focused on high-plasticity tagged nodes
        prior = 1.0 + min(0.5, 0.1 * overlap_count)

        root = WarmStartNode(
            node_id=f"root-{uuid.uuid4().hex[:6]}",
            state=state,
            phase=root_phase,
            action="WARM_START_ROOT",
            visits=1,
            value_sum=0.0,
            prior_score=round(prior, 4),
            metadata={
                "completed_count": len(state.completed_phases),
                "remaining_count": len(remaining),
                "remaining_phases": remaining,
                "active_synaptic_tags": len(active_tags),
                "overlap_candidate_tags": overlap_count,
                "warm_start_timestamp": time.time(),
            },
        )
        return root
