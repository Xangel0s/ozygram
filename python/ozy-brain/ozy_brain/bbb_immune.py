"""
Immunological Blood-Brain Barrier (BBB) for OzyBrain Federated Mesh.
Provides 3 defense layers:
  1. Cryptographic SHA-256 payload signing and provenance verification.
  2. Antiseptic Prompt-Injection & context-poisoning linter.
  3. Bayesian consensus scoring with automatic heuristic apoptosis for degraded rules.
Zero-emoji compliant.
"""

from __future__ import annotations

import hashlib
import json
import os
import re
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple


class ImmunologicalBBB:
    """Network-level immune defense filter against context poisoning and compromised heuristics."""

    APOPTOSIS_THRESHOLD = 0.35
    MIN_TRIALS_FOR_APOPTOSIS = 3
    ALPHA_0 = 1.0
    BETA_0 = 1.0

    POISONING_PATTERNS = [
        (re.compile(r"(?i)\b(?:ignore|disregard|forget)\s+(?:all\s+)?(?:previous|prior)\s+(?:instructions|prompts|context|rules)\b"), "Context hijacking: instruction reset"),
        (re.compile(r"(?i)\b(?:system\s+override|admin\s+override|jailbreak|developer\s+mode|unrestricted\s+mode)\b"), "Privilege escalation / jailbreak prompt"),
        (re.compile(r"(?i)\b(?:bypass|disable|suppress)\s+(?:all\s+)?(?:security|validation|linter|critic|guards)\b"), "Defensive barrier suppression directive"),
        (re.compile(r"(?i)\b(?:curl|wget)\s+[^\n|]+\|\s*(?:sh|bash|python|powershell)\b"), "Remote shell pipeline injection"),
        (re.compile(r"(?i)\b(?:eval|exec|os\.system|subprocess\.Popen)\s*\("), "Arbitrary code execution primitive"),
        (re.compile(r"(?i)<script[\s>]"), "Embedded HTML/script injection tag"),
    ]

    def __init__(self, workspace_root: Optional[str | Path] = None):
        self.workspace_root = Path(workspace_root or os.getcwd()).resolve()
        self.quarantine_path = self.workspace_root / ".ozymem" / "quarantine_heuristics.json"

    @staticmethod
    def compute_pattern_signature(pattern: Dict[str, Any]) -> str:
        """Computes deterministic SHA-256 digest over canonical pattern attributes."""
        pattern_id = str(pattern.get("pattern_id", "")).strip()
        category = str(pattern.get("category", "")).strip()
        rules = pattern.get("abstract_rules", [])
        rules_canonical = json.dumps(rules, sort_keys=True)
        source_tenant = str(pattern.get("source_tenant", "")).strip()

        payload = f"{pattern_id}|{category}|{rules_canonical}|{source_tenant}"
        return hashlib.sha256(payload.encode("utf-8")).hexdigest()

    def verify_pattern_signature(self, pattern: Dict[str, Any]) -> bool:
        """Verifies cryptographic integrity of incoming pattern."""
        signature = pattern.get("signature_sha256")
        if not signature:
            return False
        expected = self.compute_pattern_signature(pattern)
        return str(signature).lower() == expected.lower()

    def scan_context_poisoning(self, text: str) -> List[str]:
        """Scans arbitrary text for indirect prompt injection or context poisoning vectors."""
        if not text:
            return []
        violations: List[str] = []
        for pattern, desc in self.POISONING_PATTERNS:
            if pattern.search(text):
                violations.append(desc)
        return violations

    def inspect_heuristic_safety(self, pattern: Dict[str, Any]) -> Dict[str, Any]:
        """Full immunological inspection of incoming peer heuristic."""
        all_violations: List[str] = []

        # 1. Signature verification
        has_valid_sig = self.verify_pattern_signature(pattern)
        if not has_valid_sig:
            all_violations.append("Invalid or missing SHA-256 cryptographic signature")

        # 2. Prompt injection scan across title, description, and rules
        title = str(pattern.get("title", ""))
        desc = str(pattern.get("description", ""))
        rules = pattern.get("abstract_rules", [])

        all_violations.extend(self.scan_context_poisoning(title))
        all_violations.extend(self.scan_context_poisoning(desc))
        for r in rules:
            all_violations.extend(self.scan_context_poisoning(str(r)))

        is_safe = len(all_violations) == 0
        badge = "[STATUS: BBB_IMMUNE_CLEARED]" if is_safe else "[ALERT: UNVERIFIED_HEURISTIC_QUARANTINE]"

        return {
            "is_safe": is_safe,
            "badge": badge,
            "violations": all_violations,
            "has_valid_signature": has_valid_sig,
        }

    def evaluate_apoptosis(self, pattern: Dict[str, Any]) -> Tuple[bool, float, str]:
        """Calculates Bayesian consensus reputation R_consenso and determines if apoptosis is triggered."""
        successes = float(
            pattern.get("peer_confirmations", 0)
            + pattern.get("validation_successes", 0)
        )
        failures = float(
            pattern.get("validation_failures", 0)
            + pattern.get("amygdala_trigger_count", 0)
        )
        total_trials = successes + failures

        # Bayesian posterior mean with Beta(alpha_0, beta_0) prior
        r_consenso = (self.ALPHA_0 + successes) / (self.ALPHA_0 + self.BETA_0 + total_trials)

        should_purge = (total_trials >= self.MIN_TRIALS_FOR_APOPTOSIS) and (r_consenso < self.APOPTOSIS_THRESHOLD)
        badge = "[ALERT: HEURISTIC_APOPTOSIS_PURGED]" if should_purge else "[STATUS: CONSENSUS_ACTIVE]"

        return should_purge, round(r_consenso, 4), badge

    def quarantine_heuristic(self, pattern: Dict[str, Any], reason: str) -> None:
        """Stores quarantined heuristic in isolated JSON store for audit."""
        self.quarantine_path.parent.mkdir(parents=True, exist_ok=True)
        data = {}
        if self.quarantine_path.exists():
            try:
                with open(self.quarantine_path, "r", encoding="utf-8") as f:
                    data = json.load(f)
            except Exception:
                data = {}

        p_id = pattern.get("pattern_id") or f"unnamed_{len(data)+1}"
        record = {
            **pattern,
            "quarantine_reason": reason,
            "quarantined_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        }
        data[p_id] = record

        try:
            with open(self.quarantine_path, "w", encoding="utf-8") as f:
                json.dump(data, f, indent=2, ensure_ascii=False)
        except Exception:
            pass
