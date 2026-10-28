from __future__ import annotations

import hashlib
import re
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

from ozy_brain.planner import _structured_plan
from ozy_brain.schemas import (
    BrainResponse,
    _base_risks,
    _base_summary,
    _brain_context_pack,
    _candidate_files,
    _execution_policy,
    _extract_provenance,
    _goal,
    _items,
    _project,
    _safe_mcp_calls,
)



def triangulate_failure(
    failure_trace: str, goal: str = "", changes: list[Any] | None = None
) -> dict[str, str]:
    """
    Reflexion Framework (NeurIPS 2023): Triangulates execution failures into:
    1. Flawed Assumption
    2. Violated Invariant
    3. Prescriptive Antidote Rule
    """
    ft_lower = failure_trace.lower()
    changes_str = ", ".join(str(c) for c in (changes or [])[:3])

    if "syntax" in ft_lower or "parse" in ft_lower or "unexpected token" in ft_lower:
        assumption = "Assumed code edits conformed to Tree-Sitter grammar without local AST parsing."
        invariant = "All generated code must compile cleanly under native parser rules."
        antidote = "Validate syntax in memory using ozy_ast_patch or ghost simulation before writing to disk."
    elif "attributeerror" in ft_lower or "undefined" in ft_lower or "has no attribute" in ft_lower:
        assumption = "Assumed target object/module possessed symbol without checking contract definition."
        invariant = "External module symbols must be inspected via ozy_context contracts first."
        antidote = "Verify object schema and exported functions with AST skeleton lookup before calling."
    elif "timeout" in ft_lower or "deadlock" in ft_lower:
        assumption = "Assumed synchronous operation would complete immediately without resource bounds."
        invariant = "All external subprocesses and network calls must have bounded timeouts (< 5s)."
        antidote = "Implement non-blocking async execution or exponential backoff."
    elif "import" in ft_lower or "modulenotfound" in ft_lower or "cannot find module" in ft_lower:
        assumption = "Assumed package/module was available in current python/node environment."
        invariant = "Dependency paths must align with pyproject.toml or package.json manifests."
        antidote = "Inspect project dependencies before adding foreign package imports."
    else:
        assumption = f"Assumed change sequence for '{goal[:60]}' would succeed without contract verification."
        invariant = "Caller and callee signatures must maintain arity parity."
        antidote = f"Apply isolated boundary check and verify affected files: {changes_str or 'target files'}."

    return {
        "flawed_assumption": assumption,
        "violated_invariant": invariant,
        "prescriptive_antidote": antidote,
    }


def persist_atomic_gotcha_markdown(
    workspace_root: str | Path,
    failure_trace: str,
    triangulation: dict[str, str],
    target_files: list[str] | None = None,
) -> Path | None:
    """
    Persists an atomic Git Markdown gotcha under .ozy/memories/gotchas/
    compatible with Git version control and real-time preventive injection.
    """
    try:
        ws_path = Path(workspace_root).resolve()
        gotchas_dir = ws_path / ".ozy" / "memories" / "gotchas"
        gotchas_dir.mkdir(parents=True, exist_ok=True)

        trace_hash = hashlib.sha256(failure_trace.encode("utf-8")).hexdigest()[:8]
        gotcha_id = f"gotcha_reflexion_{trace_hash}"
        file_path = gotchas_dir / f"{gotcha_id}.md"

        now_iso = datetime.now(timezone.utc).isoformat()
        applies_to = target_files or ["src/*"]
        yaml_applies = "\n".join(f'  - "{f}"' for f in applies_to[:5])

        md_content = f"""---
id: {gotcha_id}
kind: gotcha
topic_key: gotcha_prevention
title: "[REFLEXION GOTCHA] {triangulation['violated_invariant'][:80]}"
author: ozybrain_reflexion
date: "{now_iso[:10]}"
severity: high
applies_to:
{yaml_applies}
tags:
  - reflexion
  - failure_triangulation
  - auto_learned
---

# Verbal Self-Reflection (Reflexion Engine)

- **Flawed Assumption:** {triangulation['flawed_assumption']}
- **Violated Invariant:** {triangulation['violated_invariant']}
- **Prescriptive Antidote:** {triangulation['prescriptive_antidote']}

### Error Trace Signature:
```text
{failure_trace[:300]}
```
"""
        with open(file_path, "w", encoding="utf-8") as f:
            f.write(md_content)
        return file_path
    except Exception:
        return None

