import tempfile
from pathlib import Path

from ozy_brain.ast_preflight import validate_python_patch_in_memory
from ozy_brain.cognitive_router import CognitiveSystemTier, route_cognitive_system
from ozy_brain.neuromodulators import NeuromodulatorEngine
from ozy_brain.stdp_synapse import StdpSynapseEngine
from ozy_brain.working_memory import WorkingMemoryCompactor


def test_active_inference_predictive_surprise_and_routing():
    temp_dir = Path(tempfile.mkdtemp(prefix="ozy_neuro_test_"))
    try:
        engine = NeuromodulatorEngine(workspace_root=temp_dir)
        engine.reset_to_homeostasis()
        initial_ne = engine.state.noradrenaline

        # 1. Ejecución fallida con alta sorpresa (predicho 95% éxito, pero falló)
        surprise = engine.compute_predictive_surprise(
            predicted_success_prob=0.95,
            actual_success=False,
            unexpected_files_count=3,
        )
        assert surprise > 2.0
        assert engine.state.noradrenaline > initial_ne

        # 2. Verificar que NE elevado fuerza al Cognitive Router al Sistema 2 (Comité)
        decision = route_cognitive_system({"workspace_root": str(temp_dir), "goal": "Simple edit"})
        assert decision.tier == CognitiveSystemTier.SYSTEM_2_DELIBERATIVE
        assert decision.requires_committee is True
        assert "[NEURO_ALERT: HIGH_NORADRENALINE]" in decision.reason
    finally:
        import shutil
        shutil.rmtree(temp_dir, ignore_errors=True)


def test_thompson_sampling_stdp_synapses():
    temp_dir = Path(tempfile.mkdtemp(prefix="ozy_thompson_test_"))
    try:
        engine = StdpSynapseEngine(workspace_root=temp_dir)
        # Registrar eventos para crear sinapsis
        engine.record_synapse_spike("app/services/probetas.py", "app/models/probeta.py", dt_seconds=2.0)
        engine.record_synapse_spike("app/services/probetas.py", "app/schemas/probeta.py", dt_seconds=4.0)

        # Muestreo de Thompson
        sampled = engine.sample_coupled_files_thompson("app/services/probetas.py", limit=5)
        assert len(sampled) >= 1
        for s in sampled:
            assert "thompson_sample" in s
            assert 0.0 <= s["thompson_sample"] <= 1.0
    finally:
        import shutil
        shutil.rmtree(temp_dir, ignore_errors=True)


def test_working_memory_compactor_distillation():
    compactor = WorkingMemoryCompactor(token_threshold=100)
    history = """
    Executing command: pytest -v tests/test_billing.py
    ============================= test session starts =============================
    platform win32 -- Python 3.12.0
    rootdir: C:\\Users\\Lenovo\\Documents\\crmnew
    [ALERT: HIGH_FRAGILITY] 'api-geofal-crm/app/services/billing.py' threat score 0.85
    def calculate_invoice_total(order_id: int) -> Decimal:
    def format_invoice_pdf(invoice_id: str) -> bytes:
    Traceback (most recent call last):
      File "api-geofal-crm/app/services/billing.py", line 45, in calculate_invoice_total
    ValueError: Invalid currency format
    """

    digest = compactor.compactify(
        goal="Fix invoice currency rounding bug",
        history=history,
    )

    assert digest.goal == "Fix invoice currency rounding bug"
    assert "api-geofal-crm/app/services/billing.py" in digest.active_files
    assert any(c["name"] == "calculate_invoice_total" for c in digest.symbol_contracts)
    assert len(digest.active_invariants) >= 1
    assert digest.estimated_tokens_saved > 0


def test_ast_preflight_in_memory_validation():
    valid_source = """def add_numbers(a: int, b: int) -> int:
    x = 1
    return a + b
"""
    # 1. Parche limpio y válido
    res_clean = validate_python_patch_in_memory(
        original_source=valid_source,
        target="return a + b",
        replacement="return (a + b) * 2",
    )
    assert res_clean.is_valid is True
    assert res_clean.veto_directive is None

    # 2. Parche con error sintáctico (paréntesis sin cerrar)
    res_syntax = validate_python_patch_in_memory(
        original_source=valid_source,
        target="return a + b",
        replacement="return (a + b * 2",
    )
    assert res_syntax.is_valid is False
    assert res_syntax.error_type == "SyntaxError"
    assert "[CRITICAL_VETO: AST_SYNTAX_CORRUPTION]" in res_syntax.veto_directive

    # 3. Parche con error de indentación
    res_indent = validate_python_patch_in_memory(
        original_source=valid_source,
        target="return a + b",
        replacement="  return a + b",
    )
    assert res_indent.is_valid is False
    assert res_indent.error_type == "IndentationError"
    assert "[CRITICAL_VETO: AST_INDENTATION_CORRUPTION]" in res_indent.veto_directive

    # 4. Target no encontrado
    res_not_found = validate_python_patch_in_memory(
        original_source=valid_source,
        target="def non_existent():",
        replacement="def replaced():",
    )
    assert res_not_found.is_valid is False
    assert res_not_found.error_type == "TargetNotFoundError"
