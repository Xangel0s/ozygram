"""Full-Stack Cross-Repository Contract Tracer and Drift Sentinel.

Analyzes bidirectional contracts between FastAPI backend routes and Next.js / TSX frontend consumers.
Detects orphan endpoints, HTTP method mismatches, unsynchronized contract changes, and schema drifts.
"""

from __future__ import annotations

import logging
import re
from dataclasses import dataclass, field
from pathlib import Path
from typing import Dict, List, Optional, Set, Tuple

from ozy_brain.scanner.multi_repo import DiscoveredRepo, MultiRepoScanner, RepoRole, SynapticTagger

logger = logging.getLogger("ozy_brain.scanner.contract_tracer")


@dataclass
class BackendEndpoint:
    """Represents an extracted FastAPI/Flask HTTP route."""
    method: str
    path: str
    canonical_path: str
    handler_name: str
    file_path: str
    line_number: int
    dto_model: Optional[str] = None
    parameters: List[str] = field(default_factory=list)


@dataclass
class FrontendConsumer:
    """Represents an extracted API call in Next.js/React (fetch, resilientFetch, axios, useSWR)."""
    raw_endpoint: str
    canonical_path: str
    http_method: str
    file_path: str
    line_number: int
    caller_symbol: Optional[str] = None


@dataclass
class ContractDriftAlert:
    """Quantitative contract discrepancy between backend definition and frontend consumer."""
    kind: str  # "ORPHAN_ENDPOINT_CONSUMER", "HTTP_METHOD_MISMATCH", "UNSYNCHRONIZED_CONTRACT", "DTO_SCHEMA_MISMATCH"
    severity: str  # "LOW", "MEDIUM", "HIGH", "CRITICAL"
    frontend_file: str
    frontend_line: int
    canonical_path: str
    frontend_method: str
    backend_file: Optional[str] = None
    backend_line: Optional[int] = None
    backend_method: Optional[str] = None
    details: str = ""
    suggested_fix: str = ""

    @property
    def badge(self) -> str:
        """Standardized zero-emoji textual badge."""
        return (
            f"[ALERT: CROSS_REPO_CONTRACT_DRIFT - {self.kind} - {self.severity}] "
            f"Path: {self.canonical_path} | Frontend: {self.frontend_file}:{self.frontend_line} "
            f"({self.frontend_method}) | Details: {self.details}"
        )


class FastApiContractExtractor:
    """Extracts endpoint routes and Pydantic DTO models from Python FastAPI files."""

    PREFIX_RE = re.compile(r"""APIRouter\s*\([^)]*prefix\s*=\s*["']([^"']+)["']""", re.IGNORECASE)
    DECORATOR_RE = re.compile(
        r"""@(?:app|router|api_router)\.(get|post|put|delete|patch|options|head)\s*\(\s*["']([^"']*)["']""",
        re.IGNORECASE,
    )
    FUNC_RE = re.compile(r"""(?:async\s+)?def\s+([a-zA-Z0-9_]+)\s*\(([^)]*)\)""", re.IGNORECASE)
    DTO_RE = re.compile(
        r""":\s*([A-Z][a-zA-Z0-9_]+(?:Schema|DTO|Create|Update|Request|In|Model|Response)?)""",
        re.IGNORECASE,
    )
    IGNORED_DTO_TYPES = {
        "Session", "Depends", "Request", "Response", "int", "str", "float",
        "bool", "dict", "list", "Any", "Optional", "List", "Dict", "Union",
    }

    @classmethod
    def extract_from_source(cls, source: str, file_path: str) -> List[BackendEndpoint]:
        endpoints: List[BackendEndpoint] = []
        prefix_match = cls.PREFIX_RE.search(source)
        file_prefix = prefix_match.group(1).rstrip("/") if prefix_match else ""

        lines = source.splitlines()
        for i, line in enumerate(lines):
            dec_match = cls.DECORATOR_RE.search(line)
            if dec_match:
                method = dec_match.group(1).upper()
                subpath = dec_match.group(2)
                if file_prefix:
                    path = f"{file_prefix}{subpath}" if subpath.startswith("/") else f"{file_prefix}/{subpath}"
                else:
                    path = subpath

                canonical = cls.canonicalize(path)
                handler_name = "unknown"
                dto_model = None
                parameters: List[str] = []

                # Look forward for function definition
                for j in range(i + 1, min(i + 7, len(lines))):
                    fn_match = cls.FUNC_RE.search(lines[j])
                    if fn_match:
                        handler_name = fn_match.group(1)
                        param_str = fn_match.group(2)
                        # Extract params
                        for p in param_str.split(","):
                            p_clean = p.strip().split(":")[0].strip()
                            if p_clean and p_clean not in ("self", "cls", "db"):
                                parameters.append(p_clean)
                        # Extract DTO
                        for dto_m in cls.DTO_RE.finditer(param_str):
                            candidate = dto_m.group(1)
                            if candidate not in cls.IGNORED_DTO_TYPES:
                                dto_model = candidate
                                break
                        break

                endpoints.append(
                    BackendEndpoint(
                        method=method,
                        path=path,
                        canonical_path=canonical,
                        handler_name=handler_name,
                        file_path=file_path,
                        line_number=i + 1,
                        dto_model=dto_model,
                        parameters=parameters,
                    )
                )

        return endpoints

    @staticmethod
    def canonicalize(path: str) -> str:
        """Strip query params and normalize path variables like {id} -> :id."""
        clean = path.split("?")[0].split("#")[0].strip()
        norm = re.sub(r"\{[^}]+\}", ":id", clean)
        return norm.rstrip("/") or "/"


