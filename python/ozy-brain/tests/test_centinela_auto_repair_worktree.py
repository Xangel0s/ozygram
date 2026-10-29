from __future__ import annotations

import tempfile
from pathlib import Path

from ozy_brain.brain import run
from ozy_brain.scanner.contract_tracer import (
    ContractDriftAlert,
    CrossRepoContractTracer,
    DualActiveContractShimGenerator,
)


def test_dual_active_contract_shim_generator_orphan_fallback():
    alert = ContractDriftAlert(
        kind="ORPHAN_ENDPOINT_CONSUMER",
        severity="HIGH",
        frontend_file="crm-geofal/src/views/Clients.tsx",
        frontend_line=42,
        canonical_path="/api/v1/clients/active",
        frontend_method="GET",
        backend_file="api-geofal-crm/app/routers/clients.py",
        details="Endpoint called in frontend but not registered in backend.",
    )

    shim = DualActiveContractShimGenerator.generate_shim(alert)
    assert shim is not None
    assert shim.alert_kind == "ORPHAN_ENDPOINT_CONSUMER"
    assert shim.target_route == "/api/v1/clients/active"
    assert "api-geofal-crm/app/routers/clients.py" in shim.target_file
    assert "@router.api_route(\"/api/v1/clients/active\", methods=[\"GET\"]" in shim.shim_code
    assert 'response.headers["X-Ozy-Contract-Shim"] = "active"' in shim.shim_code
    assert 'response.headers["X-Ozy-Deprecated-Path"] = "true"' in shim.shim_code


def test_dual_active_contract_shim_generator_method_mismatch():
    alert = ContractDriftAlert(
        kind="HTTP_METHOD_MISMATCH",
        severity="CRITICAL",
        frontend_file="crm-geofal/src/views/Orders.tsx",
        frontend_line=15,
        canonical_path="/api/v1/orders/cancel",
        frontend_method="POST",
        backend_file="api-geofal-crm/app/routers/orders.py",
        backend_method="DELETE",
        details="Frontend invokes with POST, backend requires DELETE.",
    )

    shim = DualActiveContractShimGenerator.generate_shim(alert)
    assert shim is not None
    assert shim.alert_kind == "HTTP_METHOD_MISMATCH"
    assert "@router.api_route" in shim.shim_code
    assert "methods=['DELETE', 'POST']" in shim.shim_code or "methods=['POST', 'DELETE']" in shim.shim_code


def test_repair_drift_action_handler_end_to_end():
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
            "router = APIRouter(prefix=\"/api/v1/prospects\")\n\n"
            "@router.post(\"\")\n"
            "def create_prospect():\n"
            "    return {'id': 1}\n"
        )
        (routers_dir / "prospects.py").write_text(lead_router_code, encoding="utf-8")

        # 2. Setup Frontend Repo with Method Mismatch and Orphan Consumer
        frontend_dir = root / "crm-geofal"
        frontend_dir.mkdir(parents=True)
        (frontend_dir / ".git").mkdir()
        (frontend_dir / "package.json").write_text('{"name": "crm-geofal", "dependencies": {"next": "16.0.0"}}', encoding="utf-8")
        components_dir = frontend_dir / "src" / "views"
        components_dir.mkdir(parents=True)

        view_code = (
            "import React from 'react';\n"
            "export const ProspectsPage = () => {\n"
            "    // HTTP Method mismatch: frontend calls GET on backend POST route\n"
            "    fetch('/api/v1/prospects', { method: 'GET' });\n"
            "    // Orphan consumer: endpoint does not exist\n"
            "    fetch('/api/v1/prospects/stats');\n"
            "    return <div>Prospects</div>;\n"
            "};\n"
        )
        (components_dir / "ProspectsPage.tsx").write_text(view_code, encoding="utf-8")

        # 3. Invoke repair_drift action
        payload = {"workspace_root": str(root)}
        res = run("repair_drift", payload)

        assert res["action"] == "repair_drift"
        assert "Centinela Self-Repair" in res["summary"]

        structured = res.get("structured_plan", {})
        assert structured.get("drift_alerts_count") >= 2
        assert structured.get("shims_count") >= 2

        shims = structured.get("shims", [])
        kinds = [s["alert_kind"] for s in shims]
        assert "HTTP_METHOD_MISMATCH" in kinds
        assert "ORPHAN_ENDPOINT_CONSUMER" in kinds


def test_repair_drift_action_no_drift_clean():
    with tempfile.TemporaryDirectory() as tmp_dir:
        root = Path(tmp_dir)
        payload = {"workspace_root": str(root)}
        res = run("repair_drift", payload)

        assert res["action"] == "repair_drift"
        assert res["structured_plan"]["shims_count"] == 0
        assert res["confidence"] == 1.0

