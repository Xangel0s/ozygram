//! Hippocampal Theta-Gamma Phase Precession Working Memory Buffer in native Rust.
//! Structures working context into 4 strictly ordered Theta phase bins (leaves to entrypoints),
//! bounding Gamma item slots to extractive +-10 line windows (<= 300 tokens per slot)
//! to prevent LLM attention dilution ("lost in the middle") and achieve > 70% compression.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GammaSlot {
    pub file_path: String,
    pub snippet: String,
    pub start_line: usize,
    pub end_line: usize,
    pub tokens: usize,
    pub symbol: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThetaGammaPhaseBin {
    pub phase_name: String,
    pub phase_index: u8,
    pub slots: Vec<GammaSlot>,
    pub tokens_total: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThetaGammaContext {
    pub phase_bins: Vec<ThetaGammaPhaseBin>,
    pub raw_tokens_total: usize,
    pub gamma_tokens_total: usize,
    pub compression_ratio: f64,
}

pub struct ThetaGammaBuffer {
    pub window_lines: usize,
    pub max_tokens_per_slot: usize,
}

impl Default for ThetaGammaBuffer {
    fn default() -> Self {
        Self {
            window_lines: 10,
            max_tokens_per_slot: 300,
        }
    }
}

impl ThetaGammaBuffer {
    pub fn new(window_lines: usize, max_tokens_per_slot: usize) -> Self {
        Self {
            window_lines,
            max_tokens_per_slot,
        }
    }

    /// Classifies a file path into its natural topological Theta phase bin (1 to 4).
    pub fn classify_file_phase(file_path: &str) -> (&'static str, u8) {
        let fp = file_path.to_lowercase().replace('\\', "/");

        if fp.contains("test") || fp.contains("spec") || fp.contains("eval") || fp.contains("mock") {
            ("validation", 4)
        } else if fp.contains("type")
            || fp.contains("model")
            || fp.contains("schema")
            || fp.contains("contract")
            || fp.ends_with(".d.ts")
            || fp.contains("entity")
            || fp.contains("dto")
        {
            ("leaf_contracts", 1)
        } else if fp.contains("route")
            || fp.contains("endpoint")
            || fp.contains("controller")
            || fp.contains("handler")
            || fp.contains("component")
            || fp.contains("page")
            || fp.contains("view")
            || fp.contains("main.")
            || fp.contains("server.")
        {
            ("entrypoints", 3)
        } else {
            // Default domain services / logic
            ("domain_services", 2)
        }
    }

    /// Extracts a bounded +-N line extractive window around the target symbol.
    pub fn extract_gamma_window(
        &self,
        file_path: &str,
        content: &str,
        symbol_hint: Option<&str>,
    ) -> GammaSlot {
        let lines: Vec<&str> = content.lines().collect();
        let total_lines = lines.len();
        if total_lines == 0 {
            return GammaSlot {
                file_path: file_path.to_string(),
                snippet: String::new(),
                start_line: 0,
                end_line: 0,
                tokens: 0,
                symbol: symbol_hint.map(|s| s.to_string()),
            };
        }

        let mut target_line_idx = 0;
        if let Some(hint) = symbol_hint {
            for (idx, line) in lines.iter().enumerate() {
                if line.contains(hint) {
                    target_line_idx = idx;
                    break;
                }
            }
        } else {
            for (idx, line) in lines.iter().enumerate() {
                if line.contains("export ")
                    || line.contains("def ")
                    || line.contains("fn ")
                    || line.contains("pub fn ")
                    || line.contains("class ")
                    || line.contains("interface ")
                {
                    target_line_idx = idx;
                    break;
                }
            }
        }

        let start = target_line_idx.saturating_sub(self.window_lines);
        let end = (target_line_idx + self.window_lines + 1).min(total_lines);
        let snippet_lines = &lines[start..end];
        let mut snippet = snippet_lines.join("\n");

        let mut tokens = (snippet.len() / 4).max(1);
        if tokens > self.max_tokens_per_slot {
            let char_limit = self.max_tokens_per_slot * 4;
            snippet.truncate(char_limit);
            snippet.push_str("\n... [TRUNCATED_GAMMA_SLOT]");
            tokens = self.max_tokens_per_slot;
        }

        GammaSlot {
            file_path: file_path.to_string(),
            snippet,
            start_line: start + 1, // 1-based indexing
            end_line: end,
            tokens,
            symbol: symbol_hint.map(|s| s.to_string()),
        }
    }

    /// Packs multiple candidate files into strictly ordered Theta bins with compact Gamma slots.
    pub fn pack_context(
        &self,
        files: &[String],
        symbol_hints: &HashMap<String, Vec<String>>,
        file_contents: &HashMap<String, String>,
    ) -> ThetaGammaContext {
        let mut bin_map: HashMap<u8, Vec<GammaSlot>> = HashMap::new();
        bin_map.insert(1, Vec::new());
        bin_map.insert(2, Vec::new());
        bin_map.insert(3, Vec::new());
        bin_map.insert(4, Vec::new());

        let mut raw_tokens_total = 0;
        let mut gamma_tokens_total = 0;

        for f in files {
            let (_, phase_idx) = Self::classify_file_phase(f);
            let empty_str = String::new();
            let body = file_contents.get(f).unwrap_or(&empty_str);
            raw_tokens_total += (body.len() / 4).max(1);

            let default_hints = Vec::new();
            let hints = symbol_hints.get(f).unwrap_or(&default_hints);

            if hints.is_empty() {
                let slot = self.extract_gamma_window(f, body, None);
                gamma_tokens_total += slot.tokens;
                bin_map.get_mut(&phase_idx).unwrap().push(slot);
            } else {
                for sym in hints {
                    let slot = self.extract_gamma_window(f, body, Some(sym));
                    gamma_tokens_total += slot.tokens;
                    bin_map.get_mut(&phase_idx).unwrap().push(slot);
                }
            }
        }

        let mut phase_bins = Vec::with_capacity(4);
        for p_idx in 1..=4 {
            let slots = bin_map.remove(&p_idx).unwrap_or_default();
            let phase_name = match p_idx {
                1 => "leaf_contracts",
                2 => "domain_services",
                3 => "entrypoints",
                4 => "validation",
                _ => "unknown",
            };
            let b_tokens: usize = slots.iter().map(|s| s.tokens).sum();
            phase_bins.push(ThetaGammaPhaseBin {
                phase_name: phase_name.to_string(),
                phase_index: p_idx,
                slots,
                tokens_total: b_tokens,
            });
        }

        let compression_ratio = if raw_tokens_total > 0 {
            let saved = raw_tokens_total.saturating_sub(gamma_tokens_total);
            ((saved as f64) / (raw_tokens_total as f64) * 10000.0).round() / 100.0
        } else {
            0.0
        };

        ThetaGammaContext {
            phase_bins,
            raw_tokens_total,
            gamma_tokens_total,
            compression_ratio,
        }
    }
}

