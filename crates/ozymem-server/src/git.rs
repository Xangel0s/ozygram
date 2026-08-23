use ozymem_core::git_backend::GitBackend;
use ozymem_core::graph_backend::GraphBackend;
use ozymem_core::McpBackend;
use ozymem_core::mcp_common::{self, ContentBlock, ToolCallResult};
use ozymem_parser::parse_source;
use serde_json::Value;
use std::collections::HashSet;
use std::path::Path;
use std::sync::{Arc, Mutex};
use crate::formatters::detect_lang;
use crate::state::{error_response, notify_subscribed, ok_response, Notifier};
pub(crate) async fn handle_learn_from_changes(
    backend: &GraphBackend,
    tool_call: &mcp_common::ToolCallParams,
    notifier: Option<&Notifier>,
    subscribed: Option<&Arc<Mutex<HashSet<String>>>>,
) -> anyhow::Result<ToolCallResult> {
    let message = tool_call
        .arguments
        .get("message")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("missing message"))?;
    let from = tool_call
        .arguments
        .get("from")
        .and_then(Value::as_str)
        .unwrap_or("HEAD~1");
    let to = tool_call
        .arguments
        .get("to")
        .and_then(Value::as_str)
        .unwrap_or("HEAD");
    let preview = tool_call
        .arguments
        .get("preview")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let max_impact = tool_call
        .arguments
        .get("max_impact")
        .and_then(Value::as_u64)
        .unwrap_or(5) as usize;

    let project_path = backend
        .project_path()
        .ok_or_else(|| anyhow::anyhow!("no project path set"))?;

    if !Path::new(&project_path).join(".git").exists() {
        return Ok(ToolCallResult {
            content: vec![ContentBlock {
                kind: "text",
                text: "Not a git repository — learn_from_changes requires git".to_string(),
            }],
            is_error: Some(true),
        });
    }

    let output = std::process::Command::new("git")
        .args(["diff", "--name-only", "--diff-filter=ACDMR", from, to, "--"])
        .current_dir(&project_path)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Ok(ToolCallResult {
            content: vec![ContentBlock {
                kind: "text",
                text: format!("Git diff failed: {stderr}"),
            }],
            is_error: Some(true),
        });
    }

    let files: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();

    if files.is_empty() {
        return Ok(ToolCallResult {
            content: vec![ContentBlock {
                kind: "text",
                text: "No changes detected between the specified refs".to_string(),
            }],
            is_error: None,
        });
    }

    let mut entries: Vec<String> = Vec::new();
    let mut parsed_files: Vec<String> = Vec::new();

    for file in &files {
        let lang = detect_lang(file);
        if lang == ozymem_parser::SupportedLanguage::Unknown {
            continue;
        }

        let old_output = std::process::Command::new("git")
            .args(["show", &format!("{from}:{file}")])
            .current_dir(&project_path)
            .output();

        let old_source = match old_output {
            Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).to_string(),
            _ => continue,
        };

        let new_output = std::process::Command::new("git")
            .args(["show", &format!("{to}:{file}")])
            .current_dir(&project_path)
            .output();

        let new_source = match new_output {
            Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).to_string(),
            _ => String::new(),
        };

        let old_parsed = parse_source(file, lang, &old_source).ok();
        let new_parsed = parse_source(file, lang, &new_source).ok();

        let old_funcs: Vec<&ozymem_parser::ExtractedFunction> =
            old_parsed.iter().flat_map(|p| p.functions.iter()).collect();
        let new_funcs: Vec<&ozymem_parser::ExtractedFunction> =
            new_parsed.iter().flat_map(|p| p.functions.iter()).collect();

        let old_names: HashSet<&str> = old_funcs.iter().map(|f| f.name.as_str()).collect();
        let new_names: HashSet<&str> = new_funcs.iter().map(|f| f.name.as_str()).collect();

        let mut file_had_changes = false;

        // New functions → lesson (confidence: 0.92 — tree-sitter confirms new AST nodes)
        for f in &new_funcs {
            if !old_names.contains(f.name.as_str()) {
                file_had_changes = true;
                let kind_str = match f.kind {
                    ozymem_parser::SymbolKind::Function => "Function",
                    ozymem_parser::SymbolKind::Class => "Class",
                };
                let solution = format!(
                    "New {} `{}` at line {} in {}",
                    kind_str, f.name, f.start_line, file
                );
                if !preview {
                    backend
                        .record_entry(file, Some(&f.name), message, &solution, "lesson")
                        .await?;
                }
                entries.push(format!(
                    "[New] {}: {} `{}` (line {}) [confidence: 0.92]",
                    file, kind_str, f.name, f.start_line
                ));
            }
        }

        // Removed functions → decision (confidence: 0.88 — tree-sitter confirms absence)
        for f in &old_funcs {
            if !new_names.contains(f.name.as_str()) {
                file_had_changes = true;
                let kind_str = match f.kind {
                    ozymem_parser::SymbolKind::Function => "Function",
                    ozymem_parser::SymbolKind::Class => "Class",
                };
                let solution = format!(
                    "{} `{}` was removed from {} (was at line {})",
                    kind_str, f.name, file, f.start_line
                );
                if !preview {
                    backend
                        .record_entry(file, Some(&f.name), message, &solution, "decision")
                        .await?;
                }
                entries.push(format!(
                    "[Removed] {}: removed {} `{}` [confidence: 0.88]",
                    file, kind_str, f.name
                ));
            }
        }

        // Modified functions (same name, different line range) → convention
        // Confidence proportional to line-range delta: more change = higher confidence
        for f in &new_funcs {
            if let Some(old_f) = old_funcs.iter().find(|o| {
                o.name == f.name && (o.start_line != f.start_line || o.end_line != f.end_line)
            }) {
                file_had_changes = true;
                let kind_str = match f.kind {
                    ozymem_parser::SymbolKind::Function => "Function",
                    ozymem_parser::SymbolKind::Class => "Class",
                };
                let old_len = old_f.end_line - old_f.start_line;
                let new_len = f.end_line - f.start_line;
                let delta = new_len.abs_diff(old_len);
                let max_len = old_len.max(new_len).max(1);
                let confidence = 0.65 + 0.30 * (delta as f64 / max_len as f64).min(1.0);
                let solution = format!(
                    "{} `{}` in {} changed (was L{}-L{}, now L{}-L{})",
                    kind_str,
                    f.name,
                    file,
                    old_f.start_line,
                    old_f.end_line,
                    f.start_line,
                    f.end_line
                );
                if !preview {
                    backend
                        .record_entry(file, Some(&f.name), message, &solution, "convention")
                        .await?;
                }
                entries.push(format!(
                    "[Modified] {}: {} `{}` modified (L{}-L{} → L{}-L{}) [confidence: {:.2}]",
                    file,
                    kind_str,
                    f.name,
                    old_f.start_line,
                    old_f.end_line,
                    f.start_line,
                    f.end_line,
                    confidence
                ));
            }
        }

        if file_had_changes {
            parsed_files.push(file.clone());
        }
    }

    // Impact analysis via graph neighbors
    let mut impact_lines: Vec<String> = Vec::new();
    for file in &parsed_files {
        if let Ok(neighbors) = backend.get_graph_neighbors(file).await {
            let dependent_count = neighbors.incoming.len();
            let dep_on_count = neighbors.outgoing.len();
            if dependent_count > 0 || dep_on_count > 0 {
                let mut line = format!("  {file}:");
                if dependent_count > 0 {
                    let show: Vec<&String> = neighbors.incoming.iter().take(max_impact).collect();
                    for p in &show {
                        line.push_str(&format!("\n    ← {}", p));
                    }
                    if dependent_count > max_impact {
                        line.push_str(&format!(
                            "\n    ... and {} more dependents",
                            dependent_count - max_impact
                        ));
                    }
                }
                if dep_on_count > 0 {
                    let show: Vec<&String> = neighbors.outgoing.iter().take(max_impact).collect();
                    for p in &show {
                        line.push_str(&format!("\n    → {}", p));
                    }
                    if dep_on_count > max_impact {
                        line.push_str(&format!(
                            "\n    ... and {} more dependencies",
                            dep_on_count - max_impact
                        ));
                    }
                }
                impact_lines.push(line);
            }
        }
    }

    if !preview && !entries.is_empty() {
        notify_subscribed(
            subscribed,
            notifier,
            &["ozymem://summary", "ozymem://recent-lessons"],
        );
    }

    let body = if entries.is_empty() {
        format!(
            "No function-level changes detected in {} files (diff {from}..{to})",
            files.len()
        )
    } else {
        let header = if preview {
            format!(
                "[PREVIEW] — {} entries from {} files (diff {from}..{to}):\n",
                entries.len(),
                parsed_files.len()
            )
        } else {
            format!(
                "Recorded {} entries from {} files (diff {from}..{to}):\n",
                entries.len(),
                parsed_files.len()
            )
        };
        let mut body = header;
        for e in &entries {
            body.push_str(e);
            body.push('\n');
        }
        if !impact_lines.is_empty() {
            body.push_str("\n═══ Impact (graph neighbors) ═══\n");
            for l in &impact_lines {
                body.push_str(l);
                body.push('\n');
            }
            body.push_str("[Notice] Run analyze_impact for full transitive depth.\n");
        }
        body
    };

    Ok(ToolCallResult {
        content: vec![ContentBlock {
            kind: "text",
            text: body,
        }],
        is_error: None,
    })
}

