import pytest
from pathlib import Path
from ozy_brain.data_engine import DataEngine

def test_audit_sql_clean_statements():
    sql = """
    CREATE TABLE IF NOT EXISTS users (id INT PRIMARY KEY);
    CREATE INDEX IF NOT EXISTS idx_users ON users(email);
    """
    findings = DataEngine.audit_sql_content(sql, "clean.sql")
    assert len(findings) == 0

def test_audit_sql_non_idempotent():
    sql = "CREATE TABLE orders (id INT PRIMARY KEY);"
    findings = DataEngine.audit_sql_content(sql, "bad.sql")
    assert len(findings) == 1
    assert findings[0]["severity"] == "NON_IDEMPOTENT_SQL"
    assert findings[0]["badge"] == "[ALERT: NON_IDEMPOTENT_SQL]"

def test_audit_sql_destructive_unguarded():
    sql = "DROP TABLE deprecated_logs;"
    findings = DataEngine.audit_sql_content(sql, "drop.sql")
    destructive = [f for f in findings if f["severity"] == "DESTRUCTIVE_UNGUARDED"]
    assert len(destructive) == 1
    assert destructive[0]["badge"] == "[ALERT: DESTRUCTIVE_UNGUARDED]"

def test_audit_sql_destructive_with_override():
    sql = """
    -- OZYMEM_ALLOW_DESTRUCTIVE
    DROP TABLE deprecated_logs;
    """
    findings = DataEngine.audit_sql_content(sql, "guarded.sql")
    destructive = [f for f in findings if f["severity"] == "DESTRUCTIVE_UNGUARDED"]
    assert len(destructive) == 0

def test_audit_migrations_directory(tmp_path: Path):
    engine = DataEngine(project_path=tmp_path, db_path=":memory:")
    
    clean_file = tmp_path / "001_clean.sql"
    clean_file.write_text("CREATE TABLE IF NOT EXISTS audit (id INT);\n", encoding="utf-8")
    
    bad_file = tmp_path / "002_bad.sql"
    bad_file.write_text("CREATE TABLE accounts (id INT);\nTRUNCATE TABLE old_data;\n", encoding="utf-8")

    res = engine.audit_migrations(tmp_path)
    assert res["status"] == "[STATUS: ALERT]"
    assert res["files_scanned"] == 2
    assert len(res["findings"]) >= 2
    assert "[ALERT: NON_IDEMPOTENT_SQL]" in res["report"]
    assert "[ALERT: DESTRUCTIVE_UNGUARDED]" in res["report"]
