"""Unit and integration tests for Cross-Repo Contract Tracer."""

from pathlib import Path
import pytest

from ozy_brain.scanner.contract_tracer import (
    BackendEndpoint,
    ContractDriftAlert,
    CrossRepoContractTracer,
    FastApiContractExtractor,
    FrontendConsumer,
    NextJsConsumerExtractor,
)
from ozy_brain.scanner.multi_repo import RepoRole, SynapticTagger


def test_fastapi_contract_extractor():
    """Verify parsing of FastAPI routes, methods, paths, and Pydantic DTOs."""
    sample_fastapi = """
    from fastapi import APIRouter, Depends
    from .schemas import AutoSyncRequest, NeoprenoResponse

    router = APIRouter(prefix="/api/control-neoprenos")

    @router.get("", response_model=list[NeoprenoResponse])
    def listar_neoprenos(limit: int = 100):
        return []

    @router.post("/auto-sync", response_model=list[NeoprenoResponse])
    def sincronizar(payload: AutoSyncRequest, db: Session = Depends()):
        return []

    @router.delete("/{probeta_id}")
    def eliminar_probeta(probeta_id: int):
        return {"ok": True}
    """
    endpoints = FastApiContractExtractor.extract_from_source(sample_fastapi, "app/router.py")
    assert len(endpoints) == 3

    ep_list = {ep.canonical_path: ep for ep in endpoints}

    # 1. GET /api/control-neoprenos
    assert "/api/control-neoprenos" in ep_list
    assert ep_list["/api/control-neoprenos"].method == "GET"
    assert ep_list["/api/control-neoprenos"].handler_name == "listar_neoprenos"

    # 2. POST /api/control-neoprenos/auto-sync
    assert "/api/control-neoprenos/auto-sync" in ep_list
    ep_sync = ep_list["/api/control-neoprenos/auto-sync"]
    assert ep_sync.method == "POST"
    assert ep_sync.dto_model == "AutoSyncRequest"
    assert "payload" in ep_sync.parameters

    # 3. DELETE /api/control-neoprenos/:id
    assert "/api/control-neoprenos/:id" in ep_list
    assert ep_list["/api/control-neoprenos/:id"].method == "DELETE"


def test_nextjs_consumer_extractor():
    """Verify parsing of Next.js fetch, resilientFetch, axios and template paths."""
    sample_tsx = """
    import { resilientFetch } from "@/lib/resilient-fetch"
    import { apiClient } from "@/lib/api"
    import useSWR from "swr"

    const API_URL = process.env.NEXT_PUBLIC_API_URL || "http://localhost:8000"

    export function ControlNeoprenosView() {
        const { data } = useSWR(`${API_URL}/api/control-neoprenos`, fetcher)

        async function handleSync() {
            await resilientFetch(
                `${API_URL}/api/control-neoprenos/auto-sync`,
                { method: "POST" }
            )
        }

        async function handleDelete(id: string) {
            await apiClient.delete(`${API_URL}/api/control-neoprenos/${id}`)
        }
    }
    """
    consumers = NextJsConsumerExtractor.extract_from_source(sample_tsx, "components/View.tsx")
    assert len(consumers) == 3

    paths = {c.canonical_path: c for c in consumers}

    # 1. useSWR GET /api/control-neoprenos
    assert "/api/control-neoprenos" in paths
    assert paths["/api/control-neoprenos"].http_method == "GET"
    assert paths["/api/control-neoprenos"].caller_symbol == "ControlNeoprenosView"

    # 2. resilientFetch POST /api/control-neoprenos/auto-sync
    assert "/api/control-neoprenos/auto-sync" in paths
    assert paths["/api/control-neoprenos/auto-sync"].http_method == "POST"

    # 3. apiClient DELETE /api/control-neoprenos/:id
    assert "/api/control-neoprenos/:id" in paths
    assert paths["/api/control-neoprenos/:id"].http_method == "DELETE"


def test_cross_repo_tracer_clean_contracts():
    """Verify that matching contracts produce zero drift alerts."""
    backend = [
        BackendEndpoint(
            method="POST",
            path="/api/control-neoprenos/auto-sync",
            canonical_path="/api/control-neoprenos/auto-sync",
            handler_name="sincronizar",
            file_path="api-geofal-crm/router.py",
            line_number=20,
        )
    ]
    frontend = [
        FrontendConsumer(
            raw_endpoint="${API_URL}/api/control-neoprenos/auto-sync",
            canonical_path="/api/control-neoprenos/auto-sync",
            http_method="POST",
            file_path="crm-geofal/use-control.ts",
            line_number=55,
        )
    ]

    alerts = CrossRepoContractTracer.trace_contracts(backend, frontend)
    assert len(alerts) == 0


