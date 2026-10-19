from __future__ import annotations

from collections import defaultdict, deque
from typing import Any

from ozy_brain.schemas import (
    BrainResponse,
    _base_risks,
    _base_summary,
    _brain_context_pack,
    _candidate_file_scores,
    _candidate_files,
    _combined_memory_titles,
    _execution_policy,
    _extract_provenance,
    _goal,
    _safe_mcp_calls,
    _validation_commands,
)


def _compute_blast_radius(
    candidate_files: list[str],
    graph_context: dict[str, Any] | list[dict[str, Any]] | None = None,
) -> dict[str, Any]:
    """Calculates the ripple effect and blast radius of modifying the candidate files."""
    impacted_nodes: set[str] = set()
    high_severity_files: list[str] = []
    total_dependents = 0

    if isinstance(graph_context, list):
        for entry in graph_context:
            if isinstance(entry, dict):
                target = entry.get("file_path") or entry.get("target") or entry.get("path")
                if target:
                    impacted_nodes.add(str(target))
                depth = entry.get("depth", 1)
                severity = entry.get("severity", "low")
                if severity in ("high", "critical") or depth > 2:
                    if target:
                        high_severity_files.append(str(target))
                total_dependents += 1

    total_touched = len(candidate_files) + len(impacted_nodes)
    if total_touched <= 2 and not high_severity_files:
        risk_tier = "low"
    elif total_touched <= 6 and len(high_severity_files) <= 1:
        risk_tier = "moderate"
    elif total_touched <= 12:
        risk_tier = "high"
    else:
        risk_tier = "critical"

    return {
        "candidate_files_count": len(candidate_files),
        "transitive_dependents_count": len(impacted_nodes),
        "total_blast_radius": total_touched,
        "risk_tier": risk_tier,
        "high_severity_files": list(set(high_severity_files))[:5],
        "safe_to_edit_in_parallel": total_touched <= 3 and risk_tier == "low",
    }


def _topological_refactor_order(
    candidate_files: list[str],
    dependencies: list[dict[str, Any]] | list[tuple[str, str]] | None = None,
) -> list[dict[str, Any]]:
    """Orders refactoring steps from Leaf dependencies (base types/contracts) to Root callers (entry points).

    Prevents intermediate broken compilation states by enforcing:
    1. Base Contracts & Data Models (Leaves with 0 incoming dependencies).
    2. Internal Services & Business Logic.
    3. Root Dispatchers, MCP Handlers & CLI Entrypoints.
    """
    if not candidate_files:
        return []

    # Build dependency graph between candidate files
    adj: dict[str, set[str]] = defaultdict(set)
    in_degree: dict[str, int] = {f: 0 for f in candidate_files}

    # Heuristic scoring based on file roles if explicit graph edges are sparse
    role_weights: dict[str, int] = {}
    for f in candidate_files:
        f_lower = f.lower()
        if any(kw in f_lower for kw in ("types", "schema", "contract", "models", "entity", "dto")):
            role_weights[f] = 0  # Phase 1: Base contracts
        elif any(kw in f_lower for kw in ("helpers", "utils", "reader", "indexing", "parser")):
            role_weights[f] = 1  # Phase 2: Core algorithms & helpers
        elif any(kw in f_lower for kw in ("store", "backend", "service", "memory", "planner")):
            role_weights[f] = 2  # Phase 3: Domain services
        elif any(kw in f_lower for kw in ("dispatch", "server", "main", "cli", "router", "api")):
            role_weights[f] = 3  # Phase 4: Entrypoints and routers
        else:
            role_weights[f] = 2

    # Process explicit edges if provided: (origin -> destination)
    if dependencies:
        for dep in dependencies:
            if isinstance(dep, dict):
                src = dep.get("origin_path") or dep.get("from") or dep.get("source")
                dst = dep.get("destination_path") or dep.get("to") or dep.get("target")
            elif isinstance(dep, (list, tuple)) and len(dep) >= 2:
                src, dst = dep[0], dep[1]
            else:
                continue

            if src and dst and src in in_degree and dst in in_degree and src != dst:
                if dst not in adj[src]:
                    adj[src].add(dst)
                    in_degree[dst] += 1

    # Sort files according to role weight and in-degrees
    sorted_files = sorted(candidate_files, key=lambda f: (role_weights.get(f, 2), in_degree.get(f, 0), f))

    sequence: list[dict[str, Any]] = []
    for idx, file_path in enumerate(sorted_files, start=1):
        weight = role_weights.get(file_path, 2)
        if weight == 0:
            phase_name = "[CONTRACTS] Base Types & Schemas"
            action_desc = "Update type definitions, structs, and interfaces first."
        elif weight == 1:
            phase_name = "[UTILITIES] Core Helpers & Parsing"
            action_desc = "Implement low-level helpers and parsing logic."
        elif weight == 2:
            phase_name = "[DOMAIN] Business Logic & State"
            action_desc = "Update business rules, storage methods, and mutations."
        else:
            phase_name = "[ENTRYPOINTS] Public Routing & MCP Handlers"
            action_desc = "Adapt public routing, MCP handlers, and CLI commands."

        sequence.append({
            "step": idx,
            "file": file_path,
            "phase": phase_name,
            "action": action_desc,
            "requires_validation_before_next": weight < 2,
        })

    return sequence


