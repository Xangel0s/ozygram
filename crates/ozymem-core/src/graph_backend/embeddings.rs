use crate::graph_backend::types::LessonEntry;
use anyhow::Result;
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use rusqlite::params;
use std::sync::Mutex;
use crate::graph_backend::types::{GraphBackend, SimilarLesson};

impl GraphBackend {
    /// Get or lazily initialize the text embedder.
    pub(crate) fn get_embedder(&self) -> Option<&Mutex<TextEmbedding>> {
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
    pub(crate) fn embed_text(&self, texts: &[&str]) -> (Option<Vec<u8>>, &'static str) {
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
            let embedding_blob: Option<Vec<u8>> = row.get(9)?;
            let lesson = LessonEntry::from_row(row)?;
            Ok((lesson, embedding_blob))
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
