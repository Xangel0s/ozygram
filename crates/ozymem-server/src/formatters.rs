use ozymem_core::graph_backend::ImpactEntry;
use ozymem_core::registry::ProjectRegistry;
use ozymem_core::graph_backend::GraphBackend;
use ozymem_core::McpBackend;
use std::path::Path;

pub(crate) fn format_engram_contract(contract: &ozymem_parser::EngramContract) -> String {
    let deps = if contract.dependencies.is_empty() {
        String::from("none")
    } else {
        contract.dependencies.join(", ")
    };
    let mut out = format!(
        "[ENGRAM_CONTRACT: {}]\nFile: {}:{}\nKind: {}\nLanguage: {}\nSignature: {}\nDependencies: [{}]",
        contract.symbol_path,
        contract.file_path,
        contract.line_number,
        contract.kind,
        contract.language,
        contract.signature,
        deps
    );
    if !contract.doc_summary.is_empty() {
        out.push_str(&format!("\nDoc: {}", contract.doc_summary));
    }
    out
}

pub(crate) fn format_lessons_list(results: &[ozymem_core::graph_backend::LessonEntry]) -> String {
    if results.is_empty() {
        return "No entries found.".to_string();
    }
    let mut body = String::new();
    for (i, entry) in results.iter().enumerate() {
        let stale_tag = if entry.stale != 0 {
            format!(
                " [STALE: {}]",
                entry.stale_reason.as_deref().unwrap_or("unknown")
            )
        } else {
            String::new()
        };
        body.push_str(&format!(
            "{}. [{}]{} {} :: {}\n   context: {}\n   solution: {}\n   created: {}\n",
            i + 1,
            entry.kind,
            stale_tag,
            entry.file_path,
            entry.symbol_name,
            entry.error_context,
            entry.solution,
            entry.created_at
        ));
    }
    body
}

pub(crate) fn format_observations_list(results: &[ozymem_core::graph_backend::ObservationEntry]) -> String {
    if results.is_empty() {
        return "No observations found.".to_string();
    }
    let mut body = String::new();
    for (i, entry) in results.iter().enumerate() {
        body.push_str(&format!(
            "{}. #{} [{}] {} ({}/{})\n   session: {}\n   topic: {}\n   revisions: {}, duplicates: {}\n   content: {}\n   updated: {}\n\n",
            i + 1,
            entry.id,
            entry.observation_type,
            entry.title,
            entry.project,
            entry.scope,
            entry.session_id,
            entry.topic_key.as_deref().unwrap_or("-"),
            entry.revision_count,
            entry.duplicate_count,
            entry.content,
            entry.updated_at,
        ));
    }
    body
}
pub(crate) fn format_impact(impacts: &[ImpactEntry], file_path: &str) -> String {
    if impacts.is_empty() {
        return format!("No impact found for {}", file_path);
    }

    let mut text = format!("Impact analysis for {}:\n", file_path);
    let mut current_depth = 0u32;

    // Count by severity
    let mut breaking = 0usize;
    let mut warnings = 0usize;
    let mut infos = 0usize;

    for entry in impacts {
        if entry.depth != current_depth {
            current_depth = entry.depth;
            text.push_str(&format!("\n  Depth {}:\n", current_depth));
        }
        let sev_tag = match entry.severity.as_str() {
            "breaking" => {
                breaking += 1;
                "[BREAKING]"
            }
            "warning" => {
                warnings += 1;
                "[WARN]"
            }
            _ => {
                infos += 1;
                "[INFO]"
            }
        };
        text.push_str(&format!(
            "    {:>10} {} (L{}-L{}) [{} | {} funcs, {} lessons]\n",
            sev_tag,
            entry.file_path,
            entry.start_line,
            entry.end_line,
            entry.language,
            entry.function_count,
            entry.lesson_count
        ));

        // Reason and suggestion
        text.push_str(&format!("           ├── reason: {}\n", entry.reason));
        text.push_str(&format!(
            "           └── suggestion: {}\n",
            entry.suggestion
        ));

        // Show key functions if available
        if !entry.functions.is_empty() {
            for f in &entry.functions {
                text.push_str(&format!("                ├── {}\n", f));
            }
        }
    }

    let total_funcs: i64 = impacts.iter().map(|e| e.function_count).sum();
    let total_lessons: i64 = impacts.iter().map(|e| e.lesson_count).sum();
    text.push_str(&format!(
        "\nTotal: {} files affected, {} functions, {} lessons registered\n",
        impacts.len(),
        total_funcs,
        total_lessons
    ));

    // Summary bar
    text.push_str(&format!(
        "Severity: {} [BREAKING] | {} [WARN] | {} [INFO]",
        breaking, warnings, infos
    ));

    text
}

