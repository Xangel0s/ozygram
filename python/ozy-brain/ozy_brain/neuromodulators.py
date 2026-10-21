"""
Global Neuromodulator Chemical Substrate for OzyBrain.
Simulates a 4-dimensional biological neurochemical vector N = [Dopamine, Noradrenaline, Acetylcholine, Serotonin]
that dynamically adjusts cognitive hyperparameters across agent workflows:
1. Dopamine (D): Reward Prediction Error (RPE = r - V(s)), triggers immediate "flashbulb" memory consolidation.
2. Noradrenaline (NE): Stress / alertness signal, dynamically modulates Amygdala firing thresholds and blast radius limits.
3. Acetylcholine (ACh): Novelty vs routine router, modulates MCTS exploration constant (c_puct).
4. Serotonin (5-HT): Stability, patience, and risk aversion, penalizing erratic jumping between modules.
Zero-emoji compliant.
"""

from __future__ import annotations

import json
import logging
import os
from dataclasses import asdict, dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Dict, List, Optional

logger = logging.getLogger("ozy_brain.neuromodulators")


@dataclass
class NeuromodulatorState:
    """Normalized neurochemical state vector N in [0.0, 1.0]^4."""
    dopamine: float = 0.50       # Reward / RPE / Motivation
    noradrenaline: float = 0.20  # Stress / Alertness / Amygdala sensitivity
    acetylcholine: float = 0.50  # Novelty / Curiosity / Exploration router
    serotonin: float = 0.60      # Stability / Patience / Risk aversion
    rpe_history: List[float] = field(default_factory=list)
    last_updated: str = ""

    def to_dict(self) -> Dict[str, Any]:
        return asdict(self)


