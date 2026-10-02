//! Task 3.1 - SQL Migration Idempotency Linter
//!
//! Rules:
//!   1. DDL without IF NOT EXISTS / IF EXISTS -> [ALERT: NON_IDEMPOTENT_SQL]
//!   2. Destructive DDL without OZYMEM_ALLOW_DESTRUCTIVE override -> [ALERT: DESTRUCTIVE_UNGUARDED]

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SqlFindingSeverity {
    NonIdempotent,
    DestructiveUnguarded,
}

impl SqlFindingSeverity {
    pub fn badge(&self) -> &'static str {
        match self {
            Self::NonIdempotent        => "[ALERT: NON_IDEMPOTENT_SQL]",
            Self::DestructiveUnguarded => "[ALERT: DESTRUCTIVE_UNGUARDED]",
        }
    }
    pub fn suggestion(&self) -> &'static str {
        match self {
            Self::NonIdempotent =>
                "Add IF NOT EXISTS / IF EXISTS guard to make this statement safe to re-run.",
            Self::DestructiveUnguarded =>
                "Destructive DDL requires '-- OZYMEM_ALLOW_DESTRUCTIVE' on the preceding line.",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SqlLintFinding {
    pub file_path: String,
    pub line_number: usize,
    pub statement_preview: String,
    pub severity: SqlFindingSeverity,
}

const NON_IDEMPOTENT_VERBS: &[&str] = &[
    "CREATE TABLE", "CREATE INDEX", "CREATE UNIQUE INDEX", "CREATE VIEW",
    "CREATE SEQUENCE", "CREATE TYPE", "CREATE SCHEMA",
    "DROP TABLE", "DROP INDEX", "DROP VIEW", "DROP SEQUENCE", "DROP TYPE", "DROP SCHEMA",
];

const DESTRUCTIVE_VERBS: &[&str] = &["DROP TABLE", "DROP SCHEMA", "TRUNCATE"];

pub const DESTRUCTIVE_OVERRIDE_COMMENT: &str = "OZYMEM_ALLOW_DESTRUCTIVE";

pub fn audit_sql_migrations(sql_content: &str, file_path: &str) -> Vec<SqlLintFinding> {
    let lines: Vec<&str> = sql_content.lines().collect();
    let mut findings: Vec<SqlLintFinding> = Vec::new();
    let mut stmt_tokens: Vec<&str> = Vec::new();
    let mut stmt_first_line: usize = 1;
    let mut statements: Vec<(usize, String, String)> = Vec::new();

    for (idx, raw) in lines.iter().enumerate() {
        let line_no = idx + 1;
        let effective = raw.find("--")
            .map(|pos| raw[..pos].trim())
            .unwrap_or_else(|| raw.trim());
        if effective.is_empty() { continue; }
        let has_semi = effective.contains(';');
        let token = effective.trim_end_matches(';').trim();
        if stmt_tokens.is_empty() && !token.is_empty() {
            stmt_first_line = line_no;
        }
        if !token.is_empty() { stmt_tokens.push(token); }
        if has_semi && !stmt_tokens.is_empty() {
            let raw_joined = stmt_tokens.join(" ");
            let upper = raw_joined.to_uppercase();
            let preview: String = raw_joined.chars().take(120).collect();
            statements.push((stmt_first_line, upper, preview));
            stmt_tokens.clear();
        }
    }
    if !stmt_tokens.is_empty() {
        let raw_joined = stmt_tokens.join(" ");
        let upper = raw_joined.to_uppercase();
        let preview: String = raw_joined.chars().take(120).collect();
        statements.push((stmt_first_line, upper, preview));
    }

    for (first_line, upper, preview) in &statements {
        for verb in NON_IDEMPOTENT_VERBS {
            if upper.contains(verb) {
                let guarded = upper.contains("IF NOT EXISTS") || upper.contains("IF EXISTS");
                if !guarded {
                    findings.push(SqlLintFinding {
                        file_path: file_path.to_string(),
                        line_number: *first_line,
                        statement_preview: preview.clone(),
                        severity: SqlFindingSeverity::NonIdempotent,
                    });
                    break;
                }
            }
        }
        for verb in DESTRUCTIVE_VERBS {
            if upper.contains(verb) {
                let lookback_start = first_line.saturating_sub(4);
                let lookback_end = first_line.saturating_sub(1).min(lines.len());
                let has_override = lines[lookback_start..lookback_end]
                    .iter()
                    .any(|l| l.contains(DESTRUCTIVE_OVERRIDE_COMMENT));
                if !has_override {
                    findings.push(SqlLintFinding {
                        file_path: file_path.to_string(),
                        line_number: *first_line,
                        statement_preview: preview.clone(),
                        severity: SqlFindingSeverity::DestructiveUnguarded,
                    });
                }
                break;
            }
        }
    }
    findings
}

pub fn format_migration_report(findings: &[SqlLintFinding], scanned_files: usize) -> String {
    let non_idempotent: Vec<&SqlLintFinding> = findings.iter()
        .filter(|f| f.severity == SqlFindingSeverity::NonIdempotent).collect();
    let destructive: Vec<&SqlLintFinding> = findings.iter()
        .filter(|f| f.severity == SqlFindingSeverity::DestructiveUnguarded).collect();

    let mut report = format!(
        "# [AUDIT: SQL_MIGRATIONS]\n\nMode: preview_safe (no files modified)\nFiles scanned: {}\n\n",
        scanned_files
    );

    if findings.is_empty() {
        report.push_str("[STATUS: IDEMPOTENT] All scanned SQL migration files are idempotent and contain no unguarded destructive statements.\n");
        return report;
    }

    report.push_str(&format!("[STATS] non_idempotent={} destructive_unguarded={}\n\n",
        non_idempotent.len(), destructive.len()));

    if !non_idempotent.is_empty() {
        report.push_str("## [ALERT: NON_IDEMPOTENT_SQL] Missing IF NOT EXISTS / IF EXISTS\n\n");
        for f in &non_idempotent {
            report.push_str(&format!(
                "- {}:L{}\n  Statement: {}\n  Suggestion: {}\n\n",
                f.file_path, f.line_number, f.statement_preview,
                SqlFindingSeverity::NonIdempotent.suggestion()
            ));
        }
    }

    if !destructive.is_empty() {
        report.push_str("## [ALERT: DESTRUCTIVE_UNGUARDED] Destructive Statements Without Override\n\n");
        for f in &destructive {
            report.push_str(&format!(
                "- {}:L{}\n  Statement: {}\n  Suggestion: {}\n  Override: Add '-- {}' on the preceding line.\n\n",
                f.file_path, f.line_number, f.statement_preview,
                SqlFindingSeverity::DestructiveUnguarded.suggestion(),
                DESTRUCTIVE_OVERRIDE_COMMENT
            ));
        }
    }
    report
}

pub fn handle_audit_migrations(
    backend: Option<&ozymem_core::graph_backend::GraphBackend>,
    migrations_path: Option<&str>,
) -> anyhow::Result<String> {
    let search_root = if let Some(mp) = migrations_path {
        std::path::PathBuf::from(mp)
    } else if let Some(b) = backend {
        b.project_path()
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from("."))
    } else {
        std::path::PathBuf::from(".")
    };

    let mut sql_files: Vec<std::path::PathBuf> = Vec::new();
    collect_sql_files(&search_root, 0, 6, &mut sql_files);

    let scanned = sql_files.len();
    let mut all_findings: Vec<SqlLintFinding> = Vec::new();

    for path in &sql_files {
        let Ok(content) = std::fs::read_to_string(path) else { continue };
        let fp = path.to_string_lossy().replace('\\', "/");
        all_findings.extend(audit_sql_migrations(&content, &fp));
    }

    Ok(format_migration_report(&all_findings, scanned))
}

