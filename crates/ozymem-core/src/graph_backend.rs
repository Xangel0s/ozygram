//! petgraph (RAM) + SQLite (disco) backend for Ozymem MCP.

use crate::mcp_common::McpBackend;
use crate::sync::{check_noise_or_huge_file, DEFAULT_MAX_DELTA_FILE_BYTES, DeltaIndexResult, NoiseFilterDecision};
use crate::{FileGraphContext, GraphSummary, StoredFunction};
use anyhow::{Context, Result};
use ozymem_parser::{
    SupportedLanguage, SymbolKind, extract_dependency_hints, is_binary_file,
    is_internal_dependency_hint, parse_source, resolve_dependency_target,
};
use petgraph::algo::all_simple_paths;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::Bfs;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicBool, Ordering},
};
use std::time::Instant;

use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};

const OZYMEM_DIR: &str = ".ozymem";
const MEMORY_DB: &str = "memory.db";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileNode {
    pub path: String,
    pub language: String,
    pub strategy: String,
    pub function_count: i64,
    pub lesson_count: i64,
}

#[derive(Debug, Clone)]
pub struct FileEdge;

pub const LESSON_KINDS: &[&str] = &["lesson", "decision", "convention", "gotcha", "module_rule"];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LessonEntry {
    pub id: i64,
    pub file_path: String,
    pub symbol_name: String,
    pub error_context: String,
    pub solution: String,
    pub kind: String,
    pub created_at: String,
    pub stale: i64,
    pub stale_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemorySession {
    pub id: String,
    pub project: String,
    pub directory: String,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub summary: Option<String>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservationEntry {
    pub id: i64,
    pub session_id: String,
    pub observation_type: String,
    pub title: String,
    pub content: String,
    pub project: String,
    pub scope: String,
    pub topic_key: Option<String>,
    pub revision_count: i64,
    pub duplicate_count: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserPromptEntry {
    pub id: i64,
    pub session_id: String,
    pub content: String,
    pub project: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NeighborInfo {
    pub file_path: String,
    pub incoming: Vec<String>,
    pub outgoing: Vec<String>,
}

impl fmt::Display for LessonEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let stale_tag = if self.stale != 0 {
            format!(
                " [STALE: {}]",
                self.stale_reason.as_deref().unwrap_or("unknown")
            )
        } else {
            String::new()
        };
        write!(
            f,
            "[{}]{} {} :: {}\n    context: {}\n    solution: {}",
            self.kind,
            stale_tag,
            self.file_path,
            self.symbol_name,
            self.error_context,
            self.solution
        )
    }
}

#[derive(Debug, Clone)]
pub struct ScanProgress {
    pub scanning: bool,
    pub total: usize,
    pub processed: usize,
    pub current_file: String,
}

struct Inner {
    graph: DiGraph<FileNode, FileEdge>,
    file_index: HashMap<String, NodeIndex>,
    sqlite: Connection,
    project_path: Option<String>,
    workspace_root: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ContractMismatchAlert {
    pub alert_type: String,
    pub file_path: String,
    pub endpoint: Option<String>,
    pub template_found: Option<String>,
    pub header_version: Option<String>,
    pub template_version: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExportContractReport {
    pub templates_reviewed: usize,
    pub endpoints_reviewed: usize,
    pub version_mismatches: Vec<ContractMismatchAlert>,
    pub missing_templates: Vec<ContractMismatchAlert>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UnifiedSearchResult {
    pub category: String,
    pub title: String,
    pub path: String,
    pub snippet: String,
    pub score: f32,
}

pub struct GraphBackend {
    inner: Mutex<Inner>,
    tenant_id: String,
    pub scan_progress: Arc<Mutex<ScanProgress>>,
    pub scanning: AtomicBool,
    last_check: Mutex<Instant>,
    embedder: OnceLock<Option<Mutex<TextEmbedding>>>,
}

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

    pub fn memory_session_start(&self, id: &str, project: &str, directory: &str) -> Result<()> {
        let now = now_ts();
        let inner = self.inner.lock().unwrap();
        inner.sqlite.execute(
            "INSERT INTO sessions (id, project, directory, started_at, status, tenant_id, workspace_root)
             VALUES (?1, ?2, ?3, ?4, 'active', ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET
                project = CASE WHEN sessions.project = '' THEN excluded.project ELSE sessions.project END,
                directory = CASE WHEN sessions.directory = '' THEN excluded.directory ELSE sessions.directory END,
                status = 'active'",
            params![id, project, directory, now, self.tenant_id, inner.workspace_root],
        )?;
        Ok(())
    }

    pub fn memory_session_end(&self, id: &str, summary: Option<&str>) -> Result<()> {
        let now = now_ts();
        let inner = self.inner.lock().unwrap();
        inner.sqlite.execute(
            "UPDATE sessions SET ended_at = ?1, summary = COALESCE(?2, summary), status = 'completed'
             WHERE id = ?3 AND tenant_id = ?4",
            params![now, summary, id, self.tenant_id],
        )?;
        Ok(())
    }

    pub fn save_user_prompt(
        &self,
        session_id: &str,
        content: &str,
        project: Option<&str>,
    ) -> Result<i64> {
        let now = now_ts();
        let inner = self.inner.lock().unwrap();
        let project = project.unwrap_or_else(|| default_project_name(&inner));
        inner.sqlite.execute(
            "INSERT INTO user_prompts (session_id, content, project, created_at, tenant_id, workspace_root)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![session_id, content, project, now, self.tenant_id, inner.workspace_root],
        )?;
        Ok(inner.sqlite.last_insert_rowid())
    }

    pub fn save_observation(
        &self,
        session_id: &str,
        observation_type: &str,
        title: &str,
        content: &str,
        project: Option<&str>,
        scope: Option<&str>,
        topic_key: Option<&str>,
        tool_name: Option<&str>,
    ) -> Result<ObservationEntry> {
        let now = now_ts();
        let normalized_hash = normalized_hash(&format!("{observation_type}\n{title}\n{content}"));
        let inner = self.inner.lock().unwrap();
        let project = project.unwrap_or_else(|| default_project_name(&inner));
        let scope = normalize_scope(scope);
        let topic = topic_key.map(normalize_topic_key).filter(|s| !s.is_empty());

        if let Some(ref topic) = topic {
            if let Some(id) = inner.sqlite.query_row(
                "SELECT id FROM observations
                 WHERE project = ?1 AND scope = ?2 AND topic_key = ?3 AND tenant_id = ?4 AND deleted_at IS NULL
                 ORDER BY id DESC LIMIT 1",
                params![project, scope, topic, self.tenant_id],
                |row| row.get::<_, i64>(0),
            ).ok() {
                inner.sqlite.execute(
                    "UPDATE observations SET session_id = ?1, type = ?2, title = ?3, content = ?4,
                        tool_name = ?5, normalized_hash = ?6, revision_count = revision_count + 1,
                        last_seen_at = ?7, updated_at = ?7
                     WHERE id = ?8",
                    params![session_id, observation_type, title, content, tool_name, normalized_hash, now, id],
                )?;
                return self.get_observation_locked(&inner, id);
            }
        }

        if let Some(id) = inner
            .sqlite
            .query_row(
                "SELECT id FROM observations
             WHERE normalized_hash = ?1 AND project = ?2 AND scope = ?3 AND type = ?4 AND title = ?5
               AND tenant_id = ?6 AND deleted_at IS NULL
             ORDER BY id DESC LIMIT 1",
                params![
                    normalized_hash,
                    project,
                    scope,
                    observation_type,
                    title,
                    self.tenant_id
                ],
                |row| row.get::<_, i64>(0),
            )
            .ok()
        {
            inner.sqlite.execute(
                "UPDATE observations SET duplicate_count = duplicate_count + 1, last_seen_at = ?1, updated_at = ?1
                 WHERE id = ?2",
                params![now, id],
            )?;
            return self.get_observation_locked(&inner, id);
        }

        inner.sqlite.execute(
            "INSERT INTO observations
             (session_id, type, title, content, tool_name, project, scope, topic_key, normalized_hash,
              last_seen_at, created_at, updated_at, tenant_id, workspace_root)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10, ?10, ?11, ?12)",
            params![session_id, observation_type, title, content, tool_name, project, scope, topic, normalized_hash, now, self.tenant_id, inner.workspace_root],
        )?;
        self.get_observation_locked(&inner, inner.sqlite.last_insert_rowid())
    }

    pub fn get_observation(&self, id: i64) -> Result<ObservationEntry> {
        let inner = self.inner.lock().unwrap();
        self.get_observation_locked(&inner, id)
    }

    pub fn soft_delete_observation(&self, id: i64) -> Result<bool> {
        let inner = self.inner.lock().unwrap();
        let affected = inner.sqlite.execute(
            "UPDATE observations SET deleted_at = ?1, updated_at = ?1 WHERE id = ?2 AND tenant_id = ?3 AND deleted_at IS NULL",
            params![now_ts(), id, self.tenant_id],
        )?;
        Ok(affected > 0)
    }

    pub fn search_observations(
        &self,
        query: &str,
        project: Option<&str>,
        scope: Option<&str>,
        observation_type: Option<&str>,
        limit: usize,
    ) -> Result<Vec<ObservationEntry>> {
        let inner = self.inner.lock().unwrap();
        let limit = (limit as i64).min(100).max(1);
        let project = project.unwrap_or_else(|| default_project_name(&inner));
        let scope = normalize_scope(scope);
        let mut out = Vec::new();
        let like = format!("%{}%", query);
        let type_filter = observation_type.unwrap_or("");
        let sql = "SELECT id FROM observations
            WHERE tenant_id = ?1 AND project = ?2 AND scope = ?3 AND deleted_at IS NULL
              AND (?4 = '' OR type = ?4)
              AND (?5 = '' OR title LIKE ?6 OR content LIKE ?6 OR COALESCE(tool_name,'') LIKE ?6)
            ORDER BY id DESC LIMIT ?7";
        let mut stmt = inner.sqlite.prepare(sql)?;
        for id in stmt
            .query_map(
                params![
                    self.tenant_id,
                    project,
                    scope,
                    type_filter,
                    query,
                    like,
                    limit
                ],
                |row| row.get::<_, i64>(0),
            )?
            .flatten()
        {
            out.push(self.get_observation_locked(&inner, id)?);
        }
        Ok(out)
    }

    pub fn recent_observations(
        &self,
        project: Option<&str>,
        scope: Option<&str>,
        limit: usize,
    ) -> Result<Vec<ObservationEntry>> {
        self.search_observations("", project, scope, None, limit)
    }

    pub fn observation_timeline(
        &self,
        id: i64,
        before: usize,
        after: usize,
    ) -> Result<Vec<ObservationEntry>> {
        let inner = self.inner.lock().unwrap();
        let session_id: String = inner.sqlite.query_row(
            "SELECT session_id FROM observations WHERE id = ?1 AND tenant_id = ?2 AND deleted_at IS NULL",
            params![id, self.tenant_id],
            |row| row.get(0),
        )?;
        let mut stmt = inner.sqlite.prepare(
            "SELECT id FROM observations
             WHERE session_id = ?1 AND tenant_id = ?2 AND deleted_at IS NULL
             ORDER BY id ASC",
        )?;
        let ids: Vec<i64> = stmt
            .query_map(params![session_id, self.tenant_id], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();
        let pos = ids.iter().position(|x| *x == id).unwrap_or(0);
        let start = pos.saturating_sub(before);
        let end = (pos + after + 1).min(ids.len());
        ids[start..end]
            .iter()
            .map(|id| self.get_observation_locked(&inner, *id))
            .collect()
    }

    pub fn passive_capture(
        &self,
        session_id: &str,
        text: &str,
        project: Option<&str>,
    ) -> Result<Vec<ObservationEntry>> {
        let mut saved = Vec::new();
        let mut in_section = false;
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.eq_ignore_ascii_case("## key learnings:")
                || trimmed.eq_ignore_ascii_case("## key learnings")
                || trimmed.eq_ignore_ascii_case("## aprendizajes clave:")
                || trimmed.eq_ignore_ascii_case("## aprendizajes clave")
            {
                in_section = true;
                continue;
            }
            if in_section && trimmed.starts_with("## ") {
                break;
            }
            if !in_section {
                continue;
            }
            let item = trimmed
                .trim_start_matches(|c: char| {
                    c == '-' || c == '*' || c.is_ascii_digit() || c == '.' || c == ')'
                })
                .trim();
            if item.len() >= 8 {
                saved.push(self.save_observation(
                    session_id,
                    "learning",
                    &item.chars().take(80).collect::<String>(),
                    item,
                    project,
                    Some("project"),
                    None,
                    Some("passive_capture"),
                )?);
            }
        }
        Ok(saved)
    }

    fn get_observation_locked(&self, inner: &Inner, id: i64) -> Result<ObservationEntry> {
        inner
            .sqlite
            .query_row(
                "SELECT id, session_id, type, title, content, project, scope, topic_key,
                    revision_count, duplicate_count, created_at, updated_at
             FROM observations
             WHERE id = ?1 AND tenant_id = ?2 AND deleted_at IS NULL",
                params![id, self.tenant_id],
                |row| {
                    Ok(ObservationEntry {
                        id: row.get(0)?,
                        session_id: row.get(1)?,
                        observation_type: row.get(2)?,
                        title: row.get(3)?,
                        content: row.get(4)?,
                        project: row.get(5)?,
                        scope: row.get(6)?,
                        topic_key: row.get(7)?,
                        revision_count: row.get(8)?,
                        duplicate_count: row.get(9)?,
                        created_at: row.get(10)?,
                        updated_at: row.get(11)?,
                    })
                },
            )
            .map_err(Into::into)
    }

    pub fn reload_if_stale(&self) {
        if self.scanning.load(Ordering::SeqCst) {
            return;
        }

        {
            let mut last = self.last_check.lock().unwrap();
            if last.elapsed() < std::time::Duration::from_millis(500) {
                return;
            }
            *last = Instant::now();
        }

        let proj_path = {
            let inner = self.inner.lock().unwrap();
            inner.project_path.clone()
        };
        let Some(ref proj_path) = proj_path else {
            return;
        };
        if !Path::new(proj_path).exists() {
            return;
        }

        let sqlite_snapshot = {
            let inner = self.inner.lock().unwrap();
            let mut stmt = inner
                .sqlite
                .prepare("SELECT path, mtime FROM files WHERE tenant_id = ?1")
                .ok();
            let file_mtimes: HashMap<String, String> = match stmt {
                Some(ref mut s) => s
                    .query_map(params![self.tenant_id], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                    })
                    .ok()
                    .map(|rows| rows.filter_map(|r| r.ok()).collect())
                    .unwrap_or_default(),
                None => HashMap::new(),
            };
            file_mtimes
        };

        if sqlite_snapshot.is_empty() {
            return;
        }

        let mut changed = false;
        let proj_ref = Path::new(proj_path);
        let ignore_patterns = load_ignore_patterns(proj_ref);
        let has_ignores = !ignore_patterns.is_empty();
        for entry in walkdir::WalkDir::new(proj_path)
            .into_iter()
            .filter_entry(|e| {
                if is_noise_dir(e.path()) {
                    return false;
                }
                if has_ignores && path_matches_ignore(e.path(), &ignore_patterns, proj_ref) {
                    return false;
                }
                true
            })
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if !path.is_file() || is_binary_file(path) {
                continue;
            }
            let abs_path = path.to_string_lossy().to_string();
            let current_mtime = file_mtime(path).unwrap_or_default();
            let stored = sqlite_snapshot.get(&abs_path);
            if stored.map_or(true, |s| *s != current_mtime) {
                changed = true;
                break;
            }
        }

        if changed {
            eprintln!("[graph] changes detected, rescanning...");
            self.scanning.store(true, Ordering::SeqCst);
            if let Err(e) = self.full_scan(proj_path, None) {
                eprintln!("[graph] scan error: {e}");
            }
            self.scanning.store(false, Ordering::SeqCst);
        }
    }

    pub fn full_scan(&self, project_path: &str, progress: Option<&dyn Fn(u64, u64)>) -> Result<()> {
        self.scanning.store(true, Ordering::SeqCst);

        // Collect scanned file paths for staleness comparison
        let mut scanned_files: HashSet<String> = HashSet::new();

        // Load ignore patterns from .ozymignore and .gitignore
        let ignore_patterns = load_ignore_patterns(Path::new(project_path));
        let has_ignores = !ignore_patterns.is_empty();

        // Clear stale data for this tenant before re-scan
        {
            let inner = self.inner.lock().unwrap();
            inner.sqlite.execute(
                "DELETE FROM file_dependencies WHERE tenant_id = ?1",
                params![self.tenant_id],
            )?;
            inner.sqlite.execute(
                "DELETE FROM functions WHERE tenant_id = ?1",
                params![self.tenant_id],
            )?;
            inner.sqlite.execute(
                "DELETE FROM files WHERE tenant_id = ?1",
                params![self.tenant_id],
            )?;
        }

        let proj_root = Path::new(project_path);

        // Pre-count total files for progress reporting
        let total_file_count: u64 = walkdir::WalkDir::new(project_path)
            .into_iter()
            .filter_entry(|e| {
                if is_noise_dir(e.path()) {
                    return false;
                }
                if has_ignores && path_matches_ignore(e.path(), &ignore_patterns, proj_root) {
                    return false;
                }
                true
            })
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_file() && !is_binary_file(e.path()))
            .count() as u64;

        let mut file_count = 0i64;
        let mut skipped_noise = 0u64;
        let mut skipped_ignore = 0u64;
        let mut skipped_binary = 0u64;
        let mut skipped_read = 0u64;
        let mut skipped_parse = 0u64;
        let mut processed: u64 = 0;
        for entry in walkdir::WalkDir::new(project_path)
            .into_iter()
            .filter_entry(|e| {
                let noise = is_noise_dir(e.path());
                if noise && e.path().is_dir() {
                    skipped_noise += 1;
                }
                if !noise
                    && has_ignores
                    && path_matches_ignore(e.path(), &ignore_patterns, proj_root)
                {
                    if e.path().is_dir() {
                        skipped_ignore += 1;
                    }
                    return false;
                }
                !noise
            })
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            if is_binary_file(path) {
                let abs_path = crate::normalize_path(&path.to_string_lossy());
                if ozymem_parser::is_excel_template_candidate(&abs_path) {
                    if let Ok(Some(meta)) = ozymem_parser::parse_excel_template(path, &abs_path) {
                        let _ = self.record_excel_template(&meta);
                    }
                }
                skipped_binary += 1;
                if let Some(cb) = progress {
                    processed += 1;
                    cb(processed, total_file_count);
                }
                continue;
            }
            let abs_path = crate::normalize_path(&path.to_string_lossy());
            scanned_files.insert(abs_path.clone());
            let mtime = file_mtime(path).unwrap_or_default();

            let source = match std::fs::read_to_string(path) {
                Ok(s) => s,
                Err(_) => {
                    skipped_read += 1;
                    if let Some(cb) = progress {
                        processed += 1;
                        cb(processed, total_file_count);
                    }
                    continue;
                }
            };
            let lang = detect_language(path);
            let parsed = match parse_source(&abs_path, lang, &source) {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("[graph] parse error {abs_path}: {e}");
                    skipped_parse += 1;
                    if let Some(cb) = progress {
                        processed += 1;
                        cb(processed, total_file_count);
                    }
                    continue;
                }
            };

            let sha256_hash = {
                let mut hasher = Sha256::new();
                hasher.update(source.as_bytes());
                hex::encode(hasher.finalize())
            };

            let inner = self.inner.lock().unwrap();
            inner.sqlite.execute(
                "INSERT OR REPLACE INTO files (path, language, strategy, mtime, tenant_id, workspace_root, sha256) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![abs_path, parsed.language, parsed.strategy.as_str(), mtime, self.tenant_id, inner.workspace_root, sha256_hash],
            )?;

            for fn_data in &parsed.functions {
                let kind_str = match fn_data.kind {
                    SymbolKind::Function => "Function",
                    SymbolKind::Class => "Class",
                };
                inner.sqlite.execute(
                    "INSERT OR REPLACE INTO functions (name, kind, start_line, end_line, strategy, file_path, tenant_id, workspace_root) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![fn_data.name, kind_str, fn_data.start_line as i64, fn_data.end_line as i64, parsed.strategy.as_str(), abs_path, self.tenant_id, inner.workspace_root],
                )?;
            }

            let hints = extract_dependency_hints(&abs_path, lang, &source).unwrap_or_default();
            for hint in &hints {
                if !is_internal_dependency_hint(hint) {
                    continue;
                }
                if let Some(target) = resolve_dependency_target(hint, &abs_path) {
                    let target_str = crate::normalize_path(&target.to_string_lossy());
                    inner.sqlite.execute(
                        "INSERT OR IGNORE INTO file_dependencies (origin_path, destination_path, tenant_id, workspace_root) VALUES (?1, ?2, ?3, ?4)",
                        params![abs_path, target_str, self.tenant_id, inner.workspace_root],
                    )?;
                }
            }

            file_count += 1;
            if let Some(cb) = progress {
                processed += 1;
                cb(processed, total_file_count);
            }
        }

        // Mark stale lessons — only for files that were part of this scan
        if !scanned_files.is_empty() {
            let inner = self.inner.lock().unwrap();
            let marked = mark_stale_lessons(
                &inner.sqlite,
                &self.tenant_id,
                &scanned_files,
                &inner.workspace_root,
            )?;
            if marked > 0 {
                eprintln!("[graph] marked {marked} lessons as stale");
            }
        }

        self.rebuild_graph()?;

        self.scanning.store(false, Ordering::SeqCst);
        let inner = self.inner.lock().unwrap();
        eprintln!(
            "[graph] scan complete: indexed={file_count}, noise_dirs={skipped_noise}, ignore_pat={skipped_ignore}, binary={skipped_binary}, read_err={skipped_read}, parse_err={skipped_parse}; graph: {} nodes, {} edges",
            inner.graph.node_count(),
            inner.graph.edge_count()
        );
        Ok(())
    }

    pub fn rebuild_graph(&self) -> Result<()> {
        let rows: Vec<(String, String, String, i64, i64)>;
        let deps: Vec<(String, String)>;

        {
            let inner = self.inner.lock().unwrap();

            let mut stmt = inner.sqlite.prepare(
                 "SELECT f.path, f.language, f.strategy,
                        (SELECT COUNT(*) FROM functions fn WHERE fn.file_path = f.path AND fn.tenant_id = f.tenant_id) AS fc,
                        (SELECT COUNT(*) FROM lessons l WHERE l.file_path = f.path AND l.tenant_id = f.tenant_id) AS lc
                 FROM files f WHERE f.tenant_id = ?1 ORDER BY f.path ASC"
            )?;
            rows = stmt
                .query_map(params![self.tenant_id], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, i64>(4)?,
                    ))
                })?
                .filter_map(|r| r.ok())
                .collect();

            let mut dep_stmt = inner.sqlite.prepare(
                "SELECT origin_path, destination_path FROM file_dependencies WHERE tenant_id = ?1",
            )?;
            deps = dep_stmt
                .query_map(params![self.tenant_id], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?
                .filter_map(|r| r.ok())
                .collect();
        }

        let mut graph = DiGraph::new();
        let mut file_index = HashMap::new();

        for (path, language, strategy, fc, lc) in &rows {
            let idx = graph.add_node(FileNode {
                path: path.clone(),
                language: language.clone(),
                strategy: strategy.clone(),
                function_count: *fc,
                lesson_count: *lc,
            });
            file_index.insert(path.clone(), idx);
        }

        for (origin, destination) in &deps {
            if let (Some(&from_idx), Some(&to_idx)) =
                (file_index.get(origin), file_index.get(destination))
            {
                graph.add_edge(from_idx, to_idx, FileEdge);
            }
        }

        let mut inner_w = self.inner.lock().unwrap();
        inner_w.graph = graph;
        inner_w.file_index = file_index;

        Ok(())
    }

    /// Incrementally indexes a single file if modified (Live Delta Indexing).
    /// Skips files that match ignore patterns, noise dirs, minified/lockfiles, or unchanged SHA-256.
    pub fn index_file_delta(&self, file_path: &Path, project_root: &Path) -> Result<DeltaIndexResult> {
        // 1. Noise, Lockfile, Minified, Size limit check
        match check_noise_or_huge_file(file_path, DEFAULT_MAX_DELTA_FILE_BYTES) {
            NoiseFilterDecision::Process => {}
            NoiseFilterDecision::NoiseDir
            | NoiseFilterDecision::MinifiedOrBundle
            | NoiseFilterDecision::LockFile
            | NoiseFilterDecision::LargeOrBinaryData => {
                return Ok(DeltaIndexResult::SkippedNoise);
            }
            NoiseFilterDecision::ExceedsSizeLimit(size_bytes) => {
                return Ok(DeltaIndexResult::SkippedTooLarge { size_bytes });
            }
        }

        // 2. Binary file check
        if is_binary_file(file_path) {
            let abs_path = crate::normalize_path(&file_path.to_string_lossy());
            if ozymem_parser::is_excel_template_candidate(&abs_path) {
                if let Ok(Some(meta)) = ozymem_parser::parse_excel_template(file_path, &abs_path) {
                    let _ = self.record_excel_template(&meta);
                }
            }
            return Ok(DeltaIndexResult::SkippedBinary);
        }

        // 3. Ignore patterns check
        let ignore_patterns = load_ignore_patterns(project_root);
        if !ignore_patterns.is_empty() && path_matches_ignore(file_path, &ignore_patterns, project_root) {
            return Ok(DeltaIndexResult::SkippedNoise);
        }

        // 4. Read source content and calculate SHA-256
        let source = match std::fs::read_to_string(file_path) {
            Ok(s) => s,
            Err(_) => return Ok(DeltaIndexResult::SkippedNoise),
        };

        let sha256_hash = {
            let mut hasher = Sha256::new();
            hasher.update(source.as_bytes());
            hex::encode(hasher.finalize())
        };

        let abs_path = crate::normalize_path(&file_path.to_string_lossy());
        let mtime = file_mtime(file_path).unwrap_or_default();

        // 5. Compare with existing SHA-256 in SQLite
        {
            let inner = self.inner.lock().unwrap();
            let mut stmt = inner.sqlite.prepare(
                "SELECT sha256 FROM files WHERE path = ?1 AND tenant_id = ?2",
            )?;
            let mut rows = stmt.query(params![abs_path, self.tenant_id])?;
            if let Some(row) = rows.next()? {
                let existing_sha: String = row.get(0)?;
                if !existing_sha.is_empty() && existing_sha == sha256_hash {
                    return Ok(DeltaIndexResult::Unchanged);
                }
            }
        }

        // 6. Parse AST
        let lang = detect_language(file_path);
        let parsed = match parse_source(&abs_path, lang, &source) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("[delta] parse error for {abs_path}: {e}");
                return Ok(DeltaIndexResult::SkippedNoise);
            }
        };

        let hints = extract_dependency_hints(&abs_path, lang, &source).unwrap_or_default();
        let mut resolved_deps: Vec<String> = Vec::new();
        for hint in &hints {
            if is_internal_dependency_hint(hint) {
                if let Some(target) = resolve_dependency_target(hint, &abs_path) {
                    resolved_deps.push(crate::normalize_path(&target.to_string_lossy()));
                }
            }
        }

        // 7. Update SQLite atomically
        {
            let inner = self.inner.lock().unwrap();
            inner.sqlite.execute(
                "DELETE FROM functions WHERE file_path = ?1 AND tenant_id = ?2",
                params![abs_path, self.tenant_id],
            )?;
            inner.sqlite.execute(
                "DELETE FROM file_dependencies WHERE origin_path = ?1 AND tenant_id = ?2",
                params![abs_path, self.tenant_id],
            )?;

            inner.sqlite.execute(
                "INSERT OR REPLACE INTO files (path, language, strategy, mtime, tenant_id, workspace_root, sha256) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![abs_path, parsed.language, parsed.strategy.as_str(), mtime, self.tenant_id, inner.workspace_root, sha256_hash],
            )?;

            for fn_data in &parsed.functions {
                let kind_str = match fn_data.kind {
                    SymbolKind::Function => "Function",
                    SymbolKind::Class => "Class",
                };
                inner.sqlite.execute(
                    "INSERT OR REPLACE INTO functions (name, kind, start_line, end_line, strategy, file_path, tenant_id, workspace_root) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![fn_data.name, kind_str, fn_data.start_line as i64, fn_data.end_line as i64, parsed.strategy.as_str(), abs_path, self.tenant_id, inner.workspace_root],
                )?;
            }

            for dep in &resolved_deps {
                inner.sqlite.execute(
                    "INSERT OR IGNORE INTO file_dependencies (origin_path, destination_path, tenant_id, workspace_root) VALUES (?1, ?2, ?3, ?4)",
                    params![abs_path, dep, self.tenant_id, inner.workspace_root],
                )?;
            }
        }

        // 8. Rebuild graph representation
        self.rebuild_graph()?;

        Ok(DeltaIndexResult::Indexed {
            symbols: parsed.functions.len(),
            dependencies: resolved_deps.len(),
        })
    }

    /// Removes a file and its relations from SQLite and in-memory graph (Delta Indexing).
    pub fn remove_file_delta(&self, file_path: &Path, _project_root: &Path) -> Result<bool> {
        let abs_path = crate::normalize_path(&file_path.to_string_lossy());
        let affected = {
            let inner = self.inner.lock().unwrap();
            inner.sqlite.execute(
                "DELETE FROM file_dependencies WHERE (origin_path = ?1 OR destination_path = ?1) AND tenant_id = ?2",
                params![abs_path, self.tenant_id],
            )?;
            inner.sqlite.execute(
                "DELETE FROM functions WHERE file_path = ?1 AND tenant_id = ?2",
                params![abs_path, self.tenant_id],
            )?;
            inner.sqlite.execute(
                "DELETE FROM files WHERE path = ?1 AND tenant_id = ?2",
                params![abs_path, self.tenant_id],
            )?
        };

        if affected > 0 {
            self.rebuild_graph()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn analyze_impact(&self, file_path: &str, depth: u32) -> Vec<ImpactEntry> {
        let inner = self.inner.lock().unwrap();
        let Some(&start) = inner.file_index.get(file_path) else {
            return vec![];
        };

        let mut results = Vec::new();
        let mut bfs = Bfs::new(&inner.graph, start);

        let mut visited: HashMap<NodeIndex, u32> = HashMap::new();
        visited.insert(start, 0);

        // Prepare function query for enriching results
        let mut func_stmt = inner.sqlite.prepare(
            "SELECT name, start_line FROM functions WHERE file_path = ?1 AND tenant_id = ?2 ORDER BY start_line LIMIT 10"
        ).ok();

        // Prepare line-range query
        let mut range_stmt = inner.sqlite.prepare(
            "SELECT COALESCE(MIN(start_line),0), COALESCE(MAX(end_line),0) FROM functions WHERE file_path = ?1 AND tenant_id = ?2"
        ).ok();

        while let Some(node) = bfs.next(&inner.graph) {
            let d = *visited.get(&node).unwrap_or(&0);
            for neighbor in inner.graph.neighbors(node) {
                visited.entry(neighbor).or_insert(d + 1);
            }

            if node == start {
                continue;
            }
            if d > depth {
                continue;
            }
            if let Some(fn_data) = inner.graph.node_weight(node) {
                let path = &fn_data.path;
                let lower = path.to_lowercase();

                // Severity
                let (severity, reason) = if lower.contains("schema")
                    || lower.contains("model")
                    || lower.contains("dto")
                    || lower.contains("entity")
                    || lower.contains("interface")
                    || lower.contains("type")
                {
                    ("breaking", "Contains schema/model/dto/entity/interface/type definitions — changes here affect data contracts".to_string())
                } else if fn_data.lesson_count > 0 {
                    (
                        "warning",
                        format!(
                            "Has {} known lesson(s) registered — previous issues found in this file",
                            fn_data.lesson_count
                        ),
                    )
                } else if fn_data.function_count > 15 {
                    (
                        "warning",
                        format!(
                            "Large file ({} functions) — high risk of cascading changes",
                            fn_data.function_count
                        ),
                    )
                } else if d <= 1 {
                    (
                        "warning",
                        "Direct dependency at depth 1 — changes propagate immediately".to_string(),
                    )
                } else {
                    (
                        "info",
                        "Indirect dependency — unlikely to require changes".to_string(),
                    )
                };

                let suggestion = match severity {
                    "breaking" => "Review and update all imports, type definitions, and data contracts in dependent files".to_string(),
                    "warning" => "Check for cascading changes in consuming code; verify tests still pass".to_string(),
                    _ => "Monitor for indirect effects; no immediate action required".to_string(),
                };

                // Get function names with line numbers
                let functions: Vec<String> = func_stmt
                    .as_mut()
                    .and_then(|stmt| {
                        stmt.query_map(params![path, self.tenant_id], |row| {
                            let name: String = row.get(0)?;
                            let line: i64 = row.get(1)?;
                            Ok(format!("{}:{}", name, line))
                        })
                        .ok()
                    })
                    .map(|rows| rows.filter_map(|r| r.ok()).collect())
                    .unwrap_or_default();

                // Get line range for this file
                let (start_line, end_line) = range_stmt
                    .as_mut()
                    .and_then(|stmt| {
                        stmt.query_row(params![path, self.tenant_id], |row| {
                            let s: i64 = row.get(0)?;
                            let e: i64 = row.get(1)?;
                            Ok((s, e))
                        })
                        .ok()
                    })
                    .unwrap_or((0, 0));

                results.push(ImpactEntry {
                    file_path: path.clone(),
                    depth: d,
                    function_count: fn_data.function_count,
                    lesson_count: fn_data.lesson_count,
                    language: fn_data.language.clone(),
                    severity: severity.to_string(),
                    functions,
                    start_line,
                    end_line,
                    reason,
                    suggestion,
                });
            }
        }
        results
    }

    pub fn get_incoming_deps(&self, file_path: &str) -> Vec<String> {
        let inner = self.inner.lock().unwrap();
        let Some(&idx) = inner.file_index.get(file_path) else {
            return vec![];
        };
        inner
            .graph
            .neighbors_directed(idx, petgraph::Direction::Incoming)
            .filter_map(|n| inner.graph.node_weight(n).map(|w| w.path.clone()))
            .collect()
    }

    pub fn get_outgoing_deps(&self, file_path: &str) -> Vec<String> {
        let inner = self.inner.lock().unwrap();
        let Some(&idx) = inner.file_index.get(file_path) else {
            return vec![];
        };
        inner
            .graph
            .neighbors_directed(idx, petgraph::Direction::Outgoing)
            .filter_map(|n| inner.graph.node_weight(n).map(|w| w.path.clone()))
            .collect()
    }

    pub fn record_excel_template(
        &self,
        meta: &ozymem_parser::ExcelTemplateMetadata,
    ) -> anyhow::Result<()> {
        let inner = self.inner.lock().unwrap();
        let sheets_json = serde_json::to_string(&meta.sheets)?;
        inner.sqlite.execute(
            "INSERT INTO excel_templates (file_path, canonical_hash, template_name, version_tag, sheets_json, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, CURRENT_TIMESTAMP)
             ON CONFLICT(file_path) DO UPDATE SET
                canonical_hash = excluded.canonical_hash,
                template_name = excluded.template_name,
                version_tag = excluded.version_tag,
                sheets_json = excluded.sheets_json,
                updated_at = CURRENT_TIMESTAMP",
            params![
                meta.rel_path,
                meta.canonical_hash,
                meta.file_name,
                meta.version_tag,
                sheets_json,
            ],
        )?;
        Ok(())
    }

    pub fn get_excel_templates(&self) -> anyhow::Result<Vec<ozymem_parser::ExcelTemplateMetadata>> {
        let inner = self.inner.lock().unwrap();
        let mut stmt = inner.sqlite.prepare(
            "SELECT file_path, canonical_hash, template_name, version_tag, sheets_json FROM excel_templates ORDER BY file_path ASC"
        )?;
        let rows = stmt.query_map([], |row| {
            let rel_path: String = row.get(0)?;
            let canonical_hash: String = row.get(1)?;
            let file_name: String = row.get(2)?;
            let version_tag: Option<String> = row.get(3)?;
            let sheets_json: String = row.get(4)?;
            let sheets: Vec<String> = serde_json::from_str(&sheets_json).unwrap_or_default();
            Ok(ozymem_parser::ExcelTemplateMetadata {
                file_name,
                rel_path,
                canonical_hash,
                version_tag,
                sheets,
                is_template_candidate: true,
            })
        })?;
        let mut res = Vec::new();
        for r in rows {
            if let Ok(m) = r {
                res.push(m);
            }
        }
        Ok(res)
    }

    pub fn verify_export_contracts(&self) -> anyhow::Result<ExportContractReport> {
        let templates = self.get_excel_templates()?;
        let templates_reviewed = templates.len();

        let inner = self.inner.lock().unwrap();
        let mut stmt = inner.sqlite.prepare("SELECT path FROM files")?;
        let file_paths: Vec<String> = stmt
            .query_map([], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();
        drop(stmt);
        drop(inner);

        let mut endpoints_count = 0;
        let mut version_mismatches = Vec::new();
        let mut missing_templates = Vec::new();

        for fp in &file_paths {
            if let Ok(source) = std::fs::read_to_string(fp) {
                let hints = ozymem_parser::parse_export_contracts(fp, &source);
                for hint in hints {
                    if hint.endpoint_path.is_some() {
                        endpoints_count += 1;
                    }

                    if let Some(ref t_ref) = hint.template_ref {
                        let exists = templates
                            .iter()
                            .any(|t| t.file_name.contains(t_ref) || t_ref.contains(&t.file_name));
                        if !exists {
                            missing_templates.push(ContractMismatchAlert {
                                alert_type: "MISSING_TEMPLATE".to_string(),
                                file_path: fp.clone(),
                                endpoint: hint.endpoint_path.clone(),
                                template_found: None,
                                header_version: None,
                                template_version: None,
                                message: format!("Plantilla referenciada '{}' no encontrada en los templates registrados", t_ref),
                            });
                        }
                    }

                    if let Some(ref cd_fn) = hint.content_disposition_filename {
                        let cd_ver = ozymem_parser::extract_version_tag(cd_fn);
                        if let Some(ref t_ref) = hint.template_ref {
                            let t_ver = ozymem_parser::extract_version_tag(t_ref);
                            if cd_ver.is_some() && t_ver.is_some() && cd_ver != t_ver {
                                version_mismatches.push(ContractMismatchAlert {
                                    alert_type: "VERSION_MISMATCH".to_string(),
                                    file_path: fp.clone(),
                                    endpoint: hint.endpoint_path.clone(),
                                    template_found: Some(t_ref.clone()),
                                    header_version: cd_ver,
                                    template_version: t_ver,
                                    message: format!("Desalineación de versión: Header genera '{cd_fn}' pero código usa plantilla '{t_ref}'"),
                                });
                            }
                        }
                    }
                }
            }
        }

        Ok(ExportContractReport {
            templates_reviewed,
            endpoints_reviewed: endpoints_count,
            version_mismatches,
            missing_templates,
            warnings: Vec::new(),
        })
    }

    pub async fn unified_search(
        &self,
        query: &str,
        scope: Option<&str>,
        _mode: Option<&str>,
        limit: usize,
    ) -> anyhow::Result<Vec<UnifiedSearchResult>> {
        let target_scope = scope.unwrap_or("all");
        let mut results = Vec::new();

        // 1. Literal & Symbol Search
        if target_scope == "all" || target_scope == "code" {
            let inner = self.inner.lock().unwrap();
            let mut stmt = inner.sqlite.prepare(
                "SELECT name, kind, file_path, start_line FROM functions WHERE name LIKE ?1 LIMIT ?2"
            )?;
            let query_like = format!("%{query}%");
            let rows = stmt.query_map(params![query_like, limit as i64], |row| {
                Ok(UnifiedSearchResult {
                    category: "symbol".to_string(),
                    title: format!("{}: {}", row.get::<_, String>(1)?, row.get::<_, String>(0)?),
                    path: row.get(2)?,
                    snippet: format!("Línea {}", row.get::<_, i64>(3)?),
                    score: 1.0,
                })
            })?;
            for r in rows.flatten() {
                results.push(r);
            }
        }

        // 2. Lessons & Memory Search
        if target_scope == "all" || target_scope == "lessons" {
            if let Ok(lessons) = self.search_lessons(query, None, limit).await {
                for l in lessons {
                    results.push(UnifiedSearchResult {
                        category: "lesson".to_string(),
                        title: format!("[{}] {}", l.kind, l.file_path),
                        path: l.file_path,
                        snippet: format!("{}: {}", l.error_context, l.solution),
                        score: 0.9,
                    });
                }
            }
        }

        // 3. Contract Search
        if target_scope == "all" || target_scope == "contracts" {
            if let Ok(templates) = self.get_excel_templates() {
                for t in templates {
                    if t.file_name.contains(query)
                        || t.rel_path.contains(query)
                        || t.sheets.iter().any(|s| s.contains(query))
                    {
                        results.push(UnifiedSearchResult {
                            category: "contract".to_string(),
                            title: format!("Excel Template: {}", t.file_name),
                            path: t.rel_path,
                            snippet: format!(
                                "Versión: {:?} | Hojas: {:?}",
                                t.version_tag, t.sheets
                            ),
                            score: 0.95,
                        });
                    }
                }
            }
        }

        results.truncate(limit);
        Ok(results)
    }

    pub async fn scan_project_background(self: Arc<Self>, project_path: String) {
        self.set_project_path(Some(&project_path));
        self.full_scan(&project_path, None).ok();
        eprintln!("[scan] complete for {project_path}");
    }

    /// Autocomplete a file path fragment against the indexed files table.
    /// Returns up to `limit` matching paths.
    pub fn complete_file_path(&self, fragment: &str, limit: usize) -> Result<Vec<String>> {
        let inner = self.inner.lock().unwrap();
        let pattern = format!("%{}%", fragment);
        let mut stmt = inner.sqlite.prepare(
            "SELECT path FROM files WHERE tenant_id = ?1 AND path LIKE ?2 ORDER BY path ASC LIMIT ?3"
        )?;
        let rows = stmt.query_map(params![self.tenant_id, pattern, limit as i64], |row| {
            row.get::<_, String>(0)
        })?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    /// Return all indexed file paths for the current tenant.
    pub fn list_all_files(&self) -> Result<Vec<String>> {
        let inner = self.inner.lock().unwrap();
        let mut stmt = inner
            .sqlite
            .prepare("SELECT path FROM files WHERE tenant_id = ?1 ORDER BY path")?;
        let rows = stmt.query_map(params![self.tenant_id], |row| row.get::<_, String>(0))?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    /// Find simple paths between two files in the dependency graph.
    /// Returns up to `max_paths` paths, each as a list of file paths from `from` to `to`.
    /// Path length is limited to `max_hops` intermediate nodes.
    pub fn find_graph_path(
        &self,
        from: &str,
        to: &str,
        max_paths: usize,
        max_hops: usize,
    ) -> Vec<Vec<String>> {
        let inner = self.inner.lock().unwrap();
        let start = match inner.file_index.get(from) {
            Some(n) => *n,
            None => return Vec::new(),
        };
        let end = match inner.file_index.get(to) {
            Some(n) => *n,
            None => return Vec::new(),
        };

        let paths = all_simple_paths::<Vec<NodeIndex>, &DiGraph<FileNode, FileEdge>>(
            &inner.graph,
            start,
            end,
            0,
            Some(max_hops),
        );

        let mut results = Vec::new();
        for (i, path) in paths.enumerate() {
            if i >= max_paths {
                break;
            }
            let file_paths: Vec<String> = path
                .iter()
                .filter_map(|n| inner.graph.node_weight(*n))
                .map(|n| n.path.clone())
                .collect();
            results.push(file_paths);
        }
        results
    }

    /// Delete all indexed data (files, functions, dependencies) but NOT lessons.
    pub fn clear_data(&self) -> Result<()> {
        let inner = self.inner.lock().unwrap();
        inner.sqlite.execute(
            "DELETE FROM file_dependencies WHERE tenant_id = ?1",
            params![self.tenant_id],
        )?;
        inner.sqlite.execute(
            "DELETE FROM functions WHERE tenant_id = ?1",
            params![self.tenant_id],
        )?;
        inner.sqlite.execute(
            "DELETE FROM files WHERE tenant_id = ?1",
            params![self.tenant_id],
        )?;
        Ok(())
    }

    /// Count lessons for the current tenant.
    pub fn lesson_count(&self) -> Result<i64> {
        let inner = self.inner.lock().unwrap();
        inner
            .sqlite
            .query_row(
                "SELECT COUNT(*) FROM lessons WHERE tenant_id = ?1",
                params![self.tenant_id],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ImpactEntry {
    pub file_path: String,
    pub depth: u32,
    pub function_count: i64,
    pub lesson_count: i64,
    pub language: String,
    pub severity: String,
    pub functions: Vec<String>,
    pub start_line: i64,
    pub end_line: i64,
    pub reason: String,
    pub suggestion: String,
}

#[async_trait::async_trait]
impl McpBackend for GraphBackend {
    fn tenant_id(&self) -> String {
        self.tenant_id.clone()
    }

    async fn get_graph_summary(&self) -> Result<GraphSummary> {
        let inner = self.inner.lock().unwrap();

        // Read everything from SQLite (single source of truth).
        // During a background scan, numbers grow incrementally instead of
        // showing stale cross-session data from petgraph.
        let file_count: i64 = inner
            .sqlite
            .query_row(
                "SELECT COUNT(*) FROM files WHERE tenant_id = ?1",
                params![self.tenant_id],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let function_count: i64 = inner
            .sqlite
            .query_row(
                "SELECT COUNT(*) FROM functions WHERE tenant_id = ?1",
                params![self.tenant_id],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let engram_count: i64 = inner
            .sqlite
            .query_row(
                "SELECT COUNT(*) FROM lessons WHERE tenant_id = ?1",
                params![self.tenant_id],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let edge_count: i64 = inner
            .sqlite
            .query_row(
                "SELECT COUNT(*) FROM file_dependencies WHERE tenant_id = ?1",
                params![self.tenant_id],
                |r| r.get(0),
            )
            .unwrap_or(0);

        let mut native = 0i64;
        let mut extension = 0i64;
        let mut text = 0i64;
        if let Ok(mut stmt) = inner.sqlite.prepare(
            "SELECT strategy, COUNT(*) FROM functions WHERE tenant_id = ?1 GROUP BY strategy",
        ) {
            if let Ok(rows) = stmt.query_map(params![self.tenant_id], |row| {
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

        let without_embedding: i64 = inner
            .sqlite
            .query_row(
                "SELECT COUNT(*) FROM lessons WHERE tenant_id = ?1 AND embedding IS NULL",
                params![self.tenant_id],
                |r| r.get(0),
            )
            .unwrap_or(0);

        Ok(GraphSummary {
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

    async fn get_file_context(&self, file_path: &str) -> Result<Option<FileGraphContext>> {
        let inner = self.inner.lock().unwrap();
        let mut stmt = inner
            .sqlite
            .prepare("SELECT language FROM files WHERE path = ?1 AND tenant_id = ?2")?;
        let language: String =
            match stmt.query_row(params![file_path, self.tenant_id], |row| row.get(0)) {
                Ok(v) => v,
                Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
                Err(e) => return Err(e.into()),
            };

        let mut func_stmt = inner.sqlite.prepare(
            "SELECT name, kind, start_line, end_line, strategy FROM functions WHERE file_path = ?1 AND tenant_id = ?2 ORDER BY start_line, name"
        )?;
        let funcs: Vec<StoredFunction> = func_stmt
            .query_map(params![file_path, self.tenant_id], |row| {
                Ok(StoredFunction {
                    name: row.get(0)?,
                    kind: row.get(1)?,
                    start_line: row.get(2)?,
                    end_line: row.get(3)?,
                    strategy: row.get(4)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(Some(FileGraphContext {
            file_path: file_path.to_string(),
            language,
            functions: funcs,
        }))
    }

    async fn get_historical_engram_solutions(&self, file_path: &str) -> Result<Vec<String>> {
        let inner = self.inner.lock().unwrap();
        let mut stmt = match inner.sqlite.prepare(
            "SELECT DISTINCT solution FROM lessons WHERE file_path = ?1 AND tenant_id = ?2 AND solution != ''"
        ) {
            Ok(s) => s,
            Err(_) => return Ok(vec![]),
        };
        let results: Vec<String> = stmt
            .query_map(params![file_path, self.tenant_id], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(results)
    }

    async fn record_lesson(
        &self,
        file_path: &str,
        symbol_name: Option<&str>,
        error_context: &str,
        solution: &str,
    ) -> Result<()> {
        self.record_entry(file_path, symbol_name, error_context, solution, "lesson")
            .await
    }

    async fn record_entry(
        &self,
        file_path: &str,
        symbol_name: Option<&str>,
        error_context: &str,
        solution: &str,
        kind: &str,
    ) -> Result<()> {
        let created_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            .to_string();

        // Generate embedding OUTSIDE the inner lock (CPU-bound)
        let text = format!("{} {}", error_context, solution);
        let (embedding_bytes, _model) = self.embed_text(&[&text]);
        let has_embedding = embedding_bytes.is_some();

        {
            let inner = self.inner.lock().unwrap();
            inner.sqlite.execute(
                "INSERT INTO lessons (file_path, symbol_name, error_context, solution, created_at, tenant_id, kind, workspace_root, embedding, embedding_model) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![file_path, symbol_name.unwrap_or(""), error_context, solution, created_at, self.tenant_id, kind, inner.workspace_root, embedding_bytes, if has_embedding { "all-MiniLM-L6-v2" } else { "" }],
            )?;
        }

        let node_idx = {
            let inner = self.inner.lock().unwrap();
            inner.file_index.get(file_path).copied()
        };

        if let Some(idx) = node_idx {
            let mut inner = self.inner.lock().unwrap();
            if let Some(node) = inner.graph.node_weight_mut(idx) {
                node.lesson_count += 1;
            }
        }

        Ok(())
    }

    async fn search_lessons(
        &self,
        query: &str,
        kind: Option<&str>,
        limit: usize,
    ) -> Result<Vec<LessonEntry>> {
        let inner = self.inner.lock().unwrap();
        let limit = (limit as i64).min(100).max(1);

        // Try FTS5 MATCH first; fall back to LIKE if query syntax is invalid
        let mut results = Vec::new();

        // FTS5 with content-sync: query the lessons table joined with fts
        let tenant_id = &self.tenant_id;
        let root = inner.workspace_root.clone();
        let (tenant_param, workspace_param, kind_clause) = match kind {
            Some(_) => ("?4", "?5", "AND l.kind = ?3".to_string()),
            None => ("?3", "?4", String::new()),
        };
        let sql = format!(
            "SELECT l.id, l.file_path, l.symbol_name, l.error_context, l.solution, l.kind, l.created_at, COALESCE(l.stale,0), l.stale_reason
             FROM lessons l
             JOIN lessons_fts fts ON l.id = fts.rowid
             WHERE lessons_fts MATCH ?1 {}
               AND l.stale = 0
               AND l.tenant_id = {}
               AND l.workspace_root = {}
             ORDER BY bm25(lessons_fts, 2.0, 1.0, 3.0, 1.0)
             LIMIT ?2", kind_clause, tenant_param, workspace_param
        );

        if kind.is_none() {
            if let Ok(mut stmt) = inner.sqlite.prepare(&sql) {
                if let Ok(rows) = stmt.query_map(params![query, limit, tenant_id, root], |row| {
                    Ok(LessonEntry {
                        id: row.get(0)?,
                        file_path: row.get(1)?,
                        symbol_name: row.get(2)?,
                        error_context: row.get(3)?,
                        solution: row.get(4)?,
                        kind: row.get(5)?,
                        created_at: row.get(6)?,
                        stale: row.get(7)?,
                        stale_reason: row.get(8)?,
                    })
                }) {
                    for row in rows.flatten() {
                        results.push(row);
                    }
                }
            }
        } else if let Some(k) = kind {
            if let Ok(mut stmt) = inner.sqlite.prepare(&sql) {
                if let Ok(rows) = stmt.query_map(params![query, limit, k, tenant_id, root], |row| {
                    Ok(LessonEntry {
                        id: row.get(0)?,
                        file_path: row.get(1)?,
                        symbol_name: row.get(2)?,
                        error_context: row.get(3)?,
                        solution: row.get(4)?,
                        kind: row.get(5)?,
                        created_at: row.get(6)?,
                        stale: row.get(7)?,
                        stale_reason: row.get(8)?,
                    })
                }) {
                    for row in rows.flatten() {
                        results.push(row);
                    }
                }
            }
        }

        // If FTS returned nothing, fall back to LIKE on all text columns
        if results.is_empty() && !query.is_empty() {
            let like = format!("%{}%", query);
            let root = inner.workspace_root.clone();
            let (kind_clause, workspace_param) = match kind {
                Some(_) => ("AND kind = ?4".to_string(), "?5"),
                None => (String::new(), "?4"),
            };
            let sql = format!(
             "SELECT id, file_path, symbol_name, error_context, solution, kind, created_at, COALESCE(stale,0), stale_reason
              FROM lessons
              WHERE tenant_id = ?1
                AND stale = 0
                AND (file_path LIKE ?2 OR symbol_name LIKE ?2 OR error_context LIKE ?2 OR solution LIKE ?2)
                {}
                AND workspace_root = {}
              ORDER BY id DESC
              LIMIT ?3", kind_clause, workspace_param
            );
            if kind.is_none() {
                if let Ok(mut stmt) = inner.sqlite.prepare(&sql) {
                    if let Ok(rows) =
                        stmt.query_map(params![self.tenant_id, like, limit, root], |row| {
                            Ok(LessonEntry {
                                id: row.get(0)?,
                                file_path: row.get(1)?,
                                symbol_name: row.get(2)?,
                                error_context: row.get(3)?,
                                solution: row.get(4)?,
                                kind: row.get(5)?,
                                created_at: row.get(6)?,
                                stale: row.get(7)?,
                                stale_reason: row.get(8)?,
                            })
                        })
                    {
                        for row in rows.flatten() {
                            results.push(row);
                        }
                    }
                }
            } else if let Some(k) = kind {
                if let Ok(mut stmt) = inner.sqlite.prepare(&sql) {
                    if let Ok(rows) =
                        stmt.query_map(params![self.tenant_id, like, limit, k, root], |row| {
                            Ok(LessonEntry {
                                id: row.get(0)?,
                                file_path: row.get(1)?,
                                symbol_name: row.get(2)?,
                                error_context: row.get(3)?,
                                solution: row.get(4)?,
                                kind: row.get(5)?,
                                created_at: row.get(6)?,
                                stale: row.get(7)?,
                                stale_reason: row.get(8)?,
                            })
                        })
                    {
                        for row in rows.flatten() {
                            results.push(row);
                        }
                    }
                }
            }
        }

        Ok(results)
    }

    async fn get_file_lessons(&self, file_path: &str) -> Result<Vec<LessonEntry>> {
        let inner = self.inner.lock().unwrap();
        let mut stmt = inner.sqlite.prepare(
            "SELECT id, file_path, symbol_name, error_context, solution, kind, created_at, COALESCE(stale,0), stale_reason
             FROM lessons
             WHERE file_path = ?1 AND tenant_id = ?2 AND workspace_root = ?3
             ORDER BY id DESC"
        )?;
        let results = stmt
            .query_map(
                params![file_path, self.tenant_id, inner.workspace_root],
                |row| {
                    Ok(LessonEntry {
                        id: row.get(0)?,
                        file_path: row.get(1)?,
                        symbol_name: row.get(2)?,
                        error_context: row.get(3)?,
                        solution: row.get(4)?,
                        kind: row.get(5)?,
                        created_at: row.get(6)?,
                        stale: row.get(7)?,
                        stale_reason: row.get(8)?,
                    })
                },
            )?
            .filter_map(|r| r.ok())
            .collect();
        Ok(results)
    }

    async fn get_symbol_lessons(
        &self,
        file_path: &str,
        symbol_name: &str,
    ) -> Result<Vec<LessonEntry>> {
        let inner = self.inner.lock().unwrap();
        let mut stmt = inner.sqlite.prepare(
            "SELECT id, file_path, symbol_name, error_context, solution, kind, created_at, COALESCE(stale,0), stale_reason
             FROM lessons
             WHERE file_path = ?1 AND symbol_name = ?2 AND tenant_id = ?3 AND workspace_root = ?4
             ORDER BY id DESC"
        )?;
        let results = stmt
            .query_map(
                params![file_path, symbol_name, self.tenant_id, inner.workspace_root],
                |row| {
                    Ok(LessonEntry {
                        id: row.get(0)?,
                        file_path: row.get(1)?,
                        symbol_name: row.get(2)?,
                        error_context: row.get(3)?,
                        solution: row.get(4)?,
                        kind: row.get(5)?,
                        created_at: row.get(6)?,
                        stale: row.get(7)?,
                        stale_reason: row.get(8)?,
                    })
                },
            )?
            .filter_map(|r| r.ok())
            .collect();
        Ok(results)
    }

    async fn recent_lessons(&self, kind: Option<&str>, limit: usize) -> Result<Vec<LessonEntry>> {
        let inner = self.inner.lock().unwrap();
        let limit = (limit as i64).min(100).max(1);

        let (sql, kind_clause) = match kind {
            Some(_) => (
                "SELECT id, file_path, symbol_name, error_context, solution, kind, created_at, COALESCE(stale,0), stale_reason
                 FROM lessons
                 WHERE tenant_id = ?1 AND kind = ?2 AND workspace_root = ?3
                 ORDER BY id DESC
                 LIMIT ?4".to_string(),
                true,
            ),
            None => (
                "SELECT id, file_path, symbol_name, error_context, solution, kind, created_at, COALESCE(stale,0), stale_reason
                 FROM lessons
                 WHERE tenant_id = ?1 AND workspace_root = ?2
                 ORDER BY id DESC
                 LIMIT ?3".to_string(),
                false,
            ),
        };

        if kind_clause {
            let mut stmt = inner.sqlite.prepare(&sql)?;
            let k = kind.unwrap();
            let results = stmt
                .query_map(
                    params![self.tenant_id, k, inner.workspace_root, limit],
                    |row| {
                        Ok(LessonEntry {
                            id: row.get(0)?,
                            file_path: row.get(1)?,
                            symbol_name: row.get(2)?,
                            error_context: row.get(3)?,
                            solution: row.get(4)?,
                            kind: row.get(5)?,
                            created_at: row.get(6)?,
                            stale: row.get(7)?,
                            stale_reason: row.get(8)?,
                        })
                    },
                )?
                .filter_map(|r| r.ok())
                .collect();
            Ok(results)
        } else {
            let mut stmt = inner.sqlite.prepare(&sql)?;
            let results = stmt
                .query_map(
                    params![self.tenant_id, inner.workspace_root, limit],
                    |row| {
                        Ok(LessonEntry {
                            id: row.get(0)?,
                            file_path: row.get(1)?,
                            symbol_name: row.get(2)?,
                            error_context: row.get(3)?,
                            solution: row.get(4)?,
                            kind: row.get(5)?,
                            created_at: row.get(6)?,
                            stale: row.get(7)?,
                            stale_reason: row.get(8)?,
                        })
                    },
                )?
                .filter_map(|r| r.ok())
                .collect();
            Ok(results)
        }
    }

    async fn get_graph_neighbors(&self, file_path: &str) -> Result<NeighborInfo> {
        let incoming = self.get_incoming_deps(file_path);
        let outgoing = self.get_outgoing_deps(file_path);
        Ok(NeighborInfo {
            file_path: file_path.to_string(),
            incoming,
            outgoing,
        })
    }

    async fn get_incoming_dependencies(&self, file_path: &str) -> Result<Vec<String>> {
        Ok(self.get_incoming_deps(file_path))
    }

    async fn find_symbol(&self, symbol_name: &str, project_path: &str) -> Result<Vec<String>> {
        let inner = self.inner.lock().unwrap();
        let like = format!("{}%", project_path);
        // Use LIKE for partial matching (supports % wildcards in symbol_name)
        let mut stmt = inner.sqlite.prepare(
            "SELECT f.path, fn.start_line, fn.kind FROM functions fn
             JOIN files f ON f.path = fn.file_path AND f.tenant_id = fn.tenant_id
             WHERE fn.name LIKE ?1 AND fn.tenant_id = ?2 AND f.path LIKE ?3
             ORDER BY fn.name LIMIT 100",
        )?;
        let results: Vec<String> = stmt
            .query_map(params![symbol_name, self.tenant_id, like], |row| {
                let path: String = row.get(0)?;
                let line: i64 = row.get(1)?;
                let kind: String = row.get(2)?;
                Ok(format!("{} [{}] (line {})", path, kind, line))
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(results)
    }
}

// ---------------------------------------------------------------------------
// Embedding / Semantic Search
// ---------------------------------------------------------------------------

pub struct SimilarLesson {
    pub lesson: LessonEntry,
    pub score: f32,
}

impl GraphBackend {
    /// Get or lazily initialize the text embedder.
    fn get_embedder(&self) -> Option<&Mutex<TextEmbedding>> {
        self.embedder.get_or_init(|| {
            eprintln!("[ozymem] initializing text embedder (all-MiniLM-L6-v2)...");
            match TextEmbedding::try_new(InitOptions::new(EmbeddingModel::AllMiniLML6V2).with_show_download_progress(true)) {
                Ok(m) => {
                    eprintln!("[ozymem] embedder ready");
                    Some(Mutex::new(m))
                }
                Err(e) => {
                    eprintln!("[ozymem] embedder init failed: {e}");
                    eprintln!("[ozymem] embeddings DISABLED — first use requires downloading ~20MB model from HuggingFace.");
                    eprintln!("[ozymem] Ensure internet connectivity on first run. Model is cached locally afterwards.");
                    None
                }
            }
        }).as_ref()
    }

    /// Pre-initialize the text embedder at startup.
    /// Call this in a spawn_blocking after server initialize to avoid blocking the tokio runtime.
    /// Safe to call multiple times (OnceLock ensures single initialization).
    pub fn init_embedder(&self) {
        self.get_embedder();
    }

    /// Check if the embedder is initialized and ready.
    pub fn embedder_ready(&self) -> bool {
        self.embedder.get().map(|v| v.is_some()).unwrap_or(false)
    }

    /// Generate embedding bytes for text (outside lock).
    /// Returns (raw f32 LE bytes, model_name) or (None, "") if embedder unavailable.
    fn embed_text(&self, texts: &[&str]) -> (Option<Vec<u8>>, &'static str) {
        let embedder = match self.get_embedder() {
            Some(m) => m,
            None => return (None, ""),
        };
        let guard = match embedder.lock() {
            Ok(g) => g,
            Err(_) => return (None, ""),
        };
        match guard.embed(texts.to_vec(), Some(1)) {
            Ok(mut embeddings) => {
                if let Some(vec) = embeddings.pop() {
                    let bytes: Vec<u8> = vec.iter().flat_map(|f| f.to_le_bytes()).collect();
                    (Some(bytes), "all-MiniLM-L6-v2")
                } else {
                    (None, "")
                }
            }
            Err(e) => {
                eprintln!("[ozymem] embedding error: {e}");
                (None, "")
            }
        }
    }

    /// Search lessons by semantic similarity.
    /// Returns up to `limit` lessons with cosine similarity >= `min_score`.
    /// Filters by stale=0, tenant_id, workspace_root, and embedding IS NOT NULL.
    pub fn similar_lessons(
        &self,
        query: &str,
        limit: usize,
        min_score: f32,
    ) -> Result<Vec<SimilarLesson>> {
        // Fast path: no lessons with embeddings → skip expensive embedding computation
        {
            let inner = self.inner.lock().unwrap();
            let count: i64 = inner.sqlite.query_row(
                "SELECT COUNT(*) FROM lessons WHERE stale = 0 AND tenant_id = ?1 AND workspace_root = ?2 AND embedding IS NOT NULL",
                params![self.tenant_id, inner.workspace_root],
                |row| row.get(0),
            )?;
            if count == 0 {
                return Ok(Vec::new());
            }
        }

        // Skip if embedder not initialized (avoids blocking on model download)
        if !self.embedder_ready() {
            return Ok(Vec::new());
        }

        // Generate query embedding (outside inner lock)
        let (embedding_bytes, _) = self.embed_text(&[query]);
        let query_vec = match embedding_bytes {
            Some(ref b) => {
                let chunks: Vec<[u8; 4]> = b
                    .chunks_exact(4)
                    .map(|c| [c[0], c[1], c[2], c[3]])
                    .collect();
                chunks
                    .iter()
                    .map(|c| f32::from_le_bytes(*c))
                    .collect::<Vec<f32>>()
            }
            None => return Ok(Vec::new()),
        };

        let dim = query_vec.len();
        if dim == 0 {
            return Ok(Vec::new());
        }

        let inner = self.inner.lock().unwrap();
        let mut stmt = inner.sqlite.prepare(
            "SELECT id, file_path, symbol_name, error_context, solution, kind, created_at, COALESCE(stale,0), stale_reason, embedding
             FROM lessons
             WHERE stale = 0 AND tenant_id = ?1 AND workspace_root = ?2 AND embedding IS NOT NULL
             ORDER BY id DESC"
        )?;

        let rows = stmt.query_map(params![self.tenant_id, inner.workspace_root], |row| {
            let id: i64 = row.get(0)?;
            let file_path: String = row.get(1)?;
            let symbol_name: String = row.get(2)?;
            let error_context: String = row.get(3)?;
            let solution: String = row.get(4)?;
            let kind: String = row.get(5)?;
            let created_at: String = row.get(6)?;
            let stale: i64 = row.get(7)?;
            let stale_reason: Option<String> = row.get(8)?;
            let embedding_blob: Option<Vec<u8>> = row.get(9)?;
            Ok((
                LessonEntry {
                    id,
                    file_path,
                    symbol_name,
                    error_context,
                    solution,
                    kind,
                    created_at,
                    stale,
                    stale_reason,
                },
                embedding_blob,
            ))
        })?;

        let mut scored: Vec<SimilarLesson> = rows
            .filter_map(|r| r.ok())
            .filter_map(|(lesson, blob)| {
                let blob = blob?;
                let chunks: Vec<[u8; 4]> = blob
                    .chunks_exact(4)
                    .map(|c| [c[0], c[1], c[2], c[3]])
                    .collect();
                let vec: Vec<f32> = chunks.iter().map(|c| f32::from_le_bytes(*c)).collect();
                if vec.len() != dim {
                    return None; // dimension mismatch — skip silently
                }
                let score = crate::cosine_similarity(&query_vec, &vec);
                if score < min_score {
                    return None;
                }
                Some(SimilarLesson { lesson, score })
            })
            .collect();

        scored.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        scored.truncate(limit.min(100));
        Ok(scored)
    }
}

/// A lightweight SQLite backend for writing scan results directly to the
/// shared SQLite database that `GraphBackend` (MCP server) reads from.
///
/// All methods are synchronous (rusqlite). The caller is responsible for
/// running them on a blocking-aware executor if needed.
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

pub fn strip_unc_prefix(path: PathBuf) -> PathBuf {
    let s = path.to_string_lossy();
    if let Some(stripped) = s.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{}", stripped))
    } else if let Some(stripped) = s.strip_prefix(r"\\?\") {
        PathBuf::from(stripped)
    } else {
        path
    }
}

/// Resolve the project-scoped database path.
///
/// Canonicalizes `project_path`, creates `.ozymem/` inside it,
/// and returns the path to `memory.db`.
pub fn resolve_project_db_path(project_path: &Path) -> Result<PathBuf> {
    if !project_path.exists() {
        let _ = std::fs::create_dir_all(project_path);
    }
    let canonical = project_path
        .canonicalize()
        .map(strip_unc_prefix)
        .unwrap_or_else(|_| strip_unc_prefix(project_path.to_path_buf()));
    let db_dir = canonical.join(OZYMEM_DIR);
    std::fs::create_dir_all(&db_dir)
        .with_context(|| format!("failed to create `{}`", db_dir.display()))?;
    let db_path = db_dir.join(MEMORY_DB);
    eprintln!("[ozymem] DB path: {}", db_path.display());
    Ok(db_path)
}

/// Ensure `.ozymem/` is listed in the project's `.gitignore`.
///
/// Returns `true` if an entry was added, `false` if already present
/// or if the project root has no `.git` repo.
pub fn auto_manage_gitignore(project_path: &Path) -> Result<bool> {
    let gitignore_path = project_path.join(".gitignore");
    let entry = format!("/{}", OZYMEM_DIR);

    // Don't create .gitignore if no .git repo exists
    if !project_path.join(".git").exists() {
        return Ok(false);
    }

    if !gitignore_path.exists() {
        std::fs::write(&gitignore_path, format!("{entry}\n"))?;
        return Ok(true);
    }

    let content = std::fs::read_to_string(&gitignore_path)?;
    if content.lines().any(|l| {
        let t = l.trim();
        t == entry || t == OZYMEM_DIR || t == format!("{}/", OZYMEM_DIR)
    }) {
        return Ok(false);
    }

    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&gitignore_path)?;
    use std::io::Write;
    writeln!(file, "\n# Ozymem project memory\n{entry}")?;
    eprintln!("[ozymem] added `{entry}` to .gitignore");
    Ok(true)
}

/// Path to the legacy global DB at `~/.ozymem/memory.db`.
pub fn legacy_global_db_path() -> PathBuf {
    let home = home::home_dir().expect("cannot find home dir");
    home.join(OZYMEM_DIR).join(MEMORY_DB)
}

/// Mark lessons as stale when their file or symbol no longer exists.
///
/// Only lessons whose `file_path` is in `scanned_files` are evaluated.
/// - File deleted: `stale_reason = "file_deleted"`
/// - Symbol removed (file exists but `symbol_name` not in `functions`): `stale_reason = "symbol_removed"`
pub fn mark_stale_lessons(
    conn: &Connection,
    tenant_id: &str,
    scanned_files: &HashSet<String>,
    workspace_root: &str,
) -> Result<u64> {
    let now = chrono::Utc::now()
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string();
    let mut marked: u64 = 0;

    if scanned_files.is_empty() {
        return Ok(0);
    }

    // Process each scanned file individually to avoid dynamic IN-clause
    // parameter juggling that rusqlite handles poorly in bulk.
    let mut check_path = conn.prepare(
        "SELECT id, file_path FROM lessons
         WHERE tenant_id = ?1 AND stale = 0 AND file_path = ?2 AND workspace_root = ?3",
    )?;
    let mut check_symbol = conn.prepare(
        "SELECT id, file_path, symbol_name FROM lessons
         WHERE tenant_id = ?1 AND stale = 0 AND symbol_name != '' AND file_path = ?2 AND workspace_root = ?3"
    )?;
    let mut count_func = conn.prepare(
        "SELECT COUNT(*) FROM functions WHERE file_path = ?1 AND name = ?2 AND tenant_id = ?3",
    )?;
    let mut update = conn.prepare(
        "UPDATE lessons SET stale = 1, stale_reason = ?1, stale_since = ?2 WHERE id = ?3",
    )?;

    for file_path in scanned_files {
        // Step 1: file_deleted
        let rows: Vec<(i64, String)> = check_path
            .query_map(params![tenant_id, file_path, workspace_root], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })?
            .filter_map(|r| r.ok())
            .collect();

        for (id, stored_path) in &rows {
            if !Path::new(&stored_path).exists() {
                update.execute(params!["file_deleted", now, id])?;
                marked += 1;
            }
        }

        // Step 2: symbol_removed (only for lessons whose file still exists)
        let rows: Vec<(i64, String, String)> = check_symbol
            .query_map(params![tenant_id, file_path, workspace_root], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })?
            .filter_map(|r| r.ok())
            .collect();

        for (id, stored_path, symbol_name) in &rows {
            if !Path::new(stored_path).exists() {
                continue;
            }
            let exists: bool = count_func
                .query_row(params![stored_path, symbol_name, tenant_id], |row| {
                    row.get::<_, i64>(0)
                })
                .map(|c| c > 0)
                .unwrap_or(false);

            if !exists {
                update.execute(params!["symbol_removed", now, id])?;
                marked += 1;
            }
        }
    }

    Ok(marked)
}

impl GraphBackend {
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

impl SqliteBackend {
    /// Open a project-scoped database.
    pub fn open_for_project(project_path: &Path) -> Result<Self> {
        let db_path = resolve_project_db_path(project_path)?;
        Self::open(Some(&db_path.to_string_lossy()))
    }
}

pub fn is_noise_dir(path: &Path) -> bool {
    if !path.is_dir() {
        return false;
    }
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        matches!(
            name,
            "target"
                | "node_modules"
                | ".git"
                | ".svn"
                | "__pycache__"
                | ".ozymem"
                | "build"
                | "dist"
                | ".next"
                | ".cache"
                | "vendor"
                | "bower_components"
                | ".idea"
                | ".vscode"
        )
    } else {
        false
    }
}

/// Load ignore patterns from `.ozymemignore` and `.gitignore` in the project root.
pub fn load_ignore_patterns(project_root: &Path) -> Vec<String> {
    let mut patterns = Vec::new();
    for filename in &[".ozymemignore", ".gitignore"] {
        let path = project_root.join(filename);
        if let Ok(content) = std::fs::read_to_string(&path) {
            for line in content.lines() {
                let t = line.trim();
                if !t.is_empty() && !t.starts_with('#') {
                    patterns.push(t.to_string());
                }
            }
        }
    }
    patterns
}

/// Check if a path matches any ignore pattern.
pub fn path_matches_ignore(path: &Path, patterns: &[String], project_root: &Path) -> bool {
    if patterns.is_empty() {
        return false;
    }
    let relative = path.strip_prefix(project_root).unwrap_or(path);
    let rel = relative.to_string_lossy().replace('\\', "/");
    let rel_lower = rel.to_lowercase();

    for p in patterns {
        let pat = p.trim().replace('\\', "/");
        if pat.is_empty() {
            continue;
        }
        let pat_lower = pat.to_lowercase();

        // Exact match or directory prefix
        if rel_lower == pat_lower || rel_lower.starts_with(&format!("{}/", pat_lower)) {
            return true;
        }
        // Match any path component (e.g. "coverage" matches "src/coverage/index.html")
        for component in relative.components() {
            if let Some(comp) = component.as_os_str().to_str() {
                if comp.to_lowercase() == pat_lower {
                    return true;
                }
            }
        }
    }
    false
}

fn detect_language(path: &Path) -> SupportedLanguage {
    match path.extension().and_then(|e| e.to_str()) {
        Some("py") => SupportedLanguage::Python,
        Some("go") => SupportedLanguage::Go,
        Some("rs") => SupportedLanguage::Rust,
        Some("js") | Some("jsx") => SupportedLanguage::JavaScript,
        Some("ts") | Some("tsx") => SupportedLanguage::TypeScriptReact,
        Some("sql") => SupportedLanguage::SQL,
        _ => SupportedLanguage::Unknown,
    }
}

fn file_mtime(path: &Path) -> Option<String> {
    let metadata = std::fs::metadata(path).ok()?;
    let modified = metadata.modified().ok()?;
    let duration = modified.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some(duration.as_secs().to_string())
}

fn now_ts() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn normalize_scope(scope: Option<&str>) -> &str {
    match scope {
        Some("personal") => "personal",
        _ => "project",
    }
}

fn normalize_topic_key(topic: &str) -> String {
    topic
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '/' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

fn normalized_hash(content: &str) -> String {
    let normalized = content
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    let mut hasher = Sha256::new();
    hasher.update(normalized.as_bytes());
    hex::encode(hasher.finalize())
}

fn default_project_name(inner: &Inner) -> &str {
    inner
        .project_path
        .as_deref()
        .and_then(|p| Path::new(p).file_name().and_then(|n| n.to_str()))
        .unwrap_or("default")
}
