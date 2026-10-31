# Copyright (c) 2026 Ozygram Project. All rights reserved.
# Unit tests for compound subdomain and multi-token co-occurrence scoring.
# Zero-Emoji Standard compliant.

from __future__ import annotations

import time
from ozy_brain.schemas import _candidate_file_scores


def test_compound_subdomain_scoring_recepcion_ingenieria():
    payload = {
        "goal": "perfil estratigrafico recepcion ingenieria correo adjunto descartar",
        "files": [
            "api-geofal-crm/app/modules/recepcion/models.py",
            "api-geofal-crm/app/modules/recepcion/excel.py",
            "api-geofal-crm/app/modules/recepcion_ingenieria/mailable.py",
            "crm-geofal/src/components/dashboard/recepcion-ingenieria/RecepcionIngenieriaEmailModal.tsx",
        ],
    }

    scores = _candidate_file_scores(payload, limit=5)
    paths_in_order = [s["path"] for s in scores]

    # Both recepcion_ingenieria targets must rank strictly ahead of the generic recepcion module
    assert "crm-geofal/src/components/dashboard/recepcion-ingenieria/RecepcionIngenieriaEmailModal.tsx" in paths_in_order[:2]
    assert "api-geofal-crm/app/modules/recepcion_ingenieria/mailable.py" in paths_in_order[:2]

    # Verify score disparity: compound domain targets must score significantly higher
    scores_by_path = {s["path"]: s["score"] for s in scores}
    assert scores_by_path["api-geofal-crm/app/modules/recepcion_ingenieria/mailable.py"] >= 18
    assert scores_by_path["crm-geofal/src/components/dashboard/recepcion-ingenieria/RecepcionIngenieriaEmailModal.tsx"] >= 25
    assert scores_by_path["api-geofal-crm/app/modules/recepcion/models.py"] <= 7


def test_base_domain_priority_when_compound_terms_absent():
    # When the goal is specifically for the base reception module, it should rank highest
    payload = {
        "goal": "crear plantilla recepcion de muestras de concreto y probetas",
        "files": [
            "api-geofal-crm/app/modules/recepcion/models.py",
            "api-geofal-crm/app/modules/recepcion/excel.py",
            "api-geofal-crm/app/modules/recepcion_ingenieria/mailable.py",
        ],
    }

    scores = _candidate_file_scores(payload, limit=5)
    top_path = scores[0]["path"]

    # Base reception models/excel must be top ranked
    assert "api-geofal-crm/app/modules/recepcion/" in top_path


def test_compound_subdomain_scoring_performance():
    # Benchmark against 5,000 synthetic paths
    synthetic_files = [f"api-geofal-crm/app/modules/module_{i}/service_{i}.py" for i in range(4998)]
    synthetic_files.append("api-geofal-crm/app/modules/recepcion_ingenieria/router.py")
    synthetic_files.append("crm-geofal/src/components/dashboard/recepcion-ingenieria/RecepcionIngenieriaEmailModal.tsx")

    payload = {
        "goal": "descartar perfil estratigrafico recepcion ingenieria",
        "files": synthetic_files,
    }

    start = time.perf_counter()
    scores = _candidate_file_scores(payload, limit=5)
    elapsed = time.perf_counter() - start

    # Must execute in under 100 ms even for 5,000 files in pure Python
    assert elapsed < 0.10, f"Scoring took {elapsed:.4f}s, expected < 0.10s"
    assert len(scores) >= 2
    assert "recepcion_ingenieria" in scores[0]["path"] or "recepcion-ingenieria" in scores[0]["path"]
