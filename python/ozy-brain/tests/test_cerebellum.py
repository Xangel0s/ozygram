"""Unit tests for Cerebellum Predictive Forward Model Engine."""

import tempfile
import unittest
from pathlib import Path

from ozy_brain.cerebellum import (
    CerebellumEngine,
    extract_subsystem,
    normalize_rel_path,
)

try:
    import duckdb
    HAS_DUCKDB = True
except ImportError:
    HAS_DUCKDB = False


class TestCerebellumEngine(unittest.TestCase):
    def setUp(self):
        self.temp_dir = tempfile.TemporaryDirectory(ignore_cleanup_errors=True)
        self.workspace = Path(self.temp_dir.name)
        self.duckdb_path = self.workspace / ".ozymem" / "analytics.duckdb"
        self.duckdb_path.parent.mkdir(parents=True, exist_ok=True)

        if HAS_DUCKDB:
            con = duckdb.connect(str(self.duckdb_path))
            con.execute(
                """CREATE TABLE IF NOT EXISTS stdp_synapses (
                    source_file VARCHAR NOT NULL,
                    target_file VARCHAR NOT NULL,
                    weight DOUBLE NOT NULL,
                    sample_count BIGINT NOT NULL DEFAULT 1,
                    last_updated TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                    tenant_id VARCHAR NOT NULL DEFAULT 'local',
                    PRIMARY KEY(source_file, target_file, tenant_id)
                );"""
            )
            con.close()

        self.engine = CerebellumEngine(
            workspace_root=self.workspace,
            duckdb_path=self.duckdb_path,
            tenant_id="test_tenant",
        )

    def tearDown(self):
        self.temp_dir.cleanup()

    def test_extract_subsystem(self):
        self.assertEqual(extract_subsystem("crates/ozymem-server/src/dispatch.rs"), "crates/ozymem-server")
        self.assertEqual(extract_subsystem("python/ozy-brain/ozy_brain/brain.py"), "python/ozy-brain")
        self.assertEqual(extract_subsystem("crmnew/api-geofal-crm/app/main.py"), "crmnew/api-geofal-crm")
        self.assertEqual(extract_subsystem("backend/auth.py"), "backend")
        self.assertEqual(extract_subsystem("root_file.txt"), "root")

    def test_kld_identical_and_divergent(self):
        p = {"crates/ozymem-server": 0.8, "crates/ozymem-core": 0.2}
        q_identical = {"crates/ozymem-server": 0.8, "crates/ozymem-core": 0.2}
        kld_same = self.engine.compute_kullback_leibler_divergence(p, q_identical)
        self.assertAlmostEqual(kld_same, 0.0, places=3)

        q_divergent = {"frontend/ui": 0.9, "docs": 0.1}
        kld_diff = self.engine.compute_kullback_leibler_divergence(p, q_divergent)
        self.assertGreater(kld_diff, 2.0)

    def test_calibrated_motor_patch(self):
        res = self.engine.evaluate_patch_dysmetria(
            target_file="crates/ozymem-server/src/dispatch.rs",
            proposed_files=[
                "crates/ozymem-server/src/dispatch.rs",
                "crates/ozymem-server/src/handlers.rs",
            ],
        )
        self.assertEqual(res["status"], "calibrated")
        self.assertIn("[STATUS: MOTOR_CALIBRATED]", res["badge"])
        self.assertFalse(res["is_dysmetric"])

    def test_motor_overshoot_hypermetria(self):
        # Propose editing 10 files across many disparate subsystems
        scattered_files = [
            "crates/ozymem-server/src/dispatch.rs",
            "crmnew/crm-geofal/pages/index.tsx",
            "crmnew/api-geofal-crm/models/user.py",
            "docs/architecture.md",
            "scripts/deploy.sh",
            "frontend/styles/main.css",
            "packages/shared/constants.ts",
            "tests/e2e/test_login.py",
        ]
        res = self.engine.evaluate_patch_dysmetria(
            target_file="crates/ozymem-server/src/dispatch.rs",
            proposed_files=scattered_files,
        )
        self.assertEqual(res["status"], "overshoot")
        self.assertIn("[ALERT: CODE_DYSMETRIA - MOTOR_OVERSHOOT]", res["badge"])
        self.assertTrue(res["is_dysmetric"])
        self.assertGreater(res["kld"], 1.5)

    def test_motor_undershoot_under_coupled(self):
        if not HAS_DUCKDB:
            return

        # Seed strong Hebbian coupling in DuckDB
        con = duckdb.connect(str(self.duckdb_path))
        con.execute(
            """INSERT INTO stdp_synapses (source_file, target_file, weight, sample_count, tenant_id)
               VALUES ('backend/auth.py', 'frontend/login.tsx', 0.88, 5, 'test_tenant');"""
        )
        con.close()

        # Propose patch only editing backend/auth.py without frontend/login.tsx
        res = self.engine.evaluate_patch_dysmetria(
            target_file="backend/auth.py",
            proposed_files=["backend/auth.py"],
        )
        self.assertEqual(res["status"], "under_coupled")
        self.assertIn("[ALERT: CODE_DYSMETRIA - UNDER_COUPLED]", res["badge"])
        self.assertIn("frontend/login.tsx", res["missing_coupled_files"])
        self.assertTrue(res["is_dysmetric"])


if __name__ == "__main__":
    unittest.main()
