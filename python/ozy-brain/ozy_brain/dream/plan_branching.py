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



@dataclass
class BranchAction:
    """Represents a candidate architectural branch action for a given phase."""
    action_id: str
    strategy: str  # e.g. "DualActiveContract", "FrontendCallerSync", "BackendFallbackAdapter"
    phase: str     # e.g. "domain_services", "entrypoints", "validation"
    target_files: list[str]
    projected_churn: int
    description: str
    metadata: dict[str, Any] = field(default_factory=dict)

    def to_dict(self) -> dict[str, Any]:
        return {
            "action_id": self.action_id,
            "strategy": self.strategy,
            "phase": self.phase,
            "target_files": list(self.target_files),
            "projected_churn": self.projected_churn,
            "description": self.description,
            "metadata": self.metadata,
        }


class PhaseActionGenerator:
    """
    Derives candidate architectural branch strategies based on the active phase and candidate files.
    """

    @classmethod
    def generate_actions(
        cls,
        phase: str,
        candidate_files: list[str],
        context: dict[str, Any] | None = None,
    ) -> list[BranchAction]:
        cands = list(candidate_files) or ["unknown_target"]
        actions: list[BranchAction] = []
        ctx = context or {}

        if phase == "leaf_contracts":
            actions.extend([
                BranchAction(
                    action_id=f"act_pydantic_v2_{uuid.uuid4().hex[:6]}",
                    strategy="VersionedDtoContract",
                    phase=phase,
                    target_files=cands,
                    projected_churn=12,
                    description="Introduce non-breaking versioned DTO models (SchemaV2) preserving backward compatibility.",
                    metadata={"is_breaking": False, "blast_radius_risk": "LOW"},
                ),
                BranchAction(
                    action_id=f"act_strict_schema_{uuid.uuid4().hex[:6]}",
                    strategy="StrictPydanticValidation",
                    phase=phase,
                    target_files=cands,
                    projected_churn=18,
                    description="Tighten Pydantic field validators directly on existing schemas.",
                    metadata={"is_breaking": True, "blast_radius_risk": "MEDIUM"},
                ),
                BranchAction(
                    action_id=f"act_permissive_schema_{uuid.uuid4().hex[:6]}",
                    strategy="PermissiveBaseSchema",
                    phase=phase,
                    target_files=cands,
                    projected_churn=6,
                    description="Add optional fields with default values to accommodate client schema variations.",
                    metadata={"is_breaking": False, "blast_radius_risk": "LOW"},
                ),
            ])
        elif phase == "domain_services":
            actions.extend([
                BranchAction(
                    action_id=f"act_dual_contract_{uuid.uuid4().hex[:6]}",
                    strategy="DualActiveContract",
                    phase=phase,
                    target_files=cands,
                    projected_churn=15,
                    description="Expose dual active contract supporting both legacy callers and modernized payloads.",
                    metadata={"is_breaking": False, "blast_radius_risk": "LOW", "protects_frontend": True},
                ),
                BranchAction(
                    action_id=f"act_frontend_sync_{uuid.uuid4().hex[:6]}",
                    strategy="FrontendCallerSync",
                    phase=phase,
                    target_files=cands,
                    projected_churn=45,
                    description="Directly migrate backend contracts and synchronize all frontend hooks/callers simultaneously.",
                    metadata={"is_breaking": True, "blast_radius_risk": "HIGH", "protects_frontend": False},
                ),
                BranchAction(
                    action_id=f"act_fallback_adapter_{uuid.uuid4().hex[:6]}",
                    strategy="BackendFallbackAdapter",
                    phase=phase,
                    target_files=cands,
                    projected_churn=22,
                    description="Insert an input/output adapter shim translating legacy requests to internal domain entities.",
                    metadata={"is_breaking": False, "blast_radius_risk": "LOW", "protects_frontend": True},
                ),
            ])
        elif phase == "entrypoints":
            actions.extend([
                BranchAction(
                    action_id=f"act_dual_router_{uuid.uuid4().hex[:6]}",
                    strategy="DualRouteDispatcher",
                    phase=phase,
                    target_files=cands,
                    projected_churn=14,
                    description="Maintain dual FastAPI endpoints (@router.get('') and @router.get('/v2')) concurrently.",
                    metadata={"is_breaking": False, "blast_radius_risk": "LOW"},
                ),
                BranchAction(
                    action_id=f"act_atomic_cutover_{uuid.uuid4().hex[:6]}",
                    strategy="AtomicApiCutover",
                    phase=phase,
                    target_files=cands,
                    projected_churn=30,
                    description="Perform an atomic in-place cutover of API routes and parameters.",
                    metadata={"is_breaking": True, "blast_radius_risk": "HIGH"},
                ),
            ])
        elif phase == "validation":
            actions.extend([
                BranchAction(
                    action_id=f"act_focused_test_{uuid.uuid4().hex[:6]}",
                    strategy="FocusedUnitTesting",
                    phase=phase,
                    target_files=cands,
                    projected_churn=10,
                    description="Execute focused unit tests for modified endpoints and domain services.",
                    metadata={"is_breaking": False, "test_tier": "unit"},
                ),
                BranchAction(
                    action_id=f"act_cross_repo_e2e_{uuid.uuid4().hex[:6]}",
                    strategy="CrossRepoContractE2E",
                    phase=phase,
                    target_files=cands,
                    projected_churn=25,
                    description="Verify end-to-end integration across FastAPI endpoints and Next.js view consumers.",
                    metadata={"is_breaking": False, "test_tier": "e2e"},
                ),
            ])
        elif phase == "learning":
            actions.extend([
                BranchAction(
                    action_id=f"act_gotcha_rec_{uuid.uuid4().hex[:6]}",
                    strategy="RecordMitigationGotcha",
                    phase=phase,
                    target_files=cands,
                    projected_churn=5,
                    description="Persist discovered contract and blast radius prevention gotcha into .ozy/memories/gotchas/.",
                    metadata={"is_breaking": False, "storage": "markdown_git"},
                ),
            ])
        else:
            actions.append(
                BranchAction(
                    action_id=f"act_direct_refactor_{uuid.uuid4().hex[:6]}",
                    strategy="DirectRefactorBranch",
                    phase=phase,
                    target_files=cands,
                    projected_churn=20,
                    description=f"Standard iterative implementation for phase '{phase}'.",
                    metadata={"is_breaking": False},
                )
            )

        return actions