fn collect_sql_files(
    dir: &std::path::Path,
    depth: usize,
    max_depth: usize,
    out: &mut Vec<std::path::PathBuf>,
) {
    if depth > max_depth { return; }
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with('.') || matches!(name, "node_modules" | "target" | "__pycache__") {
                continue;
            }
            collect_sql_files(&path, depth + 1, max_depth, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("sql") {
            out.push(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_migration_no_findings() {
        let sql = "CREATE TABLE IF NOT EXISTS users (id INT);\nCREATE INDEX IF NOT EXISTS idx ON users(email);\n";
        assert!(audit_sql_migrations(sql, "clean.sql").is_empty());
    }

    #[test]
    fn test_non_idempotent_create_table() {
        let sql = "CREATE TABLE users (id INT);";
        let f = audit_sql_migrations(sql, "bad.sql");
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].severity, SqlFindingSeverity::NonIdempotent);
    }

    #[test]
    fn test_drop_table_unguarded() {
        let sql = "DROP TABLE users;";
        let f = audit_sql_migrations(sql, "drop.sql");
        let destr: Vec<_> = f.iter().filter(|x| x.severity == SqlFindingSeverity::DestructiveUnguarded).collect();
        assert!(!destr.is_empty());
    }

    #[test]
    fn test_drop_table_with_override_suppressed() {
        let sql = "-- OZYMEM_ALLOW_DESTRUCTIVE\nDROP TABLE legacy;";
        let f = audit_sql_migrations(sql, "guarded.sql");
        let destr: Vec<_> = f.iter().filter(|x| x.severity == SqlFindingSeverity::DestructiveUnguarded).collect();
        assert!(destr.is_empty(), "Override must suppress destructive alert");
    }

    #[test]
    fn test_truncate_unguarded() {
        let sql = "TRUNCATE TABLE audit_log;";
        let f = audit_sql_migrations(sql, "trunc.sql");
        let destr: Vec<_> = f.iter().filter(|x| x.severity == SqlFindingSeverity::DestructiveUnguarded).collect();
        assert!(!destr.is_empty());
    }

    #[test]
    fn test_format_report_clean_badge() {
        let r = format_migration_report(&[], 3);
        assert!(r.contains("[STATUS: IDEMPOTENT]"));
    }

    #[test]
    fn test_format_report_alert_badge() {
        let findings = vec![SqlLintFinding {
            file_path: "mig.sql".to_string(), line_number: 1,
            statement_preview: "CREATE TABLE t (id INT)".to_string(),
            severity: SqlFindingSeverity::NonIdempotent,
        }];
        let r = format_migration_report(&findings, 1);
        assert!(r.contains("[ALERT: NON_IDEMPOTENT_SQL]"));
        assert!(r.contains("[STATS]"));
    }

    #[test]
    fn test_zero_emoji() {
        let findings = vec![SqlLintFinding {
            file_path: "t.sql".to_string(), line_number: 1,
            statement_preview: "TRUNCATE t".to_string(),
            severity: SqlFindingSeverity::DestructiveUnguarded,
        }];
        let r = format_migration_report(&findings, 1);
        let has_emoji = r.chars().any(|c| {
            let cp = c as u32;
            (0x1F300..=0x1F9FF).contains(&cp) || (0x2600..=0x26FF).contains(&cp)
        });
        assert!(!has_emoji);
    }

    #[test]
    fn test_comment_lines_ignored() {
        let sql = "-- CREATE TABLE is in a comment\nSELECT 1;";
        assert!(audit_sql_migrations(sql, "c.sql").is_empty());
    }

    #[test]
    fn test_multiline_statement_first_line() {
        let sql = "CREATE TABLE\nusers\n(id INT);\n";
        let f = audit_sql_migrations(sql, "ml.sql");
        assert_eq!(f[0].line_number, 1);
    }
}
