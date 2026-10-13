use regex::Regex;
use serde::{Deserialize, Serialize};

/// Represents an extracted table reference from SQL DDL or ORM model definitions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SqlTableReference {
    pub table_name: String,
    pub operation: String,
    pub line_number: usize,
    pub file_path: String,
    pub raw_statement: String,
}

/// Normalizes SQL table names: strips schema qualifiers (e.g. `public.users` -> `users`),
/// quotes, and lowercases the name for uniform graph matching.
pub fn normalize_table_name(raw: &str) -> String {
    let clean = raw.trim().trim_matches('"').trim_matches('`').trim_matches('\'');
    let table = if let Some(pos) = clean.rfind('.') {
        &clean[pos + 1..]
    } else {
        clean
    };
    table.trim_matches('"').trim_matches('`').trim_matches('\'').to_lowercase()
}

/// Parses SQL file content (e.g. migrations, DDL scripts) for table operations:
/// CREATE TABLE, ALTER TABLE, DROP TABLE, and REFERENCES.
pub fn parse_sql_ddl_tables(source: &str, file_path: &str) -> Vec<SqlTableReference> {
    let mut references = Vec::new();

    let create_re = Regex::new(
        r#"(?i)\bCREATE\s+(?:UNLOGGED\s+|TEMP\s+|TEMPORARY\s+)?TABLE\s+(?:IF\s+NOT\s+EXISTS\s+)?([a-zA-Z0-9_."`]+)"#
    ).unwrap();

    let alter_re = Regex::new(
        r#"(?i)\bALTER\s+TABLE\s+(?:ONLY\s+)?(?:IF\s+EXISTS\s+)?([a-zA-Z0-9_."`]+)"#
    ).unwrap();

    let drop_re = Regex::new(
        r#"(?i)\bDROP\s+TABLE\s+(?:IF\s+EXISTS\s+)?([a-zA-Z0-9_."`]+)"#
    ).unwrap();

    let ref_re = Regex::new(
        r#"(?i)\bREFERENCES\s+([a-zA-Z0-9_."`]+)\s*(?:\(|$)"#
    ).unwrap();

    for (idx, line) in source.lines().enumerate() {
        let line_no = idx + 1;
        let effective = line.find("--")
            .map(|pos| line[..pos].trim())
            .unwrap_or_else(|| line.trim());

        if effective.is_empty() {
            continue;
        }

        if let Some(caps) = create_re.captures(effective) {
            if let Some(tbl) = caps.get(1) {
                references.push(SqlTableReference {
                    table_name: normalize_table_name(tbl.as_str()),
                    operation: "CREATE".to_string(),
                    line_number: line_no,
                    file_path: file_path.to_string(),
                    raw_statement: effective.to_string(),
                });
            }
        }

        if let Some(caps) = alter_re.captures(effective) {
            if let Some(tbl) = caps.get(1) {
                references.push(SqlTableReference {
                    table_name: normalize_table_name(tbl.as_str()),
                    operation: "ALTER".to_string(),
                    line_number: line_no,
                    file_path: file_path.to_string(),
                    raw_statement: effective.to_string(),
                });
            }
        }

        if let Some(caps) = drop_re.captures(effective) {
            if let Some(tbl) = caps.get(1) {
                references.push(SqlTableReference {
                    table_name: normalize_table_name(tbl.as_str()),
                    operation: "DROP".to_string(),
                    line_number: line_no,
                    file_path: file_path.to_string(),
                    raw_statement: effective.to_string(),
                });
            }
        }

        if let Some(caps) = ref_re.captures(effective) {
            if let Some(tbl) = caps.get(1) {
                let norm = normalize_table_name(tbl.as_str());
                if !references.iter().any(|r| r.line_number == line_no && r.table_name == norm) {
                    references.push(SqlTableReference {
                        table_name: norm,
                        operation: "REFERENCES".to_string(),
                        line_number: line_no,
                        file_path: file_path.to_string(),
                        raw_statement: effective.to_string(),
                    });
                }
            }
        }
    }

    references
}