class RamBranchSimulator:
    """
    Ephemerally projects candidate branch mutations in memory with zero disk writes.
    Ensures strict RAM isolation (< 1ms execution).
    """

    @classmethod
    def simulate_mutation(
        cls,
        current_state: PlanExecutionState,
        action: BranchAction,
    ) -> tuple[PlanExecutionState, dict[str, Any]]:
        # Clone current state
        simulated_completed = list(current_state.completed_phases)
        simulated_edits = list(current_state.working_edits)
        simulated_files = set(current_state.modified_files)

        # Apply action mutations to working edits
        for f in action.target_files:
            simulated_files.add(f)
            simulated_edits.append({
                "file": f,
                "action": f"simulated_{action.strategy}",
                "phase": action.phase,
                "projected_churn": action.projected_churn,
                "simulated": True,
                "timestamp": time.time(),
            })

        # Project phase completion if moving forward
        if action.phase not in simulated_completed:
            simulated_completed.append(action.phase)

        next_phase = "validation"
        remaining = [p for p in STANDARD_PHASES if p not in simulated_completed]
        if remaining:
            next_phase = remaining[0]

        simulated_state = PlanExecutionState(
            completed_phases=simulated_completed,
            active_phase=next_phase,
            working_edits=simulated_edits,
            modified_files=sorted(simulated_files),
            synaptic_tags=list(current_state.synaptic_tags),
            validation_history=list(current_state.validation_history),
            metadata={
                **current_state.metadata,
                "last_simulated_action": action.action_id,
                "last_strategy": action.strategy,
            },
        )

        # Simulation report
        is_breaking = action.metadata.get("is_breaking", False)
        report = {
            "action_id": action.action_id,
            "strategy": action.strategy,
            "phase": action.phase,
            "is_breaking": is_breaking,
            "projected_churn": action.projected_churn,
            "total_files_touched": len(simulated_files),
            "simulated_in_ram": True,
            "disk_writes": 0,
            "invariants_satisfied": not is_breaking or action.strategy in ("DualActiveContract", "BackendFallbackAdapter"),
        }
        return simulated_state, report


class SwrBranchFilter:
    """
    Immunological filter pruning candidate branches matching known SWR failure ripples.
    Prevents MCTS from wasting search tokens exploring known failure traps.
    """

    @classmethod
    def filter_actions(
        cls,
        actions: list[BranchAction],
        ripples_or_hazards: list[Any],
    ) -> tuple[list[BranchAction], list[dict[str, Any]]]:
        approved: list[BranchAction] = []
        pruned: list[dict[str, Any]] = []

        # Extract hazard descriptions and error signatures
        hazard_texts: list[str] = []
        for item in ripples_or_hazards:
            if isinstance(item, str):
                hazard_texts.append(item.lower())
            elif isinstance(item, dict):
                sig = item.get("error_signature") or item.get("broken_state_desc") or item.get("hazard") or ""
                if sig:
                    hazard_texts.append(str(sig).lower())
            elif hasattr(item, "error_signature"):
                hazard_texts.append(str(item.error_signature).lower())

        for act in actions:
            is_pruned = False
            prune_reason = ""

            # Breaking actions are strictly audited against known hazards
            if act.metadata.get("is_breaking", False) or act.strategy in ("FrontendCallerSync", "AtomicApiCutover"):
                for hz in hazard_texts:
                    if any(term in hz for term in ("signature", "arity", "caller", "argument", "cannot find", "member access")):
                        is_pruned = True
                        prune_reason = f"Hazard match '{hz[:60]}' - breaking strategy risks recurring regression without dual active contract."
                        break

            if is_pruned:
                pruned.append({
                    "action_id": act.action_id,
                    "strategy": act.strategy,
                    "phase": act.phase,
                    "reason": prune_reason,
                    "badge": "[ALERT: SWR_TRAP_PRUNED]",
                })
            else:
                approved.append(act)

        return approved, pruned
