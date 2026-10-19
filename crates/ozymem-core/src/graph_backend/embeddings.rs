use crate::graph_backend::types::{
    EmbeddingModelStatus, ExtractiveSnippet, GraphBackend, LessonEntry, SimilarLesson,
};
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

    pub fn start_background_embedder_download(&self) {
        let in_test = cfg!(test) || std::env::current_exe().map(|p| {
            let s = p.to_string_lossy().to_lowercase();
            s.contains("\\deps\\") || s.contains("/deps/")
        }).unwrap_or(false);

        if in_test && std::env::var_os("OZYMEM_ENABLE_AUTO_DOWNLOAD_IN_TESTS").is_none() {
            return;
        }
        if std::env::var_os("OZYMEM_DISABLE_AUTO_DOWNLOAD").is_some() {
            return;
        }

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
                .with_cache_dir(cache_dir)
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

        let mut scored: Vec<(SimilarLesson, Vec<f32>)> = rows
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
                Some((
                    SimilarLesson {
                        lesson,
                        score,
                        snippet: None,
                    },
                    vec,
                ))
            })
            .collect();

        scored.sort_by(|(a, _), (b, _)| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Maximal Marginal Relevance (MMR) / Diversidad: Suprimir lecciones redundantes
        // Si dos lecciones tienen similitud coseno entre sí > 0.88, se conserva solo la de mayor ranking.
        let mut deduplicated: Vec<SimilarLesson> = Vec::new();
        let mut selected_vectors: Vec<Vec<f32>> = Vec::new();

        for (candidate, vec) in scored {
            let is_redundant = selected_vectors.iter().any(|selected| {
                crate::cosine_similarity(selected, &vec) > 0.88
            });

            if !is_redundant {
                selected_vectors.push(vec);
                deduplicated.push(candidate);
                if deduplicated.len() >= limit.min(100) {
                    break;
                }
            }
        }

        Ok(deduplicated)
    }

    /// Extrae una ventana contextual de un archivo registrado o de disco (+/- 10 líneas)
    pub fn extract_context_window(
        &self,
        file_path: &str,
        query: &str,
        preferred_line: Option<usize>,
    ) -> Option<ExtractiveSnippet> {
        let resolved = self.resolve_target_path(file_path).unwrap_or_else(|| file_path.to_string());
        let path_obj = std::path::Path::new(&resolved);
        let effective = if path_obj.exists() {
            path_obj.to_path_buf()
        } else if let Some(root) = self.project_path() {
            std::path::Path::new(&root).join(&resolved)
        } else {
            std::path::PathBuf::from(&resolved)
        };

        if let Ok(source) = std::fs::read_to_string(&effective) {
            Some(extract_snippet_from_source(file_path, &source, query, preferred_line, 10))
        } else {
            None
        }
    }

    /// Busca lecciones similares e inyecta ventanas extractivas quirúrgicas en lugar de bloques completos
    pub fn similar_lessons_with_windows(
        &self,
        query: &str,
        limit: usize,
        min_score: f32,
    ) -> Result<Vec<SimilarLesson>> {
        let mut results = self.similar_lessons(query, limit, min_score)?;
        for item in &mut results {
            if let Some(snip) = self.extract_context_window(&item.lesson.file_path, query, None) {
                item.snippet = Some(snip);
            } else if !item.lesson.error_context.is_empty() {
                let snip = extract_snippet_from_source(
                    &item.lesson.file_path,
                    &item.lesson.error_context,
                    query,
                    None,
                    10,
                );
                item.snippet = Some(snip);
            }
        }
        Ok(results)
    }
}

