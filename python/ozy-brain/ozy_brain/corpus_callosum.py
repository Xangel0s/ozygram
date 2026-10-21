"""
Corpus Callosum & Blood-Brain Barrier (BBB) Substrate for OzyBrain.
Facilitates secure interhemispheric transfer of abstract architectural heuristics
and meta-patterns across multi-workspace tenants (e.g., crmnew vs default),
guaranteed by a strict Blood-Brain Barrier (BBB) with Shannon Entropy filtering
and secret/path redaction.
Zero-emoji compliant.
"""

from __future__ import annotations

import json
import math
import os
import re
from collections import Counter
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Dict, List, Optional, Set, Tuple


def calculate_shannon_entropy(data: str) -> float:
    """Calculates Shannon entropy H(S) = - sum(p * log2(p)) for a string."""
    if not data:
        return 0.0
    length = len(data)
    counts = Counter(data)
    entropy = 0.0
    for count in counts.values():
        p = count / length
        entropy -= p * math.log2(p)
    return float(entropy)


class BloodBrainBarrier:
    """Security filter acting as a neuro-protective barrier against leaks and toxic tokens."""

    SHANNON_ENTROPY_THRESHOLD = 4.50
    MIN_ENTROPY_TOKEN_LEN = 20

    SECRET_PATTERNS = [
        (re.compile(r"sk-[A-Za-z0-9_-]{20,}"), "[REDACTED_API_KEY]"),
        (re.compile(r"AKIA[0-9A-Z]{16}"), "[REDACTED_AWS_KEY]"),
        (re.compile(r"AIzaSy[A-Za-z0-9_-]{33}"), "[REDACTED_GOOGLE_KEY]"),
        (re.compile(r"-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]*?-----END [A-Z ]*PRIVATE KEY-----"), "[REDACTED_PRIVATE_KEY]"),
        (re.compile(r"Bearer\s+[A-Za-z0-9\._-]{24,}", re.IGNORECASE), "Bearer [REDACTED_BEARER_TOKEN]"),
        (re.compile(r"(?:postgres|mysql|mongodb|redis):\/\/[^\s\"']+"), "[REDACTED_CONNECTION_STRING]"),
        (re.compile(r"(?:password|secret|api_key|token)\s*[:=]\s*['\"][^'\"]{4,}['\"]", re.IGNORECASE), "secret=[REDACTED_SECRET]"),
    ]

    LOCAL_PATH_PATTERNS = [
        (re.compile(r"[A-Za-z]:\\(?:Users|Documents|AppData)\\[^\s\"';:]+"), "[WORKSPACE_ROOT]/path"),
        (re.compile(r"\/(?:home|Users)\/[^\s\"';:]+"), "[WORKSPACE_ROOT]/path"),
    ]

    def sanitize_text(self, text: str) -> Tuple[str, List[str]]:
        """Sanitizes text through regex filtering, local path scrubbing, and Shannon entropy analysis."""
        if not text:
            return "", []

        violations: List[str] = []
        sanitized = text

        # 1. Regex secret matching
        for pattern, replacement in self.SECRET_PATTERNS:
            if pattern.search(sanitized):
                violations.append(f"Secret pattern matched: {replacement}")
                sanitized = pattern.sub(replacement, sanitized)

        # 2. Local system path scrubbing
        for pattern, replacement in self.LOCAL_PATH_PATTERNS:
            if pattern.search(sanitized):
                violations.append("Local filesystem path detected and normalized")
                sanitized = pattern.sub(replacement, sanitized)

        # 3. High-entropy token detection (Shannon Entropy)
        tokens = re.split(r"[\s,;=\'\"`()\[\]{}]+", sanitized)
        for token in tokens:
            if len(token) >= self.MIN_ENTROPY_TOKEN_LEN:
                entropy = calculate_shannon_entropy(token)
                if entropy >= self.SHANNON_ENTROPY_THRESHOLD:
                    # Token exhibits high randomness/entropy
                    violations.append(f"High-entropy token detected (H={entropy:.2f}): length={len(token)}")
                    sanitized = sanitized.replace(token, "[REDACTED_HIGH_ENTROPY_TOKEN]")

        return sanitized, violations


