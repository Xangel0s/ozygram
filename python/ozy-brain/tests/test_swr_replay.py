from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from ozy_brain.dream.generative_mcts import GenerativeMctsSearcher
from ozy_brain.dream.swr_replay import (
    BidirectionalRippleReplayer,
    ErrorGradientRipple,
    SWRReplayDetector,
)


class TestSWRReplay(unittest.TestCase):
    def setUp(self):
        self.temp_dir = tempfile.TemporaryDirectory(ignore_cleanup_errors=True)
        self.workspace = Path(self.temp_dir.name)

    def tearDown(self):
        try:
            self.temp_dir.cleanup()
        except Exception:
            pass

    def test_swr_detector_from_git_signals(self):
        signals = [
            {
                "file_path": "crates/ozymem-core/src/indexing.rs",
                "commit_hash": "a1b2c3d4",
                "commit_msg": "fix: resolve compiler trait arity mismatch in indexing",
                "symbol_name": "index_file",
            },
            {
                "file_path": "crates/ozymem-core/src/indexing.rs",
                "commit_hash": "e5f6g7h8",
                "commit_msg": "revert: bad arity modification",
                "symbol_name": "index_file",
            },
            {
                "file_path": "src/components/Table.tsx",
                "commit_hash": "99998888",
                "commit_msg": "fix: handle null cell rendering",
                "symbol_name": "renderCell",
            },
        ]

        ripples = SWRReplayDetector.detect_ripples_from_signals(signals, min_gradient=0.50)
        self.assertGreaterEqual(len(ripples), 1)

        top_ripple = ripples[0]
        self.assertEqual(top_ripple.target_file, "crates/ozymem-core/src/indexing.rs")
        self.assertGreater(top_ripple.error_gradient, 0.60)
        self.assertIn("swr_", top_ripple.ripple_id)

    def test_bidirectional_replay_and_compression(self):
        ripple = ErrorGradientRipple(
            ripple_id="swr_test_01",
            target_file="app/services/auth.py",
            error_signature="fix: argument arity mismatch on authenticate",
            broken_state_desc="Regression on auth call",
            repaired_state_desc="Contract restored",
            error_gradient=0.85,
            timestamp="2026-10-28T12:00:00Z",
            associated_symbols=["authenticate"],
        )

        replayed = BidirectionalRippleReplayer.replay_ripple(ripple)

        self.assertEqual(replayed["compression_factor"], "15x")
        self.assertEqual(replayed["recommended_mcts_operator"], "ARITY_PARITY")
        self.assertIn("parity", replayed["forward_prescriptive_rule"].lower())
        self.assertIn("hazard", replayed["reverse_boundary_hazard"].lower())
        self.assertEqual(len(replayed["compressed_trajectory"]), 3)

    def test_seed_mcts_tree_convergence_acceleration(self):
        ripple = ErrorGradientRipple(
            ripple_id="swr_mcts_seed",
            target_file="crates/ozymem-core/src/indexing.rs",
            error_signature="fix: null pointer check in parser",
            broken_state_desc="Panic on None option",
            repaired_state_desc="Guard clause added",
            error_gradient=0.90,
            timestamp="2026-10-28T12:00:00Z",
        )

        mcts = GenerativeMctsSearcher(simulations=15, workspace_root=self.workspace)
        result = BidirectionalRippleReplayer.seed_mcts_tree(mcts, ripple)

        self.assertTrue(result["swr_seed_success"])
        self.assertGreaterEqual(result["solutions_found"], 0)
        self.assertGreater(result["best_q_value"], 0.0)
        # Should save ~60% of simulations by warm-starting from SWR replay
        self.assertEqual(result["simulations_executed"], 5)
        self.assertEqual(result["simulations_saved"], 10)


if __name__ == "__main__":
    unittest.main()
