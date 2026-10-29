// Copyright (c) 2026 Ozygram Project. All rights reserved.
// Fuzzy AST Block Patching Engine with Whitespace Tolerance & Auto-Alignment
// Standard: Zero-Emoji, High Precision, Tree-Sitter Validated.

use crate::SupportedLanguage;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AstPatchResult {
    pub success: bool,
    pub file_path: String,
    pub matched_by: String, // "exact", "fuzzy_whitespace", "symbol"
    pub start_line: usize,
    pub end_line: usize,
    pub lines_diff: i64,
    pub preview_diff: String,
    pub dry_run: bool,
    pub ast_verified: bool,
    pub patched_content: Option<String>,
    pub error: Option<String>,
}

/// Normalizes a line for fuzzy matching:
/// Preserves keyword token separation while eliminating spacing variations around punctuation.
pub fn normalize_fuzzy_line(line: &str) -> String {
    let is_punct = |c: char| {
        matches!(
            c,
            '(' | ')'
                | '['
                | ']'
                | '{'
                | '}'
                | ','
                | ':'
                | ';'
                | '='
                | '+'
                | '-'
                | '*'
                | '/'
                | '<'
                | '>'
                | '!'
                | '&'
                | '|'
                | '.'
        )
    };

    let chars: Vec<char> = line.chars().collect();
    let n = chars.len();
    let mut normalized = String::with_capacity(n);
    let mut prev_is_space = false;

    for i in 0..n {
        let ch = chars[i];
        if ch.is_whitespace() {
            // Find surrounding non-whitespace characters to determine if space is needed
            let prev_non_ws = (0..i).rev().find_map(|j| {
                if !chars[j].is_whitespace() {
                    Some(chars[j])
                } else {
                    None
                }
            });
            let next_non_ws = ((i + 1)..n).find_map(|j| {
                if !chars[j].is_whitespace() {
                    Some(chars[j])
                } else {
                    None
                }
            });

            let need_space = match (prev_non_ws, next_non_ws) {
                (Some(p), Some(nxt)) => !is_punct(p) && !is_punct(nxt),
                _ => false,
            };

            if need_space && !prev_is_space && !normalized.is_empty() {
                normalized.push(' ');
                prev_is_space = true;
            }
        } else {
            normalized.push(ch);
            prev_is_space = false;
        }
    }

    normalized.trim().to_string()
}

/// Locates a target code block inside the source text.
/// Returns (start_line, end_line, match_kind) with 1-based indexing.
pub fn find_target_block(source: &str, target_code: &str) -> Option<(usize, usize, &'static str)> {
    if target_code.trim().is_empty() {
        return None;
    }

    let is_crlf = source.contains("\r\n");
    let source_lines: Vec<&str> = if is_crlf {
        source.split("\r\n").collect()
    } else {
        source.split('\n').collect()
    };

    let target_is_crlf = target_code.contains("\r\n");
    let target_lines: Vec<&str> = if target_is_crlf {
        target_code.split("\r\n").collect()
    } else {
        target_code.split('\n').collect()
    };

    let target_trimmed_lines: Vec<&str> = target_lines
        .iter()
        .copied()
        .filter(|l| !l.trim().is_empty())
        .collect();

    if target_trimmed_lines.is_empty() {
        return None;
    }

    // 1. Attempt Exact Matching
    let target_len = target_lines.len();
    if source_lines.len() >= target_len {
        for i in 0..=(source_lines.len() - target_len) {
            let window = &source_lines[i..i + target_len];
            if window == target_lines.as_slice() {
                return Some((i + 1, i + target_len, "exact"));
            }
        }
    }

    // 2. Attempt Fuzzy Whitespace Matching
    let target_norm: Vec<String> = target_trimmed_lines
        .iter()
        .map(|l| normalize_fuzzy_line(l))
        .collect();
    let target_norm_len = target_norm.len();

    let mut source_non_empty_indices = Vec::new();
    let mut source_norm_lines = Vec::new();

    for (idx, line) in source_lines.iter().enumerate() {
        if !line.trim().is_empty() {
            source_non_empty_indices.push(idx);
            source_norm_lines.push(normalize_fuzzy_line(line));
        }
    }

    if source_norm_lines.len() >= target_norm_len {
        for i in 0..=(source_norm_lines.len() - target_norm_len) {
            let window = &source_norm_lines[i..i + target_norm_len];
            if window == target_norm.as_slice() {
                let start_idx = source_non_empty_indices[i];
                let end_idx = source_non_empty_indices[i + target_norm_len - 1];
                return Some((start_idx + 1, end_idx + 1, "fuzzy_whitespace"));
            }
        }
    }

    None
}

