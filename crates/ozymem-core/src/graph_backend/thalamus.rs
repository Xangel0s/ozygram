use serde::{Deserialize, Serialize};

/// Cognitive focus modes determined by the Thalamic Attention Router.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FocusMode {
    HotfixDebug,
    ArchitectureRefactor,
    Balanced,
}

impl FocusMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            FocusMode::HotfixDebug => "HOTFIX_DEBUG",
            FocusMode::ArchitectureRefactor => "ARCHITECTURE_REFACTOR",
            FocusMode::Balanced => "BALANCED",
        }
    }
}

/// Dynamic attention weights (alpha, beta, gamma) modulated by task intent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThalamicWeights {
    /// Weight for lexical / FTS5 keyword matching
    pub alpha_lexical: f32,
    /// Weight for Amygdala threat scars and fragility reflexes
    pub beta_threat: f32,
    /// Weight for AST structural topology and PageRank
    pub gamma_topology: f32,
    /// Active cognitive focus mode
    pub mode: FocusMode,
    /// Human/Agent-readable explanation badge
    pub badge: String,
}

impl ThalamicWeights {
    /// Modulates thalamic attention weights based on incoming query intent and keywords.
    pub fn modulate(query: &str) -> Self {
        let q_lower = query.to_lowercase();

        let hotfix_keywords = [
            "fix", "bug", "error", "panic", "crash", "revert", "regression",
            "failure", "broken", "hotfix", "patch", "issue", "fragile",
        ];

        let refactor_keywords = [
            "refactor", "architecture", "migrate", "design", "structure",
            "decouple", "module", "lineage", "dependency", "extract", "topo",
        ];

        let is_hotfix = hotfix_keywords.iter().any(|&kw| q_lower.contains(kw));
        let is_refactor = refactor_keywords.iter().any(|&kw| q_lower.contains(kw));

        if is_hotfix && !is_refactor {
            ThalamicWeights {
                alpha_lexical: 0.30,
                beta_threat: 0.60,
                gamma_topology: 0.10,
                mode: FocusMode::HotfixDebug,
                badge: "[THALAMUS_FOCUS: HOTFIX_DEBUG (Beta=0.60, High Threat Alert)]".to_string(),
            }
        } else if is_refactor && !is_hotfix {
            ThalamicWeights {
                alpha_lexical: 0.30,
                beta_threat: 0.10,
                gamma_topology: 0.60,
                mode: FocusMode::ArchitectureRefactor,
                badge: "[THALAMUS_FOCUS: ARCHITECTURE_REFACTOR (Gamma=0.60, High Topology Focus)]".to_string(),
            }
        } else {
            ThalamicWeights {
                alpha_lexical: 0.40,
                beta_threat: 0.30,
                gamma_topology: 0.30,
                mode: FocusMode::Balanced,
                badge: "[THALAMUS_FOCUS: BALANCED (Alpha=0.40, Beta=0.30, Gamma=0.30)]".to_string(),
            }
        }
    }

    /// Combines multiple cognitive dimensions into a unified relevance score.
    pub fn combine(&self, lexical_score: f32, threat_score: f32, topology_score: f32) -> f32 {
        (self.alpha_lexical * lexical_score)
            + (self.beta_threat * threat_score)
            + (self.gamma_topology * topology_score)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_thalamus_hotfix_modulation() {
        let weights = ThalamicWeights::modulate("fix panic in router dispatch on broken connection");
        assert_eq!(weights.mode, FocusMode::HotfixDebug);
        assert_eq!(weights.beta_threat, 0.60);
        assert_eq!(weights.gamma_topology, 0.10);
        assert!(weights.badge.contains("HOTFIX_DEBUG"));

        // Combining with high threat score must yield high combined score
        let score = weights.combine(0.5, 1.0, 0.2);
        // 0.30 * 0.5 + 0.60 * 1.0 + 0.10 * 0.2 = 0.15 + 0.60 + 0.02 = 0.77
        assert!((score - 0.77).abs() < 1e-4);
    }

    #[test]
    fn test_thalamus_refactor_modulation() {
        let weights = ThalamicWeights::modulate("refactor module lineage and decouple service dependencies");
        assert_eq!(weights.mode, FocusMode::ArchitectureRefactor);
        assert_eq!(weights.gamma_topology, 0.60);
        assert_eq!(weights.beta_threat, 0.10);
        assert!(weights.badge.contains("ARCHITECTURE_REFACTOR"));

        // Combining with high topology score must yield high combined score
        let score = weights.combine(0.5, 0.2, 1.0);
        // 0.30 * 0.5 + 0.10 * 0.2 + 0.60 * 1.0 = 0.15 + 0.02 + 0.60 = 0.77
        assert!((score - 0.77).abs() < 1e-4);
    }

    #[test]
    fn test_thalamus_balanced_modulation() {
        let weights = ThalamicWeights::modulate("show summary for project");
        assert_eq!(weights.mode, FocusMode::Balanced);
        assert_eq!(weights.alpha_lexical, 0.40);
        assert_eq!(weights.beta_threat, 0.30);
        assert_eq!(weights.gamma_topology, 0.30);
        assert!(weights.badge.contains("BALANCED"));
    }
}
