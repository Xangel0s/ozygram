use anyhow::Result;
use ozymem_core::graph_backend::GraphBackend;
use ozymem_core::mcp_common::{ContentBlock, ToolCallParams, ToolCallResult};
use ozymem_core::McpBackend;
use serde_json::Value;
use std::fs;
use crate::formatters::{compact_tool_output, format_file_context_enriched, format_impact, get_last_commit};

pub async fn handle_refactor(
    backend: &GraphBackend,
    tool_call: &ToolCallParams,
) -> Result<ToolCallResult> {
    backend.reload_if_stale();

    let file_path = tool_call
        .arguments
        .get("file_path")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("missing 'file_path'"))?;

    let action = tool_call
        .arguments
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or("simulate");

    let max_hops = tool_call
        .arguments
        .get("max_hops")
        .and_then(Value::as_u64)
        .unwrap_or(2) as u32;

    let (effective, resolved) = crate::symbols::resolve_effective_path(backend, file_path);

    if !effective.exists() {
        return Ok(ToolCallResult {
            content: vec![ContentBlock {
                kind: "text",
                text: format!("[ALERT: FILE_NOT_FOUND] Target file '{}' does not exist.", file_path),
            }],
            is_error: Some(true),
        });
    }

    let source = fs::read_to_string(&effective)?;
    let is_apply = action == "apply";
    let dry_run = !is_apply;

    // 1. Calculate Real-Time Blast Radius & Graph Impact
    let impact_data = backend.analyze_impact(&resolved, max_hops);
    let impact_str = format_impact(&impact_data, &resolved);

    // 2. Perform AST Patch Simulation or Application
    let target_code_opt = tool_call
        .arguments
        .get("target_code")
        .or_else(|| tool_call.arguments.get("target"))
        .and_then(Value::as_str);

    let replacement_opt = tool_call
        .arguments
        .get("replacement")
        .or_else(|| tool_call.arguments.get("new_code"))
        .and_then(Value::as_str);

    let target_symbol_opt = tool_call
        .arguments
        .get("target_symbol")
        .or_else(|| tool_call.arguments.get("symbol"))
        .and_then(Value::as_str);

    let candidate_code_opt = tool_call
        .arguments
        .get("candidate_code")
        .and_then(Value::as_str);

    let mut out = String::new();
    let mode_str = if is_apply {
        "DISK WRITE / APPLIED"
    } else {
        "100% COPY-ON-WRITE RAM-ONLY SIMULATION"
    };

    out.push_str(&format!(
        "=== [OZY_REFACTOR: ONE-TURN ATOMIC REPORT] ===\n\
         Target File: {}\n\
         Resolved: {}\n\
         Mode: {}\n\n",
        file_path, resolved, mode_str
    ));

    out.push_str("--- 1. TOPOLOGICAL BLAST RADIUS ---\n");
    out.push_str(&impact_str);
    out.push_str("\n\n");

    let mut has_error = false;

    if let (Some(target_code), Some(replacement)) = (target_code_opt, replacement_opt) {
        let patch_res = ozymem_parser::apply_fuzzy_ast_patch(
            file_path,
            &source,
            target_code,
            replacement,
            dry_run,
        )?;

        out.push_str("--- 2. AST PATCH SIMULATION (IN-RAM) ---\n");
        if patch_res.success {
            out.push_str(&format!(
                "Status: [SUCCESS]\n\
                 Matched by: {}\n\
                 Lines: {}-{} (Delta: {} lines)\n\n\
                 Diff Preview:\n{}\n",
                patch_res.matched_by,
                patch_res.start_line,
                patch_res.end_line,
                patch_res.lines_diff,
                patch_res.preview_diff
            ));

            if is_apply {
                if let Some(ref patched_str) = patch_res.patched_content {
                    fs::write(&effective, patched_str)?;
                    out.push_str("\n[DISK STATUS]: Applied and written to disk successfully.\n");
                }
            } else {
                out.push_str("\n[DISK STATUS]: UNTOUCHED (Physical file preserved in pristine state).\n");
                out.push_str("[VERDICT]: [SAFE_TO_APPLY] To persist to disk, re-run with action=\"apply\".\n");
            }
        } else {
            has_error = true;
            out.push_str(&format!(
                "Status: [FAILED]\nError: {}\n",
                patch_res.error.as_deref().unwrap_or("Fuzzy AST target code not found in file")
            ));
        }
    } else if let (Some(target_symbol), Some(candidate_code)) = (target_symbol_opt, candidate_code_opt) {
        let sim = backend.simulate_patch(target_symbol, candidate_code)?;
        out.push_str("--- 2. SEMANTIC SYMBOL SIMULATION (IN-RAM) ---\n");
        out.push_str(&crate::formatters::format_simulation_result(&sim));

        if is_apply {
            let replace_args = serde_json::json!({
                "file_path": file_path,
                "symbol_name": target_symbol,
                "new_code": candidate_code,
                "dry_run": false
            });
            let tool_params = ToolCallParams {
                name: "replace_symbol".to_string(),
                arguments: replace_args.as_object().cloned().unwrap_or_default(),
            };
            let rep_res = crate::symbols::handle_replace_symbol(backend, &tool_params)?;
            out.push_str("\n\n--- 3. APPLICATION RESULT ---\n");
            if let Some(first) = rep_res.content.first() {
                out.push_str(&first.text);
            }
        }
    } else {
        return Ok(ToolCallResult {
            content: vec![ContentBlock {
                kind: "text",
                text: "[ALERT: MISSING_CODE] Provide 'target_code' and 'replacement', or 'target_symbol' and 'candidate_code'.".to_string(),
            }],
            is_error: Some(true),
        });
    }

    let mut content_blocks = vec![ContentBlock {
        kind: "text",
        text: out,
    }];

    let project_dir = backend.project_path();
    compact_tool_output(&mut content_blocks, project_dir.as_deref());

    Ok(ToolCallResult {
        content: content_blocks,
        is_error: if has_error { Some(true) } else { None },
    })
}

