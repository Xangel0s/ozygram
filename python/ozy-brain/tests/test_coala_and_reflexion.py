import shutil
import tempfile
from pathlib import Path

from ozy_brain.agents.supervisor import SupervisorAgent
from ozy_brain.bayesian_scorer import BayesianEvidenceScorer
from ozy_brain.planner import localize_hierarchical_targets, plan
from ozy_brain.reflector import persist_atomic_gotcha_markdown, reflect, triangulate_failure


def test_bayesian_evidence_scorer():
    scorer = BayesianEvidenceScorer()

    # Escenario 1: Bajo riesgo, alta cobertura -> Aprobado con alta probabilidad
    res_low = scorer.calculate_confidence(
        blast_radius=1, churn_lines=50, test_coverage=0.90, d_kl_dysmetria=0.0
    )
    assert res_low.probability_success >= 0.80
    assert res_low.risk_level == "LOW"
    assert res_low.decision == "APPROVED"
    assert res_low.credible_interval_low < res_low.probability_success < res_low.credible_interval_high

    # Escenario 2: Alto blast radius, alto churn -> Confianza baja
    res_high = scorer.calculate_confidence(
        blast_radius=10, churn_lines=1200, test_coverage=0.30, d_kl_dysmetria=1.5
    )
    assert res_high.probability_success < 0.50
    assert res_high.decision in ("CONDITIONAL_APPROVAL_WITH_SAFEGUARDS", "VETOED")

    # Escenario 3: Bloqueado por veto crítico
    res_blocked = scorer.calculate_confidence(is_blocked=True)
    assert res_blocked.decision == "REJECTED_CRITICAL_VETO"
    assert res_blocked.risk_level == "CRITICAL"


def test_supervisor_dialectic_bayesian_calibration():
    supervisor = SupervisorAgent()
    payload = {
        "goal": "Refactor billing webhooks",
        "candidate_files": ["app/services/billing.py"],
        "test_coverage": 0.85,
        "churn_lines": 120,
    }
    response = supervisor.deliberate_committee(payload)

    assert response.confidence > 0.0
    assert response.confidence != 0.95 or response.confidence != 0.82  # Probabilistic, not hardcoded
    assert "bayesian_evidence" in response.structured_plan
    assert "probability_success" in response.structured_plan["bayesian_evidence"]


def test_reflexion_triangulation_and_atomic_gotcha_persistence():
    temp_dir = Path(tempfile.mkdtemp(prefix="ozy_reflexion_test_"))
    try:
        # 1. Triangulación verbal
        trace = "SyntaxError: unexpected EOF while parsing at line 42"
        triangulation = triangulate_failure(trace, goal="Add new endpoint")
        assert "Tree-Sitter grammar" in triangulation["flawed_assumption"]
        assert "compile cleanly" in triangulation["violated_invariant"]
        assert "ozy_ast_patch" in triangulation["prescriptive_antidote"]

        # 2. Persistencia atómica de gotcha en Markdown
        gotcha_file = persist_atomic_gotcha_markdown(
            temp_dir, trace, triangulation, target_files=["src/routes/api.py"]
        )
        assert gotcha_file is not None
        assert gotcha_file.exists()
        content = gotcha_file.read_text(encoding="utf-8")
        assert "kind: gotcha" in content
        assert "src/routes/api.py" in content
        assert "Flawed Assumption:" in content

        # 3. Llamada e2e a reflect()
        payload = {
            "workspace_root": str(temp_dir),
            "goal": "Update api schema",
            "failures": [trace],
            "changes": ["src/routes/api.py"],
        }
        res = reflect(payload)
        assert "verbal_reflection" in res.structured_plan
        assert res.structured_plan["verbal_reflection"]["violated_invariant"] == triangulation["violated_invariant"]

    finally:
        shutil.rmtree(temp_dir, ignore_errors=True)


def test_agentless_hierarchical_localization():
    candidate_files = ["src/billing/service.py", "src/models/invoice.py"]
    payload = {
        "goal": "Fix invoice status calculation",
        "symbols": [
            {
                "file_path": "src/billing/service.py",
                "name": "calculate_invoice_status",
                "start_line": 45,
                "end_line": 65,
            }
        ],
    }

    locs = localize_hierarchical_targets(candidate_files, payload)
    assert len(locs) == 2

    # El primer archivo debe tener la ventana exacta calculada (+-10 lineas)
    f1 = locs[0]
    assert f1["file"] == "src/billing/service.py"
    assert len(f1["symbols"]) >= 1
    sym = f1["symbols"][0]
    assert sym["symbol"] == "calculate_invoice_status"
    assert sym["window"] == [35, 75]  # [45-10, 65+10]

    # Verificar que plan() integra la localización jerárquica
    plan_res = plan(payload)
    assert "hierarchical_localization" in plan_res.structured_plan
    assert len(plan_res.structured_plan["hierarchical_localization"]) >= 1

