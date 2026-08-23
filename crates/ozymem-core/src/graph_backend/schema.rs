use anyhow::{Context, Result};
use petgraph::graph::DiGraph;
use rusqlite::{params, Connection};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock, atomic::AtomicBool};
use std::time::Instant;
use crate::graph_backend::helpers::{resolve_project_db_path, strip_unc_prefix};
use crate::graph_backend::types::{GraphBackend, Inner, ScanProgress, OZYMEM_DIR, MEMORY_DB};

impl GraphBackend {
    pub fn open(db_path: Option<&str>) -> Result<Self> {
        let path = db_path.map(|p| p.to_string()).unwrap_or_else(|| {
            let home = home::home_dir().expect("cannot find home dir");
            let ozymem_dir = home.join(OZYMEM_DIR);
            std::fs::create_dir_all(&ozymem_dir).ok();
            ozymem_dir.join(MEMORY_DB).to_string_lossy().to_string()
        });

        let sqlite =
            Connection::open(&path).with_context(|| format!("failed to open SQLite at {path}"))?;
        sqlite.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;

        let engram_path = Path::new(&path)
            .parent()
            .unwrap_or(Path::new("."))
            .join(crate::engram_store::ENGRAM_FILE);
        let engram_store = crate::engram_store::IncrementalEngramStore::open(engram_path)?;

        let backend = Self {
            inner: Mutex::new(Inner {
                graph: DiGraph::new(),
                file_index: HashMap::new(),
                sqlite,
                project_path: None,
                workspace_root: String::new(),
            }),
            tenant_id: "local".to_string(),
            scanning: AtomicBool::new(false),
            scan_progress: Arc::new(Mutex::new(ScanProgress {
                scanning: false,
                total: 0,
                processed: 0,
                current_file: String::new(),
            })),
            last_check: Mutex::new(Instant::now()),
            embedder: OnceLock::new(),
            engram_store,
        };
        backend.init_schema()?;
        Ok(backend)
    }

    fn init_schema(&self) -> Result<()> {
        let inner = self.inner.lock().unwrap();
        inner.sqlite.execute_batch(
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
                sha256 TEXT NOT NULL DEFAULT '',
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
                confidence_score REAL NOT NULL DEFAULT 1.0,
                touch_count INTEGER NOT NULL DEFAULT 0,
                last_verified_at TEXT NOT NULL DEFAULT '',
                kind TEXT NOT NULL DEFAULT 'lesson'
                CHECK(kind IN ('lesson','decision','convention','gotcha','module_rule'))
            );

            CREATE INDEX IF NOT EXISTS idx_lessons_file ON lessons(file_path, tenant_id);
            CREATE INDEX IF NOT EXISTS idx_lessons_tenant ON lessons(tenant_id);
            CREATE INDEX IF NOT EXISTS idx_lessons_kind ON lessons(kind, tenant_id);

            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                project TEXT NOT NULL DEFAULT '',
                directory TEXT NOT NULL DEFAULT '',
                started_at TEXT NOT NULL,
                ended_at TEXT,
                summary TEXT,
                status TEXT NOT NULL DEFAULT 'active',
                tenant_id TEXT NOT NULL,
                workspace_root TEXT NOT NULL DEFAULT ''
            );

            CREATE TABLE IF NOT EXISTS observations (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                type TEXT NOT NULL DEFAULT 'discovery',
                title TEXT NOT NULL,
                content TEXT NOT NULL,
                tool_name TEXT,
                project TEXT NOT NULL DEFAULT '',
                scope TEXT NOT NULL DEFAULT 'project',
                topic_key TEXT,
                normalized_hash TEXT NOT NULL,
                revision_count INTEGER NOT NULL DEFAULT 1,
                duplicate_count INTEGER NOT NULL DEFAULT 0,
                last_seen_at TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                deleted_at TEXT,
                tenant_id TEXT NOT NULL,
                workspace_root TEXT NOT NULL DEFAULT '',
                FOREIGN KEY(session_id) REFERENCES sessions(id)
            );

            CREATE INDEX IF NOT EXISTS idx_observations_session ON observations(session_id, tenant_id);
            CREATE INDEX IF NOT EXISTS idx_observations_project_scope ON observations(project, scope, tenant_id);
            CREATE INDEX IF NOT EXISTS idx_observations_topic ON observations(project, scope, topic_key, tenant_id);
            CREATE INDEX IF NOT EXISTS idx_observations_hash ON observations(normalized_hash, project, scope, type, title, tenant_id);