pub(crate) fn format_file_context_enriched(
    context: Option<&ozymem_core::FileGraphContext>,
    file_path: &str,
    history: &[String],
    neighbors: Option<&ozymem_core::graph_backend::NeighborInfo>,
    last_commit: Option<&str>,
) -> String {
    let Some(context) = context else {
        return format!("No indexed file found for {file_path}");
    };

    let mut output = format!(
        "File: {}\nLanguage: {}\nFunctions: {}",
        context.file_path,
        context.language,
        context.functions.len()
    );

    for function in &context.functions {
        output.push_str(&format!(
            "\n- {} [{}] lines {}-{} via {}",
            function.name, function.kind, function.start_line, function.end_line, function.strategy
        ));
    }

    // Graph neighbors section
    if let Some(n) = neighbors {
        output.push_str(&format!(
            "\n\nDependents (files that import this): {}",
            n.incoming.len()
        ));
        for dep in n.incoming.iter().take(5) {
            output.push_str(&format!("\n  ← {}", dep));
        }
        if n.incoming.len() > 5 {
            output.push_str(&format!("\n  ... and {} more", n.incoming.len() - 5));
        }
        output.push_str(&format!(
            "\nDepends on (files this imports): {}",
            n.outgoing.len()
        ));
        for dep in n.outgoing.iter().take(5) {
            output.push_str(&format!("\n  → {}", dep));
        }
        if n.outgoing.len() > 5 {
            output.push_str(&format!("\n  ... and {} more", n.outgoing.len() - 5));
        }
    }

    // Last git commit
    if let Some(commit) = last_commit {
        output.push_str(&format!("\n\nLast commit touching this file: {}", commit));
    }

    // Lessons for this file
    if !history.is_empty() {
        output.push_str(&format!(
            "\n\nLessons recorded for this file ({}):",
            history.len()
        ));
        for solution in history {
            output.push_str(&format!("\n- {}", solution));
        }
    }

    output
}

pub(crate) async fn get_last_commit(file_path: &str) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["log", "-1", "--format=%h %s (%ar)", "--", file_path])
        .output()
        .ok()?;
    if output.status.success() {
        let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !s.is_empty() { Some(s) } else { None }
    } else {
        None
    }
}

pub(crate) fn format_summary(summary: &ozymem_core::GraphSummary) -> String {
    format!(
        "Files: {}\nFunctions: {}\nLessons: {} ({} without embeddings)\nNative AST: {}\nExtension WASM: {}\nText heuristic: {}",
        summary.file_count,
        summary.function_count,
        summary.engram_count,
        summary.lessons_without_embedding,
        summary.native_ast_function_count,
        summary.extension_wasm_function_count,
        summary.text_heuristic_function_count
    )
}

