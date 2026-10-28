from __future__ import annotations

from collections import defaultdict, deque
from pathlib import Path
import re
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
    _authority_recommendation,
    _validation_summary_label,
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


def _classify_file_role(file_path: str) -> tuple[str, int]:
    lower = file_path.lower().replace("\\", "/")
    parts = lower.split("/")
    filename = parts[-1]
    ext = filename.rsplit(".", 1)[-1] if "." in filename else ""
    name_no_ext = filename.rsplit(".", 1)[0]
    name_tokens = set(name_no_ext.replace("-", " ").replace("_", " ").split())
    path_segments = set(parts[:-1])

    if any(tok in name_tokens for tok in ("test", "spec", "tests")) or any(seg in path_segments for seg in ("test", "tests", "specs", "e2e")):
        return "Test / Validation Script", 4
    if ext == "sql" or "migration" in name_tokens or "migrations" in path_segments:
        return "Database Schema / Migration", 0
    if any(tok in name_tokens for tok in ("types", "schema", "schemas", "contract", "contracts", "models", "model", "dto", "interface", "constants", "data")) or any(seg in path_segments for seg in ("schemas", "models", "contracts", "types", "data")):
        return "Types / Data Contract", 0
    if ext in ("tsx", "jsx", "vue", "svelte") or any(tok in name_tokens for tok in ("modal", "dialog", "view", "component", "form", "page", "table", "drawer", "columns")):
        return "Frontend Component / UI", 3
    if any(tok in name_tokens for tok in ("helpers", "helper", "utils", "util", "parser", "formatters", "indexing")):
        return "Core Utility / Helper", 1
    if any(tok in name_tokens for tok in ("router", "routers", "route", "routes", "endpoint", "controller")) or any(seg in path_segments for seg in ("routers", "routes", "controllers")):
        return "API Route / Endpoint", 3
    if ext in ("rs", "py", "go", "java"):
        return "Backend Service / Logic", 2
    return "Module Implementation", 2


def _topological_refactor_order(
    candidate_files: list[str],
    dependencies: list[dict[str, Any]] | list[tuple[str, str]] | None = None,
    candidate_scores: dict[str, int] | None = None,
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

    role_weights: dict[str, int] = {}
    file_roles: dict[str, str] = {}
    for f in candidate_files:
        role_label, weight = _classify_file_role(f)
        role_weights[f] = weight
        file_roles[f] = role_label

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

    # Multi-tier arc balancing: if candidates span multiple distinct architectural layers,
    # ensure top sequence covers representative files from each touched tier before filling remaining slots.
    score_map = candidate_scores or {}
    tier_files: dict[int, list[str]] = defaultdict(list)
    for f in candidate_files:
        tier_files[role_weights.get(f, 2)].append(f)
    for t in tier_files:
        tier_files[t].sort(key=lambda f: (-score_map.get(f, 0), in_degree.get(f, 0), f))

    if len(tier_files) > 1 and len(candidate_files) > 4:
        selected: list[str] = []
        # Guarantee representation across available tiers in topological order (0 -> 1 -> 2 -> 3 -> 4)
        for t in sorted(tier_files.keys()):
            if tier_files[t]:
                selected.append(tier_files[t].pop(0))
        remaining = [f for t in tier_files.values() for f in t]
        remaining.sort(key=lambda f: (role_weights.get(f, 2), in_degree.get(f, 0), -score_map.get(f, 0), f))
        selected.extend(remaining)
        sorted_files = selected
    else:
        sorted_files = sorted(candidate_files, key=lambda f: (role_weights.get(f, 2), in_degree.get(f, 0), -score_map.get(f, 0), f))

    sequence: list[dict[str, Any]] = []
    for idx, file_path in enumerate(sorted_files, start=1):
        weight = role_weights.get(file_path, 2)
        role = file_roles.get(file_path, "Module Implementation")

        sequence.append({
            "step": idx,
            "file": file_path,
            "phase": f"Step {idx}",
            "action": role,
            "requires_validation_before_next": weight < 2,
        })

    return sequence


def localize_hierarchical_targets(
    candidate_files: list[str],
    payload: dict[str, Any],
    max_symbols_per_file: int = 3,
) -> list[dict[str, Any]]:
    """
    Agentless 3-Stage Hierarchical Localization (UC Berkeley / UIUC 2024):
    Stage 1: Repository -> Candidate Files
    Stage 2: Candidate Files -> Target Symbols (functions/classes)
    Stage 3: Target Symbols -> Extractive Line Windows (±10 lines)
    """
    localized: list[dict[str, Any]] = []
    goal_tokens = {w.lower() for w in re.findall(r"[a-zA-Z_][a-zA-Z0-9_]*", str(payload.get("goal", "")))}
    raw_symbols = payload.get("symbols") or payload.get("functions") or []

    target_files = list(candidate_files)
    if not target_files and raw_symbols:
        for s in raw_symbols:
            if isinstance(s, dict) and s.get("file_path") and s["file_path"] not in target_files:
                target_files.append(s["file_path"])

    for file_path in target_files:
        file_symbols: list[dict[str, Any]] = []
        # 1. Match from payload symbols if provided
        for s in raw_symbols:
            if isinstance(s, dict) and s.get("file_path") == file_path:
                s_name = str(s.get("name", "unknown"))
                start = int(s.get("start_line", 1))
                end = int(s.get("end_line", start + 20))
                file_symbols.append({
                    "symbol": s_name,
                    "window": [max(1, start - 10), end + 10],
                    "match_reason": "explicit_graph_match",
                })

        # 2. Heuristic line window if file exists on disk and no explicit symbols
        if not file_symbols:
            role, _ = _classify_file_role(file_path)
            file_symbols.append({
                "symbol": f"target_entrypoint_in_{Path(file_path).stem}",
                "window": [1, 50],
                "match_reason": f"role_{role}_heuristic",
            })

        localized.append({
            "file": file_path,
            "symbols": file_symbols[:max_symbols_per_file],
        })

    return localized



def _structured_plan(payload: dict[str, Any], autonomy: str = "advisory") -> dict[str, Any]:
    goal = _goal(payload)
    cand_scores_list = _candidate_file_scores(payload, limit=8)
    cand_files = (
        [item["path"] for item in cand_scores_list]
        or _candidate_files(payload)
        or [s["file_path"] for s in payload.get("symbols", []) if isinstance(s, dict) and s.get("file_path")]
        or [s["file_path"] for s in payload.get("functions", []) if isinstance(s, dict) and s.get("file_path")]
    )
    score_map = {item["path"]: int(item.get("score", 0)) for item in cand_scores_list}
    impact_data = payload.get("impact") or payload.get("graph_impact") or []
    dependencies = payload.get("dependencies") or payload.get("edges") or []

    blast_radius = _compute_blast_radius(cand_files, impact_data)
    topological_sequence = _topological_refactor_order(cand_files, dependencies, score_map)
    hierarchical_locs = localize_hierarchical_targets(cand_files, payload)

    # GraphRAG & Leiden Community Detection
    from ozy_brain.leiden_communities import LeidenCommunityDetector
    detector = LeidenCommunityDetector()
    communities = detector.partition_graph(cand_files, dependencies)
    cross_domain_info = detector.detect_cross_domain_blast(cand_files, communities)

    # GraphRAG & Leiden Community Detection
    from ozy_brain.leiden_communities import LeidenCommunityDetector
    detector = LeidenCommunityDetector()
    communities = detector.partition_graph(cand_files, dependencies)
    cross_domain_info = detector.detect_cross_domain_blast(cand_files, communities)

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
            {"name": "validation", "objective": "Verify behavior with focused unit tests followed by test suite", "exit_condition": f"Validation commands pass cleanly ({_validation_summary_label(payload)})"},
            {"name": "learning", "objective": "Persist durable project knowledge", "exit_condition": "Key decisions, gotchas, or patterns recorded in memory"},
        ],
        "candidate_files": cand_files,
        "suggested_commands": _validation_commands(payload),
        "architectural_domains": [comm.to_dict() for comm in communities],
        "cross_domain_analysis": cross_domain_info,
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
        "hierarchical_localization": hierarchical_locs,
    }