            CREATE TABLE IF NOT EXISTS user_prompts (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                content TEXT NOT NULL,
                project TEXT NOT NULL DEFAULT '',
                created_at TEXT NOT NULL,
                tenant_id TEXT NOT NULL,
                workspace_root TEXT NOT NULL DEFAULT '',
                FOREIGN KEY(session_id) REFERENCES sessions(id)
            );

            CREATE INDEX IF NOT EXISTS idx_user_prompts_project ON user_prompts(project, tenant_id);

            CREATE TABLE IF NOT EXISTS ast_diagnostics (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                file_path TEXT NOT NULL,
                line_number INTEGER NOT NULL DEFAULT 0,
                severity TEXT NOT NULL DEFAULT 'warning',
                message TEXT NOT NULL,
                tenant_id TEXT NOT NULL,
                workspace_root TEXT NOT NULL DEFAULT ''
            );

            CREATE INDEX IF NOT EXISTS idx_ast_diags_file ON ast_diagnostics(file_path, tenant_id);

            CREATE TABLE IF NOT EXISTS excel_templates (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                file_path TEXT NOT NULL UNIQUE,
                canonical_hash TEXT NOT NULL,
                template_name TEXT NOT NULL,
                version_tag TEXT,
                sheets_json TEXT NOT NULL,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
            );
            CREATE INDEX IF NOT EXISTS idx_excel_hash ON excel_templates(canonical_hash);

            CREATE TABLE IF NOT EXISTS ozy_brain_audit (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                request_hash TEXT NOT NULL,
                action TEXT NOT NULL,
                project_path TEXT,
                git_head TEXT,
                model_version TEXT,
                brain_version TEXT,
                schema_version TEXT,
                latency_ms INTEGER,
                confidence REAL,
                result_summary TEXT,
                raw_response TEXT,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP
            );
            CREATE INDEX IF NOT EXISTS idx_brain_audit_req_hash ON ozy_brain_audit(request_hash);
            CREATE INDEX IF NOT EXISTS idx_brain_audit_proj_date ON ozy_brain_audit(project_path, created_at);

