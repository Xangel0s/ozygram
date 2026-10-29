"""
Unit tests for Hippocampal Theta-Gamma Phase Precession Working Memory Buffer.
Standard: Zero-Emoji, SOLID, High Concurrency.
"""

import pytest
from ozy_brain.schemas import BrainResponse, ThetaGammaContext, ThetaGammaPhaseBin
from ozy_brain.working_memory import ThetaGammaWorkingMemoryBuffer


def test_theta_phase_classification():
    """Verify accurate topological Theta phase classification."""
    buffer = ThetaGammaWorkingMemoryBuffer()

    assert buffer.classify_file_phase("app/models/user_schema.py") == ("leaf_contracts", 1)
    assert buffer.classify_file_phase("frontend/src/types/table.d.ts") == ("leaf_contracts", 1)
    assert buffer.classify_file_phase("app/services/billing.py") == ("domain_services", 2)
    assert buffer.classify_file_phase("frontend/src/utils/tableDataCache.ts") == ("domain_services", 2)
    assert buffer.classify_file_phase("app/routers/auth.py") == ("entrypoints", 3)
    assert buffer.classify_file_phase("frontend/src/components/TableEditor.tsx") == ("entrypoints", 3)
    assert buffer.classify_file_phase("tests/test_billing.py") == ("validation", 4)


def test_gamma_window_extraction():
    """Verify extractive +-10 line windowing around target symbol."""
    buffer = ThetaGammaWorkingMemoryBuffer(window_lines=3, max_tokens_per_slot=200)

    # 50 lines of simulated code
    code_lines = [f"// line {i}" for i in range(50)]
    code_lines[25] = "def process_payment(amount: float, currency: str):"
    full_content = "\n".join(code_lines)

    res = buffer.extract_gamma_window(full_content, symbol_hint="process_payment")
    assert res["start_line"] == 23  # 25 - 3 + 1 (1-based)
    assert res["end_line"] == 29    # 25 + 3 + 1
    assert "process_payment" in res["snippet"]
    assert res["tokens"] < 50


def test_pack_context_and_compression():
    """Verify context packing achieves > 60% token compression ratio."""
    buffer = ThetaGammaWorkingMemoryBuffer(window_lines=5)

    files = [
        "app/models/account.py",
        "app/services/account_service.py",
        "app/routers/account_router.py",
        "tests/test_account.py",
    ]

    # Generate 150-line large files
    file_contents = {
        f: "\n".join([f"// boilerplate line {i} in {f}" for i in range(150)] + [f"def target_func_{idx}(): pass"])
        for idx, f in enumerate(files)
    }

    ctx = buffer.pack_context(
        files=files,
        symbol_hints={
            "app/models/account.py": ["target_func_0"],
            "app/services/account_service.py": ["target_func_1"],
            "app/routers/account_router.py": ["target_func_2"],
            "tests/test_account.py": ["target_func_3"],
        },
        file_contents=file_contents,
    )

    assert len(ctx["phases"]) == 4
    assert ctx["total_gamma_tokens"] > 0
    assert ctx["tokens_saved_vs_raw"] > 0
    assert ctx["compression_ratio"] >= 0.60, f"Expected >60% compression, got {ctx['compression_ratio']}"

    # Format prompt text
    prompt = buffer.format_bounded_prompt(ctx)
    assert "[THETA_GAMMA_WORKING_MEMORY - PHASE PRECESSION ACTIVE]" in prompt
    assert "Theta-1: leaf_contracts" in prompt
    assert "Theta-2: domain_services" in prompt
    assert "Theta-3: entrypoints" in prompt
    assert "Theta-4: validation" in prompt


def test_brain_response_schema_compatibility():
    """Verify BrainResponse accepts and serializes theta_gamma_context."""
    buffer = ThetaGammaWorkingMemoryBuffer()
    ctx = buffer.pack_context(files=["app/models/user.py"])

    resp = BrainResponse(
        action="plan",
        summary="Test summary",
        plan=["Step 1"],
        risks=[],
        recommendations=[],
        memory_updates=[],
        confidence=0.92,
        theta_gamma_context=ctx,
    )

    d = resp.to_dict()
    assert "theta_gamma_context" in d
    assert "phases" in d["theta_gamma_context"]
    assert d["theta_gamma_context"]["phases"][0]["phase_name"] == "leaf_contracts"
