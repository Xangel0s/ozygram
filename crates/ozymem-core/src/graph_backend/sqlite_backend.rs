use crate::graph_backend::types::{OZYMEM_DIR, MEMORY_DB};
use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use std::path::Path;
use crate::graph_backend::helpers::resolve_project_db_path;

#[derive(Clone)]
pub struct SqliteBackend {
    conn: std::sync::Arc<std::sync::Mutex<Connection>>,
}

impl SqliteBackend {
    /// Open (or create) the shared SQLite database at the standard path.
    ///
    /// Same path as `GraphBackend::open()` so data written here is visible
    /// to the MCP server's `GraphBackend` and vice versa.
    pub fn open(db_path: Option<&str>) -> Result<Self> {
        let path = match db_path {
            Some(p) => p.to_string(),
            None => {
                let home = home::home_dir().context("cannot find home dir")?;
                let ozymem_dir = home.join(OZYMEM_DIR);
                std::fs::create_dir_all(&ozymem_dir).ok();
                ozymem_dir.join(MEMORY_DB).to_string_lossy().to_string()
            }
        };

        let conn =
            Connection::open(&path).with_context(|| format!("failed to open SQLite at {path}"))?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;

        let backend = Self {
            conn: std::sync::Arc::new(std::sync::Mutex::new(conn)),
        };
        backend.init_schema()?;
        Ok(backend)
    }

    fn init_schema(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS tenants (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS files (
                path TEXT NOT NULL,
                language TEXT NOT NULL DEFAULT 'Unknown',
                strategy TEXT NOT NULL DEFAULT 'TextHeuristic',
                mtime TEXT NOT NULL DEFAULT '',
                tenant_id TEXT NOT NULL,
                workspace_root TEXT NOT NULL DEFAULT '',
                PRIMARY KEY (path, tenant_id)
            );

            CREATE INDEX IF NOT EXISTS idx_files_tenant ON files(tenant_id);

            CREATE TABLE IF NOT EXISTS functions (
                name TEXT NOT NULL,
                kind TEXT NOT NULL DEFAULT '',
                start_line INTEGER NOT NULL DEFAULT 0,
                end_line INTEGER NOT NULL DEFAULT 0,
                strategy TEXT NOT NULL DEFAULT '',
                file_path TEXT NOT NULL,
                tenant_id TEXT NOT NULL,
                workspace_root TEXT NOT NULL DEFAULT '',
                PRIMARY KEY (name, start_line, end_line, file_path, tenant_id)
            );

            CREATE INDEX IF NOT EXISTS idx_functions_file ON functions(file_path, tenant_id);

            CREATE TABLE IF NOT EXISTS file_dependencies (
                origin_path TEXT NOT NULL,
                destination_path TEXT NOT NULL,
                tenant_id TEXT NOT NULL,
                workspace_root TEXT NOT NULL DEFAULT '',
                PRIMARY KEY (origin_path, destination_path, tenant_id)
            );

            CREATE TABLE IF NOT EXISTS lessons (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                file_path TEXT NOT NULL,
                symbol_name TEXT NOT NULL DEFAULT '',
                error_context TEXT NOT NULL,
                solution TEXT NOT NULL,
                created_at TEXT NOT NULL,
                tenant_id TEXT NOT NULL,
                workspace_root TEXT NOT NULL DEFAULT '',
                kind TEXT NOT NULL DEFAULT 'lesson'
                CHECK(kind IN ('lesson','decision','convention','gotcha','module_rule'))
            );

            CREATE INDEX IF NOT EXISTS idx_lessons_file ON lessons(file_path, tenant_id);
            CREATE INDEX IF NOT EXISTS idx_lessons_tenant ON lessons(tenant_id);

            INSERT OR IGNORE INTO tenants (id, name) VALUES ('local', 'Local Tenant');",
        )?;

        // Migrate v2→v3: add workspace_root column to all data tables
        for table in &["files", "functions", "file_dependencies", "lessons"] {
            if let Err(e) = conn.execute(
                &format!(
                    "ALTER TABLE {} ADD COLUMN workspace_root TEXT NOT NULL DEFAULT ''",
                    table
                ),
                [],
            ) {
                if !e.to_string().contains("duplicate column") {
                    return Err(e.into());
                }
            }
        }

        // Migrate v0→v1: add kind column if lessons table existed without it
        if let Err(e) = conn.execute(
            "ALTER TABLE lessons ADD COLUMN kind TEXT NOT NULL DEFAULT 'lesson'",
            [],
        ) {
            if !e.to_string().contains("duplicate column") {
                return Err(e.into());
            }
        }

        // Migrate v1→v2: add stale columns for staleness tracking
        if let Err(e) = conn.execute(
            "ALTER TABLE lessons ADD COLUMN stale INTEGER NOT NULL DEFAULT 0",
            [],
        ) {
            if !e.to_string().contains("duplicate column") {
                return Err(e.into());
            }
        }
        if let Err(e) = conn.execute("ALTER TABLE lessons ADD COLUMN stale_reason TEXT", []) {
            if !e.to_string().contains("duplicate column") {
                return Err(e.into());
            }
        }
        if let Err(e) = conn.execute("ALTER TABLE lessons ADD COLUMN stale_since TEXT", []) {
            if !e.to_string().contains("duplicate column") {
                return Err(e.into());
            }
        }

        // Create kind index AFTER migration so it works on old DBs too
        conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_lessons_kind ON lessons(kind, tenant_id);",
        )?;

