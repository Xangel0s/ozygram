"""Multi-Repository Reverse Blast Radius Engine.

Calculates the transitive blast radius in frontend components when backend routes,
schemas, or Pydantic DTOs mutate. Performs BFS traversal over inverted AST import graphs.
"""

from __future__ import annotations

import logging
import re
from collections import deque
from dataclasses import dataclass, field
from pathlib import Path
from typing import Dict, List, Optional, Set

from ozy_brain.scanner.compute_config import MonsterComputeConfig
from ozy_brain.scanner.contract_tracer import FrontendConsumer

logger = logging.getLogger("ozy_brain.scanner.blast_radius")


@dataclass
class BlastRadiusReport:
    """Quantitative measurement of blast radius impact across repositories."""
    target_identifier: str  # Endpoint canonical path or schema name
    direct_consumers: List[str]
    indirect_dependents: List[str]
    affected_views: List[str]
    total_affected: int
    severity: str  # "LOW", "MEDIUM", "CRITICAL_BREAKING"

    @property
    def badge(self) -> str:
        """Standardized zero-emoji textual badge."""
        return (
            f"[ALERT: BLAST_RADIUS_RISK - {self.severity}] Target: {self.target_identifier} | "
            f"Total Affected: {self.total_affected} files (Direct: {len(self.direct_consumers)}, "
            f"Indirect: {len(self.indirect_dependents)}, Views: {len(self.affected_views)})"
        )


class ReverseBlastRadiusEngine:
    """Computes reverse dependency blast radius across full-stack repositories."""

    # Matches ES module imports: import ... from './path' or '@/lib/path'
    IMPORT_RE = re.compile(
        r"""(?:import\s+.*?\s+from\s+["']([^"']+)["']|require\s*\(\s*["']([^"']+)["']\))""",
        re.DOTALL,
    )

    @classmethod
    def build_import_graph(cls, frontend_root: Path) -> Dict[str, Set[str]]:
        """
        Build an inverted dependency graph for the frontend repository.
        Returns: {imported_file_relative: {importer_file_relative, ...}}
        """
        inverted_graph: Dict[str, Set[str]] = {}

        for file_path in frontend_root.rglob("*"):
            if file_path.suffix in (".ts", ".tsx", ".js", ".jsx"):
                if "node_modules" in file_path.parts or ".next" in file_path.parts:
                    continue
                try:
                    content = file_path.read_text(encoding="utf-8", errors="ignore")
                    importer_rel = str(file_path.relative_to(frontend_root)).replace("\\", "/")

                    for match in cls.IMPORT_RE.finditer(content):
                        import_path = match.group(1) or match.group(2)
                        resolved = cls._resolve_import(file_path, import_path, frontend_root)
                        if resolved:
                            inverted_graph.setdefault(resolved, set()).add(importer_rel)
                except Exception as e:
                    logger.debug("Error parsing imports in %s: %s", file_path, e)

        return inverted_graph

    @classmethod
    def _resolve_import(cls, source_file: Path, import_path: str, root_dir: Path) -> Optional[str]:
        """Resolve relative and alias imports (@/...) to relative canonical paths."""
        target: Optional[Path] = None

        if import_path.startswith("."):
            # Relative import
            target = (source_file.parent / import_path).resolve()
        elif import_path.startswith("@/"):
            # Alias import "@/..." -> root_dir / "src" / ... or root_dir / ...
            sub = import_path[2:]
            cand1 = root_dir / "src" / sub
            cand2 = root_dir / sub
            target = cand1 if cand1.parent.exists() else cand2
        else:
            return None  # External npm package

        if not target:
            return None

        # Check candidate extensions
        extensions = ["", ".ts", ".tsx", ".js", ".jsx", "/index.ts", "/index.tsx"]
        for ext in extensions:
            cand = Path(f"{target}{ext}")
            if cand.is_file():
                try:
                    return str(cand.relative_to(root_dir)).replace("\\", "/")
                except ValueError:
                    return str(cand).replace("\\", "/")

        return None

    @classmethod
    def calculate_blast_radius(
        cls,
        target_path: str,
        frontend_consumers: List[FrontendConsumer],
        inverted_graph: Dict[str, Set[str]],
        config: Optional[MonsterComputeConfig] = None,
    ) -> BlastRadiusReport:
        """Compute direct and transitive blast radius for an endpoint or schema."""
        cfg = config or MonsterComputeConfig.from_env()
        max_depth = cfg.blast_radius_max_depth

        # Direct consumers calling this endpoint
        direct_files = sorted(
            list(set(fc.file_path for fc in frontend_consumers if fc.canonical_path == target_path))
        )

        visited: Set[str] = set()
        queue: deque[tuple[str, int]] = deque([(f, 0) for f in direct_files])
        indirect: Set[str] = set()

        while queue:
            current_file, depth = queue.popleft()
            if current_file in visited:
                continue
            visited.add(current_file)

            if depth > 0 and current_file not in direct_files:
                indirect.add(current_file)

            if depth < max_depth:
                # Find importers of current_file
                importers = inverted_graph.get(current_file, set())
                # Also check normalized basename or path variations
                if not importers:
                    for k, v in inverted_graph.items():
                        if current_file.endswith(k) or k.endswith(current_file):
                            importers = importers.union(v)

                for imp in importers:
                    if imp not in visited:
                        queue.append((imp, depth + 1))

        total_affected = len(direct_files) + len(indirect)
        affected_views = sorted(
            [f for f in (set(direct_files) | indirect) if f.endswith((".tsx", ".jsx")) and ("page" in f.lower() or "view" in f.lower() or "sheet" in f.lower() or "table" in f.lower())]
        )

        # Severity classification
        if total_affected <= 2:
            severity = "LOW"
        elif total_affected <= 7:
            severity = "MEDIUM"
        else:
            severity = "CRITICAL_BREAKING"

        return BlastRadiusReport(
            target_identifier=target_path,
            direct_consumers=direct_files,
            indirect_dependents=sorted(list(indirect)),
            affected_views=affected_views,
            total_affected=total_affected,
            severity=severity,
        )

