from __future__ import annotations

import tempfile
from pathlib import Path
from unittest.mock import MagicMock

from ozy_brain.scanner.contract_tracer import (
    BackendEndpoint,
    CrossRepoContractTracer,
    FrontendConsumer,
)
from ozy_brain.scanner.postgres_engine import PostgresConfig, PostgresNativeEngine
from ozy_brain.scanner.tri_hybrid_orchestrator import TriHybridMemoryOrchestrator


def test_extract_cross_repo_dependency_edges():
    backend_endpoints = [
        BackendEndpoint(
            method="GET",
            path="/api/v1/users",
            canonical_path="/api/v1/users",
            handler_name="get_users",
            file_path="app/routers/users.py",
            line_number=25,
        ),
        BackendEndpoint(
            method="POST",
            path="/api/v1/users",
            canonical_path="/api/v1/users",
            handler_name="create_user",
            file_path="app/routers/users.py",
            line_number=45,
        ),
    ]

    frontend_consumers = [
        FrontendConsumer(
            raw_endpoint="/api/v1/users",
            canonical_path="/api/v1/users",
            http_method="GET",
            file_path="src/components/UserTable.tsx",
            line_number=18,
            caller_symbol="UserTable",
        ),
    ]

    edges = CrossRepoContractTracer.extract_cross_repo_dependency_edges(
        backend_endpoints=backend_endpoints,
        frontend_consumers=frontend_consumers,
    )

    assert len(edges) == 1
    edge = edges[0]
    assert edge["source_file"] == "src/components/UserTable.tsx"
    assert edge["target_file"] == "app/routers/users.py"
    assert edge["edge_kind"] == "CONSUMES_ENDPOINT"
    assert "GET /api/v1/users" in edge["import_statement"]


def test_crawl_workspace_dependency_lineage():
    with tempfile.TemporaryDirectory() as tmp_dir:
        root = Path(tmp_dir)

        # 1. Setup Backend Repo
        backend_dir = root / "api-geofal-crm"
        backend_dir.mkdir(parents=True)
        (backend_dir / ".git").mkdir()
        (backend_dir / "requirements.txt").write_text("fastapi\nuvicorn\n", encoding="utf-8")
        routers_dir = backend_dir / "app" / "routers"
        routers_dir.mkdir(parents=True)

        lead_router_code = (
            "from fastapi import APIRouter\n"
            "router = APIRouter(prefix=\"/api/v1/leads\")\n\n"
            "@router.get(\"\")\n"
            "def list_leads():\n"
            "    return []\n"
        )
        (routers_dir / "leads.py").write_text(lead_router_code, encoding="utf-8")

        # 2. Setup Frontend Repo
        frontend_dir = root / "crm-geofal"
        frontend_dir.mkdir(parents=True)
        (frontend_dir / ".git").mkdir()
        (frontend_dir / "package.json").write_text('{"name": "crm-geofal", "dependencies": {"next": "16.0.0"}}', encoding="utf-8")
        components_dir = frontend_dir / "src" / "views"
        components_dir.mkdir(parents=True)

        view_code = (
            "import React from 'react';\n"
            "export const LeadsPage = () => {\n"
            "    fetch('/api/v1/leads');\n"
            "    return <div>Leads</div>;\n"
            "};\n"
        )
        (components_dir / "LeadsPage.tsx").write_text(view_code, encoding="utf-8")

        # 3. Run crawl
        result = CrossRepoContractTracer.crawl_workspace_dependency_lineage(root)

        assert result["backend_endpoints_count"] >= 1
        assert result["frontend_consumers_count"] >= 1
        assert len(result["cross_repo_edges"]) >= 1

        first_edge = result["cross_repo_edges"][0]
        assert "crm-geofal/src/views/LeadsPage.tsx" in first_edge["source_file"]
        assert "api-geofal-crm/app/routers/leads.py" in first_edge["target_file"]
        assert first_edge["edge_kind"] == "CONSUMES_ENDPOINT"


def test_tri_hybrid_lineage_ingest_graceful_offline():
    with tempfile.TemporaryDirectory(ignore_cleanup_errors=True) as tmp_dir:
        root = Path(tmp_dir)
        sqlite_db = root / "test_memory.db"
        chroma_dir = root / "chroma_test"

        orchestrator = TriHybridMemoryOrchestrator(
            sqlite_path=sqlite_db,
            chroma_dir=chroma_dir,
            pg_config=PostgresConfig(dsn=None),  # Offline mode
        )

        # Bulk insert when offline should return 0 without throwing
        edges = [
            {
                "source_file": "frontend/App.tsx",
                "target_file": "backend/main.py",
                "edge_kind": "CONSUMES_ENDPOINT",
                "import_statement": "GET /api/test",
            }
        ]
        inserted = orchestrator.bulk_insert_ast_dependency_edges(edges)
        assert inserted == 0

        # Ingest workspace lineage should also complete safely
        summary = orchestrator.ingest_workspace_lineage(root)
        assert summary["cross_repo_edges_found"] == 0
        assert summary["edges_inserted"] == 0
        assert summary["drift_alerts_count"] == 0

        orchestrator.close()


def test_bulk_insert_ast_dependency_edges_mocked():
    pg_engine = PostgresNativeEngine(config=PostgresConfig(dsn="postgresql://mock:mock@localhost:5432/mock"))
    pg_engine._is_online = True
    pg_engine.get_connection = MagicMock()

    mock_conn = MagicMock()
    mock_cursor = MagicMock()
    mock_cursor.mogrify.return_value = b"INSERT INTO ozy_ast_dependency_edges VALUES ('src/App.tsx');"
    pg_engine.get_connection.return_value.__enter__.return_value = mock_conn
    mock_conn.cursor.return_value.__enter__.return_value = mock_cursor

    edges = [
        {
            "source_file": "src/App.tsx",
            "target_file": "app/main.py",
            "import_statement": "GET /api/v1/health",
            "edge_kind": "CONSUMES_ENDPOINT",
        },
        {
            "source_file": "src/Users.tsx",
            "target_file": "app/users.py",
            "import_statement": "POST /api/v1/users",
            "edge_kind": "CONSUMES_ENDPOINT",
        },
    ]

    count = pg_engine.bulk_insert_ast_dependency_edges(edges)
    assert count == 2
    assert mock_cursor.execute.called
