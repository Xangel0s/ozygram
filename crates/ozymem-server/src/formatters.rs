use ozymem_core::graph_backend::ImpactEntry;
use ozymem_core::registry::ProjectRegistry;
use ozymem_core::graph_backend::GraphBackend;
use ozymem_core::mcp_common::ContentBlock;
use ozymem_core::McpBackend;
use sha2::{Digest, Sha256};
use std::path::Path;

pub(crate) fn format_archived_engram_contract(contract: &ozymem_parser::ArchivedEngramContract) -> String {
    let deps = if contract.dependencies.is_empty() {
        String::from("none")
    } else {
        contract.dependencies.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
    };
    let mut out = format!(
        "[ENGRAM_CONTRACT: {}]\nFile: {}:{}\nKind: {}\nLanguage: {}\nSignature: {}\nDependencies: [{}]",
        contract.symbol_path.as_str(),
        contract.file_path.as_str(),
        contract.line_number,
        contract.kind.as_str(),
        contract.language.as_str(),
        contract.signature.as_str(),
        deps
    );
    if !contract.doc_summary.is_empty() {
        out.push_str(&format!("\nDoc: {}", contract.doc_summary.as_str()));
    }
    out
}

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
        let warning_tag = if let Some(ref warn) = entry.freshness_warning {
            format!(" {}", warn)
        } else if entry.stale != 0 {
            format!(
                " [ALERT: STALE_MEMORY: {}]",
                entry.stale_reason.as_deref().unwrap_or("unknown")
            )
        } else {
            String::new()
        };
        body.push_str(&format!(
            "{}. [{}]{} {} :: {}\n   context: {}\n   solution: {}\n   created: {}\n",
            i + 1,
            entry.kind,
            warning_tag,
            entry.file_path,
            entry.symbol_name,
            entry.error_context,
            entry.solution,
            entry.created_at
        ));
        if entry.freshness_warning.is_some() {
            body.push_str("   [ALERT: STALE_MEMORY] This memory may be out of date; verify live code first.\n");
        }
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
            let warn_tag = if let Some(ref w) = e.freshness_warning {
                format!(" {}", w)
            } else {
                String::new()
            };
            body.push_str(&format!(
                "{}. [{}]{} {} :: {}\n   {}\n",
                i + 1,
                e.kind,
                warn_tag,
                e.file_path,
                e.symbol_name,
                e.solution
            ));
            if e.freshness_warning.is_some() {
                body.push_str("   [ALERT: STALE_MEMORY] This memory may be out of date; verify live code first.\n");
            }
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

    // 3. Inyección de Topología de Grafo de Memoria (GraphRAG Multi-Hop Neighborhood)
    let mut memory_graph_block = String::new();
    let mut seen_graph_nodes = std::collections::HashSet::new();
    for fp in files.iter().take(3) {
        if let Ok(neighbors) = backend.get_memory_neighborhood(&ozymem_core::graph_backend::types::GraphEntityType::File, fp, 2) {
            for n in neighbors {
                if seen_graph_nodes.insert(format!("{}:{}", n.entity.entity_type, n.entity.id)) {
                    memory_graph_block.push_str(&format!(
                        "- [{}] {} (type: {}, depth: {}, weight: {:.2}, dir: {})\n",
                        n.edge_type.to_string().to_uppercase(),
                        n.entity.id,
                        n.entity.entity_type,
                        n.depth,
                        n.effective_weight,
                        n.direction
                    ));
                }
            }
        }
    }
    if !memory_graph_block.is_empty() {
        body.push_str("\n[GRAPH_TOPOLOGY: Multi-Hop Memory & Component Neighborhood]\n");
        body.push_str(&memory_graph_block);
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
        Some("js") | Some("mjs") | Some("cjs") => ozymem_parser::SupportedLanguage::JavaScript,
        Some("ts") => ozymem_parser::SupportedLanguage::TypeScript,
        Some("tsx") | Some("jsx") => ozymem_parser::SupportedLanguage::TypeScriptReact,
        Some("sql") => ozymem_parser::SupportedLanguage::SQL,
        _ => ozymem_parser::SupportedLanguage::Unknown,
    }
}

pub(crate) fn format_memory_neighborhood(
    entity_type: &ozymem_core::graph_backend::types::GraphEntityType,
    entity_id: &str,
    neighbors: &[ozymem_core::graph_backend::types::NeighborhoodResult],
) -> String {
    if neighbors.is_empty() {
        return format!(
            "[GRAPH_NEIGHBORHOOD: {}:{}]\nNo connected neighbors found within depth limit.\n",
            entity_type, entity_id
        );
    }
    let mut out = format!(
        "[GRAPH_NEIGHBORHOOD: {}:{}]\nTotal neighbors: {}\n",
        entity_type, entity_id, neighbors.len()
    );
    for n in neighbors {
        let stale_badge = if n.entity.stale {
            " [ALERT: STALE_MEMORY]"
        } else {
            ""
        };
        out.push_str(&format!(
            "- [{}] {} (type: {}, depth: {}, weight: {:.2}, dir: {}){}\n",
            n.edge_type.to_string().to_uppercase(),
            n.entity.id,
            n.entity.entity_type,
            n.depth,
            n.effective_weight,
            n.direction,
            stale_badge
        ));
    }
    out
}

