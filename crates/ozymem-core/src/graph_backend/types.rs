use fastembed::TextEmbedding;
use petgraph::graph::{DiGraph, NodeIndex};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::sync::{
    Arc, Mutex,
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

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphEntityType {
    File,
    Memory,
    Symbol,
    TrajectoryNode,
}

impl fmt::Display for GraphEntityType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::File => write!(f, "file"),
            Self::Memory => write!(f, "memory"),
            Self::Symbol => write!(f, "symbol"),
            Self::TrajectoryNode => write!(f, "trajectory_node"),
        }
    }
}

impl std::str::FromStr for GraphEntityType {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "file" => Ok(Self::File),
            "memory" => Ok(Self::Memory),
            "symbol" => Ok(Self::Symbol),
            "trajectory_node" => Ok(Self::TrajectoryNode),
            other => Err(format!("Unknown GraphEntityType: {}", other)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryEdgeType {
    AppliesTo,
    CoupledWith,
    CausesRegression,
    Supersedes,
    Reinforces,
    DerivedFrom,
}

impl fmt::Display for MemoryEdgeType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AppliesTo => write!(f, "applies_to"),
            Self::CoupledWith => write!(f, "coupled_with"),
            Self::CausesRegression => write!(f, "causes_regression"),
            Self::Supersedes => write!(f, "supersedes"),
            Self::Reinforces => write!(f, "reinforces"),
            Self::DerivedFrom => write!(f, "derived_from"),
        }
    }
}