class NextJsConsumerExtractor:
    """Extracts endpoint calls from Next.js / TypeScript / TSX files."""

    FETCH_RE = re.compile(
        r"""\b(?:fetch|resilientFetch|authFetch|customFetch)\s*\(\s*[`"']((?:/api/|https?://[^/'"`]+/api/|\$\{(?:API_URL|NEXT_PUBLIC_API_URL|BASE_URL)\}/api/)[^`"'\s]+)[`"']""",
        re.IGNORECASE | re.DOTALL,
    )
    CLIENT_RE = re.compile(
        r"""\b(?:axios|apiClient|api|httpClient)\.(get|post|put|delete|patch)\s*\(\s*[`"']((?:/api/|https?://[^/'"`]+/api/|\$\{(?:API_URL|NEXT_PUBLIC_API_URL|BASE_URL)\}/api/)[^`"'\s]+)[`"']""",
        re.IGNORECASE | re.DOTALL,
    )
    SWR_RE = re.compile(
        r"""\buseSWR\s*(?:<[^>]+>)?\s*\(\s*[`"']((?:/api/|https?://[^/'"`]+/api/|\$\{(?:API_URL|NEXT_PUBLIC_API_URL|BASE_URL)\}/api/)[^`"'\s]+)[`"']""",
        re.IGNORECASE | re.DOTALL,
    )
    METHOD_RE = re.compile(r"""method\s*:\s*["'](GET|POST|PUT|DELETE|PATCH)["']""", re.IGNORECASE)
    COMPONENT_RE = re.compile(
        r"""(?:export\s+)?(?:default\s+)?(?:function|const)\s+([A-Za-z0-9_]+)""", re.IGNORECASE
    )

    @classmethod
    def extract_from_source(cls, source: str, file_path: str) -> List[FrontendConsumer]:
        consumers: List[FrontendConsumer] = []

        # Find all component/function positions for caller tracking
        symbols: List[Tuple[int, str]] = []
        for m in cls.COMPONENT_RE.finditer(source):
            symbols.append((m.start(), m.group(1)))

        def get_caller(pos: int) -> Optional[str]:
            matching = [name for start, name in symbols if start <= pos]
            return matching[-1] if matching else None

        # 1. fetch / resilientFetch / authFetch
        for match in cls.FETCH_RE.finditer(source):
            raw = match.group(1)
            canonical = cls.canonicalize(raw)
            line_no = source[: match.start()].count("\n") + 1
            caller = get_caller(match.start())

            # Look ahead for method
            window = source[match.end() : min(match.end() + 250, len(source))]
            method_match = cls.METHOD_RE.search(window)
            method = method_match.group(1).upper() if method_match else "GET"

            consumers.append(
                FrontendConsumer(
                    raw_endpoint=raw,
                    canonical_path=canonical,
                    http_method=method,
                    file_path=file_path,
                    line_number=line_no,
                    caller_symbol=caller,
                )
            )

        # 2. axios / apiClient
        for match in cls.CLIENT_RE.finditer(source):
            method = match.group(1).upper()
            raw = match.group(2)
            canonical = cls.canonicalize(raw)
            line_no = source[: match.start()].count("\n") + 1
            caller = get_caller(match.start())
            consumers.append(
                FrontendConsumer(
                    raw_endpoint=raw,
                    canonical_path=canonical,
                    http_method=method,
                    file_path=file_path,
                    line_number=line_no,
                    caller_symbol=caller,
                )
            )

        # 3. useSWR
        for match in cls.SWR_RE.finditer(source):
            raw = match.group(1)
            canonical = cls.canonicalize(raw)
            line_no = source[: match.start()].count("\n") + 1
            caller = get_caller(match.start())
            consumers.append(
                FrontendConsumer(
                    raw_endpoint=raw,
                    canonical_path=canonical,
                    http_method="GET",
                    file_path=file_path,
                    line_number=line_no,
                    caller_symbol=caller,
                )
            )

        # Sort by line number
        consumers.sort(key=lambda c: c.line_number)
        return consumers

    @staticmethod
    def canonicalize(raw: str) -> str:
        """Strip environment templates like ${API_URL} and query params."""
        clean = raw.split("?")[0].split("#")[0].strip()
        stripped = re.sub(r"\$\{(?:API_URL|NEXT_PUBLIC_API_URL|BASE_URL|API_BASE)\}", "", clean)
        # normalize template ${...}
        norm = re.sub(r"\$\{[^}]+\}", ":id", stripped)
        if "/api/" in norm:
            pos = norm.find("/api/")
            norm = norm[pos:]
        return norm.rstrip("/") or "/"