/// Parses Python (SQLAlchemy, SQLModel), TypeScript (Drizzle, TypeORM), or Prisma files
/// to identify backend models associated with database tables.
pub fn parse_model_tables(source: &str, file_path: &str) -> Vec<SqlTableReference> {
    let mut references = Vec::new();

    // 1. Python SQLAlchemy / SQLModel: __tablename__ = "users"
    let py_tablename_re = Regex::new(
        r#"(?i)__tablename__\s*=\s*["']([a-zA-Z0-9_]+)["']"#
    ).unwrap();

    // 2. TypeScript Drizzle: pgTable("users", ...) or mysqlTable("users", ...)
    let drizzle_re = Regex::new(
        r#"(?i)\b(?:pgTable|mysqlTable|sqliteTable)\s*\(\s*["']([a-zA-Z0-9_]+)["']"#
    ).unwrap();

    // 3. TypeORM / MikroORM: @Entity("users")
    let entity_re = Regex::new(
        r#"(?i)@Entity\s*\(\s*["']([a-zA-Z0-9_]+)["']"#
    ).unwrap();

    // 4. Prisma: @@map("users") or model Users {
    let prisma_map_re = Regex::new(
        r#"(?i)@@map\s*\(\s*["']([a-zA-Z0-9_]+)["']"#
    ).unwrap();

    for (idx, line) in source.lines().enumerate() {
        let line_no = idx + 1;
        let effective = line.trim();

        if let Some(caps) = py_tablename_re.captures(effective) {
            if let Some(tbl) = caps.get(1) {
                references.push(SqlTableReference {
                    table_name: normalize_table_name(tbl.as_str()),
                    operation: "MODEL".to_string(),
                    line_number: line_no,
                    file_path: file_path.to_string(),
                    raw_statement: effective.to_string(),
                });
            }
        }

        if let Some(caps) = drizzle_re.captures(effective) {
            if let Some(tbl) = caps.get(1) {
                references.push(SqlTableReference {
                    table_name: normalize_table_name(tbl.as_str()),
                    operation: "MODEL".to_string(),
                    line_number: line_no,
                    file_path: file_path.to_string(),
                    raw_statement: effective.to_string(),
                });
            }
        }

        if let Some(caps) = entity_re.captures(effective) {
            if let Some(tbl) = caps.get(1) {
                references.push(SqlTableReference {
                    table_name: normalize_table_name(tbl.as_str()),
                    operation: "MODEL".to_string(),
                    line_number: line_no,
                    file_path: file_path.to_string(),
                    raw_statement: effective.to_string(),
                });
            }
        }

        if let Some(caps) = prisma_map_re.captures(effective) {
            if let Some(tbl) = caps.get(1) {
                references.push(SqlTableReference {
                    table_name: normalize_table_name(tbl.as_str()),
                    operation: "MODEL".to_string(),
                    line_number: line_no,
                    file_path: file_path.to_string(),
                    raw_statement: effective.to_string(),
                });
            }
        }
    }

    references
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_sql_ddl_tables_create_alter_references() {
        let sql = r#"
-- Migration 067_clasificar_jinshi_antiguo_silvia.sql
CREATE TABLE IF NOT EXISTS public.control_probetas (
    id SERIAL PRIMARY KEY,
    codigo VARCHAR(50) NOT NULL,
    cotizacion_id INT REFERENCES cotizaciones(id)
);

ALTER TABLE control_probetas ADD COLUMN costo_servicio NUMERIC(10, 2);

DROP TABLE IF EXISTS temp_probetas_backup;
"#;
        let refs = parse_sql_ddl_tables(sql, "migrations/067.sql");
        assert_eq!(refs.len(), 4);

        assert_eq!(refs[0].table_name, "control_probetas");
        assert_eq!(refs[0].operation, "CREATE");
        assert_eq!(refs[0].line_number, 3);

        assert_eq!(refs[1].table_name, "cotizaciones");
        assert_eq!(refs[1].operation, "REFERENCES");
        assert_eq!(refs[1].line_number, 6);

        assert_eq!(refs[2].table_name, "control_probetas");
        assert_eq!(refs[2].operation, "ALTER");
        assert_eq!(refs[2].line_number, 9);

        assert_eq!(refs[3].table_name, "temp_probetas_backup");
        assert_eq!(refs[3].operation, "DROP");
        assert_eq!(refs[3].line_number, 11);
    }

    #[test]
    fn test_parse_model_tables_python_and_drizzle() {
        let py_code = r#"
from sqlmodel import SQLModel, Field

class ControlProbeta(SQLModel, table=True):
    __tablename__ = "control_probetas"
    id: int = Field(primary_key=True)
"#;
        let refs = parse_model_tables(py_code, "models/probeta.py");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].table_name, "control_probetas");
        assert_eq!(refs[0].operation, "MODEL");

        let ts_code = r#"
import { pgTable, serial, text } from 'drizzle-orm/pg-core';
export const cotizaciones = pgTable('cotizaciones', {
    id: serial('id').primaryKey(),
});
"#;
        let ts_refs = parse_model_tables(ts_code, "schema/cotizaciones.ts");
        assert_eq!(ts_refs.len(), 1);
        assert_eq!(ts_refs[0].table_name, "cotizaciones");
    }
}