class NeuromodulatorEngine:
    """Engine maintaining and modulating the global neurochemical balance of OzyBrain."""

    def __init__(
        self,
        workspace_root: Optional[str | Path] = None,
        state_file: Optional[str | Path] = None,
    ):
        self.workspace_root = Path(workspace_root or os.getcwd()).resolve()
        self.state_file = Path(state_file) if state_file else self.workspace_root / ".ozymem" / "neuromodulator_state.json"
        self.state = self.load_state()

    def load_state(self) -> NeuromodulatorState:
        """Loads state from disk or initializes homeostatic baseline."""
        if self.state_file.exists():
            try:
                with open(self.state_file, "r", encoding="utf-8") as f:
                    data = json.load(f)
                return NeuromodulatorState(
                    dopamine=float(data.get("dopamine", 0.50)),
                    noradrenaline=float(data.get("noradrenaline", 0.20)),
                    acetylcholine=float(data.get("acetylcholine", 0.50)),
                    serotonin=float(data.get("serotonin", 0.60)),
                    rpe_history=list(data.get("rpe_history", []))[-20:],  # keep last 20
                    last_updated=str(data.get("last_updated", "")),
                )
            except Exception as e:
                logger.warning(f"Failed to read neuromodulator state: {e}. Using baseline.")

        now_iso = datetime.now(timezone.utc).isoformat()
        return NeuromodulatorState(last_updated=now_iso)

    def save_state(self) -> bool:
        """Persists current state to JSON atomically."""
        try:
            self.state_file.parent.mkdir(parents=True, exist_ok=True)
            self.state.last_updated = datetime.now(timezone.utc).isoformat()
            tmp_path = self.state_file.with_suffix(".tmp")
            with open(tmp_path, "w", encoding="utf-8") as f:
                json.dump(self.state.to_dict(), f, indent=2)
            tmp_path.replace(self.state_file)
            return True
        except Exception as e:
            logger.error(f"Failed to save neuromodulator state: {e}")
            return False

    def record_reward_event(
        self,
        reward: float,
        expected_reward: float = 0.50,
        task_label: str = "",
    ) -> Dict[str, Any]:
        """Calculates Reward Prediction Error (RPE = r - V) and potentiates Dopamine."""
        rpe = round(reward - expected_reward, 3)
        self.state.rpe_history.append(rpe)
        if len(self.state.rpe_history) > 20:
            self.state.rpe_history.pop(0)

        # Dopamine surges with positive RPE
        self.state.dopamine = max(0.01, min(1.0, round(self.state.dopamine + 0.30 * rpe, 3)))
        # Stress relieves upon success
        self.state.noradrenaline = max(0.05, min(1.0, round(self.state.noradrenaline - 0.15, 3)))
        # Serotonin elevates with stability
        self.state.serotonin = max(0.10, min(1.0, round(self.state.serotonin + 0.05, 3)))

        flashbulb = self.state.dopamine >= 0.75
        self.save_state()

        return {
            "rpe": rpe,
            "dopamine": self.state.dopamine,
            "noradrenaline": self.state.noradrenaline,
            "flashbulb_consolidation_active": flashbulb,
            "badge": f"[NEURO_REWARD: RPE={rpe:+.2f}, D={self.state.dopamine:.2f}, FLASHBULB={'ON' if flashbulb else 'OFF'}]",
        }

    def record_pain_event(
        self,
        penalty: float = 0.50,
        error_context: str = "",
    ) -> Dict[str, Any]:
        """Records unexpected failure, causing Noradrenaline spike and Dopamine depression."""
        # Noradrenaline spikes (alertness / anxiety)
        self.state.noradrenaline = max(0.05, min(1.0, round(self.state.noradrenaline + 0.25 * penalty, 3)))
        # Dopamine drops
        self.state.dopamine = max(0.05, min(1.0, round(self.state.dopamine - 0.20 * penalty, 3)))
        # Serotonin decreases
        self.state.serotonin = max(0.10, min(1.0, round(self.state.serotonin - 0.10 * penalty, 3)))

        hyper_alert = self.state.noradrenaline >= 0.60
        self.save_state()

        return {
            "penalty": penalty,
            "noradrenaline": self.state.noradrenaline,
            "dopamine": self.state.dopamine,
            "amygdala_hyper_alert": hyper_alert,
            "badge": f"[NEURO_STRESS: NE={self.state.noradrenaline:.2f}, HYPER_ALERT={'ON' if hyper_alert else 'OFF'}]",
        }

    def record_code_novelty(
        self,
        is_new_subsystem: bool,
        visit_count: int = 0,
    ) -> Dict[str, Any]:
        """Modulates Acetylcholine (ACh) based on environment familiarity vs novelty."""
        if is_new_subsystem or visit_count <= 1:
            # High novelty: switch from habit execution to exploratory MCTS
            self.state.acetylcholine = max(0.10, min(1.0, round(self.state.acetylcholine + 0.30, 3)))
        elif visit_count >= 10:
            # High familiarity: engage Basal Ganglia procedural playbooks
            self.state.acetylcholine = max(0.10, min(1.0, round(self.state.acetylcholine - 0.15, 3)))

        exploration_boost = self.state.acetylcholine >= 0.65
        self.save_state()

        return {
            "acetylcholine": self.state.acetylcholine,
            "mcts_exploration_boost": exploration_boost,
            "mode": "EXPLORATION_MCTS" if exploration_boost else "PROCEDURAL_HABIT",
            "badge": f"[NEURO_NOVELTY: ACh={self.state.acetylcholine:.2f}, ROUTE={'EXPLORATION' if exploration_boost else 'HABIT'}]",
        }

    def get_effective_hyperparameters(self) -> Dict[str, Any]:
        """Calculates dynamic operational parameters modulated by current neurochemical state."""
        # High noradrenaline lowers the Amygdala threshold (making it hyper-vigilant)
        amygdala_threshold = max(0.25, round(0.80 - 0.45 * self.state.noradrenaline, 2))

        # High acetylcholine raises MCTS exploration constant c_puct
        mcts_c_puct = round(1.0 + 1.50 * self.state.acetylcholine, 2)

        # High dopamine allows immediate one-shot flashbulb memory writing
        flashbulb_enabled = self.state.dopamine >= 0.75

        # Serotonin + (1 - NE) dictates risk tolerance and patience
        risk_tolerance = round(0.50 * self.state.serotonin + 0.50 * (1.0 - self.state.noradrenaline), 2)

        return {
            "amygdala_threat_threshold": amygdala_threshold,
            "mcts_c_puct": mcts_c_puct,
            "flashbulb_consolidation": flashbulb_enabled,
            "risk_tolerance": risk_tolerance,
            "neurochemical_vector": {
                "dopamine": self.state.dopamine,
                "noradrenaline": self.state.noradrenaline,
                "acetylcholine": self.state.acetylcholine,
                "serotonin": self.state.serotonin,
            },
            "badge": (
                f"[NEURO_STATE: D={self.state.dopamine:.2f}, NE={self.state.noradrenaline:.2f}, "
                f"ACh={self.state.acetylcholine:.2f}, 5HT={self.state.serotonin:.2f} | "
                f"AmygdalaThreshold={amygdala_threshold:.2f}, c_puct={mcts_c_puct:.2f}]"
            ),
        }

    def reset_to_homeostasis(self) -> None:
        """Resets all chemical levels to baseline homeostatic equilibrium."""
        self.state.dopamine = 0.50
        self.state.noradrenaline = 0.20
        self.state.acetylcholine = 0.50
        self.state.serotonin = 0.60
        self.save_state()