            INSERT OR IGNORE INTO tenants (id, name) VALUES ('local', 'Local Tenant');"
        )?;

        // Migrate v2→v3: add workspace_root column to all data tables
        for table in &["files", "functions", "file_dependencies", "lessons"] {
            if let Err(e) = inner.sqlite.execute(
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
        if let Err(e) = inner.sqlite.execute(
            "ALTER TABLE lessons ADD COLUMN kind TEXT NOT NULL DEFAULT 'lesson'",
            [],
        ) {
            if !e.to_string().contains("duplicate column") {
                return Err(e.into());
            }
        }

        // Migrate v1→v2: add stale columns for staleness tracking
        if let Err(e) = inner.sqlite.execute(
            "ALTER TABLE lessons ADD COLUMN stale INTEGER NOT NULL DEFAULT 0",
            [],
        ) {
            if !e.to_string().contains("duplicate column") {
                return Err(e.into());
            }
        }
        if let Err(e) = inner
            .sqlite
            .execute("ALTER TABLE lessons ADD COLUMN stale_reason TEXT", [])
        {
            if !e.to_string().contains("duplicate column") {
                return Err(e.into());
            }
        }
        if let Err(e) = inner
            .sqlite
            .execute("ALTER TABLE lessons ADD COLUMN stale_since TEXT", [])
        {
            if !e.to_string().contains("duplicate column") {
                return Err(e.into());
            }
        }

        // Migrate v3→v4: add embedding columns for semantic search
        for col in &["embedding BLOB", "embedding_model TEXT NOT NULL DEFAULT ''"] {
            if let Err(e) = inner
                .sqlite
                .execute(&format!("ALTER TABLE lessons ADD COLUMN {col}"), [])
            {
                if !e.to_string().contains("duplicate column") {
                    return Err(e.into());
                }
            }
        }

        // Migrate v4→v5: add sha256 column to files for delta indexing
        if let Err(e) = inner.sqlite.execute(
            "ALTER TABLE files ADD COLUMN sha256 TEXT NOT NULL DEFAULT ''",
            [],
        ) {
            if !e.to_string().contains("duplicate column") {
                return Err(e.into());
            }
        }

        // Migrate v5→v6: add confidence_score, touch_count, last_verified_at to lessons
        for col in &[
            "confidence_score REAL NOT NULL DEFAULT 1.0",
            "touch_count INTEGER NOT NULL DEFAULT 0",
            "last_verified_at TEXT NOT NULL DEFAULT ''",
        ] {
            if let Err(e) = inner
                .sqlite
                .execute(&format!("ALTER TABLE lessons ADD COLUMN {col}"), [])
            {
                if !e.to_string().contains("duplicate column") {
                    return Err(e.into());
                }
            }
        }

        // Backfill workspace_root for existing rows using project_path
        if let Some(ref root) = inner.project_path {
            if !root.is_empty() {
                for table in &["files", "functions", "file_dependencies", "lessons"] {
                    let affected = inner.sqlite.execute(
                        &format!("UPDATE {} SET workspace_root = ?1 WHERE workspace_root = '' AND tenant_id = ?2", table),
                        params![root, self.tenant_id],
                    )?;
                    if affected > 0 {
                        eprintln!("[ozymem] backfilled {} rows in {}", affected, table);
                    }
                }
            }
        }

        // FTS5 virtual table for full-text search across lessons
        inner.sqlite.execute_batch(
            "CREATE VIRTUAL TABLE IF NOT EXISTS lessons_fts USING fts5(
                error_context, solution, symbol_name, file_path,
                content='lessons',
                content_rowid='id',
                tokenize='unicode61'
            );

            DROP TRIGGER IF EXISTS lessons_ai;
            DROP TRIGGER IF EXISTS lessons_ad;
            DROP TRIGGER IF EXISTS lessons_au;

            INSERT OR IGNORE INTO lessons_fts(rowid, error_context, solution, symbol_name, file_path)
            SELECT id, error_context, solution, symbol_name, file_path FROM lessons;

            CREATE TRIGGER lessons_ai AFTER INSERT ON lessons BEGIN
                INSERT INTO lessons_fts(rowid, error_context, solution, symbol_name, file_path)
                VALUES (new.id, new.error_context, new.solution, new.symbol_name, new.file_path);
            END;

            CREATE TRIGGER lessons_ad AFTER DELETE ON lessons BEGIN
                INSERT INTO lessons_fts(lessons_fts, rowid, error_context, solution, symbol_name, file_path)
                VALUES ('delete', old.id, old.error_context, old.solution, old.symbol_name, old.file_path);
            END;

            CREATE TRIGGER lessons_au AFTER UPDATE ON lessons BEGIN
                INSERT INTO lessons_fts(lessons_fts, rowid, error_context, solution, symbol_name, file_path)
                VALUES ('delete', old.id, old.error_context, old.solution, old.symbol_name, old.file_path);
                INSERT INTO lessons_fts(rowid, error_context, solution, symbol_name, file_path)
                VALUES (new.id, new.error_context, new.solution, new.symbol_name, new.file_path);
            END;"
        )?;

        inner.sqlite.execute_batch(
            "CREATE VIRTUAL TABLE IF NOT EXISTS observations_fts USING fts5(
                title, content, tool_name, type, project,
                content='observations',
                content_rowid='id',
                tokenize='unicode61'
            );

            DROP TRIGGER IF EXISTS observations_ai;
            DROP TRIGGER IF EXISTS observations_ad;
            DROP TRIGGER IF EXISTS observations_au;

            INSERT OR IGNORE INTO observations_fts(rowid, title, content, tool_name, type, project)
            SELECT id, title, content, COALESCE(tool_name,''), type, project FROM observations WHERE deleted_at IS NULL;

            CREATE TRIGGER observations_ai AFTER INSERT ON observations BEGIN
                INSERT INTO observations_fts(rowid, title, content, tool_name, type, project)
                VALUES (new.id, new.title, new.content, COALESCE(new.tool_name,''), new.type, new.project);
            END;

            CREATE TRIGGER observations_ad AFTER DELETE ON observations BEGIN
                INSERT INTO observations_fts(observations_fts, rowid, title, content, tool_name, type, project)
                VALUES ('delete', old.id, old.title, old.content, COALESCE(old.tool_name,''), old.type, old.project);
            END;

            CREATE TRIGGER observations_au AFTER UPDATE ON observations BEGIN
                INSERT INTO observations_fts(observations_fts, rowid, title, content, tool_name, type, project)
                VALUES ('delete', old.id, old.title, old.content, COALESCE(old.tool_name,''), old.type, old.project);
                INSERT INTO observations_fts(rowid, title, content, tool_name, type, project)
                SELECT new.id, new.title, new.content, COALESCE(new.tool_name,''), new.type, new.project
                WHERE new.deleted_at IS NULL;
            END;

            CREATE VIRTUAL TABLE IF NOT EXISTS prompts_fts USING fts5(
                content, project,
                content='user_prompts',
                content_rowid='id',
                tokenize='unicode61'
            );

            DROP TRIGGER IF EXISTS prompts_ai;
            DROP TRIGGER IF EXISTS prompts_ad;
            DROP TRIGGER IF EXISTS prompts_au;

            INSERT OR IGNORE INTO prompts_fts(rowid, content, project)
            SELECT id, content, project FROM user_prompts;

            CREATE TRIGGER prompts_ai AFTER INSERT ON user_prompts BEGIN
                INSERT INTO prompts_fts(rowid, content, project)
                VALUES (new.id, new.content, new.project);
            END;

            CREATE TRIGGER prompts_ad AFTER DELETE ON user_prompts BEGIN
                INSERT INTO prompts_fts(prompts_fts, rowid, content, project)
                VALUES ('delete', old.id, old.content, old.project);
            END;

            CREATE TRIGGER prompts_au AFTER UPDATE ON user_prompts BEGIN
                INSERT INTO prompts_fts(prompts_fts, rowid, content, project)
                VALUES ('delete', old.id, old.content, old.project);
                INSERT INTO prompts_fts(rowid, content, project)
                VALUES (new.id, new.content, new.project);
            END;"
        )?;

        Ok(())
    }

    pub fn set_project_path(&self, path: Option<&str>) {
        let mut inner = self.inner.lock().unwrap();
        inner.project_path = path.map(|p| p.to_string());
        if let Some(ref p) = path {
            if inner.workspace_root.is_empty() {
                inner.workspace_root = p.to_string();
            }
        }
    }

    pub fn project_path(&self) -> Option<String> {
        let inner = self.inner.lock().unwrap();
        inner.project_path.clone()
    }

    /// Open a project-scoped database.
    ///
    /// The DB is placed at `<project_path>/.ozymem/memory.db`.
    pub fn open_for_project(project_path: &Path) -> Result<Self> {
        let clean_path = strip_unc_prefix(project_path.to_path_buf());
        let db_path = resolve_project_db_path(&clean_path)?;
        let backend = Self::open(Some(&db_path.to_string_lossy()))?;
        backend.set_project_path(Some(&clean_path.to_string_lossy()));
        Ok(backend)
    }

    pub fn record_brain_audit(
        &self,
        request_hash: &str,
        action: &str,
        project_path: Option<&str>,
        git_head: Option<&str>,
        model_version: Option<&str>,
        brain_version: Option<&str>,
        schema_version: Option<&str>,
        latency_ms: u64,
        confidence: f64,
        result_summary: Option<&str>,
        raw_response: Option<&str>,
    ) -> Result<()> {
        let inner = self.inner.lock().unwrap();
        inner.sqlite.execute(
            "INSERT INTO ozy_brain_audit (
                request_hash, action, project_path, git_head, model_version,
                brain_version, schema_version, latency_ms, confidence, result_summary, raw_response
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                request_hash,
                action,
                project_path,
                git_head,
                model_version,
                brain_version.unwrap_or("0.2.0"),
                schema_version.unwrap_or("v1"),
                latency_ms as i64,
                confidence,
                result_summary,
                raw_response
            ],
        )?;
        Ok(())
    }

    pub fn clean_old_brain_audits(&self, days: u32) -> Result<usize> {
        let inner = self.inner.lock().unwrap();
        let rows = inner.sqlite.execute(
            "DELETE FROM ozy_brain_audit WHERE created_at < datetime('now', '-' || ?1 || ' days')",
            params![days],
        )?;
        Ok(rows)
    }
}