/// Extrae una ventana contextual quirúrgica alrededor de la línea de mayor coincidencia (+/- window_padding líneas)
pub fn extract_snippet_from_source(
    file_path: &str,
    source: &str,
    query: &str,
    preferred_line: Option<usize>,
    window_padding: usize,
) -> ExtractiveSnippet {
    let lines: Vec<&str> = source.lines().collect();
    if lines.is_empty() {
        return ExtractiveSnippet {
            file_path: file_path.to_string(),
            start_line: 1,
            end_line: 1,
            match_line: 1,
            content: String::new(),
            token_estimate: 0,
        };
    }

    let peak_line = if let Some(pref) = preferred_line {
        pref.clamp(1, lines.len())
    } else {
        let query_tokens: Vec<String> = query
            .split_whitespace()
            .map(|t| t.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase())
            .filter(|t| !t.is_empty())
            .collect();

        let mut best_line = 1;
        let mut best_score = 0;

        for (idx, line) in lines.iter().enumerate() {
            let line_lower = line.to_lowercase();
            let mut score = 0;
            if !query.is_empty() && line_lower.contains(&query.to_lowercase()) {
                score += 100;
            }
            for token in &query_tokens {
                if line_lower.contains(token) {
                    score += 10;
                }
            }
            if score > best_score {
                best_score = score;
                best_line = idx + 1;
            }
        }
        best_line
    };

    let start_line = peak_line.saturating_sub(window_padding).max(1);
    let end_line = (peak_line + window_padding).min(lines.len());

    let mut snippet_lines = Vec::new();
    for l_idx in start_line..=end_line {
        let content_line = lines[l_idx - 1];
        snippet_lines.push(format!("{:4} | {}", l_idx, content_line));
    }

    let content = snippet_lines.join("\n");
    let token_estimate = (content.len() / 4).max(1);

    ExtractiveSnippet {
        file_path: file_path.to_string(),
        start_line,
        end_line,
        match_line: peak_line,
        content,
        token_estimate,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_snippet_centered_window() {
        let mut source_lines = Vec::new();
        for i in 1..=100 {
            if i == 50 {
                source_lines.push(format!("line {}: critical recalculate target logic here", i));
            } else {
                source_lines.push(format!("line {}: normal code", i));
            }
        }
        let source = source_lines.join("\n");

        let snippet = extract_snippet_from_source(
            "src/services/recalculo.ts",
            &source,
            "recalculate target logic",
            None,
            10,
        );

        assert_eq!(snippet.match_line, 50);
        assert_eq!(snippet.start_line, 40);
        assert_eq!(snippet.end_line, 60);
        assert!(snippet.content.contains("50 | line 50: critical recalculate target logic here"));
        assert!(snippet.content.contains("40 | line 40: normal code"));
        assert!(snippet.content.contains("60 | line 60: normal code"));
        assert!(!snippet.content.contains("39 | line 39: normal code"));
        assert!(!snippet.content.contains("61 | line 61: normal code"));
        assert!(snippet.token_estimate > 0 && snippet.token_estimate < 300);
    }

    #[test]
    fn test_extract_snippet_boundaries() {
        let source = "line 1\nline 2\nline 3: target match\nline 4\nline 5";
        let snippet = extract_snippet_from_source("small.ts", source, "target match", None, 10);
        assert_eq!(snippet.start_line, 1);
        assert_eq!(snippet.end_line, 5);
        assert_eq!(snippet.match_line, 3);

        let snippet_pref = extract_snippet_from_source("small.ts", source, "irrelevant", Some(4), 2);
        assert_eq!(snippet_pref.start_line, 2);
        assert_eq!(snippet_pref.end_line, 5);
        assert_eq!(snippet_pref.match_line, 4);
    }

    #[test]
    fn test_mmr_deduplication_suppresses_redundant_vectors() {
        let v1 = vec![1.0f32, 0.0, 0.0];
        let v2_sister = vec![0.99f32, 0.05, 0.0]; // Cosine similarity > 0.99 with v1
        let v3_diverse = vec![0.0f32, 1.0, 0.0]; // Cosine similarity 0.0 with v1

        assert!(crate::cosine_similarity(&v1, &v2_sister) > 0.88);
        assert!(crate::cosine_similarity(&v1, &v3_diverse) < 0.88);

        let candidates = vec![
            ("Lesson 1: Fidelidad Estricta", v1),
            ("Lesson 2: Fidelidad Directa", v2_sister),
            ("Lesson 3: Auth Token Expiry", v3_diverse),
        ];

        let mut deduplicated = Vec::new();
        let mut selected_vectors: Vec<Vec<f32>> = Vec::new();

        for (title, vec) in candidates {
            let is_redundant = selected_vectors.iter().any(|selected| {
                crate::cosine_similarity(selected, &vec) > 0.88
            });
            if !is_redundant {
                selected_vectors.push(vec);
                deduplicated.push(title);
            }
        }

        assert_eq!(deduplicated.len(), 2);
        assert_eq!(deduplicated[0], "Lesson 1: Fidelidad Estricta");
        assert_eq!(deduplicated[1], "Lesson 3: Auth Token Expiry");
        assert!(!deduplicated.contains(&"Lesson 2: Fidelidad Directa"));
    }
}