pub(crate) async fn handle_export_memory_notes(
    backend: &GraphBackend,
    tool_call: &mcp_common::ToolCallParams,
) -> anyhow::Result<ToolCallResult> {
    let commit_ref = tool_call.arguments.get("commit_ref").and_then(Value::as_str);
    let note_ref = tool_call.arguments.get("note_ref").and_then(Value::as_str);
    let rules = tool_call
        .arguments
        .get("procedural_rules")
        .and_then(Value::as_array)
        .cloned();

    let (note_oid, payload) = backend.export_to_git_note(commit_ref, note_ref, rules).await?;
    let body = format!(
        "[GIT_NOTES_EXPORTED]\nNote OID: {}\nCommit: {}\nBranch: {}\nLessons: {}\nEngram Contracts: {}\nProcedural Rules: {}\nTimestamp: {}\n",
        note_oid,
        payload.commit_hash,
        payload.branch.as_deref().unwrap_or("unknown"),
        payload.lessons.len(),
        payload.engram_contracts.len(),
        payload.procedural_rules.len(),
        payload.timestamp
    );

    Ok(ToolCallResult {
        content: vec![ContentBlock {
            kind: "text",
            text: body,
        }],
        is_error: None,
    })
}

pub(crate) async fn handle_import_memory_notes(
    backend: &GraphBackend,
    tool_call: &mcp_common::ToolCallParams,
) -> anyhow::Result<ToolCallResult> {
    let commit_ref = tool_call.arguments.get("commit_ref").and_then(Value::as_str);
    let note_ref = tool_call.arguments.get("note_ref").and_then(Value::as_str);

    let payload = backend.import_from_git_note(commit_ref, note_ref).await?;
    let body = format!(
        "[GIT_NOTES_IMPORTED]\nCommit: {}\nBranch: {}\nMerged Lessons: {}\nMerged Engram Contracts: {}\nRestored Procedural Rules: {}\nTimestamp: {}\n",
        payload.commit_hash,
        payload.branch.as_deref().unwrap_or("unknown"),
        payload.lessons.len(),
        payload.engram_contracts.len(),
        payload.procedural_rules.len(),
        payload.timestamp
    );

    Ok(ToolCallResult {
        content: vec![ContentBlock {
            kind: "text",
            text: body,
        }],
        is_error: None,
    })
}

