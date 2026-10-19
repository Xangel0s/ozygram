use serde::{Deserialize, Serialize};

pub mod bundle;
pub mod git_backend;
pub mod graph_backend;
pub mod mcp_common;
pub mod registry;
pub mod sync;
pub mod engram_store;

pub use bundle::{export_bundle, import_bundle, BundleExportSummary, BundleImportSummary, KnowledgeBundle};
pub use engram_store::{FastEngramReader, IncrementalEngramStore, ENGRAM_FILE};
pub use graph_backend::{DriftAlert, PruneReport, PrunedLessonInfo};
pub use mcp_common::McpBackend;
pub use ozymem_parser::AstDiagnostic;
pub use registry::{CrossRepoMemory, ProjectLink};
pub use sync::{read_file_with_backoff, DeltaFileEvent, DeltaIndexResult, LiveWatcher, OzymemGitNotePayload};

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
    #[serde(default)]
    pub engram_contracts: Vec<ozymem_parser::EngramContract>,
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

/// Normalizes a filesystem path for consistent storage and cross-platform lookup.
/// - Strips the `\\?\` prefix that Windows canonicalization adds.
/// - Unifies all backslashes (`\`) to forward slashes (`/`).
/// - Capitalizes the Windows drive letter (e.g. `c:/` -> `C:/`).
/// - Trims trailing slashes (preserving the root slash if it is just `/`).
pub fn normalize_path(path: &str) -> String {
    let cleaned = if path.starts_with(r"\\?\") {
        &path[4..]
    } else {
        path
    };

    let mut s = cleaned.replace('\\', "/");

    if s.len() >= 2 && s.as_bytes()[1] == b':' {
        let drive = s.as_bytes()[0];
        if drive.is_ascii_alphabetic() {
            let upper = (drive.to_ascii_uppercase() as char).to_string();
            s.replace_range(0..1, &upper);
        }
    }

    if s.len() > 1 {
        let trimmed = s.trim_end_matches('/');
        if !trimmed.is_empty() {
            s = trimmed.to_string();
        }
    }

    s
}
