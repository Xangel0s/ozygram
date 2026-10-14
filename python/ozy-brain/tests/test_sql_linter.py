import tempfile
import unittest
from pathlib import Path
from ozy_brain.data_engine import DataEngine


class TestSqlLinter(unittest.TestCase):
    def test_audit_sql_clean_statements(self):
        sql = """
        CREATE TABLE IF NOT EXISTS users (id INT PRIMARY KEY);
        CREATE INDEX IF NOT EXISTS idx_users ON users(email);
        """
        findings = DataEngine.audit_sql_content(sql, "clean.sql")
        self.assertEqual(len(findings), 0)

    def test_audit_sql_non_idempotent(self):
        sql = "CREATE TABLE orders (id INT PRIMARY KEY);"
        findings = DataEngine.audit_sql_content(sql, "bad.sql")
        self.assertEqual(len(findings), 1)
        self.assertEqual(findings[0]["severity"], "NON_IDEMPOTENT_SQL")
        self.assertEqual(findings[0]["badge"], "[ALERT: NON_IDEMPOTENT_SQL]")

    def test_audit_sql_destructive_unguarded(self):
        sql = "DROP TABLE deprecated_logs;"
        findings = DataEngine.audit_sql_content(sql, "drop.sql")
        destructive = [f for f in findings if f["severity"] == "DESTRUCTIVE_UNGUARDED"]
        self.assertEqual(len(destructive), 1)
        self.assertEqual(destructive[0]["badge"], "[ALERT: DESTRUCTIVE_UNGUARDED]")

    def test_audit_sql_destructive_with_override(self):
        sql = """
        -- OZYMEM_ALLOW_DESTRUCTIVE
        DROP TABLE deprecated_logs;
        """
        findings = DataEngine.audit_sql_content(sql, "guarded.sql")
        destructive = [f for f in findings if f["severity"] == "DESTRUCTIVE_UNGUARDED"]
        self.assertEqual(len(destructive), 0)

    def test_audit_migrations_directory(self):
        with tempfile.TemporaryDirectory() as tmp_str:
            tmp_path = Path(tmp_str)
            engine = DataEngine(project_path=tmp_path, db_path=":memory:")

            clean_file = tmp_path / "001_clean.sql"
            clean_file.write_text("CREATE TABLE IF NOT EXISTS audit (id INT);\n", encoding="utf-8")

            bad_file = tmp_path / "002_bad.sql"
            bad_file.write_text("CREATE TABLE accounts (id INT);\nTRUNCATE TABLE old_data;\n", encoding="utf-8")

            res = engine.audit_migrations(tmp_path)
            self.assertEqual(res["status"], "[STATUS: ALERT]")
            self.assertEqual(res["files_scanned"], 2)
            self.assertGreaterEqual(len(res["findings"]), 2)
            self.assertIn("[ALERT: NON_IDEMPOTENT_SQL]", res["report"])
            self.assertIn("[ALERT: DESTRUCTIVE_UNGUARDED]", res["report"])


if __name__ == "__main__":
    unittest.main()
