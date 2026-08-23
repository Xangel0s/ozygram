use fastembed::TextEmbedding;
use petgraph::graph::{DiGraph, NodeIndex};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::AtomicBool,
};
use std::time::Instant;

pub const OZYMEM_DIR: &str = ".ozymem";
pub const MEMORY_DB: &str = "memory.db";

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
    #[serde(default = "default_confidence")]
    pub confidence_score: f64,
    #[serde(default)]
    pub touch_count: i64,
    #[serde(default)]
    pub last_verified_at: String,
}

fn default_confidence() -> f64 {
    1.0
}

impl LessonEntry {
    pub fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            file_path: row.get(1)?,
            symbol_name: row.get(2)?,
            error_context: row.get(3)?,
            solution: row.get(4)?,
            kind: row.get(5)?,
            created_at: row.get(6)?,
            stale: row.get(7)?,
            stale_reason: row.get(8)?,
            confidence_score: row.get::<_, Option<f64>>(9).unwrap_or(None).unwrap_or(1.0),
            touch_count: row.get::<_, Option<i64>>(10).unwrap_or(None).unwrap_or(0),
            last_verified_at: row.get::<_, Option<String>>(11).unwrap_or(None).unwrap_or_default(),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrunedLessonInfo {
    pub id: i64,
    pub file_path: String,
    pub symbol_name: String,
    pub solution: String,
    pub confidence_score: f64,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PruneReport {
    pub active_count: usize,
    pub stale_count: usize,
    pub pruned_count: usize,
    pub pruned_lessons: Vec<PrunedLessonInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriftAlert {
    pub file_path: String,
    pub convention_id: i64,
    pub rule_snippet: String,
    pub diff_snippet: String,
    pub severity: String,
    pub description: String,
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

pub(crate) struct Inner {
    pub(crate) graph: DiGraph<FileNode, FileEdge>,
    pub(crate) file_index: HashMap<String, NodeIndex>,
    pub(crate) sqlite: Connection,
    pub(crate) project_path: Option<String>,
    pub(crate) workspace_root: String,
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
    pub(crate) inner: Mutex<Inner>,
    pub(crate) tenant_id: String,
    pub scan_progress: Arc<Mutex<ScanProgress>>,
    pub scanning: AtomicBool,
    pub(crate) last_check: Mutex<Instant>,
    pub(crate) embedder: OnceLock<Option<Mutex<TextEmbedding>>>,
    pub engram_store: crate::engram_store::IncrementalEngramStore,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimilarLesson {
    pub lesson: LessonEntry,
    pub score: f32,
}

