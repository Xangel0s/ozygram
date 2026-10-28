from __future__ import annotations

import math
import unittest

from ozy_brain.dream.generative_mcts import GenerativeMctsSearcher, MctsNode
from ozy_brain.neuromodulators import LocusCoeruleusModulator, NeuromodulatorEngine


class TestLocusCoeruleusMCTS(unittest.TestCase):
    def test_phasic_mode_low_stress(self):
        # Baseline NE = 0.20, Surprise = 0.10
        lc_eval = LocusCoeruleusModulator.evaluate_firing_mode(noradrenaline=0.20, surprise=0.10)

        self.assertEqual(lc_eval["firing_mode"], "PHASIC_EXPLOIT")
        self.assertEqual(lc_eval["cognitive_system"], "SYSTEM_1_FAST")
        self.assertAlmostEqual(lc_eval["mcts_c_puct"], math.sqrt(2.0), delta=0.01)
        self.assertEqual(lc_eval["temperature"], 0.10)
        self.assertEqual(lc_eval["badge"], "[LOCUS_COERULEUS: PHASIC_FOCUSED_EXPLOITATION]")

    def test_tonic_mode_high_stress_or_surprise(self):
        # High stress NE = 0.75, Surprise = 2.40
        lc_eval = LocusCoeruleusModulator.evaluate_firing_mode(noradrenaline=0.75, surprise=2.40)

        self.assertEqual(lc_eval["firing_mode"], "TONIC_DIVERGENT")
        self.assertEqual(lc_eval["cognitive_system"], "SYSTEM_2_DELIBERATIVE")
        # Exploration constant should be significantly elevated
        self.assertGreater(lc_eval["mcts_c_puct"], 2.20)
        self.assertGreaterEqual(lc_eval["temperature"], 0.50)
        self.assertEqual(lc_eval["badge"], "[LOCUS_COERULEUS: TONIC_DIVERGENT_EXPLORATION]")

    def test_dynamic_mcts_exploration_modulation(self):
        # 1. Low exploration under phasic firing
        phasic_mode = LocusCoeruleusModulator.evaluate_firing_mode(noradrenaline=0.15)
        mcts_focused = GenerativeMctsSearcher(c_puct=phasic_mode["mcts_c_puct"])

        root = MctsNode(state={"file": "test.py"})
        child = MctsNode(state={"file": "test.py"}, parent=root, visits=2, value_sum=1.0)
        uct_focused = mcts_focused.compute_uct(child, parent_visits=10)

        # 2. Elevated exploration under tonic firing
        tonic_mode = LocusCoeruleusModulator.evaluate_firing_mode(noradrenaline=0.85)
        mcts_divergent = GenerativeMctsSearcher(c_puct=tonic_mode["mcts_c_puct"])
        uct_divergent = mcts_divergent.compute_uct(child, parent_visits=10)

        # UCT exploration score must be strictly higher in tonic mode
        self.assertGreater(uct_divergent, uct_focused)


if __name__ == "__main__":
    unittest.main()

