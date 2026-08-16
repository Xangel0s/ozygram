// bundle.rs — Knowledge Bundle Portability (.ozymem)
//
// Encapsulates lessons, conventions, API routes, and graph metadata
// into a single portable, verifiable file for instant context onboarding
// and cross-machine sharing.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::graph_backend::{GraphBackend, LessonEntry};
use ozymem_parser::api_routes::ApiRouteDefinition;

pub const BUNDLE_FORMAT_VERSION: u32 = 1;
pub const DEFAULT_BUNDLE_EXT: &str = "ozymem";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleHeader {
    pub format_version: u32,
    pub project_name: String,
    pub exported_at: String,
    pub lesson_count: usize,
    pub route_count: usize,
    pub metadata: HashMap<String, String>,
    pub sha256_payload: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundlePayload {
    pub lessons: Vec<LessonEntry>,
    pub api_routes: Vec<ApiRouteDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeBundle {
    pub header: BundleHeader,
    pub payload: BundlePayload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleExportSummary {
    pub file_path: String,
    pub project_name: String,
    pub lessons_exported: usize,
    pub routes_exported: usize,
    pub sha256: String,
    pub file_size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleImportSummary {
    pub file_path: String,
    pub project_name: String,
    pub lessons_imported: usize,
    pub lessons_skipped_duplicate: usize,
    pub routes_imported: usize,
    pub mode: String, // "merge" | "overwrite"
}

/// Exports project lessons, conventions, and mapped API routes to a `.ozymem` knowledge bundle.
pub fn export_bundle(
    backend: &GraphBackend,
    project_name: &str,
    output_path: &Path,
) -> Result<BundleExportSummary> {
    // 1. Collect all lessons & conventions
    let lessons = backend.recent_lessons_sync(None, 10000)?;
    let api_routes = backend.map_api_routes(None).unwrap_or_default();

    let payload = BundlePayload {
        lessons: lessons.clone(),
        api_routes: api_routes.clone(),
    };

    let payload_bytes = serde_json::to_vec(&payload)
        .context("Failed to serialize bundle payload")?;

    let mut hasher = Sha256::new();
    hasher.update(&payload_bytes);
    let sha256_payload = format!("{:x}", hasher.finalize());

    let header = BundleHeader {
        format_version: BUNDLE_FORMAT_VERSION,
        project_name: project_name.to_string(),
        exported_at: chrono::Utc::now().to_rfc3339(),
        lesson_count: lessons.len(),
        route_count: api_routes.len(),
        metadata: HashMap::new(),
        sha256_payload: sha256_payload.clone(),
    };

    let bundle = KnowledgeBundle { header, payload };
    let json_bytes = serde_json::to_vec_pretty(&bundle)
        .context("Failed to serialize knowledge bundle")?;

    if let Some(parent) = output_path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }

    fs::write(output_path, &json_bytes)
        .with_context(|| format!("Failed to write knowledge bundle to {}", output_path.display()))?;

    Ok(BundleExportSummary {
        file_path: output_path.to_string_lossy().to_string(),
        project_name: project_name.to_string(),
        lessons_exported: lessons.len(),
        routes_exported: api_routes.len(),
        sha256: sha256_payload,
        file_size_bytes: json_bytes.len() as u64,
    })
}

/// Imports a `.ozymem` knowledge bundle into the target `GraphBackend`.
pub async fn import_bundle(
    backend: &GraphBackend,
    input_path: &Path,
    merge: bool,
) -> Result<BundleImportSummary> {
    let bytes = fs::read(input_path)
        .with_context(|| format!("Failed to read bundle from {}", input_path.display()))?;

    let bundle: KnowledgeBundle = serde_json::from_slice(&bytes)
        .context("Invalid knowledge bundle format")?;

    // Verify SHA-256 integrity
    let payload_bytes = serde_json::to_vec(&bundle.payload)?;
    let mut hasher = Sha256::new();
    hasher.update(&payload_bytes);
    let calculated_sha = format!("{:x}", hasher.finalize());

    if calculated_sha != bundle.header.sha256_payload {
        anyhow::bail!(
            "Knowledge bundle checksum mismatch! Expected: {}, Computed: {}",
            bundle.header.sha256_payload,
            calculated_sha
        );
    }

    let mut imported = 0;
    let mut skipped = 0;

    let existing_lessons = backend.recent_lessons_sync(None, 10000).unwrap_or_default();

    for lesson in bundle.payload.lessons {
        let is_dup = existing_lessons.iter().any(|e| {
            e.file_path == lesson.file_path
                && e.symbol_name == lesson.symbol_name
                && e.error_context == lesson.error_context
                && e.solution == lesson.solution
        });

        if is_dup && merge {
            skipped += 1;
            continue;
        }

        backend
            .record_entry_sync(
                &lesson.file_path,
                if lesson.symbol_name.is_empty() { None } else { Some(&lesson.symbol_name) },
                &lesson.error_context,
                &lesson.solution,
                &lesson.kind,
            )?;
        imported += 1;
    }

    Ok(BundleImportSummary {
        file_path: input_path.to_string_lossy().to_string(),
        project_name: bundle.header.project_name,
        lessons_imported: imported,
        lessons_skipped_duplicate: skipped,
        routes_imported: bundle.payload.api_routes.len(),
        mode: if merge { "merge".into() } else { "overwrite".into() },
    })
}
