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
import subprocess
from collections import Counter
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Dict, List, Optional, Set, Tuple

OZY_HEURISTICS_REF = "refs/notes/ozy-heuristics"


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

        from ozy_brain.bbb_immune import ImmunologicalBBB

        record = {
            "pattern_id": pattern_id,
            "title": clean_title,
            "category": category,
            "description": clean_desc,
            "abstract_rules": clean_rules,
            "source_tenant": src_tenant,
            "bbb_inspected": True,
            "bbb_violations_redacted": len(all_violations),
            "peer_confirmations": 0,
            "validation_successes": 1,
            "validation_failures": 0,
            "amygdala_trigger_count": 0,
            "created_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        }
        record["signature_sha256"] = ImmunologicalBBB.compute_pattern_signature(record)

        patterns = self._load_patterns()
        patterns[pattern_id] = record
        self._save_patterns(patterns)

        return {
            "status": "pattern_exported",
            "badge": "[STATUS: CORPUS_CALLOSUM_TRANSFERRED]",
            "pattern_id": pattern_id,
            "signature_sha256": record["signature_sha256"],
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
            if category and p_data.get("category") != category:
                continue

            results.append(p_data)

        return results

    def export_to_git_notes(
        self,
        note_ref: str = OZY_HEURISTICS_REF,
        commit_ref: str = "HEAD",
    ) -> Dict[str, Any]:
        """Serializes patterns to deterministic JSONL sorted by pattern_id and attaches to Git note."""
        patterns = self._load_patterns()
        if not patterns:
            return {"status": "empty", "patterns_count": 0, "badge": "[STATUS: NO_PATTERNS]"}

        lines = []
        for p_id in sorted(patterns.keys()):
            p_data = patterns[p_id]
            lines.append(json.dumps(p_data, sort_keys=True, ensure_ascii=False))
        jsonl_content = "\n".join(lines)

        try:
            cmd = ["git", "notes", f"--ref={note_ref}", "add", "-f", "-m", jsonl_content, commit_ref]
            res = subprocess.run(
                cmd,
                cwd=str(self.workspace_root),
                capture_output=True,
                text=True,
                encoding="utf-8",
                errors="replace",
                timeout=10,
                check=False,
            )
            if res.returncode != 0:
                return {
                    "status": "error",
                    "error": res.stderr.strip(),
                    "badge": "[ALERT: GIT_NOTES_EXPORT_FAILED]",
                }
            return {
                "status": "ok",
                "badge": "[STATUS: CORPUS_GIT_NOTES_EXPORTED]",
                "patterns_count": len(patterns),
                "ref": note_ref,
                "commit": commit_ref,
            }
        except Exception as e:
            return {"status": "error", "error": str(e), "badge": "[ALERT: GIT_NOTES_EXPORT_FAILED]"}

    def import_from_git_notes(
        self,
        note_ref: str = OZY_HEURISTICS_REF,
        limit: int = 50,
    ) -> Dict[str, Any]:
        """Imports peer heuristics from Git note ref, validates through ImmunologicalBBB, and merges."""
        from ozy_brain.bbb_immune import ImmunologicalBBB

        try:
            cmd = ["git", "notes", f"--ref={note_ref}", "list"]
            res = subprocess.run(
                cmd,
                cwd=str(self.workspace_root),
                capture_output=True,
                text=True,
                encoding="utf-8",
                errors="replace",
                timeout=10,
                check=False,
            )
            if res.returncode != 0 or not res.stdout.strip():
                return {
                    "status": "no_notes",
                    "imported_count": 0,
                    "quarantined_count": 0,
                    "apoptosis_count": 0,
                    "badge": "[STATUS: NO_REMOTE_NOTES]",
                }

            commit_hashes = [
                line.strip().split()[-1]
                for line in res.stdout.strip().splitlines()
                if line.strip()
            ][:limit]

            immune_bbb = ImmunologicalBBB(workspace_root=self.workspace_root)
            from ozy_brain.semantic_merger import SemanticHeuristicMerger
            merger = SemanticHeuristicMerger()

            imported_count = 0
            fused_count = 0
            quarantined_count = 0
            apoptosis_count = 0
            local_patterns = self._load_patterns()

            for c_hash in commit_hashes:
                show_cmd = ["git", "notes", f"--ref={note_ref}", "show", c_hash]
                show_res = subprocess.run(
                    show_cmd,
                    cwd=str(self.workspace_root),
                    capture_output=True,
                    text=True,
                    encoding="utf-8",
                    errors="replace",
                    timeout=5,
                    check=False,
                )
                if show_res.returncode != 0 or not show_res.stdout.strip():
                    continue

                for line in show_res.stdout.strip().splitlines():
                    line = line.strip()
                    if not line:
                        continue
                    try:
                        pat = json.loads(line)
                    except Exception:
                        continue

                    # 1. Immunological inspection (Signature + Injection scan)
                    inspection = immune_bbb.inspect_heuristic_safety(pat)
                    if not inspection["is_safe"]:
                        immune_bbb.quarantine_heuristic(pat, reason="; ".join(inspection["violations"]))
                        quarantined_count += 1
                        continue

                    # 2. Apoptosis evaluation
                    should_purge, r_score, _ = immune_bbb.evaluate_apoptosis(pat)
                    if should_purge:
                        apoptosis_count += 1
                        p_id = pat.get("pattern_id")
                        if p_id in local_patterns:
                            del local_patterns[p_id]
                        continue

                    # 3. Semantic deduplication & fusion
                    best_match, sim = merger.find_most_similar_pattern(
                        pat, list(local_patterns.values())
                    )
                    if best_match and sim >= merger.similarity_threshold:
                        canonical_id = best_match.get("pattern_id")
                        if canonical_id and canonical_id in local_patterns:
                            fused = merger.fuse_duplicate_patterns(local_patterns[canonical_id], pat)
                            local_patterns[canonical_id] = fused
                            fused_count += 1
                    else:
                        p_id = pat.get("pattern_id")
                        if p_id in local_patterns:
                            fused = merger.fuse_duplicate_patterns(local_patterns[p_id], pat)
                            local_patterns[p_id] = fused
                            fused_count += 1
                        elif p_id:
                            local_patterns[p_id] = pat
                            imported_count += 1

            self._save_patterns(local_patterns)
            return {
                "status": "ok",
                "badge": "[STATUS: FEDERATED_HEURISTICS_SYNCED]",
                "imported_count": imported_count,
                "fused_count": fused_count,
                "quarantined_count": quarantined_count,
                "apoptosis_count": apoptosis_count,
                "total_active_patterns": len(local_patterns),
            }
        except Exception as e:
            return {"status": "error", "error": str(e), "badge": "[ALERT: GIT_NOTES_IMPORT_FAILED]"}

    def record_peer_feedback(
        self,
        pattern_id: str,
        success: bool,
        error_context: str = "",
    ) -> Dict[str, Any]:
        """Records validation outcome for a pattern and triggers apoptosis if confidence degrades."""
        from ozy_brain.bbb_immune import ImmunologicalBBB

        patterns = self._load_patterns()
        if pattern_id not in patterns:
            return {"status": "not_found", "pattern_id": pattern_id}

        pat = patterns[pattern_id]
        if success:
            pat["validation_successes"] = pat.get("validation_successes", 0) + 1
        else:
            pat["validation_failures"] = pat.get("validation_failures", 0) + 1
            if error_context:
                pat["last_failure_context"] = error_context

        immune_bbb = ImmunologicalBBB(workspace_root=self.workspace_root)
        should_purge, r_score, badge = immune_bbb.evaluate_apoptosis(pat)
        pat["reputation_score"] = r_score

        if should_purge:
            del patterns[pattern_id]
            self._save_patterns(patterns)
            immune_bbb.quarantine_heuristic(pat, reason=f"Apoptosis: reputation dropped to {r_score:.2f}")
            return {
                "status": "apoptosis_purged",
                "badge": badge,
                "pattern_id": pattern_id,
                "reputation_score": r_score,
            }

        patterns[pattern_id] = pat
        self._save_patterns(patterns)
        return {
            "status": "feedback_recorded",
            "badge": badge,
            "pattern_id": pattern_id,
            "reputation_score": r_score,
            "successes": pat.get("validation_successes", 0),
            "failures": pat.get("validation_failures", 0),
        }

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