/// Applies code patch with indentation alignment and Tree-Sitter validation.
pub fn apply_fuzzy_ast_patch(
    file_path: &str,
    source: &str,
    target_code: &str,
    replacement: &str,
    dry_run: bool,
) -> Result<AstPatchResult> {
    let is_crlf = source.contains("\r\n");
    let newline = if is_crlf { "\r\n" } else { "\n" };

    let source_lines: Vec<&str> = if is_crlf {
        source.split("\r\n").collect()
    } else {
        source.split('\n').collect()
    };

    let Some((start_line, end_line, match_kind)) = find_target_block(source, target_code) else {
        return Ok(AstPatchResult {
            success: false,
            file_path: file_path.to_string(),
            matched_by: "none".to_string(),
            start_line: 0,
            end_line: 0,
            lines_diff: 0,
            preview_diff: String::new(),
            dry_run,
            ast_verified: false,
            patched_content: None,
            error: Some(format!(
                "[ALERT: TARGET_NOT_FOUND] Could not locate target block in '{}'. Checked exact and fuzzy whitespace matching.",
                file_path
            )),
        });
    };

    let before_lines = &source_lines[..(start_line - 1)];
    let target_slice = &source_lines[(start_line - 1)..end_line];
    let after_lines = &source_lines[end_line..];

    // Detect target base indentation
    let target_indent = target_slice
        .iter()
        .find(|l| !l.trim().is_empty())
        .map(|l| {
            let non_ws = l.find(|c: char| !c.is_whitespace()).unwrap_or(0);
            &l[..non_ws]
        })
        .unwrap_or("");

    // Detect replacement base indentation
    let repl_lines: Vec<&str> = if replacement.contains("\r\n") {
        replacement.split("\r\n").collect()
    } else {
        replacement.split('\n').collect()
    };

    let repl_indent = repl_lines
        .iter()
        .find(|l| !l.trim().is_empty())
        .map(|l| {
            let non_ws = l.find(|c: char| !c.is_whitespace()).unwrap_or(0);
            &l[..non_ws]
        })
        .unwrap_or("");

    // Align replacement lines to target indentation
    let aligned_repl_lines: Vec<String> = if repl_indent != target_indent {
        repl_lines
            .iter()
            .map(|l| {
                if l.trim().is_empty() {
                    String::new()
                } else if let Some(stripped) = l.strip_prefix(repl_indent) {
                    format!("{}{}", target_indent, stripped)
                } else {
                    format!("{}{}", target_indent, l.trim_start())
                }
            })
            .collect()
    } else {
        repl_lines.iter().map(|s| s.to_string()).collect()
    };

    // Construct full new content
    let mut patched_lines: Vec<String> = before_lines.iter().map(|s| s.to_string()).collect();
    patched_lines.extend(aligned_repl_lines);
    patched_lines.extend(after_lines.iter().map(|s| s.to_string()));

    let patched_content = patched_lines.join(newline);

    // Verify Tree-Sitter AST syntax integrity
    let path_obj = Path::new(file_path);
    let lang = SupportedLanguage::from_path(path_obj);
    let ast_verified = if let Some(ts_lang) = lang.native_tree_sitter_language() {
        let mut parser = tree_sitter::Parser::new();
        if parser.set_language(&ts_lang).is_ok() {
            if let Some(tree) = parser.parse(&patched_content, None) {
                !tree.root_node().has_error()
            } else {
                false
            }
        } else {
            true
        }
    } else {
        true
    };

    let lines_diff = (patched_lines.len() as i64) - (source_lines.len() as i64);

    let preview_diff = format!(
        "--- {}\n+++ {}\n@@ -{},{} +{},{} @@\n- {}\n+ {}",
        file_path,
        file_path,
        start_line,
        target_slice.len(),
        start_line,
        repl_lines.len(),
        target_slice.join("\n- "),
        repl_lines.join("\n+ ")
    );

    Ok(AstPatchResult {
        success: true,
        file_path: file_path.to_string(),
        matched_by: match_kind.to_string(),
        start_line,
        end_line,
        lines_diff,
        preview_diff,
        dry_run,
        ast_verified,
        patched_content: Some(patched_content),
        error: None,
    })
}
