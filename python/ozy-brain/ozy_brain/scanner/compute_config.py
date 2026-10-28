"""Compute configuration and ergonomics for Monster Compute vs Lightweight mode.

Allows seamless toggling between unconstrained parallel MCTS compute (for powerful developer rigs)
and throttled single-thread operation (for laptops on battery or low-spec machines).
"""

from __future__ import annotations

import logging
import os
import time
from dataclasses import dataclass
from enum import Enum
from typing import Optional

logger = logging.getLogger("ozy_brain.scanner.compute_config")


class ComputeMode(str, Enum):
    """Execution mode governing compute intensity and CPU resource usage."""
    MONSTER = "MonsterCompute"
    LIGHTWEIGHT = "Lightweight"
    BALANCED = "Balanced"


@dataclass
class MonsterComputeConfig:
    """Configures MCTS depth, parallel workers, blast radius horizons, and CPU throttling."""
    mode: ComputeMode
    mcts_simulations: int
    mcts_max_depth: int
    parallel_workers: int
    blast_radius_max_depth: int
    throttle_sleep_ms: int  # Cooperative CPU yield between evaluations
    max_memory_mb: int

    @classmethod
    def from_env(cls) -> MonsterComputeConfig:
        """Derive compute mode from environment variables OZY_MAX_COMPUTE or OZY_LIGHTWEIGHT_MODE."""
        is_monster = os.environ.get("OZY_MAX_COMPUTE", "").strip().lower() in ("1", "true", "yes")
        is_lightweight = os.environ.get("OZY_LIGHTWEIGHT_MODE", "").strip().lower() in ("1", "true", "yes")

        if is_monster:
            return cls(
                mode=ComputeMode.MONSTER,
                mcts_simulations=60,
                mcts_max_depth=10,
                parallel_workers=max(4, os.cpu_count() or 4),
                blast_radius_max_depth=6,
                throttle_sleep_ms=0,
                max_memory_mb=4096,
            )
        elif is_lightweight:
            return cls(
                mode=ComputeMode.LIGHTWEIGHT,
                mcts_simulations=10,
                mcts_max_depth=4,
                parallel_workers=1,
                blast_radius_max_depth=2,
                throttle_sleep_ms=30,  # Strict CPU throttling < 5%
                max_memory_mb=512,
            )
        else:
            # Default balanced mode
            return cls(
                mode=ComputeMode.BALANCED,
                mcts_simulations=25,
                mcts_max_depth=6,
                parallel_workers=2,
                blast_radius_max_depth=4,
                throttle_sleep_ms=5,
                max_memory_mb=1536,
            )

    def yield_cpu_if_needed(self) -> None:
        """Cooperative yield to keep CPU temperature and battery usage low in lightweight mode."""
        if self.throttle_sleep_ms > 0:
            time.sleep(self.throttle_sleep_ms / 1000.0)

    @property
    def badge(self) -> str:
        """Formatted zero-emoji textual badge."""
        return (
            f"[STATUS: COMPUTE_MODE - {self.mode.value}] "
            f"Simulations: {self.mcts_simulations} | Workers: {self.parallel_workers} | "
            f"Throttle: {self.throttle_sleep_ms}ms"
        )

