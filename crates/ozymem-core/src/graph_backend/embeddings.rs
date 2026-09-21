use crate::graph_backend::types::{EmbeddingModelStatus, GraphBackend, LessonEntry, SimilarLesson};
use anyhow::Result;
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use rusqlite::params;
use std::path::PathBuf;
use std::sync::Mutex;

impl GraphBackend {
    /// Directorio de caché predeterminado para los modelos de embeddings locales
    pub fn default_model_cache_dir() -> PathBuf {
        let home = std::env::var_os("OZYMEM_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("."));
        home.join(".ozymem").join("models").join("fastembed")
    }

    /// Comprueba el estado de los archivos físicos del modelo en disco.
    pub fn check_model_files_status() -> EmbeddingModelStatus {
        let cache_dir = Self::default_model_cache_dir();
        if !cache_dir.exists() {
            return EmbeddingModelStatus::NotDownloaded;
        }
        // fastembed descarga bajo models--Qdrant--all-MiniLM-L6-v2-onnx
        let model_dir = cache_dir.join("models--Qdrant--all-MiniLM-L6-v2-onnx");
        if !model_dir.exists() {
            return EmbeddingModelStatus::NotDownloaded;
        }
        // Buscar model.onnx recursivamente dentro del directorio de snapshots
        let has_valid_onnx = walkdir::WalkDir::new(&model_dir)
            .into_iter()
            .filter_map(|e| e.ok())
            .any(|e| {
                if e.file_name() == "model.onnx" {
                    if let Ok(meta) = e.metadata() {
                        return meta.len() > 1_000_000;
                    }
                }
                false
            });

        if has_valid_onnx {
            EmbeddingModelStatus::Ready
        } else {
            EmbeddingModelStatus::CorruptedOrDeleted
        }
    }

    /// Obtiene el estado actual del modelo de embeddings
    pub fn get_embedding_status(&self) -> EmbeddingModelStatus {
        self.embedding_status.lock().unwrap().clone()
    }

    /// Comprueba si el embedder está inicializado y listo para inferencia
    pub fn embedder_ready(&self) -> bool {
        matches!(*self.embedding_status.lock().unwrap(), EmbeddingModelStatus::Ready)
    }

    /// Inicia la descarga o carga en segundo plano del modelo ONNX sin bloquear Tokio RPC.
    pub fn start_background_embedder_download(&self) {
        let current = self.get_embedding_status();
        if current == EmbeddingModelStatus::Ready || current == EmbeddingModelStatus::Downloading {
            return;
        }

        *self.embedding_status.lock().unwrap() = EmbeddingModelStatus::Downloading;
        let status_arc = self.embedding_status.clone();
        let embedder_arc = self.embedder.clone();
        let cache_dir = Self::default_model_cache_dir();

        std::thread::spawn(move || {
            eprintln!("[ozymem] Iniciando descarga/carga de embedding model en segundo plano (all-MiniLM-L6-v2)...");
            std::fs::create_dir_all(&cache_dir).ok();
            let opts = InitOptions::new(EmbeddingModel::AllMiniLML6V2)
                .with_show_download_progress(false);
            match TextEmbedding::try_new(opts) {
                Ok(m) => {
                    eprintln!("[ozymem] Embedding model descargado y listo.");
                    let mut guard = embedder_arc.lock().unwrap();
                    *guard = Some(Mutex::new(m));
                    *status_arc.lock().unwrap() = EmbeddingModelStatus::Ready;
                }
                Err(e) => {
                    eprintln!("[ozymem] Fallo en descarga/carga de embedding model: {e}");
                    *status_arc.lock().unwrap() = EmbeddingModelStatus::Failed(e.to_string());
                }
            }
        });
    }

    /// Pre-inicializa el text embedder en background.
    pub fn init_embedder(&self) {
        self.start_background_embedder_download();
    }

    /// Generate embedding bytes for text (outside lock).
    /// Returns (raw f32 LE bytes, model_name) or (None, "") if embedder unavailable.
    pub(crate) fn embed_text(&self, texts: &[&str]) -> (Option<Vec<u8>>, &'static str) {
        if !self.embedder_ready() {
            return (None, "");
        }
        let guard = self.embedder.lock().unwrap();
        let embedder_mutex = match guard.as_ref() {
            Some(m) => m,
            None => return (None, ""),
        };
        let m = match embedder_mutex.lock() {
            Ok(g) => g,
            Err(_) => return (None, ""),
        };
        match m.embed(texts.to_vec(), Some(1)) {
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