pub(crate) fn format_endpoint_dependents(report: &ozymem_core::graph_backend::types::EndpointDependentsReport) -> String {
    let mut out = format!(
        "[CROSS-LANGUAGE IMPACT: ENDPOINT]\nTarget: {}\nCanonical Endpoint: {}\n\n",
        report.endpoint, report.canonical_path
    );

    out.push_str(&format!(
        "[BACKEND DEFINITIONS] ({})\n",
        report.backend_definitions.len()
    ));
    if report.backend_definitions.is_empty() {
        out.push_str("  No backend route definitions found in indexed backend files.\n");
    } else {
        for def in &report.backend_definitions {
            out.push_str(&format!(
                "  - [{}] {} -> {} ({}:{})\n",
                def.method.to_uppercase(),
                def.path,
                def.handler_name,
                def.file_path,
                def.line_number
            ));
        }
    }
    out.push('\n');

    out.push_str(&format!(
        "[IMPACTED FRONTEND CONSUMERS] ({} files, {} call sites)\n",
        report.total_consumer_files,
        report.consumers.len()
    ));
    if report.consumers.is_empty() {
        out.push_str("  No frontend consumers detected consuming this endpoint.\n");
    } else {
        let mut grouped: std::collections::BTreeMap<&str, Vec<&ozymem_parser::EndpointConsumer>> = std::collections::BTreeMap::new();
        for c in &report.consumers {
            grouped.entry(&c.file_path).or_default().push(c);
        }

        for (fp, call_sites) in grouped {
            out.push_str(&format!("  - {}:\n", fp));
            for site in call_sites {
                let method_badge = if site.http_method != "UNKNOWN" {
                    format!(" [{}]", site.http_method.to_uppercase())
                } else {
                    String::new()
                };
                let caller_str = site.caller_symbol.as_deref().unwrap_or("fetch");
                out.push_str(&format!(
                    "      L{}: in {}() -> {}{}\n",
                    site.line_number,
                    caller_str,
                    site.raw_endpoint,
                    method_badge
                ));
            }
        }
    }

    out
}

pub(crate) fn format_table_lineage(report: &ozymem_core::graph_backend::types::TableLineageReport) -> String {
    let mut out = format!(
        "[DATABASE TABLE LINEAGE]\nTable: {}\nTotal Referencing Files: {}\n\n",
        report.table_name, report.total_referencing_files
    );

    out.push_str(&format!(
        "[MIGRATIONS & DDL DEFINITIONS] ({})\n",
        report.migration_definitions.len()
    ));
    if report.migration_definitions.is_empty() {
        out.push_str("  No SQL migration files found defining or altering this table.\n");
    } else {
        for def in &report.migration_definitions {
            out.push_str(&format!(
                "  - [{}] {}:{} -> {}\n",
                def.operation,
                def.file_path,
                def.line_number,
                def.raw_statement
            ));
        }
    }
    out.push('\n');

    out.push_str(&format!(
        "[BACKEND MODELS & SERVICES] ({})\n",
        report.backend_models.len()
    ));
    if report.backend_models.is_empty() {
        out.push_str("  No backend ORM models found referencing this table.\n");
    } else {
        for model in &report.backend_models {
            out.push_str(&format!(
                "  - [{}] {}:{} -> {}\n",
                model.operation,
                model.file_path,
                model.line_number,
                model.raw_statement
            ));
        }
    }

    out
}

pub(crate) fn format_simulation_result(sim: &ozymem_core::graph_backend::types::SimulationResult) -> String {
    let verdict = if sim.breaks_dependents {
        "[STATUS: REJECTED: CONTRACT BREAKAGE]"
    } else {
        "[STATUS: APPROVED: NO CONTRACT BREAKAGE]"
    };

    let mut out = format!(
        "[SPECULATIVE GHOST PATCH REPORT]\nTarget Symbol: {}\nTarget File: {}\nVerdict: {}\nIsolation: [{}]\nExecution Time: {} ms\n\n",
        sim.target_symbol,
        if sim.target_file.is_empty() { "unknown" } else { &sim.target_file },
        verdict,
        sim.isolation_guarantee,
        sim.execution_time_ms
    );

    out.push_str("[CONTRACT ANALYSIS]\n");
    out.push_str(&format!("  • Arity Changed: {}\n", sim.arity_changed));
    out.push_str(&format!("  • Required Parameters: {} -> {}\n", sim.old_required_param_count, sim.new_required_param_count));
    out.push_str(&format!("  • Total Parameters: {} -> {}\n", sim.old_param_count, sim.new_param_count));
    out.push_str(&format!("  • Async Converted: {}\n", sim.async_changed));
    out.push_str(&format!("  • Impacted Callers Inspected: {}\n", sim.impacted_callers_count));
    out.push_str(&format!("  • Breaks Callers: {}\n\n", sim.breaks_dependents));

    out.push_str(&format!("[DETECTED BREAKAGES] ({})\n", sim.breakages.len()));
    if sim.breakages.is_empty() {
        out.push_str("  No broken callers found. The proposed change is backward-compatible with all existing callers.\n");
    } else {
        for b in &sim.breakages {
            out.push_str(&format!(
                "  - [CALLER_BREAKAGE] {}:{} in `{}`: {}\n    Code: {}\n",
                b.caller_file, b.line_number, b.caller_symbol, b.reason, b.line_content
            ));
        }
    }

    out
}