def plan(payload: dict[str, Any]) -> BrainResponse:
    goal = _goal(payload)
    memories = _combined_memory_titles(payload, limit=5)
    cand_scores_list = _candidate_file_scores(payload, limit=8)
    cand_files = [item["path"] for item in cand_scores_list] or _candidate_files(payload)
    score_map = {item["path"]: int(item.get("score", 0)) for item in cand_scores_list}
    impact_data = payload.get("impact") or payload.get("graph_impact") or []
    dependencies = payload.get("dependencies") or payload.get("edges") or []

    blast_radius = _compute_blast_radius(cand_files, impact_data)
    topological_sequence = _topological_refactor_order(cand_files, dependencies, score_map)
    hierarchical_locs = localize_hierarchical_targets(cand_files, payload)

    # GraphRAG & Leiden Community Detection
    from ozy_brain.leiden_communities import LeidenCommunityDetector
    detector = LeidenCommunityDetector()
    communities = detector.partition_graph(cand_files, dependencies)
    cross_domain_info = detector.detect_cross_domain_blast(cand_files, communities)

    # GraphRAG & Leiden Community Detection
    from ozy_brain.leiden_communities import LeidenCommunityDetector
    detector = LeidenCommunityDetector()
    communities = detector.partition_graph(cand_files, dependencies)
    cross_domain_info = detector.detect_cross_domain_blast(cand_files, communities)

    steps = [
        f"1. [PRE-FLIGHT] Baseline check: Goal '{goal}' | Blast Radius: {blast_radius['risk_tier'].upper()} ({blast_radius['total_blast_radius']} files).",
        "2. [TOPOLOGY] Topological Edit Sequence (Leaves to Root):",
    ]

    for item in topological_sequence[:5]:
        steps.append(f"   • Step {item['step']}: {item['file']} → {item['action']}")

    steps.extend([
        f"3. [TESTS] Run focused unit tests followed by integration test suite ({_validation_summary_label(payload)}).",
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
            _authority_recommendation(payload),
        ],
        memory_updates=["Save decisions, bug resolutions, and non-obvious discoveries after verification."],
        confidence=0.88 if blast_radius["risk_tier"] != "critical" else 0.72,
        suggested_mcp_calls=_safe_mcp_calls(payload),
        structured_plan=_structured_plan(payload),
        execution_policy=_execution_policy(payload),
        brain_context_pack=_brain_context_pack(payload),
        provenance=_extract_provenance(payload),
    )
