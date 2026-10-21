"""Unit tests for Corpus Callosum & Blood-Brain Barrier (BBB) Substrate."""

import tempfile
import unittest
from pathlib import Path

from ozy_brain.corpus_callosum import (
    BloodBrainBarrier,
    CorpusCallosumEngine,
    calculate_shannon_entropy,
)


class TestCorpusCallosum(unittest.TestCase):
    def setUp(self):
        self.temp_dir = tempfile.TemporaryDirectory(ignore_cleanup_errors=True)
        self.workspace = Path(self.temp_dir.name)
        self.bbb = BloodBrainBarrier()
        self.engine = CorpusCallosumEngine(
            workspace_root=self.workspace,
            tenant_id="crmnew",
            bbb=self.bbb,
        )

    def tearDown(self):
        self.temp_dir.cleanup()

    def test_shannon_entropy(self):
        low_entropy = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        h_low = calculate_shannon_entropy(low_entropy)
        self.assertAlmostEqual(h_low, 0.0, places=2)

        # High-entropy random Base64-like string
        high_entropy = "aB3$kL9#mP0!xQ7&zW2@vR5*yT8^"
        h_high = calculate_shannon_entropy(high_entropy)
        self.assertGreaterEqual(h_high, 4.5)

    def test_bbb_secret_redaction(self):
        raw_text = "Here is the key: sk-ant-api03-abcdef1234567890abcdef1234567890 and password='super_secret_password_123'"
        sanitized, violations = self.bbb.sanitize_text(raw_text)

        self.assertNotIn("sk-ant-api03", sanitized)
        self.assertNotIn("super_secret_password_123", sanitized)
        self.assertIn("[REDACTED_API_KEY]", sanitized)
        self.assertGreaterEqual(len(violations), 1)

    def test_bbb_local_path_scrubbing(self):
        raw_text = r"Config located at C:\Users\Lenovo\Documents\secrets.env and /home/deploy/certs.pem"
        sanitized, violations = self.bbb.sanitize_text(raw_text)

        self.assertNotIn("C:\\Users\\Lenovo", sanitized)
        self.assertNotIn("/home/deploy", sanitized)
        self.assertIn("[WORKSPACE_ROOT]/path", sanitized)
        self.assertGreaterEqual(len(violations), 1)

    def test_export_and_import_transferable_pattern(self):
        res = self.engine.export_transferable_pattern(
            pattern_id="outbox_resilient_sync",
            title="Outbox Pattern with SQLite WAL",
            category="backend_architecture",
            description="Uses SQLite WAL for local queues and flushes to Postgres with backoff. Tested at C:\\Users\\Lenovo\\Documents\\app.py",
            abstract_rules=[
                "Always run PRAGMA journal_mode=WAL on SQLite initialization.",
                "Flush pending jobs asynchronously in FastAPI background tasks.",
            ],
            source_tenant="crmnew",
        )

        self.assertEqual(res["status"], "pattern_exported")
        self.assertIn("[STATUS: CORPUS_CALLOSUM_TRANSFERRED]", res["badge"])
        self.assertNotIn("C:\\Users\\Lenovo", res["record"]["description"])

        # Import in another tenant
        target_engine = CorpusCallosumEngine(
            workspace_root=self.workspace,
            tenant_id="default",
            bbb=self.bbb,
        )
        imported = target_engine.import_transferable_patterns(category="backend_architecture")
        self.assertEqual(len(imported), 1)
        self.assertEqual(imported[0]["pattern_id"], "outbox_resilient_sync")
        self.assertEqual(imported[0]["source_tenant"], "crmnew")

    def test_inspect_safety_evaluation(self):
        clean_text = "Standard SOLID refactoring separating data access from service layer."
        clean_report = self.engine.inspect_safety(clean_text)
        self.assertTrue(clean_report["is_safe"])
        self.assertEqual(clean_report["badge"], "[STATUS: BBB_CLEARED]")

        toxic_text = "Use this private key:\n-----BEGIN RSA PRIVATE KEY-----\nMIIEowIBAAKCAQEA...\n-----END RSA PRIVATE KEY-----"
        toxic_report = self.engine.inspect_safety(toxic_text)
        self.assertFalse(toxic_report["is_safe"])
        self.assertEqual(toxic_report["badge"], "[ALERT: BBB_PATHOGEN_INTERCEPTED]")
        self.assertGreaterEqual(toxic_report["violations_count"], 1)


if __name__ == "__main__":
    unittest.main()
