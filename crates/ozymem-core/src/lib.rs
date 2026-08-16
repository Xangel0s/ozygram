use serde::{Deserialize, Serialize};

pub mod git_backend;
pub mod graph_backend;
pub mod mcp_common;
pub mod registry;
pub mod sync;

pub use graph_backend::{DriftAlert, PruneReport, PrunedLessonInfo};
pub use mcp_common::McpBackend;
pub use sync::{DeltaFileEvent, DeltaIndexResult, LiveWatcher};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredFunction {
    pub name: String,
    pub kind: String,
    pub start_line: i64,
    pub end_line: i64,
    pub strategy: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileGraphContext {
    pub file_path: String,
    pub language: String,
    pub functions: Vec<StoredFunction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphSummary {
    pub file_count: i64,
    pub function_count: i64,
    pub engram_count: i64,
    pub native_ast_function_count: i64,
    pub extension_wasm_function_count: i64,
    pub text_heuristic_function_count: i64,
    pub vertex_count: i64,
    pub edge_count: i64,
    pub memory_usage: String,
    pub lessons_without_embedding: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LessonRecord {
    pub file_path: String,
    pub error_type: String,
    pub solution: String,
    pub timestamp: String,
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Cosine similarity between two embedding vectors.
/// Both slices must have the same length, or 0.0 is returned.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    (dot / (norm_a * norm_b)).clamp(0.0, 1.0)
}

/// Normalizes a filesystem path for consistent storage.
/// Strips the `\\?\` prefix that Windows canonicalization adds.
pub fn normalize_path(path: &str) -> String {
    let cleaned = if path.starts_with(r"\\?\") {
        &path[4..]
    } else {
        path
    };
    cleaned
        .trim_end_matches('\\')
        .trim_end_matches('/')
        .to_string()
}