pub async fn handle_git_tool(
    id: Value,
    backend: &GraphBackend,
    tool_call: &mcp_common::ToolCallParams,
    notifier: Option<&Notifier>,
    subscribed: Option<&Arc<Mutex<HashSet<String>>>>,
) -> anyhow::Result<Option<mcp_common::JsonRpcResponse>> {
    if !matches!(
        tool_call.name.as_str(),
        "learn_from_changes"
            | "ozy_export_memory_notes"
            | "ozy_import_memory_notes"
            | "git_recent_changes"
            | "git_diff_summary"
            | "git_diff_file"
            | "git_blame_line"
            | "recent_changes_with_impact"
    ) {
        return Ok(None);
    }
    let proj = backend.project_path().unwrap_or_else(|| ".".to_string());
    let git_res = GitBackend::open(Path::new(&proj));
    let res = match tool_call.name.as_str() {
        "learn_from_changes" => handle_learn_from_changes(backend, tool_call, notifier, subscribed).await?,
        "ozy_export_memory_notes" => handle_export_memory_notes(backend, tool_call).await?,
        "ozy_import_memory_notes" => handle_import_memory_notes(backend, tool_call).await?,
        _ => {
            let git = match git_res {
                Ok(g) => g,
                Err(msg) => return Ok(Some(error_response(id, -32603, &msg))),
            };
            match tool_call.name.as_str() {
        "learn_from_changes" => handle_learn_from_changes(backend, tool_call, notifier, subscribed).await?,
        "ozy_export_memory_notes" => handle_export_memory_notes(backend, tool_call).await?,
        "ozy_import_memory_notes" => handle_import_memory_notes(backend, tool_call).await?,
                    "git_recent_changes" => {
                        let limit = tool_call
                            .arguments
                            .get("limit")
                            .and_then(Value::as_u64)
                            .unwrap_or(10) as usize;
                        match git.recent_changes(limit) {
                            Ok(changes) => {
                                let mut body = format!("Recent commits (up to {limit}):\n");
                                for c in &changes {
                                    let merge_tag = if c.is_merge { " [MERGE]" } else { "" };
                                    body.push_str(&format!(
                                        "\n{} {}{}\n  Author: {}\n  Date:   {}\n",
                                        &c.hash[..7],
                                        c.message,
                                        merge_tag,
                                        c.author,
                                        c.timestamp,
                                    ));
                                    if !c.files.is_empty() {
                                        body.push_str("  Files:\n");
                                        for f in &c.files {
                                            body.push_str(&format!("    - {f}\n"));
                                        }
                                    }
                                }
                                ToolCallResult {
                                    content: vec![ContentBlock {
                                        kind: "text",
                                        text: body,
                                    }],
                                    is_error: None,
                                }
                            }
                            Err(msg) => {
                                return Ok(Some(error_response(id, -32603, &msg)));
                            }
                        }
                    }
                    "git_diff_summary" => {
                        let from = match tool_call.arguments.get("from").and_then(Value::as_str) {
                            Some(f) => f,
                            None => {
                                return Ok(Some(error_response(
                                    id,
                                    -32602,
                                    "Missing required parameter: from",
                                )));
                            }
                        };
                        let to = tool_call.arguments.get("to").and_then(Value::as_str);
                        match git.diff_summary(from, to) {
                            Ok(summary) => {
                                let mut body = format!(
                                    "Diff summary ({} files changed, {} insertions, {} deletions)\n",
                                    summary.files_changed, summary.insertions, summary.deletions,
                                );
                                for f in &summary.files {
                                    body.push_str(&format!("\n  {}  {}", f.status, f.path));
                                }
                                ToolCallResult {
                                    content: vec![ContentBlock {
                                        kind: "text",
                                        text: body,
                                    }],
                                    is_error: None,
                                }
                            }
                            Err(msg) => {
                                return Ok(Some(error_response(id, -32603, &msg)));
                            }
                        }
                    }
                    "git_diff_file" => {
                        let from = match tool_call.arguments.get("from").and_then(Value::as_str) {
                            Some(f) => f,
                            None => {
                                return Ok(Some(error_response(
                                    id,
                                    -32602,
                                    "Missing required parameter: from",
                                )));
                            }
                        };
                        let to = tool_call.arguments.get("to").and_then(Value::as_str);
                        let file_path =
                            match tool_call.arguments.get("file_path").and_then(Value::as_str) {
                                Some(p) => p,
                                None => {
                                    return Ok(Some(error_response(
                                        id,
                                        -32602,
                                        "Missing required parameter: file_path",
                                    )));
                                }
                            };
                        match git.diff_file(from, to, file_path) {
                            Ok(diff) => ToolCallResult {
                                content: vec![ContentBlock {
                                    kind: "text",
                                    text: diff,
                                }],
                                is_error: None,
                            },
                            Err(msg) => return Ok(Some(error_response(id, -32603, &msg))),
                        }
                    }
                    "git_blame_line" => {
                        let file_path =
                            match tool_call.arguments.get("file_path").and_then(Value::as_str) {
                                Some(p) => p,
                                None => {
                                    return Ok(Some(error_response(
                                        id,
                                        -32602,
                                        "Missing required parameter: file_path",
                                    )));
                                }
                            };
                        let start_line = tool_call
                            .arguments
                            .get("start_line")
                            .and_then(Value::as_u64)
                            .map(|v| v as usize);
                        let end_line = tool_call
                            .arguments
                            .get("end_line")
                            .and_then(Value::as_u64)
                            .map(|v| v as usize);
                        match git.blame_file(file_path, start_line, end_line) {
                            Ok(entries) => {
                                if entries.is_empty() {
                                    ToolCallResult {
                                        content: vec![ContentBlock { kind: "text", text: "No blame information found for the specified range.".to_string() }],
                                        is_error: None,
                                    }
                                } else {
                                    let mut body = format!(
                                        "Blame for {} ({} entries):\n",
                                        file_path,
                                        entries.len()
                                    );
                                    for e in &entries {
                                        body.push_str(&format!(
                                            "\n  {}..{}  {}  {}  {}",
                                            e.start_line,
                                            e.end_line,
                                            &e.hash[..7],
                                            e.author,
                                            e.timestamp
                                        ));
                                    }
                                    ToolCallResult {
                                        content: vec![ContentBlock {
                                            kind: "text",
                                            text: body,
                                        }],
                                        is_error: None,
                                    }
                                }
                            }
                            Err(msg) => return Ok(Some(error_response(id, -32603, &msg))),
                        }
                    }
                    "recent_changes_with_impact" => {
                        let limit = tool_call
                            .arguments
                            .get("limit")
                            .and_then(Value::as_u64)
                            .unwrap_or(5) as usize;
                        let depth = tool_call
                            .arguments
                            .get("depth")
                            .and_then(Value::as_u64)
                            .unwrap_or(1) as u32;

                        let changes = match git.recent_changes(limit) {
                            Ok(c) => c,
                            Err(msg) => return Ok(Some(error_response(id, -32603, &msg))),
                        };

                        // Collect unique touched file paths, resolve to absolute
                        let repo_root = git.repo_path().to_path_buf();
                        let mut touched_set: Vec<String> = Vec::new();
                        for c in &changes {
                            for f in &c.files {
                                let abs = repo_root.join(f).to_string_lossy().to_string();
                                if !touched_set.contains(&abs) {
                                    touched_set.push(abs);
                                }
                            }
                        }

                        // Single lock: batch analyze_impact for all touched files
                        let impacts: std::collections::HashMap<
                            String,
                            Vec<ozymem_core::graph_backend::ImpactEntry>,
                        > = {
                            
                            let gb = backend;
                            gb.reload_if_stale();
                            let touched_set: std::collections::HashSet<String> =
                                touched_set.into_iter().collect();
                            touched_set
                                .iter()
                                .map(|f| {
                                    let impact = gb.analyze_impact(f, depth);
                                    (f.clone(), impact)
                                })
                                .collect()
                        };

                        // Combine: build response text
                        let mut body = format!("Recent commits with impact (depth {depth}):\n");
                        for c in &changes {
                            body.push_str(&format!(
                                "\n{} {}\n  Author: {}\n  Date:   {}\n",
                                &c.hash[..7],
                                c.message,
                                c.author,
                                c.timestamp,
                            ));
                            for f in &c.files {
                                let abs = repo_root.join(f).to_string_lossy().to_string();
                                let exists = std::path::Path::new(&abs).exists();
                                let impact = impacts.get(&abs).cloned().unwrap_or_default();
                                let status = if exists { "" } else { " [DELETED]" };
                                body.push_str(&format!("  - {f}{status}\n"));
                                if !impact.is_empty() {
                                    for imp in &impact {
                                        body.push_str(&format!("      → {}\n", imp.file_path));
                                    }
                                } else if exists {
                                    body.push_str("      (no impact recorded)\n");
                                }
                            }
                        }

                        ToolCallResult {
                            content: vec![ContentBlock {
                                kind: "text",
                                text: body,
                            }],
                            is_error: None,
                        }
                    }
                _ => unreachable!(),
            }
        }
    };
    Ok(Some(ok_response(id, serde_json::to_value(res)?)))
}
