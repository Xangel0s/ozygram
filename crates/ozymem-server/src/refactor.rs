use anyhow::Result;
use ozymem_core::graph_backend::GraphBackend;
use ozymem_core::mcp_common::{ContentBlock, ToolCallParams, ToolCallResult};
use ozymem_core::McpBackend;
use serde_json::{json, Value};
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

    // 2. Spec-Driven Development (SDD) Boundary Audit
    let ws_root = backend.project_path().unwrap_or_else(|| ".".to_string());
    let target_mod = ozymem_core::graph_backend::extract_module_id(&resolved, &ws_root);
    let mut external_dependents = Vec::new();
    for entry in &impact_data {
        let dep_mod = ozymem_core::graph_backend::extract_module_id(&entry.file_path, &ws_root);
        if dep_mod != target_mod && dep_mod != "root" && !entry.file_path.contains("test") {
            external_dependents.push(entry.file_path.clone());
        }
    }

    out.push_str("--- 2. SPEC-DRIVEN DEVELOPMENT (SDD) BOUNDARY AUDIT ---\n");
    if external_dependents.is_empty() {
        out.push_str(&format!(
            "Status: [VERIFIED]\n\
             Target Module: {}\n\
             Boundary Check: No external module boundaries breached. All {} dependent file(s) are internal to module or test runners.\n\n",
            target_mod,
            impact_data.len()
        ));
    } else {
        out.push_str(&format!(
            "Status: [SPEC_BOUNDARY_WARN]\n\
             Target Module: {}\n\
             Warning: Proposed change impacts {} file(s) outside the declared module boundary:\n",
            target_mod,
            external_dependents.len()
        ));
        for ext in external_dependents.iter().take(8) {
            out.push_str(&format!("  • {}\n", ext));
        }
        if external_dependents.len() > 8 {
            out.push_str(&format!("  ... and {} more\n", external_dependents.len() - 8));
        }
        out.push_str("Recommendation: Verify module contracts in SPEC-<module>.md before applying to disk.\n\n");
    }

    let mut has_error = false;

    if let (Some(target_code), Some(replacement)) = (target_code_opt, replacement_opt) {
        let patch_res = ozymem_parser::apply_fuzzy_ast_patch(
            file_path,
            &source,
            target_code,
            replacement,
            dry_run,
        )?;

        out.push_str("--- 3. AST PATCH SIMULATION (IN-RAM) ---\n");
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
        out.push_str("--- 3. SEMANTIC SYMBOL SIMULATION (IN-RAM) ---\n");
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

    // Fast-path: bypass reload_if_stale for routing queries to guarantee sub-20ms response
    if action != "route" && action != "suggest" {
        backend.reload_if_stale();
    }

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
        "route" | "suggest" => {
            let query = tool_call
                .arguments
                .get("query")
                .or_else(|| tool_call.arguments.get("goal"))
                .or_else(|| tool_call.arguments.get("task"))
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("missing 'query' or 'goal' for action='route'"))?;

            let project_path = backend.project_path().unwrap_or_else(|| ".".to_string());
            let q_lower = query.to_lowercase();
            let is_refactor = q_lower.contains("refactor")
                || q_lower.contains("fix")
                || q_lower.contains("modify")
                || q_lower.contains("change")
                || q_lower.contains("update")
                || q_lower.contains("optim");

            // Extract candidate keywords
            let words: Vec<&str> = query
                .split(|c: char| !c.is_alphanumeric() && c != '_')
                .filter(|w| w.len() > 3 && !["refactor", "modify", "change", "update", "function", "class", "module", "please", "help", "this", "that", "from", "with"].contains(&w.to_lowercase().as_str()))
                .collect();

            let mut matched_symbol = None;
            let mut matched_file = None;

            // 1. Ultra-fast In-RAM Token Index Lookup (< 0.1ms, zero SQLite locks)
            let ram_candidates = backend.find_files_by_tokens_ram(&words, 5);
            if let Some(first) = ram_candidates.first() {
                matched_file = Some(first.clone());
                for word in &words {
                    if first.to_lowercase().contains(&word.to_lowercase()) {
                        matched_symbol = Some(word.to_string());
                        break;
                    }
                }
            }

            // 2. Fallback to symbol search if RAM token index didn't identify the file
            if matched_file.is_none() {
                for word in &words {
                    if let Ok(matches) = backend.find_symbol(word, &project_path).await {
                        if let Some(first) = matches.first() {
                            let file_part = first.split(" [").next().unwrap_or(first).trim();
                            matched_symbol = Some(word.to_string());
                            matched_file = Some(file_part.to_string());
                            break;
                        }
                    }
                }
            }

            // 3. Fallback to SQLite indexed file paths if still not matched
            if matched_file.is_none() {
                for word in &words {
                    if let Some(resolved) = backend.resolve_target_path(word) {
                        matched_file = Some(resolved);
                        break;
                    }
                    if let Ok(found) = backend.find_files_by_keyword(word, 3) {
                        if let Some(first) = found.first() {
                            matched_file = Some(first.clone());
                            break;
                        }
                    }
                }
            }

            let mut out = String::new();
            out.push_str("=== [OZY_BRAIN: COGNITIVE TOOL ROUTER] ===\n");
            out.push_str(&format!("User Objective: \"{}\"\n", query));
            out.push_str("Status: OPTIMAL_WORKFLOW_RESOLVED\n\n");

            out.push_str("[DIRECTIVE: ANTI_PATTERNS_VETO]\n");
            out.push_str("⛔ DO NOT use grep, findstr, or shell searches.\n");
            out.push_str("⛔ DO NOT read files line-by-line (chunked slices).\n");
            out.push_str("✅ Execute the preloaded tool call below.\n\n");

            if let Some(file) = &matched_file {
                let (effective, resolved) = crate::symbols::resolve_effective_path(backend, file);
                let target_path = if effective.exists() { resolved } else { file.to_string() };
                let impact = backend.analyze_impact(&target_path, 2);
                let dependents_count = impact.len();

                out.push_str(&format!("Target Resolved: {}\n", target_path));
                if let Some(sym) = &matched_symbol {
                    out.push_str(&format!("Symbol Identified: {}\n", sym));
                }
                out.push_str(&format!("Blast Radius: {} dependent file(s)\n\n", dependents_count));

                if is_refactor {
                    out.push_str("[RECOMMENDED_NEXT_TOOL]: ozy_refactor (In-RAM Simulation)\n");
                    out.push_str("```json\n");
                    let mut preloaded_args = json!({
                        "file_path": target_path,
                        "action": "simulate"
                    });
                    if let Some(sym) = &matched_symbol {
                        preloaded_args["target_symbol"] = json!(sym);
                    }
                    out.push_str(&serde_json::to_string_pretty(&json!({
                        "tool": "ozy_refactor",
                        "arguments": preloaded_args,
                        "rationale": "Performs in-RAM Copy-On-Write patch simulation and calculates blast radius with 0 disk writes."
                    }))?);
                    out.push_str("\n```\n");
                } else {
                    out.push_str("[RECOMMENDED_NEXT_TOOL]: ozy_explore (Contracts)\n");
                    out.push_str("```json\n");
                    out.push_str(&serde_json::to_string_pretty(&json!({
                        "tool": "ozy_explore",
                        "arguments": {
                            "file_path": target_path,
                            "action": "contracts"
                        },
                        "rationale": "Extracts structural AST signatures saving 70-95% tokens compared to raw file reads."
                    }))?);
                    out.push_str("\n```\n");
                }
            } else {
                out.push_str("[RECOMMENDED_NEXT_TOOL]: ozy_explore (Contracts / Search)\n");
                out.push_str("```json\n");
                out.push_str(&serde_json::to_string_pretty(&json!({
                    "tool": "ozy_explore",
                    "arguments": {
                        "action": "contracts",
                        "file_path": "<target_file>"
                    }
                }))?);
                out.push_str("\n```\n");
            }

            out
        }
        "search" => {
            let query = tool_call
                .arguments
                .get("query")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("missing 'query'"))?;
            let max_matches = tool_call
                .arguments
                .get("max_matches")
                .or_else(|| tool_call.arguments.get("limit"))
                .and_then(Value::as_u64)
                .unwrap_or(30) as usize;
            let path_prefix = tool_call
                .arguments
                .get("file_path")
                .or_else(|| tool_call.arguments.get("path"))
                .and_then(Value::as_str);

            let matches = backend.search_literal(query, path_prefix, false, false, 5000, max_matches, 2)?;
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
        "view" | "read" | "slice" => {
            let file_path = tool_call
                .arguments
                .get("file_path")
                .or_else(|| tool_call.arguments.get("path"))
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("missing 'file_path' for action='view'"))?;

            let (effective, resolved) = crate::symbols::resolve_effective_path(backend, file_path);
            let target_path = if effective.exists() { &effective } else { std::path::Path::new(file_path) };

            if !target_path.exists() {
                format!("File not found on disk: {}", file_path)
            } else {
                let content = std::fs::read_to_string(target_path)
                    .unwrap_or_else(|e| format!("[ERROR: Failed to read file: {e}]"));
                let lines: Vec<&str> = content.lines().collect();
                let total_lines = lines.len();

                let (mut start, mut end) = (
                    tool_call.arguments.get("start_line").and_then(Value::as_u64).unwrap_or(1) as usize,
                    tool_call.arguments.get("end_line").and_then(Value::as_u64).unwrap_or(total_lines as u64) as usize,
                );

                let symbol_opt = tool_call
                    .arguments
                    .get("symbol")
                    .or_else(|| tool_call.arguments.get("symbol_name"))
                    .and_then(Value::as_str);

                if let Some(sym) = symbol_opt {
                    if let Ok(Some(ctx)) = backend.get_file_context(file_path).await {
                        if let Some(f) = ctx.functions.iter().find(|func| func.name.eq_ignore_ascii_case(sym)) {
                            start = (f.start_line as usize).max(1);
                            end = (f.end_line as usize).min(total_lines);
                        }
                    }
                }

                start = start.max(1);
                end = end.min(total_lines).max(start);

                let impact = backend.analyze_impact(&resolved, 2);
                let dependents_count = impact.len();

                let mut out = format!(
                    "=== [OZY_VIEW: {}] ===\nLines: {}-{} of {} | Blast Radius: {} dependent file(s)\n--------------------------------------------------\n",
                    resolved, start, end, total_lines, dependents_count
                );

                for (idx, line) in lines[start - 1..end].iter().enumerate() {
                    out.push_str(&format!("{:4} | {}\n", start + idx, line));
                }
                out.push_str("--------------------------------------------------\n");
                out.push_str("[COGNITIVE_DIRECTIVE]: To edit or refactor this code, call ozy_refactor(file_path=\"...\") with action=\"simulate\" directly in volatile RAM.");
                out
            }
        }
        "capability_map" => {
            let initiative = tool_call
                .arguments
                .get("initiative")
                .or_else(|| tool_call.arguments.get("query"))
                .and_then(Value::as_str);
            let res = backend.generate_capability_map(initiative);
            res.markdown_table
        }
        "spec" => {
            let file_path_opt = tool_call
                .arguments
                .get("file_path")
                .or_else(|| tool_call.arguments.get("path"))
                .and_then(Value::as_str);
            let objective_opt = tool_call
                .arguments
                .get("objective")
                .or_else(|| tool_call.arguments.get("query"))
                .or_else(|| tool_call.arguments.get("goal"))
                .and_then(Value::as_str);
            let module_opt = tool_call
                .arguments
                .get("module")
                .and_then(Value::as_str);

            generate_sdd_spec(backend, file_path_opt, objective_opt, module_opt).await?
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
            let mut ctx = backend.get_file_context(file_path).await?;
            if ctx.is_none() {
                let (effective, _) = crate::symbols::resolve_effective_path(backend, file_path);
                if effective.exists() {
                    let root_buf = backend.project_path().map(std::path::PathBuf::from).unwrap_or_else(|| std::path::PathBuf::from("."));
                    let _ = backend.index_file_delta(&effective, &root_buf);
                    ctx = backend.get_file_context(file_path).await.ok().flatten();
                }
            }
            let history = backend.get_historical_engram_solutions(file_path).await.unwrap_or_default();
            let neighbors = backend.get_graph_neighbors(file_path).await.ok();
            let last_git = get_last_commit(file_path).await;
            let mut formatted = format_file_context_enriched(
                ctx.as_ref(),
                file_path,
                &history,
                neighbors.as_ref(),
                last_git.as_deref(),
            );
            formatted.push_str("\n\n[COGNITIVE_DIRECTIVE]: To edit or refactor this file, DO NOT use read_file line-by-line. Call ozy_refactor(file_path=\"...\") with action=\"simulate\" directly in volatile RAM.");
            formatted
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

/// Generates a living Spec-Driven Development (SDD) specification document for a module.
///
/// Complies with the official 6-area SDD specification format (Objective, Tech Stack,
/// Commands, Project Structure, Code Style & AST Contracts, Boundaries, Success Criteria)
/// populated directly from the codebase's AST graph, dependencies, and build configuration.
async fn generate_sdd_spec(
    backend: &GraphBackend,
    file_path_opt: Option<&str>,
    objective_opt: Option<&str>,
    module_opt: Option<&str>,
) -> anyhow::Result<String> {
    let ws_root = backend.project_path().unwrap_or_else(|| ".".to_string());
    let target_file = file_path_opt.unwrap_or(".");
    let (effective, resolved) = crate::symbols::resolve_effective_path(backend, target_file);

    let mod_id = module_opt
        .map(|s| s.to_string())
        .unwrap_or_else(|| ozymem_core::graph_backend::extract_module_id(&resolved, &ws_root));

    let objective = objective_opt
        .unwrap_or("Implement core feature capabilities according to requirements.");

    // Detect tech stack and commands from project root
    let root_path = std::path::Path::new(&ws_root);
    let mut detected_stack = Vec::new();
    let mut build_cmd = "cargo build".to_string();
    let mut test_cmd = "cargo test".to_string();
    let mut lint_cmd = "cargo clippy".to_string();
    let mut dev_cmd = "cargo run".to_string();

    if root_path.join("Cargo.toml").exists() {
        detected_stack.push("Rust (Cargo workspace, Tokio, Serde)".to_string());
    } else if root_path.join("pyproject.toml").exists() || root_path.join("requirements.txt").exists() {
        detected_stack.push("Python (FastAPI / Pydantic / SQLAlchemy / pytest)".to_string());
        build_cmd = "python -m compileall .".to_string();
        test_cmd = "pytest -v".to_string();
        lint_cmd = "ruff check .".to_string();
        dev_cmd = "python -m uvicorn app.main:app --reload".to_string();
    } else if root_path.join("package.json").exists() {
        detected_stack.push("TypeScript / JavaScript (Node.js, Vite/React)".to_string());
        build_cmd = "npm run build".to_string();
        test_cmd = "npm test".to_string();
        lint_cmd = "npm run lint".to_string();
        dev_cmd = "npm run dev".to_string();
    } else {
        detected_stack.push("Multi-language repository".to_string());
    }

    // AST contracts from target file
    let mut ast_snippet = String::new();
    let mut file_lines = 0;
    if effective.exists() {
        if let Ok(source) = std::fs::read_to_string(&effective) {
            file_lines = source.lines().count();
        }
        if let Ok(Some(ctx)) = backend.get_file_context(&resolved).await {
            if !ctx.functions.is_empty() {
                for f in &ctx.functions {
                    ast_snippet.push_str(&format!("  • {} [{}] (lines {}-{})\n", f.name, f.kind, f.start_line, f.end_line));
                }
            }
        }
    }

    if ast_snippet.is_empty() {
        ast_snippet = "  • (No existing AST functions indexed; new module interface to be specified)\n".to_string();
    }

    // Blast Radius & Impact
    let impact = backend.analyze_impact(&resolved, 2);
    let mut external_allowed = Vec::new();
    for dep in &impact {
        let dep_mod = ozymem_core::graph_backend::extract_module_id(&dep.file_path, &ws_root);
        if dep_mod != mod_id && !dep.file_path.contains("test") {
            external_allowed.push(dep.file_path.clone());
        }
    }

    // Build the canonical 6-area SDD Spec
    let mut spec = format!("# Spec: {}\n\n", mod_id);
    spec.push_str("## Objective\n");
    spec.push_str(&format!("{}\n\n", objective));

    spec.push_str("## Tech Stack\n");
    for stack in &detected_stack {
        spec.push_str(&format!("- {}\n", stack));
    }
    spec.push_str("\n");

    spec.push_str("## Commands\n");
    spec.push_str(&format!("- Build: `{}`\n", build_cmd));
    spec.push_str(&format!("- Test: `{}`\n", test_cmd));
    spec.push_str(&format!("- Lint: `{}`\n", lint_cmd));
    spec.push_str(&format!("- Dev: `{}`\n\n", dev_cmd));

    spec.push_str("## Project Structure\n");
    spec.push_str(&format!("- Target Module: `{}`\n", mod_id));
    if effective.exists() {
        spec.push_str(&format!("- Primary File: `{}` ({} lines)\n", resolved, file_lines));
    }
    spec.push_str("\n");

    spec.push_str("## Code Style & AST Contracts\n");
    spec.push_str("Existing AST signatures and contract entrypoints:\n```\n");
    spec.push_str(&ast_snippet);
    spec.push_str("```\n\n");

    spec.push_str("## Testing Strategy\n");
    spec.push_str("- Unit test coverage target: minimum 80% for new and modified functions.\n");
    spec.push_str(&format!("- Verification command: `{}`\n", test_cmd));
    spec.push_str("- Test edge cases, validation errors, and nullability.\n\n");

    spec.push_str("## Boundaries\n");
    spec.push_str("- **Always**: Run automated tests before commit, follow naming conventions, validate external inputs.\n");
    spec.push_str("- **Ask first**: Database schema changes, modifying public API contracts, adding third-party dependencies.\n");
    spec.push_str("- **Never**: Commit secrets or credentials, bypass failing test assertions, modify vendor files.\n");
    spec.push_str("- **Allowed External Dependents (Blast Radius)**:\n");
    if external_allowed.is_empty() {
        spec.push_str("  • None (Self-contained module)\n");
    } else {
        for ext in external_allowed.iter().take(10) {
            spec.push_str(&format!("  • `{}`\n", ext));
        }
        if external_allowed.len() > 10 {
            spec.push_str(&format!("  • ... and {} more dependent files\n", external_allowed.len() - 10));
        }
    }
    spec.push_str("\n");

    spec.push_str("## Success Criteria\n");
    spec.push_str(&format!("- [ ] `{}` requirements implemented according to objective.\n", mod_id));
    spec.push_str(&format!("- [ ] Automated verification passes via `{}`.\n", test_cmd));
    spec.push_str("- [ ] Zero regressions in dependent callers within the Blast Radius.\n");
    spec.push_str("- [ ] In-RAM refactor simulation completed without boundary warnings.\n\n");

    spec.push_str("## Open Questions\n");
    spec.push_str("- None currently blocking. Update this living document if design decisions evolve.\n");

    Ok(spec)
}