def extract_procedural_rules(failures: list[Any], changes: list[Any], project: str) -> list[dict[str, Any]]:
    """Distills failures, compiler diagnostics, and execution traces into deterministic Trigger -> Action procedural rules."""
    rules: list[dict[str, Any]] = []

    for fail in failures:
        fail_str = str(fail).strip()
        if not fail_str:
            continue

        # 1. Rust Trait Method Not in Scope (E0599)
        if "error[E0599]" in fail_str or "no method named" in fail_str:
            method_match = re.search(r"no method named [`']([^`']+)['`]", fail_str)
            trait_match = re.search(r"trait [`']([^`']+)['`]", fail_str)
            struct_match = re.search(r"for struct [`']([^`']+)['`]", fail_str)
            method = method_match.group(1) if method_match else "method"
            trait_name = trait_match.group(1) if trait_match else ""
            struct_name = struct_match.group(1) if struct_match else "struct"

            action = f"use {trait_name};" if trait_name else f"Implement or import trait providing `{method}` for `{struct_name}`"
            trigger = f"error[E0599]: no method named `{method}` for `{struct_name}`"
            rules.append({
                "trigger": trigger,
                "action": action,
                "rule_type": "compiler_trait_import",
                "scope": project,
                "confidence": 0.95,
                "engram_block": f"[TRIGGER: {trigger}] -> [ACTION: {action}]",
            })

        # 2. Rust Missing Struct Field (E0063)
        elif "error[E0063]" in fail_str or "missing field" in fail_str:
            field_match = re.search(r"missing field [`']([^`']+)['`]", fail_str)
            struct_match = re.search(r"of [`']([^`']+)['`]", fail_str)
            field = field_match.group(1) if field_match else "field"
            struct_name = struct_match.group(1) if struct_match else "struct"
            trigger = f"error[E0063]: missing field `{field}` in `{struct_name}`"
            action = f"Initialize `{field}` field in struct `{struct_name}`"
            rules.append({
                "trigger": trigger,
                "action": action,
                "rule_type": "struct_field_invariance",
                "scope": project,
                "confidence": 0.95,
                "engram_block": f"[TRIGGER: {trigger}] -> [ACTION: {action}]",
            })

        # 3. Rust Unresolved Import / Module (E0432 / E0433)
        elif "error[E0432]" in fail_str or "error[E0433]" in fail_str or "unresolved import" in fail_str:
            mod_match = re.search(r"unresolved import [`']([^`']+)['`]", fail_str)
            mod_name = mod_match.group(1) if mod_match else "module"
            trigger = f"unresolved import `{mod_name}`"
            action = f"Add `pub mod {mod_name};` or add crate to Cargo.toml"
            rules.append({
                "trigger": trigger,
                "action": action,
                "rule_type": "module_resolution",
                "scope": project,
                "confidence": 0.90,
                "engram_block": f"[TRIGGER: {trigger}] -> [ACTION: {action}]",
            })

        # 4. Rust Mismatched Types (E0308)
        elif "error[E0308]" in fail_str or "mismatched types" in fail_str:
            exp_match = re.search(r"expected [`']([^`']+)['`]", fail_str)
            found_match = re.search(r"found [`']([^`']+)['`]", fail_str)
            expected = exp_match.group(1) if exp_match else "expected_type"
            found = found_match.group(1) if found_match else "found_type"
            trigger = f"error[E0308]: mismatched types: expected `{expected}`, found `{found}`"
            action = f"Convert or cast `{found}` to `{expected}`, or update function signature"
            rules.append({
                "trigger": trigger,
                "action": action,
                "rule_type": "type_mismatch",
                "scope": project,
                "confidence": 0.94,
                "engram_block": f"[TRIGGER: {trigger}] -> [ACTION: {action}]",
            })

        # 5. Rust Cannot Find Value / Function (E0425)
        elif "error[E0425]" in fail_str or "cannot find value" in fail_str:
            val_match = re.search(r"cannot find (?:value|function) [`']([^`']+)['`]", fail_str)
            val_name = val_match.group(1) if val_match else "symbol"
            trigger = f"error[E0425]: cannot find `{val_name}` in this scope"
            action = f"Import `{val_name}` or declare variable/function before use"
            rules.append({
                "trigger": trigger,
                "action": action,
                "rule_type": "scope_resolution",
                "scope": project,
                "confidence": 0.90,
                "engram_block": f"[TRIGGER: {trigger}] -> [ACTION: {action}]",
            })

        # 6. Python ModuleNotFoundError / ImportError
        elif "ModuleNotFoundError" in fail_str or "No module named" in fail_str:
            mod_match = re.search(r"No module named ['\"]([^'\"]+)['\"]", fail_str)
            mod_name = mod_match.group(1) if mod_match else "module"
            trigger = f"ModuleNotFoundError: No module named '{mod_name}'"
            action = f"Add '{mod_name}' to pyproject.toml / requirements.txt or check relative import"
            rules.append({
                "trigger": trigger,
                "action": action,
                "rule_type": "python_dependency",
                "scope": project,
                "confidence": 0.92,
                "engram_block": f"[TRIGGER: {trigger}] -> [ACTION: {action}]",
            })

        # 7. TypeScript / JS Type Errors (TS2322 / TS2339)
        elif "TS2322" in fail_str or "TS2339" in fail_str or "Property" in fail_str and "does not exist on type" in fail_str:
            prop_match = re.search(r"Property ['\"]([^'\"]+)['\"] does not exist on type", fail_str)
            prop = prop_match.group(1) if prop_match else "property"
            trigger = f"TypeScript: Property '{prop}' does not exist on target type"
            action = f"Add '{prop}' to interface or use proper type assertion"
            rules.append({
                "trigger": trigger,
                "action": action,
                "rule_type": "typescript_type_contract",
                "scope": project,
                "confidence": 0.91,
                "engram_block": f"[TRIGGER: {trigger}] -> [ACTION: {action}]",
            })

        # 5. Permission / File Lock Access Denied
        elif "permission" in fail_str.lower() or "access denied" in fail_str.lower():
            trigger = "PermissionDenied / FileLock: Access denied on file resource"
            action = "Close open file handles or apply retry backoff for Windows file lock release"
            rules.append({
                "trigger": trigger,
                "action": action,
                "rule_type": "runtime_permission_lock",
                "scope": project,
                "confidence": 0.88,
                "engram_block": f"[TRIGGER: {trigger}] -> [ACTION: {action}]",
            })

        # 6. Timeout
        elif "timeout" in fail_str.lower() or "timed out" in fail_str.lower():
            trigger = "Timeout: Subprocess or operation exceeded time limit"
            action = "Increase timeout threshold, split batch, or avoid synchronous blocking calls"
            rules.append({
                "trigger": trigger,
                "action": action,
                "rule_type": "runtime_timeout",
                "scope": project,
                "confidence": 0.85,
                "engram_block": f"[TRIGGER: {trigger}] -> [ACTION: {action}]",
            })

        # 7. Generic Fallback Rule
        else:
            short_fail = fail_str[:100]
            trigger = f"ExecutionFailure: {short_fail}"
            action = f"Validate preconditions and add regression test in {project}"
            rules.append({
                "trigger": trigger,
                "action": action,
                "rule_type": "general_invariant",
                "scope": project,
                "confidence": 0.75,
                "engram_block": f"[TRIGGER: {trigger}] -> [ACTION: {action}]",
            })

    return rules