/// Compact tool outputs exceeding token budget (~3,500 tokens / 14,000 characters).
/// Persists the raw payload to `.ozymem/blobs/<short_hash>.txt` and replaces the text
/// with a summarized preview (head 40 lines, tail 10 lines) and an `ozymem://blob/<hash>` pointer.
pub fn compact_tool_output(content: &mut Vec<ContentBlock>, project_path: Option<&str>) {
    const COMPACT_THRESHOLD_CHARS: usize = 14_000;

    for block in content.iter_mut() {
        if block.text.len() > COMPACT_THRESHOLD_CHARS {
            let mut hasher = Sha256::new();
            hasher.update(block.text.as_bytes());
            let full_hash = format!("{:x}", hasher.finalize());
            let short_hash = &full_hash[..12];

            let blob_dir = match project_path {
                Some(p) => Path::new(p).join(".ozymem").join("blobs"),
                None => Path::new(".ozymem").join("blobs"),
            };

            if let Err(e) = std::fs::create_dir_all(&blob_dir) {
                eprintln!("[WARN: COMPACTOR] Failed to create blob directory {}: {e}", blob_dir.display());
            }

            let blob_file = blob_dir.join(format!("{short_hash}.txt"));
            if let Err(e) = std::fs::write(&blob_file, &block.text) {
                eprintln!("[WARN: COMPACTOR] Failed to persist blob {}: {e}", blob_file.display());
            }

            let lines: Vec<&str> = block.text.lines().collect();
            let total_lines = lines.len();
            let orig_len = block.text.len();
            let approx_tokens = orig_len / 4;

            let head_count = 40.min(total_lines);
            let head_lines = lines[..head_count].join("\n");

            let tail_lines = if total_lines > 50 {
                let tail_start = total_lines.saturating_sub(10);
                format!("\n\n--- Tail Preview (Last 10 lines) ---\n{}", lines[tail_start..].join("\n"))
            } else {
                String::new()
            };

            let compacted_msg = format!(
                "[ALERT: OUTPUT_COMPACTED_TO_PRESERVE_CONTEXT]\n\
                 Output exceeded context budget (original: {orig_len} chars, {total_lines} lines, ~{approx_tokens} tokens).\n\
                 Full uncompressed output persisted to:\n\
                 URI: ozymem://blob/{short_hash} (Path: .ozymem/blobs/{short_hash}.txt)\n\n\
                 --- Head Preview (Lines 1-{head_count}) ---\n\
                 {head_lines}{tail_lines}\n\n\
                 [NOTE: Read full output with resource ozymem://blob/{short_hash} or read_file on .ozymem/blobs/{short_hash}.txt]"
            );

            block.text = compacted_msg;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compact_tool_output_spillover() {
        let temp_dir = std::env::temp_dir().join(format!(
            "ozymem_test_compactor_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let project_path = temp_dir.to_str().unwrap();

        // Create a large payload > 14,000 chars
        let mut large_text = String::with_capacity(20_000);
        for i in 1..=600 {
            large_text.push_str(&format!("Line {}: sample data content exceeding token limits\n", i));
        }
        assert!(large_text.len() > 14_000);

        let mut content = vec![ContentBlock {
            kind: "text",
            text: large_text.clone(),
        }];

        compact_tool_output(&mut content, Some(project_path));

        assert_eq!(content.len(), 1);
        assert!(content[0].text.contains("[ALERT: OUTPUT_COMPACTED_TO_PRESERVE_CONTEXT]"));
        assert!(content[0].text.contains("URI: ozymem://blob/"));
        assert!(content[0].text.contains("--- Head Preview (Lines 1-40) ---"));
        assert!(content[0].text.contains("--- Tail Preview (Last 10 lines) ---"));

        // Verify blob file was written
        let blobs_dir = temp_dir.join(".ozymem").join("blobs");
        assert!(blobs_dir.exists());
        let entries: Vec<_> = std::fs::read_dir(&blobs_dir).unwrap().collect();
        assert_eq!(entries.len(), 1);
        let blob_content = std::fs::read_to_string(entries[0].as_ref().unwrap().path()).unwrap();
        assert_eq!(blob_content, large_text);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_compact_tool_output_under_threshold_untouched() {
        let small_text = "Short output under budget".to_string();
        let mut content = vec![ContentBlock {
            kind: "text",
            text: small_text.clone(),
        }];

        compact_tool_output(&mut content, None);
        assert_eq!(content[0].text, small_text);
    }
}





