use anyhow::{Context, Result};
use petgraph::graph::DiGraph;
use rusqlite::{params, Connection};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, atomic::AtomicBool};
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
        sqlite.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA foreign_keys=ON;
             PRAGMA busy_timeout = 5000;
             PRAGMA synchronous = NORMAL;
             PRAGMA cache_size = -64000;",
        )?;

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
            embedder: Arc::new(Mutex::new(None)),
            embedding_status: Arc::new(Mutex::new(Self::check_model_files_status())),
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

            CREATE TABLE IF NOT EXISTS memory_outbox (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                entity_type TEXT NOT NULL,
                entity_id TEXT NOT NULL,
                operation TEXT NOT NULL,
                payload TEXT NOT NULL,
                created_at TEXT NOT NULL,
                processed_at TEXT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_outbox_unprocessed ON memory_outbox(processed_at, id);
            CREATE INDEX IF NOT EXISTS idx_outbox_entity ON memory_outbox(entity_type, entity_id);

            CREATE TRIGGER IF NOT EXISTS lessons_outbox_ai AFTER INSERT ON lessons BEGIN
                INSERT INTO memory_outbox (entity_type, entity_id, operation, payload, created_at)
                VALUES ('lesson', CAST(new.id AS TEXT), 'UPSERT',
                    json_object('id', new.id, 'file_path', new.file_path, 'symbol_name', new.symbol_name,
                                'error_context', new.error_context, 'solution', new.solution, 'kind', new.kind,
                                'confidence_score', new.confidence_score, 'tenant_id', new.tenant_id, 'workspace_root', new.workspace_root),
                    datetime('now'));
            END;

            CREATE TRIGGER IF NOT EXISTS lessons_outbox_ad AFTER DELETE ON lessons BEGIN
                INSERT INTO memory_outbox (entity_type, entity_id, operation, payload, created_at)
                VALUES ('lesson', CAST(old.id AS TEXT), 'DELETE',
                    json_object('id', old.id, 'file_path', old.file_path),
                    datetime('now'));
            END;

            CREATE TRIGGER IF NOT EXISTS observations_outbox_ai AFTER INSERT ON observations BEGIN
                INSERT INTO memory_outbox (entity_type, entity_id, operation, payload, created_at)
                VALUES ('observation', CAST(new.id AS TEXT), 'UPSERT',
                    json_object('id', new.id, 'title', new.title, 'content', new.content, 'type', new.type,
                                'project', new.project, 'scope', new.scope, 'topic_key', new.topic_key,
                                'tenant_id', new.tenant_id, 'workspace_root', new.workspace_root),
                    datetime('now'));
            END;

            CREATE TRIGGER IF NOT EXISTS observations_outbox_au AFTER UPDATE ON observations BEGIN
                INSERT INTO memory_outbox (entity_type, entity_id, operation, payload, created_at)
                VALUES ('observation', CAST(new.id AS TEXT),
                    CASE WHEN new.deleted_at IS NOT NULL THEN 'DELETE' ELSE 'UPSERT' END,
                    json_object('id', new.id, 'title', new.title, 'content', new.content, 'type', new.type,
                                'project', new.project, 'scope', new.scope, 'topic_key', new.topic_key,
                                'deleted_at', new.deleted_at, 'tenant_id', new.tenant_id, 'workspace_root', new.workspace_root),
                    datetime('now'));
            END;

            CREATE TRIGGER IF NOT EXISTS observations_outbox_ad AFTER DELETE ON observations BEGIN
                INSERT INTO memory_outbox (entity_type, entity_id, operation, payload, created_at)
                VALUES ('observation', CAST(old.id AS TEXT), 'DELETE',
                    json_object('id', old.id, 'title', old.title),
                    datetime('now'));
            END;

            CREATE TABLE IF NOT EXISTS exploration_trajectories (
                id TEXT PRIMARY KEY,
                project_path TEXT NOT NULL,
                task_description TEXT NOT NULL,
                policy_version TEXT NOT NULL DEFAULT 'v1.0.0',
                status TEXT NOT NULL CHECK(status IN ('in_progress', 'completed', 'failed', 'abandoned')) DEFAULT 'in_progress',
                total_steps INTEGER NOT NULL DEFAULT 0,
                cumulative_reward REAL NOT NULL DEFAULT 0.0,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                completed_at DATETIME NULL
            );
            CREATE INDEX IF NOT EXISTS idx_traj_proj ON exploration_trajectories(project_path, status);

            CREATE TABLE IF NOT EXISTS exploration_nodes (
                id TEXT PRIMARY KEY,
                trajectory_id TEXT NOT NULL,
                parent_id TEXT NULL,
                depth INTEGER NOT NULL DEFAULT 0,
                action_type TEXT NOT NULL,
                action_payload TEXT NOT NULL,
                observation TEXT NOT NULL,
                cost_tokens INTEGER NOT NULL DEFAULT 0,
                latency_ms INTEGER NOT NULL DEFAULT 0,
                reward_score REAL NOT NULL DEFAULT 0.0,
                visit_count INTEGER NOT NULL DEFAULT 1,
                value_estimate REAL NOT NULL DEFAULT 0.0,
                is_solution BOOLEAN NOT NULL DEFAULT 0,
                is_pruned BOOLEAN NOT NULL DEFAULT 0,
                rollback_snapshot TEXT NULL,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY(trajectory_id) REFERENCES exploration_trajectories(id) ON DELETE CASCADE,
                FOREIGN KEY(parent_id) REFERENCES exploration_nodes(id) ON DELETE CASCADE
            );
            CREATE INDEX IF NOT EXISTS idx_nodes_traj_depth ON exploration_nodes(trajectory_id, depth);
            CREATE INDEX IF NOT EXISTS idx_nodes_parent ON exploration_nodes(parent_id);

            INSERT OR IGNORE INTO tenants (id, name) VALUES ('local', 'Local Tenant');"
        )?;

        // Migrate exploration_nodes: add rollback_snapshot if table existed without it
        let _ = inner.sqlite.execute("ALTER TABLE exploration_nodes ADD COLUMN rollback_snapshot TEXT NULL", []);

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

        // --- Graph Memory Architecture v1.3.0 (Task 1.1 & Task 1.2) ---
        inner.sqlite.execute_batch(
            "CREATE TABLE IF NOT EXISTS memory_nodes (
                id TEXT PRIMARY KEY,
                kind TEXT NOT NULL DEFAULT 'lesson'
                    CHECK(kind IN ('lesson','decision','convention','gotcha','module_rule','architecture')),
                title TEXT NOT NULL DEFAULT '',
                content TEXT NOT NULL,
                error_context TEXT NOT NULL DEFAULT '',
                solution TEXT NOT NULL DEFAULT '',
                confidence_score REAL NOT NULL DEFAULT 1.0,
                touch_count INTEGER NOT NULL DEFAULT 0,
                stale INTEGER NOT NULL DEFAULT 0,
                stale_reason TEXT NULL,
                created_at TEXT NOT NULL,
                last_verified_at TEXT NOT NULL DEFAULT '',
                tenant_id TEXT NOT NULL,
                workspace_root TEXT NOT NULL DEFAULT '',
                embedding BLOB NULL,
                embedding_model TEXT NOT NULL DEFAULT ''
            );
            CREATE INDEX IF NOT EXISTS idx_memory_nodes_tenant ON memory_nodes(tenant_id);
            CREATE INDEX IF NOT EXISTS idx_memory_nodes_kind ON memory_nodes(kind, tenant_id);
            CREATE INDEX IF NOT EXISTS idx_memory_nodes_stale ON memory_nodes(stale, tenant_id);

            CREATE TABLE IF NOT EXISTS memory_edges (
                source_type TEXT NOT NULL CHECK(source_type IN ('memory', 'file', 'symbol', 'trajectory_node')),
                source_id TEXT NOT NULL,
                target_type TEXT NOT NULL CHECK(target_type IN ('memory', 'file', 'symbol', 'trajectory_node')),
                target_id TEXT NOT NULL,
                edge_type TEXT NOT NULL CHECK(edge_type IN (
                    'applies_to',
                    'coupled_with',
                    'causes_regression',
                    'supersedes',
                    'reinforces',
                    'derived_from'
                )),
                weight REAL NOT NULL DEFAULT 1.0,
                created_at TEXT NOT NULL,
                tenant_id TEXT NOT NULL,
                workspace_root TEXT NOT NULL DEFAULT '',
                PRIMARY KEY (source_type, source_id, target_type, target_id, edge_type, tenant_id)
            );
            CREATE INDEX IF NOT EXISTS idx_memory_edges_src ON memory_edges(source_type, source_id, tenant_id);
            CREATE INDEX IF NOT EXISTS idx_memory_edges_dst ON memory_edges(target_type, target_id, tenant_id);
            CREATE INDEX IF NOT EXISTS idx_memory_edges_type ON memory_edges(edge_type, tenant_id);

            CREATE TRIGGER IF NOT EXISTS lessons_sync_nodes_ai AFTER INSERT ON lessons BEGIN
                INSERT OR IGNORE INTO memory_nodes (
                    id, kind, title, content, error_context, solution,
                    confidence_score, touch_count, stale, stale_reason,
                    created_at, last_verified_at, tenant_id, workspace_root,
                    embedding, embedding_model
                )
                VALUES (
                    'legacy_lesson_' || CAST(new.id AS TEXT),
                    new.kind,
                    CASE WHEN new.symbol_name != '' THEN new.symbol_name ELSE new.file_path END,
                    new.error_context || CASE WHEN new.solution != '' THEN ' -> ' || new.solution ELSE '' END,
                    new.error_context,
                    new.solution,
                    new.confidence_score,
                    new.touch_count,
                    new.stale,
                    new.stale_reason,
                    new.created_at,
                    new.last_verified_at,
                    new.tenant_id,
                    new.workspace_root,
                    new.embedding,
                    new.embedding_model
                );

                INSERT OR IGNORE INTO memory_edges (
                    source_type, source_id, target_type, target_id, edge_type, weight, created_at, tenant_id, workspace_root
                )
                SELECT
                    'memory',
                    'legacy_lesson_' || CAST(new.id AS TEXT),
                    'file',
                    new.file_path,
                    'applies_to',
                    1.0,
                    new.created_at,
                    new.tenant_id,
                    new.workspace_root
                WHERE new.file_path != '' AND new.file_path != 'global' AND new.file_path != 'unknown';
            END;

            CREATE TRIGGER IF NOT EXISTS lessons_sync_nodes_au AFTER UPDATE ON lessons BEGIN
                UPDATE memory_nodes SET
                    kind = new.kind,
                    title = CASE WHEN new.symbol_name != '' THEN new.symbol_name ELSE new.file_path END,
                    content = new.error_context || CASE WHEN new.solution != '' THEN ' -> ' || new.solution ELSE '' END,
                    error_context = new.error_context,
                    solution = new.solution,
                    confidence_score = new.confidence_score,
                    touch_count = new.touch_count,
                    stale = new.stale,
                    stale_reason = new.stale_reason,
                    last_verified_at = new.last_verified_at,
                    embedding = new.embedding,
                    embedding_model = new.embedding_model
                WHERE id = 'legacy_lesson_' || CAST(old.id AS TEXT);
            END;

            CREATE TRIGGER IF NOT EXISTS lessons_sync_nodes_ad AFTER DELETE ON lessons BEGIN
                DELETE FROM memory_nodes WHERE id = 'legacy_lesson_' || CAST(old.id AS TEXT);
                DELETE FROM memory_edges WHERE source_type = 'memory' AND source_id = 'legacy_lesson_' || CAST(old.id AS TEXT);
                DELETE FROM memory_edges WHERE target_type = 'memory' AND target_id = 'legacy_lesson_' || CAST(old.id AS TEXT);
            END;

            CREATE VIRTUAL TABLE IF NOT EXISTS memory_nodes_fts USING fts5(
                title, content, error_context, solution,
                content='memory_nodes',
                content_rowid='rowid',
                tokenize='unicode61'
            );

            CREATE TRIGGER IF NOT EXISTS memory_nodes_ai AFTER INSERT ON memory_nodes BEGIN
                INSERT INTO memory_nodes_fts(rowid, title, content, error_context, solution)
                VALUES (new.rowid, new.title, new.content, new.error_context, new.solution);
            END;

            CREATE TRIGGER IF NOT EXISTS memory_nodes_ad AFTER DELETE ON memory_nodes BEGIN
                INSERT INTO memory_nodes_fts(memory_nodes_fts, rowid, title, content, error_context, solution)
                VALUES ('delete', old.rowid, old.title, old.content, old.error_context, old.solution);
            END;

            CREATE TRIGGER IF NOT EXISTS memory_nodes_au AFTER UPDATE ON memory_nodes BEGIN
                INSERT INTO memory_nodes_fts(memory_nodes_fts, rowid, title, content, error_context, solution)
                VALUES ('delete', old.rowid, old.title, old.content, old.error_context, old.solution);
                INSERT INTO memory_nodes_fts(rowid, title, content, error_context, solution)
                VALUES (new.rowid, new.title, new.content, new.error_context, new.solution);
            END;

            -- One-time backfill from lessons into memory_nodes and memory_edges
            INSERT OR IGNORE INTO memory_nodes (
                id, kind, title, content, error_context, solution,
                confidence_score, touch_count, stale, stale_reason,
                created_at, last_verified_at, tenant_id, workspace_root,
                embedding, embedding_model
            )
            SELECT
                'legacy_lesson_' || CAST(id AS TEXT),
                kind,
                CASE WHEN symbol_name != '' THEN symbol_name ELSE file_path END,
                error_context || CASE WHEN solution != '' THEN ' -> ' || solution ELSE '' END,
                error_context,
                solution,
                confidence_score,
                touch_count,
                stale,
                stale_reason,
                created_at,
                last_verified_at,
                tenant_id,
                workspace_root,
                embedding,
                embedding_model
            FROM lessons;

            INSERT OR IGNORE INTO memory_edges (
                source_type, source_id, target_type, target_id, edge_type, weight, created_at, tenant_id, workspace_root
            )
            SELECT
                'memory',
                'legacy_lesson_' || CAST(id AS TEXT),
                'file',
                file_path,
                'applies_to',
                1.0,
                created_at,
                tenant_id,
                workspace_root
            FROM lessons
            WHERE file_path != '' AND file_path != 'global' AND file_path != 'unknown';

            INSERT OR IGNORE INTO memory_nodes_fts(rowid, title, content, error_context, solution)
            SELECT rowid, title, content, error_context, solution FROM memory_nodes;"
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

