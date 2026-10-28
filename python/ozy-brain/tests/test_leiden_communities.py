from __future__ import annotations

import pytest
from ozy_brain.leiden_communities import (
    CodeCommunity,
    LeidenCommunityDetector,
)


def test_leiden_partitioning_modularity():
    # Construct a synthetic multi-module dependency graph:
    # Cluster 1: backend auth
    # Cluster 2: frontend auth
    # Cluster 3: billing service
    files = [
        "app/services/auth.py",
        "app/models/user.py",
        "app/core/security.py",
        "src/components/LoginForm.tsx",
        "src/hooks/useAuth.ts",
        "src/types/user.ts",
        "app/services/billing.py",
        "app/models/invoice.py",
        "app/models/payment.py",
    ]

    edges = [
        {"source": "app/services/auth.py", "target": "app/models/user.py", "weight": 0.9},
        {"source": "app/models/user.py", "target": "app/core/security.py", "weight": 0.85},
        {"source": "app/core/security.py", "target": "app/services/auth.py", "weight": 0.95},
        
        {"source": "src/components/LoginForm.tsx", "target": "src/hooks/useAuth.ts", "weight": 0.9},
        {"source": "src/hooks/useAuth.ts", "target": "src/types/user.ts", "weight": 0.8},
        
        {"source": "app/services/billing.py", "target": "app/models/invoice.py", "weight": 0.9},
        {"source": "app/models/invoice.py", "target": "app/models/payment.py", "weight": 0.85},
    ]

    detector = LeidenCommunityDetector()
    communities = detector.partition_graph(files, edges)

    assert len(communities) >= 2, f"Should detect modular communities, got {len(communities)}"
    
    # Check cross-domain detection:
    # Intra-domain edit (only auth)
    intra = detector.detect_cross_domain_blast(["app/services/auth.py", "app/models/user.py"], communities)
    assert not intra["cross_domain_detected"]

    # Cross-domain edit (auth + billing + frontend)
    cross = detector.detect_cross_domain_blast(
        ["app/services/auth.py", "app/services/billing.py", "src/components/LoginForm.tsx"],
        communities,
    )
    assert cross["cross_domain_detected"]
    assert cross["domains_count"] >= 2
    assert "[CROSS_DOMAIN_ALERT]" in cross["warning"]
