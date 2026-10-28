use fastembed::{RerankInitOptions, RerankerModel, TextRerank};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

/// Modo de re-ranking semántico para recuperación de lecciones y memorias.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RerankMode {
    NeuralOnnx,
    DeterministicOverlap,
}

impl RerankMode {
    pub fn from_env() -> Self {
        if std::env::var("OZYMEM_NEURAL_RERANKER")
            .map(|v| matches!(v.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
            .unwrap_or(false)
        {
            RerankMode::NeuralOnnx
        } else {
            RerankMode::DeterministicOverlap
        }
    }
}

/// Re-ranker híbrido neuronal/simbólico.
/// Proporciona un Cross-Encoder local en ONNX Runtime (BGE-Reranker-base)
/// con degradación transparente e instantánea al re-ranker determinista de solapamiento de tokens ($0 costo).
pub struct NeuralReranker {
    mode: RerankMode,
    model: Arc<Mutex<Option<TextRerank>>>,
}

impl Default for NeuralReranker {
    fn default() -> Self {
        Self::new(RerankMode::from_env())
    }
}

impl NeuralReranker {
    pub fn new(mode: RerankMode) -> Self {
        let reranker = Self {
            mode,
            model: Arc::new(Mutex::new(None)),
        };

        if mode == RerankMode::NeuralOnnx {
            reranker.init_model();
        }

        reranker
    }

    /// Intenta inicializar el modelo local de FastEmbed TextRerank.
    pub fn init_model(&self) {
        let mut guard = self.model.lock().unwrap();
        if guard.is_none() {
            let mut opts = RerankInitOptions::default();
            opts.model_name = RerankerModel::BGERerankerBase;
            if let Ok(model) = TextRerank::try_new(opts) {
                *guard = Some(model);
            }
        }
    }

    pub fn mode(&self) -> RerankMode {
        self.mode
    }

    pub fn is_neural_ready(&self) -> bool {
        self.model.lock().unwrap().is_some()
    }

    /// Calcula la puntuación determinista de solapamiento de tokens (Jaccard + frecuencia normalizada).
    pub fn token_overlap_score(query: &str, document: &str) -> f32 {
        let q_tokens: HashSet<String> = query
            .split_whitespace()
            .map(|t| t.to_lowercase())
            .filter(|t| t.len() > 1)
            .collect();

        if q_tokens.is_empty() {
            return 0.0;
        }

        let doc_lower = document.to_lowercase();
        let doc_tokens: HashSet<String> = doc_lower
            .split_whitespace()
            .map(|t| t.to_string())
            .collect();

        let intersection_count = q_tokens.intersection(&doc_tokens).count();
        let overlap_ratio = intersection_count as f32 / q_tokens.len() as f32;

        // Bonificación si el término de consulta completo aparece como subcadena exacta
        let exact_bonus = if doc_lower.contains(&query.to_lowercase()) {
            0.25
        } else {
            0.0
        };

        (overlap_ratio * 0.75 + exact_bonus).min(1.0)
    }

    /// Re-clasifica un conjunto de documentos candidatos frente a una consulta.
    pub fn rerank(&self, query: &str, documents: &[&str]) -> Vec<f32> {
        if documents.is_empty() {
            return Vec::new();
        }

        // 1. Intentar re-ranking neural si el modelo está disponible
        if self.mode == RerankMode::NeuralOnnx {
            let guard = self.model.lock().unwrap();
            if let Some(ref model) = *guard {
                let docs_vec: Vec<&str> = documents.to_vec();
                if let Ok(results) = model.rerank(query, docs_vec, false, None) {
                    let mut scores = vec![0.0; documents.len()];
                    for res in results {
                        if res.index < scores.len() {
                            scores[res.index] = res.score;
                        }
                    }
                    return scores;
                }
            }
        }

        // 2. Fallback determinista de cero latencia
        documents
            .iter()
            .map(|doc| Self::token_overlap_score(query, doc))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_overlap_score_calculation() {
        let query = "calculate invoice total probetas";
        let doc_match = "Function to calculate invoice total for probetas laboratory orders";
        let doc_irrelevant = "Database connection pool timeout configuration";

        let score_match = NeuralReranker::token_overlap_score(query, doc_match);
        let score_irrelevant = NeuralReranker::token_overlap_score(query, doc_irrelevant);

        assert!(score_match > 0.50);
        assert_eq!(score_irrelevant, 0.0);
    }

    #[test]
    fn test_deterministic_rerank_batch() {
        let reranker = NeuralReranker::new(RerankMode::DeterministicOverlap);
        let query = "auth token jwt";
        let docs = [
            "Validates user auth token with jwt signature",
            "Unrelated file rendering react dashboard table",
        ];

        let scores = reranker.rerank(query, &docs);
        assert_eq!(scores.len(), 2);
        assert!(scores[0] > scores[1]);
    }

    #[test]
    fn test_neural_mode_graceful_fallback() {
        // En modo neural, si el modelo no está en cache, degrada limpiamente a determinista
        let reranker = NeuralReranker::new(RerankMode::NeuralOnnx);
        let query = "database migration probe";
        let docs = ["Database migration adding probe table", "CSS layout styling"];

        let scores = reranker.rerank(query, &docs);
        assert_eq!(scores.len(), 2);
        assert!(scores[0] > scores[1]);
    }
}
