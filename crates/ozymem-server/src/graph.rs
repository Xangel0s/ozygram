use ozymem_core::graph_backend::GraphBackend;
use ozymem_core::mcp_common::{self, ContentBlock, ToolCallResult};
use ozymem_core::McpBackend;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use crate::formatters::{format_file_context_enriched, get_last_commit, format_impact, format_summary};
use crate::state::{error_response, ok_response, resolve_project_root, Notifier};

pub async fn handle_graph_tool(
    id: Value,
    backend: &GraphBackend,
    tool_call: &mcp_common::ToolCallParams,
    notifier: Option<&Notifier>,
    _subscribed: Option<&Arc<Mutex<HashSet<String>>>>,
) -> anyhow::Result<Option<mcp_common::JsonRpcResponse>> {
    let res = match tool_call.name.as_str() {
        "ozymem_map_api_routes" => {
                    backend.reload_if_stale();
                    let file_path = tool_call.arguments.get("file_path").and_then(Value::as_str);
                    let routes = backend.map_api_routes(file_path)?;
                    ToolCallResult {
                        content: vec![ContentBlock {
                            kind: "text",
                            text: serde_json::to_string_pretty(&routes)?,
                        }],
                        is_error: None,
                    }
                }
                "detect_code_drift" => {
                    backend.reload_if_stale();
                    let changed_files: Vec<String> = tool_call
                        .arguments
                        .get("changed_files")
                        .and_then(Value::as_array)
                        .map(|arr| {
                            arr.iter()
                                .filter_map(Value::as_str)
                                .map(|s| s.to_string())
                                .collect()
                        })
                        .unwrap_or_default();
                    let diff_content = tool_call
                        .arguments
                        .get("diff_content")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    let alerts = backend.detect_code_drift(&changed_files, diff_content)?;
                    ToolCallResult {
                        content: vec![ContentBlock {
                            kind: "text",
                            text: if alerts.is_empty() {
                                "No code drift or convention violations detected.".to_string()
                            } else {
                                serde_json::to_string_pretty(&alerts)?
                            },
                        }],
                        is_error: None,
                    }
                }
                "rank_memories" => {
                    backend.reload_if_stale();
                    let min_confidence = tool_call
                        .arguments
                        .get("min_confidence")
                        .and_then(Value::as_f64)
                        .unwrap_or(0.5);
                    let report = backend.rank_and_prune_lessons(min_confidence)?;
                    ToolCallResult {
                        content: vec![ContentBlock {
                            kind: "text",
                            text: serde_json::to_string_pretty(&report)?,
                        }],
                        is_error: None,
                    }
                }
                "export_knowledge_bundle" => {
                    backend.reload_if_stale();
                    let proj_name = tool_call
                        .arguments
                        .get("project_name")
                        .and_then(Value::as_str)
                        .unwrap_or("project");
                    let default_path = format!("{}.ozymem", proj_name);
                    let out_path_str = tool_call
                        .arguments
                        .get("output_path")
                        .and_then(Value::as_str)
                        .unwrap_or(&default_path);
                    let summary = ozymem_core::export_bundle(backend, proj_name, std::path::Path::new(out_path_str))?;
                    ToolCallResult {
                        content: vec![ContentBlock {
                            kind: "text",
                            text: serde_json::to_string_pretty(&summary)?,
                        }],
                        is_error: None,
                    }
                }
                "import_knowledge_bundle" => {
                    backend.reload_if_stale();
                    let file_path = tool_call
                        .arguments
                        .get("file_path")
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow::anyhow!("missing required file_path parameter"))?;
                    let merge = tool_call
                        .arguments
                        .get("merge")
                        .and_then(Value::as_bool)
                        .unwrap_or(true);
                    let summary = ozymem_core::import_bundle(backend, std::path::Path::new(file_path), merge).await?;
                    ToolCallResult {
                        content: vec![ContentBlock {
                            kind: "text",
                            text: serde_json::to_string_pretty(&summary)?,
                        }],
                        is_error: None,
                    }
                }
                "cross_repo_query" => {
                    let query = tool_call
                        .arguments
                        .get("query")
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow::anyhow!("missing required query parameter"))?;
                    let limit = tool_call
                        .arguments
                        .get("limit")
                        .and_then(Value::as_u64)
                        .unwrap_or(20) as usize;
                    let project_names: Option<Vec<String>> = tool_call
                        .arguments
                        .get("project_names")
                        .and_then(Value::as_array)
                        .map(|arr| arr.iter().filter_map(Value::as_str).map(|s| s.to_string()).collect());

                    let reg = ozymem_core::registry::ProjectRegistry::open()?;
                    let name_refs: Option<Vec<&str>> = project_names.as_ref().map(|v| v.iter().map(|s| s.as_str()).collect());
                    let results = reg.search_cross_repo_memories(query, name_refs.as_deref(), limit)?;
                    ToolCallResult {
                        content: vec![ContentBlock {
                            kind: "text",
                            text: serde_json::to_string_pretty(&results)?,
                        }],
                        is_error: None,
                    }
                }
                "link_projects" => {
                    let source = tool_call
                        .arguments
                        .get("source")
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow::anyhow!("missing required source parameter"))?;
                    let target = tool_call
                        .arguments
                        .get("target")
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow::anyhow!("missing required target parameter"))?;
                    let relation = tool_call
                        .arguments
                        .get("relation")
                        .and_then(Value::as_str)
                        .unwrap_or("depends_on");

                    let reg = ozymem_core::registry::ProjectRegistry::open()?;
                    reg.link_projects(source, target, relation)?;
                    ToolCallResult {
                        content: vec![ContentBlock {
                            kind: "text",
                            text: format!("Successfully linked '{}' -> '{}' with relation '{}'", source, target, relation),
                        }],
                        is_error: None,
                    }
                }
                "ozymem_python_typecheck" => {
                    let file_path = tool_call.arguments.get("file_path").and_then(Value::as_str);
                    let strict = tool_call.arguments.get("strict").and_then(Value::as_bool).unwrap_or(false);

                    let target = file_path.unwrap_or(".");
                    // Try Pyrefly if available
                    let pyrefly_res = std::process::Command::new("pyrefly")
                        .args(["check", target])
                        .output();

                    let report = if let Ok(out) = pyrefly_res {
                        json!({
                            "engine": "pyrefly",
                            "success": out.status.success(),
                            "exit_code": out.status.code(),
                            "output": String::from_utf8_lossy(&out.stdout).to_string(),
                            "errors": String::from_utf8_lossy(&out.stderr).to_string(),
                            "strict": strict,
                        })
                    } else {
                        // Fallback to python syntax check
                        if target != "." && std::path::Path::new(target).exists() {
                            let out = std::process::Command::new("python")
                                .args(["-m", "py_compile", target])
                                .output();
                            match out {
                                Ok(o) => json!({
                                    "engine": "python-ast-compiler",
                                    "syntax_valid": o.status.success(),
                                    "file": target,
                                    "details": if o.status.success() { "Syntax and AST valid. For full deep type inference, install pyrefly or pyright." } else { "Syntax errors encountered during compilation." }
                                }),
                                Err(e) => json!({
                                    "engine": "python-ast-compiler",
                                    "syntax_valid": false,
                                    "error": e.to_string(),
                                }),
                            }
                        } else {
                            json!({
                                "engine": "advisory",
                                "target": target,
                                "status": "ready",
                                "message": "Python typecheck adapter active. Install pyrefly via cargo install pyrefly for rust-accelerated deep inference."
                            })
                        }
                    };

                    ToolCallResult {
                        content: vec![ContentBlock {
                            kind: "text",
                            text: serde_json::to_string_pretty(&report)?,
                        }],
                        is_error: None,
                    }
                }
                "analyze_impact" => {
                    backend.reload_if_stale();

                    let scanning = backend.scan_progress.lock().unwrap().scanning;
                    if scanning {
                        let sp = backend.scan_progress.lock().unwrap();
                        ToolCallResult {
                            content: vec![ContentBlock {
                                kind: "text",
                                text: format!(
                                    "Scanning project... {} files processed",
                                    sp.processed
                                ),
                            }],
                            is_error: None,
                        }
                    } else {
                        let file_path = tool_call
                            .arguments
                            .get("file_path")
                            .and_then(Value::as_str)
                            .ok_or_else(|| anyhow::anyhow!("missing file_path"))?;
                        let depth = tool_call
                            .arguments
                            .get("depth")
                            .and_then(Value::as_u64)
                            .unwrap_or(3) as u32;

                        let impacts = backend.analyze_impact(file_path, depth);
                        let body = format_impact(&impacts, file_path);
                        ToolCallResult {
                            content: vec![ContentBlock {
                                kind: "text",
                                text: body,
                            }],
                            is_error: None,
                        }
                    }
                }
                "file_context" => {
                    backend.reload_if_stale();
                    let file_path = tool_call
                        .arguments
                        .get("file_path")
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow::anyhow!("missing file_path"))?;

                    let ctx = backend.get_file_context(file_path).await?;
                    let history = backend.get_historical_engram_solutions(file_path).await?;

                    // Enriched: graph neighbors (dependent count)
                    let neighbor_info = backend.get_graph_neighbors(file_path).await.ok();

                    // Enriched: last git commit
                    let last_git = get_last_commit(file_path).await;

                    let body = format_file_context_enriched(
                        ctx.as_ref(),
                        file_path,
                        &history,
                        neighbor_info.as_ref(),
                        last_git.as_deref(),
                    );
                    ToolCallResult {
                        content: vec![ContentBlock {
                            kind: "text",
                            text: body,
                        }],
                        is_error: None,
                    }
                }
                "graph_summary" | "ozymem_get_schema" => {
                    backend.reload_if_stale();
                    let summary = backend.get_graph_summary().await?;
                    let sp = backend.scan_progress.lock().unwrap();
                    let scanning_note = if sp.scanning {
                        format!(
                            "\n\n[Warning] Scan in progress: {} of {} files processed (results may be partial)",
                            sp.processed, sp.total
                        )
                    } else {
                        String::new()
                    };
                    drop(sp);
                    ToolCallResult {
                        content: vec![ContentBlock {
                            kind: "text",
                            text: format!("{}{}", format_summary(&summary), scanning_note),
                        }],
                        is_error: None,
                    }
                }
                "ozymem_find_symbol" => {
                    let symbol_name = tool_call
                        .arguments
                        .get("symbol_name")
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow::anyhow!("missing symbol_name"))?;
                    backend.reload_if_stale();
                    let proj_path = backend.project_path().unwrap_or_default();
                    let res = backend.find_symbol(symbol_name, &proj_path).await?;
                    let body = if res.is_empty() {
                        format!("Symbol '{}' not found in indexed project.", symbol_name)
                    } else {
                        format!(
                            "Symbol matches for '{}':\n\n{}",
                            symbol_name,
                            res.join("\n")
                        )
                    };
                    ToolCallResult {
                        content: vec![ContentBlock {
                            kind: "text",
                            text: body,
                        }],
                        is_error: None,
                    }
                }
                "ozymem_hybrid_search" => {
                    let query = tool_call
                        .arguments
                        .get("query")
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow::anyhow!("missing query"))?;
                    let limit = tool_call
                        .arguments
                        .get("limit")
                        .and_then(Value::as_u64)
                        .unwrap_or(5) as usize;
                    backend.reload_if_stale();
                    let matches = backend.search_lessons(query, None, limit).await?;
                    let mut body = format!("Hybrid search results for '{}':\n\n", query);
                    if matches.is_empty() {
                        body.push_str("No matching lessons or code snippets found.");
                    } else {
                        for (i, m) in matches.iter().enumerate() {
                            body.push_str(&format!(
                                "{}. [{}] {} -> {}\n",
                                i + 1,
                                m.file_path,
                                m.error_context,
                                m.solution
                            ));
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
                "list_files" => {
                    backend.reload_if_stale();
                    let files = backend.list_all_files()?;
                    if files.is_empty() {
                        ToolCallResult {
                            content: vec![ContentBlock {
                                kind: "text",
                                text: "No files indexed. Run refresh_project_index first."
                                    .to_string(),
                            }],
                            is_error: None,
                        }
                    } else {
                        let body =
                            format!("Indexed files ({}):\n\n{}", files.len(), files.join("\n"));
                        ToolCallResult {
                            content: vec![ContentBlock {
                                kind: "text",
                                text: body,
                            }],
                            is_error: None,
                        }
                    }
                }
                // Creates or updates .ozymignore, appends patterns, re-scans the project.
                // Falls back to the active backend's project_path if no explicit path given.
                "create_ozymignore" => {
                    let patterns = match tool_call
                        .arguments
                        .get("patterns")
                        .and_then(Value::as_array)
                    {
                        Some(arr) => arr
                            .iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect::<Vec<_>>(),
                        None => {
                            return Ok(Some(error_response(
                                id,
                                -32602,
                                "Missing required parameter: patterns",
                            )));
                        }
                    };
                    if patterns.is_empty() {
                        return Ok(Some(error_response(
                            id,
                            -32602,
                            "patterns must not be empty",
                        )));
                    }
                    let explicit_path = tool_call
                        .arguments
                        .get("project_path")
                        .and_then(Value::as_str)
                        .map(|s| s.to_string());
                    let resolved = match resolve_project_root(explicit_path.clone(), None) {
                        Ok(p) => p,
                        Err(_) if explicit_path.is_none() => match backend.project_path() {
                            Some(p) => PathBuf::from(p),
                            None => {
                                return Ok(Some(error_response(
                                    id,
                                    -32602,
                                    "No project path set. Provide project_path or ensure initialize was sent with workspaceFolders.",
                                )));
                            }
                        },
                        Err(msg) => return Ok(Some(error_response(id, -32602, &msg))),
                    };
                    let ozymemignore_path = resolved.join(".ozymignore");
                    let mut existing = Vec::new();
                    if ozymemignore_path.exists() {
                        if let Ok(content) = std::fs::read_to_string(&ozymemignore_path) {
                            for line in content.lines() {
                                let t = line.trim();
                                if !t.is_empty() && !t.starts_with('#') {
                                    existing.push(t.to_string());
                                }
                            }
                        }
                    }
                    let mut added = Vec::new();
                    for p in &patterns {
                        if !existing.contains(p) {
                            existing.push(p.clone());
                            added.push(p.clone());
                        }
                    }
                    let mut content = String::from(
                        "# OzyMem ignore patterns\n# Lines starting with # are comments\n\n",
                    );
                    for p in &existing {
                        content.push_str(p);
                        content.push('\n');
                    }
                    std::fs::write(&ozymemignore_path, content)?;
                    
                    let resolved_str = resolved.to_string_lossy().to_string();
                    let progress = tool_call.arguments.get("_meta").and_then(|m| m.get("progressToken"));
                    backend.full_scan(
                        &resolved_str,
                        Some(&|processed, total| {
                            if let Some(ref n) = notifier {
                                if let Some(ref pt) = progress {
                                    n.progress(pt, processed, Some(total));
                                }
                            }
                        }),
                    )?;
                    let body = format!(
                        "Created/updated .ozymignore at {}.\n  Total patterns: {}\n  Newly added: {}\n  The project has been re-scanned ignoring these patterns.",
                        ozymemignore_path.display(),
                        existing.len(),
                        if added.is_empty() {
                            "none (all already present)".to_string()
                        } else {
                            added.join(", ")
                        },
                    );
                    ToolCallResult {
                        content: vec![ContentBlock {
                            kind: "text",
                            text: body,
                        }],
                        is_error: None,
                    }
                }
                "graph_neighbors" => {
                    backend.reload_if_stale();
                    let file_path = tool_call
                        .arguments
                        .get("file_path")
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow::anyhow!("missing file_path"))?;
                    let info = backend.get_graph_neighbors(file_path).await?;
                    let mut body = format!("Neighbors of {}\n", info.file_path);
                    body.push_str(&format!(
                        "  Incoming (dependents): {}\n",
                        info.incoming.len()
                    ));
                    for p in &info.incoming {
                        body.push_str(&format!("    - {p}\n"));
                    }
                    body.push_str(&format!(
                        "  Outgoing (dependencies): {}\n",
                        info.outgoing.len()
                    ));
                    for p in &info.outgoing {
                        body.push_str(&format!("    - {p}\n"));
                    }
                    ToolCallResult {
                        content: vec![ContentBlock {
                            kind: "text",
                            text: body,
                        }],
                        is_error: None,
                    }
                }
                "find_symbol" => {
                    backend.reload_if_stale();

                    let symbol_name = tool_call
                        .arguments
                        .get("symbol_name")
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow::anyhow!("missing symbol_name"))?;
                    let kind_filter = tool_call.arguments.get("kind").and_then(Value::as_str);
                    let max_results = tool_call
                        .arguments
                        .get("max_results")
                        .and_then(Value::as_u64)
                        .unwrap_or(20) as usize;

                    let project_path = backend
                        .project_path()
                        .ok_or_else(|| anyhow::anyhow!("no project path set"))?;

                    let results = backend.find_symbol(symbol_name, &project_path).await?;

                    // Post-filter by kind (parsed from "[Kind]" in result string)
                    let filtered: Vec<&String> = if let Some(kind) = kind_filter {
                        results
                            .iter()
                            .filter(|r| {
                                r.split('[')
                                    .nth(1)
                                    .and_then(|s| s.split(']').next())
                                    .map(|k| k == kind)
                                    .unwrap_or(false)
                            })
                            .collect()
                    } else {
                        results.iter().collect()
                    };

                    let truncated: Vec<&&String> = filtered.iter().take(max_results).collect();

                    let body = if truncated.is_empty() {
                        format!(
                            "No definitions found for '{}'{}",
                            symbol_name,
                            kind_filter.map_or(String::new(), |k| format!(" (kind: {k})"))
                        )
                    } else {
                        let mut text = format!(
                            "Definitions matching '{}' ({} total, filtered to {}, showing {}):\n",
                            symbol_name,
                            results.len(),
                            filtered.len(),
                            truncated.len()
                        );
                        for (i, entry) in truncated.iter().enumerate() {
                            text.push_str(&format!("{}. {}\n", i + 1, entry));
                        }
                        text
                    };

                    ToolCallResult {
                        content: vec![ContentBlock {
                            kind: "text",
                            text: body,
                        }],
                        is_error: None,
                    }
                }
                "context_for_task" => {
                    backend.reload_if_stale();

                    let query = tool_call
                        .arguments
                        .get("query")
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow::anyhow!("missing query"))?;
                    let max_tokens = tool_call
                        .arguments
                        .get("max_tokens")
                        .and_then(Value::as_u64)
                        .unwrap_or(4000) as usize;

                    // Wrap body-building in a timeout to prevent MCP client timeout
                    let body = tokio::time::timeout(std::time::Duration::from_secs(15), async {
                        // 1. Search lessons (filter out stale)
                        let lessons: Vec<_> = backend
                            .search_lessons(query, None, 20)
                            .await?
                            .into_iter()
                            .filter(|l| l.stale == 0)
                            .collect();

                        let mut ast_symbols: Vec<ozymem_core::StoredFunction> = Vec::new();

                        // 2. Collect unique file paths from matched lessons (or AST fallback)
                        let mut file_paths: Vec<String> = if !lessons.is_empty() {
                            lessons
                                .iter()
                                .map(|l| l.file_path.clone())
                                .collect::<std::collections::HashSet<String>>()
                                .into_iter()
                                .collect()
                        } else {
                            if let Ok(syms) = backend.search_ast_symbols(query, 10) {
                                let paths: Vec<String> = syms
                                    .iter()
                                    .filter_map(|s| {
                                        let parts: Vec<&str> = s.strategy.split(" in ").collect();
                                        parts.get(1).map(|p| p.to_string())
                                    })
                                    .collect::<std::collections::HashSet<String>>()
                                    .into_iter()
                                    .collect();
                                ast_symbols = syms;
                                paths
                            } else {
                                Vec::new()
                            }
                        };
                        file_paths.sort();
                        file_paths.truncate(5);

                        // 3. Build response
                        let mut body = String::new();

                        // Lessons section or AST fallback
                        body.push_str(&format!("[Lessons matching \"{query}\"]\n"));
                        if lessons.is_empty() {
                            if ast_symbols.is_empty() {
                                body.push_str("(none)\n");
                            } else {
                                body.push_str("(no explicit lessons found — falling back to indexed AST symbols)\n\n[AST Symbols matching query]\n");
                                for (i, s) in ast_symbols.iter().enumerate() {
                                    body.push_str(&format!(
                                        "{}. {} [{}] L{}-L{} ({})\n",
                                        i + 1,
                                        s.name,
                                        s.kind,
                                        s.start_line,
                                        s.end_line,
                                        s.strategy
                                    ));
                                }
                            }
                        } else {
                            for (i, e) in lessons.iter().enumerate() {
                                body.push_str(&format!(
                                    "{}. [{}] {} :: {}\n   {}\n",
                                    i + 1,
                                    e.kind,
                                    e.file_path,
                                    e.symbol_name,
                                    e.solution
                                ));
                            }
                        }

                        // File context and neighbors for each relevant file
                        for fp in &file_paths {
                            if body.len() / 4 >= max_tokens {
                                body.push_str(&format!("\n... (truncated at {max_tokens} tokens)"));
                                break;
                            }

                            body.push_str(&format!("\n\n=== {fp} ==="));

                            // File context
                            if let Ok(Some(ctx)) = backend.get_file_context(fp).await {
                                body.push_str(&format!("\nLanguage: {}", ctx.language));
                                if !ctx.functions.is_empty() {
                                    body.push_str(&format!(
                                        "\nFunctions ({})",
                                        ctx.functions.len()
                                    ));
                                    for fn_data in &ctx.functions {
                                        body.push_str(&format!(
                                            "\n  {} [{}] L{}-L{}",
                                            fn_data.name,
                                            fn_data.kind,
                                            fn_data.start_line,
                                            fn_data.end_line
                                        ));
                                    }
                                }
                            }

                            // Graph neighbors
                            if let Ok(neighbors) = backend.get_graph_neighbors(fp).await {
                                if !neighbors.incoming.is_empty() {
                                    body.push_str(&format!(
                                        "\nDependents ({}):",
                                        neighbors.incoming.len()
                                    ));
                                    for p in &neighbors.incoming {
                                        body.push_str(&format!("\n  {p}"));
                                    }
                                }
                                if !neighbors.outgoing.is_empty() {
                                    body.push_str(&format!(
                                        "\nDepends on ({}):",
                                        neighbors.outgoing.len()
                                    ));
                                    for p in &neighbors.outgoing {
                                        body.push_str(&format!("\n  {p}"));
                                    }
                                }
                            }

                            // Impact analysis (depth 1, called inline — safe under outer lock)
                            let impacts = backend.analyze_impact(fp, 1);
                            if !impacts.is_empty() {
                                body.push_str(&format!(
                                    "\nTransitive impact (depth 1, {} files):",
                                    impacts.len()
                                ));
                                for imp in &impacts {
                                    body.push_str(&format!(
                                        "\n  {} ({} funcs, {} lessons)",
                                        imp.file_path, imp.function_count, imp.lesson_count
                                    ));
                                }
                            }
                        }

                        Ok::<String, anyhow::Error>(body)
                    })
                    .await
                    .map_err(|_| anyhow::anyhow!("context_for_task timed out after 15s"))?
                    .map_err(|e| anyhow::anyhow!("context_for_task error: {e}"))?;

                    ToolCallResult {
                        content: vec![ContentBlock {
                            kind: "text",
                            text: body,
                        }],
                        is_error: None,
                    }
                }
                "graph_path" => {
                    backend.reload_if_stale();

                    let from = tool_call
                        .arguments
                        .get("from")
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow::anyhow!("missing from"))?;
                    let to = tool_call
                        .arguments
                        .get("to")
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow::anyhow!("missing to"))?;
                    let max_paths = tool_call
                        .arguments
                        .get("max_paths")
                        .and_then(Value::as_u64)
                        .unwrap_or(1) as usize;
                    let max_hops = tool_call
                        .arguments
                        .get("max_hops")
                        .and_then(Value::as_u64)
                        .unwrap_or(10) as usize;

                    let paths = backend.find_graph_path(from, to, max_paths, max_hops);

                    let body = if paths.is_empty() {
                        format!(
                            "No path found between '{}' and '{}' (they may not be connected in the dependency graph)",
                            from, to
                        )
                    } else {
                        let shortest = paths
                            .iter()
                            .map(|p| p.len())
                            .min()
                            .unwrap_or(0)
                            .saturating_sub(1);
                        let mut text = format!(
                            "Found {} path(s) between '{}' and '{}' (shortest: {} hops, max allowed: {}):\n",
                            paths.len(),
                            from,
                            to,
                            shortest,
                            max_hops
                        );
                        for (i, path) in paths.iter().enumerate() {
                            let hops = path.len() - 1;
                            let marker = if hops == shortest {
                                " ◀ SHORTEST"
                            } else {
                                ""
                            };
                            text.push_str(&format!(
                                "\nPath {} ({} hop(s)){}:\n",
                                i + 1,
                                hops,
                                marker
                            ));
                            for (j, file) in path.iter().enumerate() {
                                let arrow = if j < path.len() - 1 { " → " } else { "" };
                                text.push_str(&format!("  {}{}", file, arrow));
                                if j < path.len() - 1 {
                                    text.push('\n');
                                }
                            }
                            text.push('\n');
                        }
                        text
                    };

                    ToolCallResult {
                        content: vec![ContentBlock {
                            kind: "text",
                            text: body,
                        }],
                        is_error: None,
                    }
                }
                "smart_search" => {
                    backend.reload_if_stale();

                    let query = tool_call
                        .arguments
                        .get("query")
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow::anyhow!("missing query"))?;
                    let max_results = tool_call
                        .arguments
                        .get("max_results")
                        .and_then(Value::as_u64)
                        .unwrap_or(10) as usize;
                    let min_score = tool_call
                        .arguments
                        .get("min_score")
                        .and_then(Value::as_f64)
                        .unwrap_or(0.3) as f32;

                    let project_path = backend
                        .project_path()
                        .ok_or_else(|| anyhow::anyhow!("no project path set"))?;

                    // Run async searches in parallel (non-blocking)
                    let (symbols, lessons) = tokio::join!(
                        backend.find_symbol(query, &project_path),
                        backend.search_lessons(query, None, max_results),
                    );

                    // Run sync semantic search separately (blocks but has early-return)
                    let semantic = backend
                        .similar_lessons(&format!("search:{}", query), max_results, min_score)
                        .unwrap_or_default();

                    let symbol_results = symbols.unwrap_or_default();
                    let lesson_results = lessons.unwrap_or_default();

                    let mut body = format!("Smart search for '{}':\n", query);

                    // Symbols section
                    body.push_str(&format!(
                        "\n── Symbol definitions ({}):\n",
                        symbol_results.len()
                    ));
                    for (i, s) in symbol_results.iter().enumerate().take(max_results) {
                        body.push_str(&format!("  {}. {}\n", i + 1, s));
                    }

                    // Lesson section (FTS5)
                    body.push_str(&format!("\n── Lessons (FTS5, {}):\n", lesson_results.len()));
                    for (i, l) in lesson_results.iter().enumerate().take(max_results) {
                        body.push_str(&format!(
                            "  {}. [{}] {} :: {}\n     {}\n",
                            i + 1,
                            l.kind,
                            l.symbol_name,
                            l.error_context,
                            l.solution
                        ));
                    }

                    // Semantic section
                    body.push_str(&format!(
                        "\n── Semantic (embeddings, {}):\n",
                        semantic.len()
                    ));
                    for (i, s) in semantic.iter().enumerate().take(max_results) {
                        body.push_str(&format!(
                            "  {}. [score: {:.2}] [{}] {} :: {}\n     {}\n",
                            i + 1,
                            s.score,
                            s.lesson.kind,
                            s.lesson.symbol_name,
                            s.lesson.error_context,
                            s.lesson.solution
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
        _ => return Ok(None),
    };
    Ok(Some(ok_response(id, serde_json::to_value(res)?)))
}