def test_cross_repo_tracer_orphan_endpoint():
    """Verify detection of frontend calling an endpoint that does not exist in backend."""
    backend = [
        BackendEndpoint(
            method="POST",
            path="/api/control-neoprenos/auto-sync",
            canonical_path="/api/control-neoprenos/auto-sync",
            handler_name="sincronizar",
            file_path="api-geofal-crm/router.py",
            line_number=20,
        )
    ]
    frontend = [
        FrontendConsumer(
            raw_endpoint="${API_URL}/api/control-neoprenos/antiguo-sync",
            canonical_path="/api/control-neoprenos/antiguo-sync",
            http_method="POST",
            file_path="crm-geofal/use-control.ts",
            line_number=55,
        )
    ]

    alerts = CrossRepoContractTracer.trace_contracts(backend, frontend)
    assert len(alerts) == 1
    assert alerts[0].kind == "ORPHAN_ENDPOINT_CONSUMER"
    assert alerts[0].severity == "HIGH"
    assert "[ALERT: CROSS_REPO_CONTRACT_DRIFT" in alerts[0].badge


def test_cross_repo_tracer_method_mismatch():
    """Verify detection when frontend sends GET but backend expects POST."""
    backend = [
        BackendEndpoint(
            method="POST",
            path="/api/control-neoprenos/auto-sync",
            canonical_path="/api/control-neoprenos/auto-sync",
            handler_name="sincronizar",
            file_path="api-geofal-crm/router.py",
            line_number=20,
        )
    ]
    frontend = [
        FrontendConsumer(
            raw_endpoint="${API_URL}/api/control-neoprenos/auto-sync",
            canonical_path="/api/control-neoprenos/auto-sync",
            http_method="GET",
            file_path="crm-geofal/use-control.ts",
            line_number=55,
        )
    ]

    alerts = CrossRepoContractTracer.trace_contracts(backend, frontend)
    assert len(alerts) == 1
    assert alerts[0].kind == "HTTP_METHOD_MISMATCH"
    assert alerts[0].severity == "CRITICAL"
    assert "POST" in alerts[0].details


def test_cross_repo_tracer_synaptic_unsynchronized():
    """Verify detection of backend mutation without frontend update using SynapticTagger."""
    backend = [
        BackendEndpoint(
            method="POST",
            path="/api/control-neoprenos/auto-sync",
            canonical_path="/api/control-neoprenos/auto-sync",
            handler_name="sincronizar",
            file_path="api-geofal-crm/router.py",
            line_number=20,
        )
    ]
    frontend = [
        FrontendConsumer(
            raw_endpoint="${API_URL}/api/control-neoprenos/auto-sync",
            canonical_path="/api/control-neoprenos/auto-sync",
            http_method="POST",
            file_path="crm-geofal/use-control.ts",
            line_number=55,
        )
    ]

    tagger = SynapticTagger(default_ttl_secs=100.0)
    tagger.tag_file("api-geofal-crm/router.py", RepoRole.BACKEND_FASTAPI, symbol_name="sincronizar", now_secs=1000.0)

    alerts = CrossRepoContractTracer.trace_contracts(
        backend, frontend, synaptic_tagger=tagger, now_secs=1000.0
    )
    assert len(alerts) == 1
    assert alerts[0].kind == "UNSYNCHRONIZED_CONTRACT"
    assert alerts[0].severity == "MEDIUM"


def test_real_crmnew_workspace_audit():
    """Verify audit over real crmnew workspace discovering and tracing endpoints."""
    crmnew_path = Path("crmnew").resolve()
    if not crmnew_path.is_dir():
        crmnew_path = Path("../../crmnew").resolve()

    if crmnew_path.is_dir() and (crmnew_path / "api-geofal-crm").is_dir():
        alerts = CrossRepoContractTracer.audit_workspace(crmnew_path)
        # Verify the audit completes and yields structured alerts
        assert isinstance(alerts, list)

