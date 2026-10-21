from __future__ import annotations

import gc
import json
import tempfile
import unittest
from pathlib import Path

from ozy_brain.brain import run
from ozy_brain.neuromodulators import NeuromodulatorEngine, NeuromodulatorState


class TestNeuromodulators(unittest.TestCase):
    def setUp(self):
        self.temp_dir = tempfile.TemporaryDirectory(ignore_cleanup_errors=True)
        self.workspace = Path(self.temp_dir.name)
        self.state_file = self.workspace / ".ozymem" / "neuromodulator_state.json"
        self.engine = NeuromodulatorEngine(
            workspace_root=self.workspace,
            state_file=self.state_file,
        )

    def tearDown(self):
        gc.collect()
        try:
            self.temp_dir.cleanup()
        except Exception:
            pass

    def test_reward_event_produces_positive_rpe_and_dopamine_boost(self):
        initial_d = self.engine.state.dopamine
        res = self.engine.record_reward_event(reward=1.0, expected_reward=0.20)

        self.assertGreater(res["rpe"], 0.0)
        self.assertGreater(res["dopamine"], initial_d)
        self.assertLess(res["noradrenaline"], 0.20)

        # Extreme positive surprise produces flashbulb consolidation
        res_huge = self.engine.record_reward_event(reward=1.0, expected_reward=0.0)
        self.assertTrue(res_huge["flashbulb_consolidation_active"], "High positive surprise must activate flashbulb memory")

    def test_pain_event_causes_noradrenaline_spike_and_amygdala_sensitivity(self):
        res = self.engine.record_pain_event(penalty=0.90, error_context="Repeated compiler crashes")

        self.assertGreaterEqual(res["noradrenaline"], 0.40)
        self.assertLessEqual(res["dopamine"], 0.40)

        hyperparams = self.engine.get_effective_hyperparameters()
        # Noradrenaline spike lowers amygdala trigger threshold
        self.assertLess(hyperparams["amygdala_threat_threshold"], 0.80)
        self.assertIn("NEURO_STATE", hyperparams["badge"])

    def test_novelty_and_familiarity_modulate_acetylcholine(self):
        # 1. Entering unfamiliar code increases Acetylcholine
        res_novel = self.engine.record_code_novelty(is_new_subsystem=True, visit_count=0)
        self.assertGreaterEqual(res_novel["acetylcholine"], 0.70)
        self.assertTrue(res_novel["mcts_exploration_boost"])
        self.assertEqual(res_novel["mode"], "EXPLORATION_MCTS")

        # 2. Familiar code suppresses Acetylcholine to favor Basal Ganglia habits
        res_familiar = self.engine.record_code_novelty(is_new_subsystem=False, visit_count=20)
        self.assertLess(res_familiar["acetylcholine"], res_novel["acetylcholine"])

    def test_persistence_and_reset(self):
        self.engine.record_reward_event(reward=1.0, expected_reward=0.1)
        d_val = self.engine.state.dopamine

        self.assertTrue(self.state_file.exists(), "State must be saved to disk")

        # Reload in fresh engine instance
        reloaded = NeuromodulatorEngine(workspace_root=self.workspace, state_file=self.state_file)
        self.assertEqual(reloaded.state.dopamine, d_val)

        # Reset to homeostasis
        reloaded.reset_to_homeostasis()
        self.assertEqual(reloaded.state.dopamine, 0.50)
        self.assertEqual(reloaded.state.noradrenaline, 0.20)

    def test_brain_dispatch_neuromodulators_action(self):
        payload = {
            "workspace_root": str(self.workspace),
            "sub_action": "reward",
            "reward": 0.95,
            "expected": 0.40,
        }
        res = run("neuromodulators", payload)

        self.assertEqual(res["action"], "neuromodulators")
        self.assertEqual(res["engine"], "ozy-brain-python")
        self.assertIn("NEURO_STATE", res["summary"])
        self.assertIn("neurochemical_vector", res["structured_plan"])


if __name__ == "__main__":
    unittest.main()