def _build_reflection_report(payload: dict[str, Any]) -> dict[str, Any]:
    failures = _items(payload, "failures")
    changes = _items(payload, "changes")
    goal = _goal(payload)
    project = _project(payload)

    root_causes: list[str] = []
    for fail in failures:
        fail_str = str(fail)
        if "timeout" in fail_str.lower():
            root_causes.append("Operation timed out — potential deadlock, infinite loop, or slow subprocess execution.")
        elif "permission" in fail_str.lower() or "access denied" in fail_str.lower():
            root_causes.append("Permission or access failure — verify file/directory permissions or process privileges.")
        elif "missing" in fail_str.lower() or "not found" in fail_str.lower() or "null" in fail_str.lower():
            root_causes.append("Missing dependency, uninitialized reference, or broken file path contract.")
        elif "syntax" in fail_str.lower() or "parse" in fail_str.lower():
            root_causes.append("Syntax error or invalid format specification.")
        else:
            root_causes.append(f"Execution failure: {fail_str[:120]}")

    if not root_causes:
        root_causes.append("No runtime errors logged; reflection focused on change scope and architectural alignment.")

    candidate_paths = set(_candidate_files(payload, limit=15))
    out_of_scope: list[str] = []
    for change in changes:
        ch_str = str(change)
        if ch_str not in candidate_paths and not any(ch_str in p for p in candidate_paths):
            out_of_scope.append(ch_str)

    extracted_gotchas: list[str] = []
    if failures:
        extracted_gotchas.append(f"Gotcha for {project}: Verify precondition before running {goal[:50]}. Root cause: {root_causes[0]}")
    if out_of_scope:
        extracted_gotchas.append(f"Scope Gotcha: Editing {out_of_scope[0]} was outside initial candidate file bounds.")

    procedural_rules = extract_procedural_rules(failures, changes, project)

    return {
        "project": project,
        "goal": goal,
        "total_failures": len(failures),
        "total_changes": len(changes),
        "root_causes": root_causes,
        "scope_creep_detected": len(out_of_scope) > 0,
        "out_of_scope_files": out_of_scope,
        "extracted_gotchas": extracted_gotchas,
        "procedural_rules": procedural_rules,
        "recommended_memory_actions": [
            f"Record {len(procedural_rules)} procedural rule(s) in Engram table via ozy_memory." if procedural_rules else "No new procedural rules required.",
            f"Record gotcha via ozy_memory action=passive in ## Key Learnings section." if extracted_gotchas else "No new gotchas required.",
            "Summarize clean resolution into session observations." if not failures else "Resolve root causes before session compaction.",
        ],
    }