pub async fn handle_explore(
    backend: &GraphBackend,
    tool_call: &ToolCallParams,
) -> Result<ToolCallResult> {
    backend.reload_if_stale();

    let action = tool_call
        .arguments
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or_else(|| {
            if tool_call.arguments.contains_key("file_path") {
                "contracts"
            } else if tool_call.arguments.contains_key("symbol") || tool_call.arguments.contains_key("symbol_name") {
                "symbol"
            } else {
                "summary"
            }
        });

    let text_output = match action {
        "impact" => {
            let file_path = tool_call
                .arguments
                .get("file_path")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("missing 'file_path' for action='impact'"))?;
            let depth = tool_call
                .arguments
                .get("depth")
                .or_else(|| tool_call.arguments.get("max_hops"))
                .and_then(Value::as_u64)
                .unwrap_or(2) as u32;
            let (effective, resolved) = crate::symbols::resolve_effective_path(backend, file_path);
            let target_path = if effective.exists() { resolved } else { file_path.to_string() };
            let impact = backend.analyze_impact(&target_path, depth);
            format_impact(&impact, &target_path)
        }
        "symbol" => {
            let symbol_name = tool_call
                .arguments
                .get("symbol_name")
                .or_else(|| tool_call.arguments.get("symbol"))
                .or_else(|| tool_call.arguments.get("query"))
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("missing 'symbol_name'"))?;
            let project_path = backend.project_path().unwrap_or_else(|| ".".to_string());
            let matches = backend.find_symbol(symbol_name, &project_path).await?;
            if matches.is_empty() {
                format!("Symbol '{}' not found in indexed codebase.", symbol_name)
            } else {
                let mut out = format!("Found {} match(es) for symbol '{}':\n", matches.len(), symbol_name);
                for m in matches {
                    out.push_str(&format!("  • {}\n", m));
                }
                out
            }
        }
        "tree" => {
            let depth = tool_call
                .arguments
                .get("depth")
                .or_else(|| tool_call.arguments.get("max_depth"))
                .and_then(Value::as_u64)
                .unwrap_or(3) as u32;
            let file_path = tool_call
                .arguments
                .get("file_path")
                .and_then(Value::as_str)
                .unwrap_or(".");
            backend.get_file_tree(file_path, depth, "text")?
        }
        "summary" => {
            let summary = backend.get_graph_summary().await?;
            crate::formatters::format_summary(&summary)
        }
        "search" => {
            let query = tool_call
                .arguments
                .get("query")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("missing 'query'"))?;
            let matches = backend.search_literal(query, None, false, false, 20, 2, 2)?;
            if matches.matches.is_empty() {
                format!("No occurrences found for query '{}'.", query)
            } else {
                let mut out = format!("Found {} occurrence(s) for '{}':\n", matches.matches.len(), query);
                for m in matches.matches {
                    out.push_str(&format!("  • {}:{} - {}\n", m.file_path, m.line_number, m.line_content.trim()));
                }
                out
            }
        }
        _ => {
            // Default: "contracts"
            let file_path = tool_call
                .arguments
                .get("file_path")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("missing 'file_path'"))?;
            let _lod = tool_call
                .arguments
                .get("lod")
                .and_then(Value::as_str)
                .unwrap_or("contracts");
            let ctx = backend.get_file_context(file_path).await?;
            let history = backend.get_historical_engram_solutions(file_path).await.unwrap_or_default();
            let neighbors = backend.get_graph_neighbors(file_path).await.ok();
            let last_git = get_last_commit(file_path).await;
            format_file_context_enriched(
                ctx.as_ref(),
                file_path,
                &history,
                neighbors.as_ref(),
                last_git.as_deref(),
            )
        }
    };

    let mut content_blocks = vec![ContentBlock {
        kind: "text",
        text: text_output,
    }];

    let project_dir = backend.project_path();
    compact_tool_output(&mut content_blocks, project_dir.as_deref());

    Ok(ToolCallResult {
        content: content_blocks,
        is_error: None,
    })
}