        // Migrate v3→v4: add embedding columns for semantic search
        for col in &["embedding BLOB", "embedding_model TEXT NOT NULL DEFAULT ''"] {
            if let Err(e) = conn.execute(&format!("ALTER TABLE lessons ADD COLUMN {col}"), []) {
                if !e.to_string().contains("duplicate column") {
                    return Err(e.into());
                }
            }
        }

        Ok(())
    }

    /// Returns a display name for this backend.
    pub fn display_name(&self) -> String {
        format!("sqlite://{}", MEMORY_DB)
    }

    /// Ping: run a trivial query to verify the database is reachable.
    pub fn ping(&self) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.query_row("SELECT 1", [], |row| row.get::<_, i64>(0))
            .map_err(Into::into)
    }

    /// Delete all files, functions, and dependencies for a tenant.
    pub fn clear_graph(&self, tenant_id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM file_dependencies WHERE tenant_id = ?1",
            params![tenant_id],
        )?;
        conn.execute(
            "DELETE FROM functions WHERE tenant_id = ?1",
            params![tenant_id],
        )?;
        conn.execute("DELETE FROM files WHERE tenant_id = ?1", params![tenant_id])?;
        Ok(())
    }

    /// Write a parsed file (File + Functions) into SQLite.
    ///
    /// Replaces any existing functions for the same file (DELETE + INSERT).
    pub fn save_file_definition(
        &self,
        tenant_id: &str,
        file_map: &ozymem_parser::FileDefinitionMap,
        workspace_root: &str,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO files (path, language, strategy, mtime, tenant_id, workspace_root) VALUES (?1, ?2, ?3, '', ?4, ?5)",
            params![file_map.file_path, file_map.language, file_map.strategy.as_str(), tenant_id, workspace_root],
        )?;

        // Delete old functions for this file
        conn.execute(
            "DELETE FROM functions WHERE file_path = ?1 AND tenant_id = ?2",
            params![file_map.file_path, tenant_id],
        )?;

        for fn_data in &file_map.functions {
            let kind_str = match fn_data.kind {
                ozymem_parser::SymbolKind::Function => "Function",
                ozymem_parser::SymbolKind::Class => "Class",
            };
            conn.execute(
                "INSERT INTO functions (name, kind, start_line, end_line, strategy, file_path, tenant_id, workspace_root) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![fn_data.name, kind_str, fn_data.start_line as i64, fn_data.end_line as i64, file_map.strategy.as_str(), file_map.file_path, tenant_id, workspace_root],
            )?;
        }

        Ok(())
    }

    /// Write a single dependency edge (origin -> destination).
    pub fn save_dependency_relation(
        &self,
        tenant_id: &str,
        origin_path: &str,
        destination_path: &str,
        workspace_root: &str,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO file_dependencies (origin_path, destination_path, tenant_id, workspace_root) VALUES (?1, ?2, ?3, ?4)",
            params![origin_path, destination_path, tenant_id, workspace_root],
        )?;
        Ok(())
    }

    /// Delete functions and dependencies for a single file.
    pub fn clear_file_symbols_and_dependencies(
        &self,
        tenant_id: &str,
        file_path: &str,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM file_dependencies WHERE (origin_path = ?1 OR destination_path = ?1) AND tenant_id = ?2",
            params![file_path, tenant_id],
        )?;
        conn.execute(
            "DELETE FROM functions WHERE file_path = ?1 AND tenant_id = ?2",
            params![file_path, tenant_id],
        )?;
        Ok(())
    }

    /// Delete a file and its functions. Returns true if the file existed.
    pub fn delete_file_definition(&self, tenant_id: &str, file_path: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM files WHERE path = ?1 AND tenant_id = ?2",
            params![file_path, tenant_id],
            |row| row.get(0),
        )?;
        if count == 0 {
            return Ok(false);
        }
        conn.execute(
            "DELETE FROM functions WHERE file_path = ?1 AND tenant_id = ?2",
            params![file_path, tenant_id],
        )?;
        conn.execute(
            "DELETE FROM file_dependencies WHERE (origin_path = ?1 OR destination_path = ?1) AND tenant_id = ?2",
            params![file_path, tenant_id],
        )?;
        conn.execute(
            "DELETE FROM files WHERE path = ?1 AND tenant_id = ?2",
            params![file_path, tenant_id],
        )?;
        Ok(true)
    }

    /// Delete all files under a project path prefix.
    pub fn delete_project_files(&self, tenant_id: &str, project_path: &str) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        // Use LIKE to match all files under the project directory
        let like = format!("{}%", project_path.replace('\\', "\\\\"));
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM files WHERE tenant_id = ?1 AND (path = ?2 OR path LIKE ?3)",
            params![tenant_id, project_path, like],
            |row| row.get(0),
        )?;
        if count > 0 {
            conn.execute(
                "DELETE FROM functions WHERE tenant_id = ?1 AND (file_path = ?2 OR file_path LIKE ?3)",
                params![tenant_id, project_path, like],
            )?;
            conn.execute(
                "DELETE FROM file_dependencies WHERE tenant_id = ?1 AND ((origin_path = ?2 OR origin_path LIKE ?3) OR (destination_path = ?2 OR destination_path LIKE ?3))",
                params![tenant_id, project_path, like],
            )?;
            conn.execute(
                "DELETE FROM files WHERE tenant_id = ?1 AND (path = ?2 OR path LIKE ?3)",
                params![tenant_id, project_path, like],
            )?;
        }
        Ok(count)
    }

    /// Get all file paths for a tenant.
    pub fn get_all_file_paths(&self, tenant_id: &str) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT path FROM files WHERE tenant_id = ?1 ORDER BY path")?;
        let paths = stmt
            .query_map(params![tenant_id], |row| row.get::<_, String>(0))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(paths)
    }

    /// Get distinct lesson solutions for a file.
    pub fn get_historical_engram_solutions(
        &self,
        tenant_id: &str,
        file_path: &str,
    ) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = match conn.prepare(
            "SELECT DISTINCT solution FROM lessons WHERE file_path = ?1 AND tenant_id = ?2 AND solution != ''"
        ) {
            Ok(s) => s,
            Err(_) => return Ok(vec![]),
        };
        let solutions = stmt
            .query_map(params![file_path, tenant_id], |row| row.get::<_, String>(0))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(solutions)
    }

    /// Get recent lessons, optionally filtered by file path.
    pub fn get_recent_lessons(
        &self,
        tenant_id: &str,
        limit: i64,
        file_filter: Option<String>,
    ) -> Result<Vec<crate::LessonRecord>> {
        let conn = self.conn.lock().unwrap();
        let (sql, filter_param): (String, bool) = if file_filter.is_some() {
            (
                "SELECT file_path, error_context, solution, created_at FROM lessons WHERE tenant_id = ?1 AND file_path LIKE ?2 ORDER BY id DESC LIMIT ?3".to_string(),
                true,
            )
        } else {
            (
                "SELECT file_path, error_context, solution, created_at FROM lessons WHERE tenant_id = ?1 ORDER BY id DESC LIMIT ?2".to_string(),
                false,
            )
        };

        let mut stmt = conn.prepare(&sql)?;
        if filter_param {
            let like = format!("%{}%", file_filter.as_ref().unwrap());
            let rows = stmt.query_map(params![tenant_id, like, limit], |row| {
                Ok(crate::LessonRecord {
                    file_path: row.get::<_, String>(0)?,
                    error_type: row.get::<_, String>(1)?,
                    solution: row.get::<_, String>(2)?,
                    timestamp: row.get::<_, String>(3)?,
                })
            })?;
            Ok(rows.filter_map(|r| r.ok()).collect())
        } else {
            let rows = stmt.query_map(params![tenant_id, limit], |row| {
                Ok(crate::LessonRecord {
                    file_path: row.get::<_, String>(0)?,
                    error_type: row.get::<_, String>(1)?,
                    solution: row.get::<_, String>(2)?,
                    timestamp: row.get::<_, String>(3)?,
                })
            })?;
            Ok(rows.filter_map(|r| r.ok()).collect())
        }
    }

    /// Get outgoing dependencies (what this file imports/uses).
    pub fn get_outgoing_dependencies(
        &self,
        tenant_id: &str,
        file_path: &str,
    ) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT DISTINCT destination_path FROM file_dependencies WHERE origin_path = ?1 AND tenant_id = ?2 ORDER BY destination_path"
        )?;
        let deps = stmt
            .query_map(params![file_path, tenant_id], |row| row.get::<_, String>(0))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(deps)
    }

    /// Get incoming dependencies (what files depend on this one).
    pub fn get_incoming_dependencies(
        &self,
        tenant_id: &str,
        file_path: &str,
    ) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT DISTINCT origin_path FROM file_dependencies WHERE destination_path = ?1 AND tenant_id = ?2 ORDER BY origin_path"
        )?;
        let deps = stmt
            .query_map(params![file_path, tenant_id], |row| row.get::<_, String>(0))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(deps)
    }

    /// Get file context (language + functions) for a file.
    pub fn get_file_context(
        &self,
        tenant_id: &str,
        file_path: &str,
    ) -> Result<Option<crate::FileGraphContext>> {
        let conn = self.conn.lock().unwrap();
        let language: String = match conn.query_row(
            "SELECT language FROM files WHERE path = ?1 AND tenant_id = ?2",
            params![file_path, tenant_id],
            |row| row.get(0),
        ) {
            Ok(v) => v,
            Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
            Err(e) => return Err(e.into()),
        };

        let mut func_stmt = conn.prepare(
            "SELECT name, kind, start_line, end_line, strategy FROM functions WHERE file_path = ?1 AND tenant_id = ?2 ORDER BY start_line, name"
        )?;
        let functions = func_stmt
            .query_map(params![file_path, tenant_id], |row| {
                Ok(crate::StoredFunction {
                    name: row.get(0)?,
                    kind: row.get(1)?,
                    start_line: row.get(2)?,
                    end_line: row.get(3)?,
                    strategy: row.get(4)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(Some(crate::FileGraphContext {
            file_path: file_path.to_string(),
            language,
            functions,
            engram_contracts: Vec::new(),
        }))
    }

    /// Get a summary of the graph for a tenant.
    pub fn get_graph_summary(&self, tenant_id: &str) -> Result<crate::GraphSummary> {
        let conn = self.conn.lock().unwrap();

        let file_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM files WHERE tenant_id = ?1",
                params![tenant_id],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let function_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM functions WHERE tenant_id = ?1",
                params![tenant_id],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let engram_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM lessons WHERE tenant_id = ?1",
                params![tenant_id],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let edge_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM file_dependencies WHERE tenant_id = ?1",
                params![tenant_id],
                |r| r.get(0),
            )
            .unwrap_or(0);

        let mut native = 0i64;
        let mut extension = 0i64;
        let mut text = 0i64;
        if let Ok(mut stmt) = conn.prepare(
            "SELECT strategy, COUNT(*) FROM functions WHERE tenant_id = ?1 GROUP BY strategy",
        ) {
            if let Ok(rows) = stmt.query_map(params![tenant_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            }) {
                for row in rows.flatten() {
                    match row.0.as_str() {
                        "NativeAST" => native += row.1,
                        "ExtensionWASM" => extension += row.1,
                        _ => text += row.1,
                    }
                }
            }
        }

        let without_embedding: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM lessons WHERE tenant_id = ?1 AND embedding IS NULL",
                params![tenant_id],
                |r| r.get(0),
            )
            .unwrap_or(0);

        Ok(crate::GraphSummary {
            file_count,
            function_count,
            engram_count,
            native_ast_function_count: native,
            extension_wasm_function_count: extension,
            text_heuristic_function_count: text,
            vertex_count: file_count,
            edge_count,
            memory_usage: format!("{} MB", (file_count * 200) / 1024 / 1024 + 1),
            lessons_without_embedding: without_embedding,
        })
    }

    /// Find a symbol across all files in a project. Returns "path (line N)" strings.
    pub fn find_symbol(
        &self,
        tenant_id: &str,
        symbol_name: &str,
        project_path: &str,
    ) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let like = format!("{}%", project_path);
        let mut stmt = conn.prepare(
            "SELECT f.path, fn.start_line, fn.kind FROM functions fn
             JOIN files f ON f.path = fn.file_path AND f.tenant_id = fn.tenant_id
             WHERE fn.name LIKE ?1 AND fn.tenant_id = ?2 AND f.path LIKE ?3
             ORDER BY fn.name LIMIT 100",
        )?;
        let results: Vec<String> = stmt
            .query_map(params![symbol_name, tenant_id, like], |row| {
                let path: String = row.get(0)?;
                let line: i64 = row.get(1)?;
                let kind: String = row.get(2)?;
                Ok(format!("{} [{}] (Línea: {})", path, kind, line))
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(results)
    }

    /// Record a lesson entry for a file+symbol.
    pub fn record_lesson(
        &self,
        tenant_id: &str,
        file_path: &str,
        symbol_name: Option<&str>,
        error_context: &str,
        solution: &str,
        workspace_root: &str,
    ) -> Result<()> {
        let created_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            .to_string();

        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO lessons (file_path, symbol_name, error_context, solution, created_at, tenant_id, kind, workspace_root) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'lesson', ?7)",
            params![file_path, symbol_name.unwrap_or(""), error_context, solution, created_at, tenant_id, workspace_root],
        )?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Shared helpers: project DB path resolution, .gitignore, legacy detection
// ---------------------------------------------------------------------------


impl SqliteBackend {
    /// Open a project-scoped database.
    pub fn open_for_project(project_path: &Path) -> Result<Self> {
        let db_path = resolve_project_db_path(project_path)?;
        Self::open(Some(&db_path.to_string_lossy()))
    }
}