def reflect(payload: dict[str, Any]) -> BrainResponse:
    failures = _items(payload, "failures")
    changes = _items(payload, "changes")
    goal = _goal(payload)
    ws_root = payload.get("workspace_root") or payload.get("project") or "."
    report = _build_reflection_report(payload)
    procedural_rules = report.get("procedural_rules", [])

    # Reflexion Framework: Verbal Self-Reflection & Failure Triangulation
    verbal_reflection = {}
    persisted_gotcha = None
    if failures:
        primary_fail = str(failures[0])
        verbal_reflection = triangulate_failure(primary_fail, goal=goal, changes=changes)
        target_files = [str(c) for c in changes] or _candidate_files(payload)
        persisted_gotcha = persist_atomic_gotcha_markdown(
            ws_root, primary_fail, verbal_reflection, target_files=target_files
        )

    plan_steps = [
        "Compare intended goal with actual changed files and test output.",
        "Extract root causes and synthesize deterministic Trigger -> Action procedural rules.",
        "Verify if changed files remained within bounded scope.",
        "Consolidate verified rules into durable project gotchas, conventions, and Engram memory.",
    ]

    if verbal_reflection:
        plan_steps.append("[VERBAL REFLECTION] Identified flawed assumption:")
        plan_steps.append(f"  • Assumption: {verbal_reflection['flawed_assumption']}")
        plan_steps.append(f"  • Invariant: {verbal_reflection['violated_invariant']}")
        plan_steps.append(f"  • Antidote: {verbal_reflection['prescriptive_antidote']}")
        if persisted_gotcha:
            plan_steps.append(f"  • Materialized gotcha memory: {persisted_gotcha.name}")

    if procedural_rules:
        plan_steps.append(f"Synthesized {len(procedural_rules)} procedural rule(s):")
        for rule in procedural_rules[:3]:
            plan_steps.append(f"  • {rule['engram_block']}")

    risks = _base_risks(payload)
    if failures:
        risks.append(f"Repeated failures ({len(failures)}) detected; escalate context depth before re-executing fixes.")
    if report["scope_creep_detected"]:
        risks.append(f"Scope creep detected: {len(report['out_of_scope_files'])} file(s) modified outside initial candidate scope.")

    recommendations = [
        f"Review {len(changes)} changed item(s) for compliance with SOLID/DRY principles.",
        "Persist lessons and procedural rules using ozy_memory action=passive when tests pass cleanly.",
    ]

    memory_updates = [r["engram_block"] for r in procedural_rules] if procedural_rules else (report["extracted_gotchas"] or ["Capture reusable gotchas, conventions, and validation commands."])

    structured_plan_dict = _structured_plan(payload, autonomy="reflection_only")
    structured_plan_dict["verbal_reflection"] = verbal_reflection
    structured_plan_dict["persisted_gotcha"] = str(persisted_gotcha) if persisted_gotcha else None

    return BrainResponse(
        action="reflect",
        summary=_base_summary("reflection and procedural consolidation", payload),
        plan=plan_steps,
        risks=risks,
        recommendations=recommendations,
        memory_updates=memory_updates,
        confidence=0.88 if not failures else 0.75,
        suggested_mcp_calls=_safe_mcp_calls(payload, include_graph=False),
        structured_plan=structured_plan_dict,
        execution_policy=_execution_policy(payload, autonomy="reflection_only"),
        brain_context_pack=_brain_context_pack(payload),
        reflection_report=report,
        procedural_rules=procedural_rules,
        provenance=_extract_provenance(payload),
    )
