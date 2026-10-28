import os
import shutil
import tempfile
import pytest
from pathlib import Path

from ozy_brain.dream.generative_mcts import GenerativeMctsSearcher, MctsNode


def test_mcts_uct_calculation():
    searcher = GenerativeMctsSearcher(c_puct=1.414, beta_threat=0.35)
    node = MctsNode(visits=4, value_sum=2.8)  # Q = 0.70
    parent_visits = 16

    # Sin amenaza
    uct_clean = searcher.compute_uct(node, parent_visits, threat_scar=0.0)
    assert uct_clean > 0.70

    # Con cicatriz de amenaza (Amygdala)
    uct_threat = searcher.compute_uct(node, parent_visits, threat_scar=1.0)
    assert uct_threat < uct_clean
    assert abs((uct_clean - uct_threat) - 0.35) < 1e-4


def test_mcts_generative_expansion_and_search():
    temp_dir = Path(tempfile.mkdtemp(prefix="ozy_mcts_test_"))
    try:
        searcher = GenerativeMctsSearcher(
            simulations=10,
            max_depth=3,
            workspace_root=temp_dir,
        )

        root_state = {
            "symbol": "process_payment_webhook",
            "file_path": "app/services/billing.py",
            "syntax_valid": True,
            "backward_compatible": True,
        }

        result = searcher.search(root_state=root_state, threat_scar=0.5)

        assert result["root_visits"] == 10
        assert result["best_q_value"] > 0.0
        assert result["solutions_found"] >= 1
        assert result["vaccine_generated"] is True

        # Verificar que se creó el archivo JSON de vacunas
        vaccine_json = temp_dir / ".ozymem" / "postmortem_vaccines.json"
        assert vaccine_json.exists(), "Debe existir .ozymem/postmortem_vaccines.json"

        # Verificar que se materializó el archivo Markdown en .ozy/memories/pattern/
        pattern_dir = temp_dir / ".ozy" / "memories" / "pattern"
        assert pattern_dir.exists()
        md_files = list(pattern_dir.glob("*.md"))
        assert len(md_files) >= 1
        content = md_files[0].read_text(encoding="utf-8")
        assert "kind: pattern" in content
        assert "process_payment_webhook" in content
        assert "[MCTS VACCINE]" in content

    finally:
        shutil.rmtree(temp_dir, ignore_errors=True)