pub(crate) async fn build_ozy_task_context(
    backend: &GraphBackend,
    query: &str,
    max_tokens: usize,
) -> anyhow::Result<String> {
    let lessons: Vec<_> = backend
        .search_lessons(query, None, 20)
        .await?
        .into_iter()
        .filter(|l| l.stale == 0)
        .collect();
    let mut ast_symbols: Vec<ozymem_core::StoredFunction> = Vec::new();
    let mut body = format!("[Ozy Context for \"{query}\"]\n");
    if lessons.is_empty() {
        if let Ok(syms) = backend.search_ast_symbols(query, 10) {
            if syms.is_empty() {
                body.push_str("No matching memories found.\n");
            } else {
                body.push_str("(no explicit lessons found — falling back to indexed AST symbols)\n\n[AST Symbols matching query]\n");
                for (i, s) in syms.iter().enumerate() {
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
                ast_symbols = syms;
            }
        } else {
            body.push_str("No matching memories found.\n");
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
    let mut files: Vec<String> = if !lessons.is_empty() {
        lessons
            .iter()
            .map(|l| l.file_path.clone())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect()
    } else {
        ast_symbols
            .iter()
            .filter_map(|s| {
                let parts: Vec<&str> = s.strategy.split(" in ").collect();
                parts.get(1).map(|p| p.to_string())
            })
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect()
    };
    files.sort();

    // 1. Inyección de Contratos Engram Deterministas (Prefill Steering para Prompt Cache Hit)
    let mut engram_block = String::new();
    for fp in files.iter().take(5) {
        if let Ok(Some(ctx)) = backend.get_file_context(fp).await {
            for c in &ctx.engram_contracts {
                engram_block.push_str(&format_engram_contract(c));
                engram_block.push('\n');
            }
        }
    }
    if !engram_block.is_empty() {
        body.push_str("\n[ENGRAM_CACHE: Deterministic Symbol Contracts]\n");
        body.push_str(&engram_block);
        body.push('\n');
    }

    // 2. Inyección de Pre-fetching Especulativo (Speculative Context Decoding de vecinos)
    let mut speculative_block = String::new();
    let mut seen_speculative = std::collections::HashSet::new();
    for fp in files.iter().take(3) {
        let speculative = backend.get_speculative_engrams(fp, 3);
        for c in speculative {
            if seen_speculative.insert(c.symbol_path.clone()) {
                speculative_block.push_str(&format_engram_contract(&c));
                speculative_block.push('\n');
            }
        }
    }
    if !speculative_block.is_empty() {
        body.push_str("\n[SPECULATIVE_PREFETCH: Next Likely Contracts from Dependency Neighbors]\n");
        body.push_str(&speculative_block);
        body.push('\n');
    }

    for fp in files.iter().take(5) {
        if body.len() / 4 >= max_tokens {
            body.push_str(&format!("\n... truncated at ~{max_tokens} tokens"));
            break;
        }
        if let Ok(Some(ctx)) = backend.get_file_context(fp).await {
            body.push_str(&format!(
                "\n=== {fp} ===\nLanguage: {}\nFunctions: {}\n",
                ctx.language,
                ctx.functions.len()
            ));
        }
        if let Ok(neighbors) = backend.get_graph_neighbors(fp).await {
            body.push_str(&format!(
                "Dependents: {} | Dependencies: {}\n",
                neighbors.incoming.len(),
                neighbors.outgoing.len()
            ));
        }
    }
    Ok(body)
}


pub(crate) async fn build_ozy_project_context(
    project_name: Option<&str>,
    query: Option<&str>,
    limit: usize,
    max_tokens: usize,
) -> anyhow::Result<String> {
    let reg = ProjectRegistry::open()?;
    let project = match project_name {
        Some(name) => reg
            .get_project_by_name(name)?
            .ok_or_else(|| anyhow::anyhow!("Project '{}' not found in registry", name))?,
        None => reg
            .list_projects()?
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("No registered projects found"))?,
    };

    let db_path = Path::new(&project.path).join(".ozymem").join("memory.db");
    let mut body = format!(
        "## Ozy Project Context\n\nProject: {}\nPath: {}\nStatus: {}\nLast activity: {}\nRegistry files: {}\n\n",
        project.name,
        project.path,
        project.status.as_str(),
        project.last_opened.as_deref().unwrap_or("never"),
        project.file_count,
    );

    if !db_path.exists() {
        body.push_str(&format!("No memory.db found at {}\n", db_path.display()));
        return Ok(truncate_chars(body, max_tokens.saturating_mul(4)));
    }

    let gb = GraphBackend::open(Some(&db_path.to_string_lossy()))?;
    gb.set_project_path(Some(&project.path));
    let summary = gb
        .get_graph_summary()
        .await
        .unwrap_or(ozymem_core::GraphSummary {
            file_count: 0,
            function_count: 0,
            engram_count: 0,
            native_ast_function_count: 0,
            extension_wasm_function_count: 0,
            text_heuristic_function_count: 0,
            vertex_count: 0,
            edge_count: 0,
            memory_usage: String::new(),
            lessons_without_embedding: 0,
        });
    body.push_str(&format!("{}\n\n", format_summary(&summary)));

    let lessons = if let Some(q) = query.filter(|q| !q.trim().is_empty()) {
        body.push_str(&format!("### Relevant memories for \"{}\"\n", q));
        gb.search_lessons(q, None, limit).await?
    } else {
        body.push_str("### Recent project memories\n");
        gb.recent_lessons(None, limit).await?
    };
    body.push_str(&format_lessons_list(&lessons));

    Ok(truncate_chars(body, max_tokens.saturating_mul(4)))
}

fn truncate_chars(mut text: String, max_chars: usize) -> String {
    if max_chars == 0 || text.chars().count() <= max_chars {
        return text;
    }
    text = text.chars().take(max_chars).collect();
    text.push_str("\n\n...[truncated]");
    text
}

pub(crate) async fn build_architecture_report(backend: &GraphBackend) -> anyhow::Result<String> {
    let summary = backend.get_graph_summary().await?;
    let files = backend.list_all_files().unwrap_or_default();
    let mut hotspots = Vec::new();
    for fp in files.iter().take(500) {
        if let Ok(n) = backend.get_graph_neighbors(fp).await {
            let score = n.incoming.len() + n.outgoing.len();
            if score > 0 {
                hotspots.push((score, n.incoming.len(), n.outgoing.len(), fp.clone()));
            }
        }
    }
    hotspots.sort_by(|a, b| b.0.cmp(&a.0));
    let mut body = format!(
        "# Architecture Report\n\n{}\n\nTop coupling hotspots:\n",
        format_summary(&summary)
    );
    for (score, incoming, outgoing, fp) in hotspots.iter().take(10) {
        body.push_str(&format!(
            "- score {score} | incoming {incoming} | outgoing {outgoing}: {fp}\n"
        ));
    }
    body.push_str("\nPreview-safe recommendations:\n- Review high incoming files before refactors.\n- Use graph impact before edits.\n- Extract shared code only after duplicate evidence is confirmed.\n");
    Ok(body)
}


pub(crate) fn days_since_str(date_str: Option<&str>) -> i64 {
    let s = match date_str {
        Some(s) => s,
        None => return 0,
    };
    let date_part = s
        .replace('T', " ")
        .split(' ')
        .next()
        .unwrap_or("")
        .to_string();
    let parts: Vec<i64> = date_part
        .split('-')
        .filter_map(|p| p.parse().ok())
        .collect();
    if parts.len() != 3 {
        return 0;
    }
    let (y, m, d) = (parts[0], parts[1], parts[2]);

    // Days since epoch (1970-01-01) for the given date
    let mut total = 0i64;
    for year in 1970..y {
        let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
        total += if leap { 366 } else { 365 };
    }
    for month in 1..m {
        let dim = match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 => {
                let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
                if leap { 29 } else { 28 }
            }
            _ => 0,
        };
        total += dim;
    }
    total += d - 1;

    // Days since epoch for now
    let now_days = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|dur| dur.as_secs() as i64 / 86400)
        .unwrap_or(0);

    (now_days - total).max(0)
}

/// Detect tree-sitter language from file path.
pub(crate) fn detect_lang(path: &str) -> ozymem_parser::SupportedLanguage {
    match Path::new(path).extension().and_then(|e| e.to_str()) {
        Some("py") => ozymem_parser::SupportedLanguage::Python,
        Some("go") => ozymem_parser::SupportedLanguage::Go,
        Some("rs") => ozymem_parser::SupportedLanguage::Rust,
        Some("js") | Some("jsx") => ozymem_parser::SupportedLanguage::JavaScript,
        Some("ts") | Some("tsx") => ozymem_parser::SupportedLanguage::TypeScriptReact,
        Some("sql") => ozymem_parser::SupportedLanguage::SQL,
        _ => ozymem_parser::SupportedLanguage::Unknown,
    }
}

