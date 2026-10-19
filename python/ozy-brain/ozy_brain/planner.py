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
            phase_name = "[CONTRATOS] Tipos Base y Esquemas"
            action_desc = "Actualizar definiciones de tipos, estructuras e interfaces primero."
        elif weight == 1:
            phase_name = "[UTILIDADES] Helpers e Indexación Base"
            action_desc = "Implementar funciones auxiliares y lógica de parsing de bajo nivel."
        elif weight == 2:
            phase_name = "[DOMINIO] Lógica de Negocio y Estado"
            action_desc = "Actualizar reglas de negocio, métodos de persistencia y mutaciones."
        else:
            phase_name = "[ENTRYPOINTS] Enrutamiento Público y MCP"
            action_desc = "Adaptar enrutamiento público, manejadores MCP y comandos CLI."

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
            {"name": "context", "objective": "Recopilar evidencia del estado actual antes de editar", "exit_condition": "Archivos relevantes, memorias, grafo y riesgos identificados"},
            {"name": "leaf_contracts", "objective": "Refactorizar contratos base y esquemas primero", "exit_condition": "Estructuras base compilan sin violaciones de contrato"},
            {"name": "domain_services", "objective": "Propagar cambios a través de la lógica de dominio", "exit_condition": "Mutaciones internas y helpers sincronizados"},
            {"name": "entrypoints", "objective": "Adaptar API pública, dispatchers MCP y CLI", "exit_condition": "Manejadores alineados con nuevos contratos de retorno"},
            {"name": "validation", "objective": "Demostrar comportamiento con tests focalizados y generales", "exit_condition": "Comandos de validación pasan exitosamente (cargo test / pytest)"},
            {"name": "learning", "objective": "Persistir conocimiento duradero del proyecto", "exit_condition": "Decisiones, gotchas o patrones guardados en memoria"},
        ],
        "candidate_files": cand_files,
        "suggested_commands": _validation_commands(payload),
        "validation_checks": [
            "Confirmar proyecto activo y branch",
            "Inspeccionar git status antes y después de cambios",
            "Verificar que no se requieran operaciones destructivas",
            "Seguir secuencia topológica de edición (hojas a raíz) para evitar estados rotos",
            "Ejecutar tests relevantes al código modificado",
        ],
        "stop_conditions": [
            "Contexto requerido ausente o contradictorio",
            "Se requeriría una acción destructiva sin aprobación explícita",
            "Fallo de validación no relacionado con el cambio previsto",
            "El alcance de archivos tocados excede el radio de impacto planificado",
        ],
        "what_not_to_touch": [
            "No modificar persistencia de estado core ni esquemas salvo petición explícita",
            "No editar archivos fuera del alcance de archivos candidatos sin recalcular impacto",
            "No alterar configuraciones de seguridad, auth o credenciales",
            "No remover manejo de errores existente ni contratos de aserción",
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
        f"1. [PRE-FLIGHT] Verificación previa: Objetivo '{goal}' | Radio de Impacto: {blast_radius['risk_tier'].upper()} ({blast_radius['total_blast_radius']} archivos).",
        "2. [TOPOLOGÍA] Secuencia de Edición de Hojas a Raíz:",
    ]

    for item in topological_sequence[:5]:
        steps.append(f"   • Paso {item['step']} [{item['phase']}]: {item['file']} → {item['action']}")

    steps.extend([
        "3. [TESTS] Ejecutar tests unitarios focalizados y luego suite de integración (cargo test / pytest).",
        "4. [VALIDACIÓN] Validar que no se rompan contratos ni invariantes en la cadena de dependencias.",
        "5. [MEMORIA] Registrar aprendizajes duraderos y decisiones clave en la memoria de Ozy.",
    ])

    if memories:
        steps.insert(1, "Incorporar lecciones relevantes de memoria: " + "; ".join(memories))

    return BrainResponse(
        action="plan",
        summary=_base_summary("topological plan", payload),
        plan=steps,
        risks=_base_risks(payload) + ([f"Alto radio de impacto: {len(blast_radius['high_severity_files'])} archivo(s) crítico(s) afectado(s)."] if blast_radius["high_severity_files"] else []),
        recommendations=[
            "Refactorizar contratos base y tipos antes de modificar dispatchers públicos.",
            "No ejecutar operaciones destructivas sin confirmación explícita.",
            "Mantener Rust como autoridad de validación y Python como guía consultiva.",
        ],
        memory_updates=["Guardar decisiones, soluciones a bugs y descubrimientos no obvios tras validar."],
        confidence=0.88 if blast_radius["risk_tier"] != "critical" else 0.72,
        suggested_mcp_calls=_safe_mcp_calls(payload),
        structured_plan=_structured_plan(payload),
        execution_policy=_execution_policy(payload),
        brain_context_pack=_brain_context_pack(payload),
        provenance=_extract_provenance(payload),
    )