impl std::str::FromStr for MemoryEdgeType {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "applies_to" => Ok(Self::AppliesTo),
            "coupled_with" => Ok(Self::CoupledWith),
            "causes_regression" => Ok(Self::CausesRegression),
            "supersedes" => Ok(Self::Supersedes),
            "reinforces" => Ok(Self::Reinforces),
            "derived_from" => Ok(Self::DerivedFrom),
            other => Err(format!("Unknown MemoryEdgeType: {}", other)),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryNodeRecord {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub content: String,
    #[serde(default)]
    pub error_context: String,
    #[serde(default)]
    pub solution: String,
    #[serde(default = "default_confidence")]
    pub confidence_score: f64,
    #[serde(default)]
    pub touch_count: i64,
    #[serde(default)]
    pub stale: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stale_reason: Option<String>,
    pub created_at: String,
    #[serde(default)]
    pub last_verified_at: String,
    #[serde(default = "default_tenant")]
    pub tenant_id: String,
    #[serde(default)]
    pub workspace_root: String,
}

fn default_tenant() -> String {
    "local".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEdgeRecord {
    pub source_type: GraphEntityType,
    pub source_id: String,
    pub target_type: GraphEntityType,
    pub target_id: String,
    pub edge_type: MemoryEdgeType,
    #[serde(default = "default_confidence")]
    pub weight: f64,
    pub created_at: String,
    #[serde(default = "default_tenant")]
    pub tenant_id: String,
    #[serde(default)]
    pub workspace_root: String,
}

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub freshness_warning: Option<String>,
}

fn default_confidence() -> f64 {
    1.0
}

/// Parsea un timestamp de forma flexible: segundos epoch UNIX o RFC3339/ISO-8601
pub fn parse_timestamp_seconds(ts_str: &str) -> Option<u64> {
    let trimmed = ts_str.trim();
    if let Ok(secs) = trimmed.parse::<u64>() {
        return Some(secs);
    }
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(trimmed) {
        return Some(dt.timestamp().max(0) as u64);
    }
    if let Ok(ndt) = chrono::NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%d %H:%M:%S") {
        return Some(ndt.and_utc().timestamp().max(0) as u64);
    }
    if let Ok(ndt) = chrono::NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%dT%H:%M:%S") {
        return Some(ndt.and_utc().timestamp().max(0) as u64);
    }
    None
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
            freshness_warning: None,
        })
    }

    /// Verifica la frescura de la lección contra el archivo activo en disco.
    /// Si el archivo fue modificado después de haberse guardado la lección (mtime > created_at),
    /// o si el archivo fue eliminado, añade una advertencia estructurada [ALERT: STALE_MEMORY].
    pub fn check_freshness(&mut self, workspace_root: Option<&std::path::Path>) {
        if self.stale != 0 {
            let reason = self.stale_reason.as_deref().unwrap_or("unknown");
            self.freshness_warning = Some(format!("[ALERT: STALE_MEMORY: {reason}]"));
            return;
        }

        let fp = self.file_path.trim();
        if fp.is_empty() || fp == "global" || fp == "unknown" {
            return;
        }

        let path = std::path::Path::new(fp);
        let effective_path = if path.is_absolute() {
            path.to_path_buf()
        } else if let Some(root) = workspace_root {
            root.join(path)
        } else {
            path.to_path_buf()
        };

        if !effective_path.exists() {
            self.freshness_warning = Some("[ALERT: STALE_MEMORY: target file not found on disk]".to_string());
            return;
        }

        if let Ok(metadata) = std::fs::metadata(&effective_path) {
            if let Ok(mtime) = metadata.modified() {
                let mtime_secs = mtime
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);

                if let Some(created_secs) = parse_timestamp_seconds(&self.created_at) {
                    if mtime_secs > created_secs + 1 {
                        self.freshness_warning = Some("[ALERT: STALE_MEMORY: file modified after lesson created]".to_string());
                    }
                }
            }
        }
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
        let warning_tag = if let Some(ref warn) = self.freshness_warning {
            format!(" {}", warn)
        } else if self.stale != 0 {
            format!(
                " [ALERT: STALE_MEMORY: {}]",
                self.stale_reason.as_deref().unwrap_or("unknown")
            )
        } else {
            String::new()
        };
        write!(
            f,
            "[{}]{} {} :: {}\n    context: {}\n    solution: {}",
            self.kind,
            warning_tag,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEntityNode {
    pub entity_type: GraphEntityType,
    pub id: String,
    pub title: String,
    pub kind: String,
    #[serde(default = "default_confidence")]
    pub confidence_score: f64,
    #[serde(default)]
    pub stale: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryGraphEdge {
    pub edge_type: MemoryEdgeType,
    pub weight: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NeighborhoodResult {
    pub entity: GraphEntityNode,
    pub edge_type: MemoryEdgeType,
    pub depth: usize,
    pub effective_weight: f64,
    pub direction: String,
}

pub(crate) struct Inner {
    pub(crate) graph: DiGraph<FileNode, FileEdge>,
    pub(crate) file_index: HashMap<String, NodeIndex>,
    pub(crate) memory_graph: DiGraph<GraphEntityNode, MemoryGraphEdge>,
    pub(crate) memory_index: HashMap<String, NodeIndex>,
    pub(crate) sqlite: Connection,
    pub(crate) project_path: Option<String>,
    pub(crate) workspace_root: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UnifiedSearchResult {
    pub category: String,
    pub title: String,
    pub path: String,
    pub snippet: String,
    pub score: f32,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub enum EmbeddingModelStatus {
    Ready,
    Downloading,
    NotDownloaded,
    CorruptedOrDeleted,
    Failed(String),
}

pub struct GraphBackend {
    pub(crate) inner: Mutex<Inner>,
    pub(crate) tenant_id: String,
    pub scan_progress: Arc<Mutex<ScanProgress>>,
    pub scanning: AtomicBool,
    pub(crate) last_check: Mutex<Instant>,
    pub(crate) embedder: Arc<Mutex<Option<Mutex<TextEmbedding>>>>,
    pub(crate) embedding_status: Arc<Mutex<EmbeddingModelStatus>>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboxEvent {
    pub id: i64,
    pub entity_type: String,
    pub entity_id: String,
    pub operation: String,
    pub payload: String,
    pub created_at: String,
    pub processed_at: Option<String>,
}