def _structured_plan(payload: dict[str, Any], autonomy: str = "advisory") -> dict[str, Any]:
    goal = _goal(payload)
    cand_files = [item["path"] for item in _candidate_file_scores(payload, limit=8)] or _candidate_files(payload)
    impact_data = payload.get("impact") or payload.get("graph_impact") or []
    dependencies = payload.get("dependencies") or payload.get("edges") or []

    blast_radius = _compute_blast_radius(cand_files, impact_data)
    topological_sequence = _topological_refactor_order(cand_files, dependencies)

    return {
        "autonomy_level": autonomy,
        "goal": goal,
        "blast_radius_analysis": blast_radius,
        "topological_edit_sequence": topological_sequence,
        "phases": [
            {"name": "context", "objective": "Gather baseline evidence before making edits", "exit_condition": "Relevant candidate files, memories, graph impact, and risks identified"},
            {"name": "leaf_contracts", "objective": "Refactor base contracts, models, and schemas first", "exit_condition": "Base structures compile cleanly without contract violations"},
            {"name": "domain_services", "objective": "Propagate changes through domain and persistence logic", "exit_condition": "Internal mutations and service helpers synchronized"},
            {"name": "entrypoints", "objective": "Adapt public API, MCP dispatchers, and CLI interfaces", "exit_condition": "Handlers aligned with updated return contracts"},
            {"name": "validation", "objective": "Verify behavior with focused unit tests followed by test suite", "exit_condition": "Validation commands pass cleanly (cargo test / pytest)"},
            {"name": "learning", "objective": "Persist durable project knowledge", "exit_condition": "Key decisions, gotchas, or patterns recorded in memory"},
        ],
        "candidate_files": cand_files,
        "suggested_commands": _validation_commands(payload),
        "validation_checks": [
            "Confirm active project and git branch",
            "Inspect git status before and after changes",
            "Verify no destructive operations are required without explicit approval",
            "Follow topological edit sequence (leaves to root) to prevent broken compilation states",
            "Run focused tests covering modified code",
        ],
        "stop_conditions": [
            "Required context is missing or contradictory",
            "Destructive action would be required without explicit user approval",
            "Validation failure unrelated to planned change",
            "Scope of touched files exceeds planned blast radius",
        ],
        "what_not_to_touch": [
            "Do not modify core persistence state or schemas unless explicitly requested",
            "Do not edit files outside candidate scope without recalculating graph blast radius",
            "Do not alter security, auth, or credential configurations",
            "Do not remove existing error handling or assertion contracts",
        ],
    }


def plan(payload: dict[str, Any]) -> BrainResponse:
    goal = _goal(payload)
    memories = _combined_memory_titles(payload, limit=5)
    cand_files = [item["path"] for item in _candidate_file_scores(payload, limit=8)] or _candidate_files(payload)
    impact_data = payload.get("impact") or payload.get("graph_impact") or []
    dependencies = payload.get("dependencies") or payload.get("edges") or []

    blast_radius = _compute_blast_radius(cand_files, impact_data)
    topological_sequence = _topological_refactor_order(cand_files, dependencies)

    steps = [
        f"1. [PRE-FLIGHT] Baseline check: Goal '{goal}' | Blast Radius: {blast_radius['risk_tier'].upper()} ({blast_radius['total_blast_radius']} files).",
        "2. [TOPOLOGY] Topological Edit Sequence (Leaves to Root):",
    ]

    for item in topological_sequence[:5]:
        steps.append(f"   • Step {item['step']} [{item['phase']}]: {item['file']} → {item['action']}")

    steps.extend([
        "3. [TESTS] Run focused unit tests followed by integration test suite (cargo test / pytest).",
        "4. [VALIDATION] Verify no broken contracts or invariant violations across the dependency chain.",
        "5. [MEMORY] Record durable learnings, bug solutions, and architectural decisions in Ozy memory.",
    ])

    if memories:
        steps.insert(1, "Incorporate relevant past lessons from memory: " + "; ".join(memories))

    return BrainResponse(
        action="plan",
        summary=_base_summary("topological plan", payload),
        plan=steps,
        risks=_base_risks(payload) + ([f"High blast radius: {len(blast_radius['high_severity_files'])} critical file(s) affected."] if blast_radius["high_severity_files"] else []),
        recommendations=[
            "Refactor base contracts and types before modifying public dispatchers.",
            "Do not execute destructive operations without explicit confirmation.",
            "Maintain Rust as the validation authority and Python as consultative advisory.",
        ],
        memory_updates=["Save decisions, bug resolutions, and non-obvious discoveries after verification."],
        confidence=0.88 if blast_radius["risk_tier"] != "critical" else 0.72,
        suggested_mcp_calls=_safe_mcp_calls(payload),
        structured_plan=_structured_plan(payload),
        execution_policy=_execution_policy(payload),
        brain_context_pack=_brain_context_pack(payload),
        provenance=_extract_provenance(payload),
    )