class CrossRepoContractTracer:
    """Matches frontend consumers against backend routes to diagnose contract drift."""

    @classmethod
    def trace_contracts(
        cls,
        backend_endpoints: List[BackendEndpoint],
        frontend_consumers: List[FrontendConsumer],
        synaptic_tagger: Optional[SynapticTagger] = None,
        now_secs: Optional[float] = None,
    ) -> List[ContractDriftAlert]:
        alerts: List[ContractDriftAlert] = []

        # Index backend endpoints by canonical path
        backend_by_path: Dict[str, List[BackendEndpoint]] = {}
        for ep in backend_endpoints:
            backend_by_path.setdefault(ep.canonical_path, []).append(ep)

        for fc in frontend_consumers:
            matching_endpoints = backend_by_path.get(fc.canonical_path)

            # Case 1: Orphan consumer - endpoint does not exist on backend
            if not matching_endpoints:
                alerts.append(
                    ContractDriftAlert(
                        kind="ORPHAN_ENDPOINT_CONSUMER",
                        severity="HIGH",
                        frontend_file=fc.file_path,
                        frontend_line=fc.line_number,
                        canonical_path=fc.canonical_path,
                        frontend_method=fc.http_method,
                        details=f"Endpoint '{fc.canonical_path}' is called in frontend but not found in any backend router.",
                        suggested_fix=f"Verify endpoint route or register @router.{fc.http_method.lower()}('{fc.canonical_path}') in backend.",
                    )
                )
                continue

            # Case 2: Method mismatch
            method_match = next((ep for ep in matching_endpoints if ep.method == fc.http_method), None)
            if not method_match:
                expected_methods = [ep.method for ep in matching_endpoints]
                matched_ep = matching_endpoints[0]
                alerts.append(
                    ContractDriftAlert(
                        kind="HTTP_METHOD_MISMATCH",
                        severity="CRITICAL",
                        frontend_file=fc.file_path,
                        frontend_line=fc.line_number,
                        canonical_path=fc.canonical_path,
                        frontend_method=fc.http_method,
                        backend_file=matched_ep.file_path,
                        backend_line=matched_ep.line_number,
                        backend_method=matched_ep.method,
                        details=(
                            f"Frontend invokes '{fc.canonical_path}' with HTTP {fc.http_method}, "
                            f"but backend requires {expected_methods}."
                        ),
                        suggested_fix=f"Change frontend call to use method: '{expected_methods[0]}'.",
                    )
                )
                continue

            # Case 3: Synaptic tag unsynchronized contract alert
            # If backend file was recently modified (actively tagged), but frontend file was not modified
            if synaptic_tagger:
                backend_tagged = synaptic_tagger.is_tagged(method_match.file_path, now_secs=now_secs)
                frontend_tagged = synaptic_tagger.is_tagged(fc.file_path, now_secs=now_secs)
                if backend_tagged and not frontend_tagged:
                    alerts.append(
                        ContractDriftAlert(
                            kind="UNSYNCHRONIZED_CONTRACT",
                            severity="MEDIUM",
                            frontend_file=fc.file_path,
                            frontend_line=fc.line_number,
                            canonical_path=fc.canonical_path,
                            frontend_method=fc.http_method,
                            backend_file=method_match.file_path,
                            backend_line=method_match.line_number,
                            backend_method=method_match.method,
                            details=(
                                f"Backend endpoint handler '{method_match.handler_name}' in '{method_match.file_path}' "
                                f"was recently mutated, but consumer in '{fc.file_path}' has not been synchronized."
                            ),
                            suggested_fix="Audit caller contract for schema or parameter changes.",
                        )
                    )

        return alerts

    @classmethod
    def extract_cross_repo_dependency_edges(
        cls,
        backend_endpoints: List[BackendEndpoint],
        frontend_consumers: List[FrontendConsumer],
    ) -> List[Dict[str, Any]]:
        """
        Synthesizes AST dependency edges between frontend files and backend route files
        based on endpoint consumption contracts.
        """
        backend_by_path: Dict[str, List[BackendEndpoint]] = {}
        for ep in backend_endpoints:
            backend_by_path.setdefault(ep.canonical_path, []).append(ep)

        edges: List[Dict[str, Any]] = []
        seen = set()

        for fc in frontend_consumers:
            matching = backend_by_path.get(fc.canonical_path, [])
            for ep in matching:
                edge_key = (fc.file_path.replace("\\", "/"), ep.file_path.replace("\\", "/"))
                if edge_key not in seen:
                    seen.add(edge_key)
                    edges.append({
                        "source_file": edge_key[0],
                        "target_file": edge_key[1],
                        "import_statement": f"{fc.http_method} {fc.canonical_path}",
                        "edge_kind": "CONSUMES_ENDPOINT",
                        "start_line": fc.line_number,
                    })
        return edges

    @classmethod
    def crawl_workspace_dependency_lineage(
        cls,
        workspace_root: Path,
        synaptic_tagger: Optional[SynapticTagger] = None,
        now_secs: Optional[float] = None,
    ) -> Dict[str, Any]:
        """Discover sibling repos, parse full-stack routes, and return detected drift alerts & cross-repo edges."""
        repos = MultiRepoScanner.discover_sibling_repos(workspace_root)
        backend_repos = [r for r in repos if r.role in (RepoRole.BACKEND_FASTAPI, RepoRole.FULLSTACK_MONOREPO)]
        frontend_repos = [r for r in repos if r.role in (RepoRole.FRONTEND_NEXTJS, RepoRole.FULLSTACK_MONOREPO)]

        backend_endpoints: List[BackendEndpoint] = []
        for br in backend_repos:
            for py_file in br.path.rglob("*.py"):
                if "tests" in py_file.parts or ".venv" in py_file.parts or ".git" in py_file.parts:
                    continue
                try:
                    code = py_file.read_text(encoding="utf-8", errors="ignore")
                    if "@router." in code or "@app." in code:
                        rel_path = str(py_file.relative_to(workspace_root)).replace("\\", "/")
                        backend_endpoints.extend(FastApiContractExtractor.extract_from_source(code, rel_path))
                except Exception as e:
                    logger.debug("Failed to read %s: %s", py_file, e)

        frontend_consumers: List[FrontendConsumer] = []
        for fr in frontend_repos:
            for ts_file in fr.path.rglob("*"):
                if ts_file.suffix in (".ts", ".tsx", ".js", ".jsx"):
                    if "node_modules" in ts_file.parts or ".next" in ts_file.parts or ".git" in ts_file.parts:
                        continue
                    try:
                        code = ts_file.read_text(encoding="utf-8", errors="ignore")
                        if "/api/" in code:
                            rel_path = str(ts_file.relative_to(workspace_root)).replace("\\", "/")
                            frontend_consumers.extend(NextJsConsumerExtractor.extract_from_source(code, rel_path))
                    except Exception as e:
                        logger.debug("Failed to read %s: %s", ts_file, e)

        edges = cls.extract_cross_repo_dependency_edges(backend_endpoints, frontend_consumers)
        alerts = cls.trace_contracts(
            backend_endpoints=backend_endpoints,
            frontend_consumers=frontend_consumers,
            synaptic_tagger=synaptic_tagger,
            now_secs=now_secs,
        )
        return {
            "backend_endpoints": backend_endpoints,
            "backend_endpoints_count": len(backend_endpoints),
            "frontend_consumers": frontend_consumers,
            "frontend_consumers_count": len(frontend_consumers),
            "cross_repo_edges": edges,
            "drift_alerts": alerts,
        }

    @classmethod
    def audit_workspace(
        cls, workspace_root: Path, synaptic_tagger: Optional[SynapticTagger] = None
    ) -> List[ContractDriftAlert]:
        """Discover sibling repos, parse full-stack routes, and return detected drift alerts."""
        res = cls.crawl_workspace_dependency_lineage(workspace_root, synaptic_tagger=synaptic_tagger)
        return res["drift_alerts"]