class CorpusCallosumEngine:
    """Interhemispheric Pattern Transfer Engine connecting disparate project workspaces."""

    def __init__(
        self,
        workspace_root: Optional[str | Path] = None,
        tenant_id: str = "default",
        bbb: Optional[BloodBrainBarrier] = None,
    ):
        self.workspace_root = Path(workspace_root or os.getcwd()).resolve()
        self.tenant_id = tenant_id
        self.bbb = bbb or BloodBrainBarrier()
        self.pattern_store_path = self.workspace_root / ".ozymem" / "corpus_callosum_patterns.json"

    def _load_patterns(self) -> Dict[str, Any]:
        if not self.pattern_store_path.exists():
            return {}
        try:
            with open(self.pattern_store_path, "r", encoding="utf-8") as f:
                return json.load(f)
        except Exception:
            return {}

    def _save_patterns(self, patterns: Dict[str, Any]) -> None:
        self.pattern_store_path.parent.mkdir(parents=True, exist_ok=True)
        with open(self.pattern_store_path, "w", encoding="utf-8") as f:
            json.dump(patterns, f, indent=2, ensure_ascii=False)

    def export_transferable_pattern(
        self,
        pattern_id: str,
        title: str,
        category: str,
        description: str,
        abstract_rules: List[str],
        source_tenant: Optional[str] = None,
    ) -> Dict[str, Any]:
        """Sanitizes an architectural pattern through the BBB and persists it for cross-workspace transfer."""
        src_tenant = source_tenant or self.tenant_id
        all_violations: List[str] = []

        # Sanitize all user-provided strings through BBB
        clean_title, v1 = self.bbb.sanitize_text(title)
        clean_desc, v2 = self.bbb.sanitize_text(description)
        all_violations.extend(v1 + v2)

        clean_rules: List[str] = []
        for rule in abstract_rules:
            c_rule, v = self.bbb.sanitize_text(rule)
            clean_rules.append(c_rule)
            all_violations.extend(v)

        record = {
            "pattern_id": pattern_id,
            "title": clean_title,
            "category": category,
            "description": clean_desc,
            "abstract_rules": clean_rules,
            "source_tenant": src_tenant,
            "bbb_inspected": True,
            "bbb_violations_redacted": len(all_violations),
            "created_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        }

        patterns = self._load_patterns()
        patterns[pattern_id] = record
        self._save_patterns(patterns)

        return {
            "status": "pattern_exported",
            "badge": "[STATUS: CORPUS_CALLOSUM_TRANSFERRED]",
            "pattern_id": pattern_id,
            "bbb_violations_count": len(all_violations),
            "record": record,
        }

    def import_transferable_patterns(
        self,
        target_tenant: Optional[str] = None,
        category: Optional[str] = None,
    ) -> List[Dict[str, Any]]:
        """Retrieves BBB-cleared architectural patterns available for cross-workspace application."""
        tgt_tenant = target_tenant or self.tenant_id
        patterns = self._load_patterns()

        results: List[Dict[str, Any]] = []
        for p_id, p_data in patterns.items():
            # Allow patterns from other workspaces or global catalog
            if category and p_data.get("category") != category:
                continue

            results.append(p_data)

        return results

    def inspect_safety(self, text: str) -> Dict[str, Any]:
        """Evaluates whether arbitrary text or patch content passes the Blood-Brain Barrier."""
        sanitized, violations = self.bbb.sanitize_text(text)
        is_safe = len(violations) == 0
        badge = "[STATUS: BBB_CLEARED]" if is_safe else "[ALERT: BBB_PATHOGEN_INTERCEPTED]"

        return {
            "is_safe": is_safe,
            "badge": badge,
            "violations_count": len(violations),
            "violations": violations,
            "sanitized_text": sanitized,
        }
