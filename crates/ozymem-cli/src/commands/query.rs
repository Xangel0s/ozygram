use std::fs;
use std::path::{Path, PathBuf};
use ozymem_core::GraphSummary;
use crate::commands::scan::{canonicalize_file, clean_path};
use anyhow::Context;
use ozymem_core::{FileGraphContext, LessonRecord, StoredFunction};
use ozymem_parser::is_binary_file;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use crate::client::BackendClient;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryTranslatorOutput {
    pub mode: &'static str,
    pub intent: String,
    pub query: String,
    pub results: Vec<QueryTranslatorResult>,
    pub suggestions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryTranslatorResult {
    pub kind: String,
    pub path: Option<String>,
    pub line: Option<usize>,
    pub title: String,
    pub detail: String,
}

pub async fn run_query_translator(connection: &BackendClient, input: Vec<String>, json_output: bool, limit: usize, token_budget: usize) -> anyhow::Result<()> {
    let raw = input.join(" ").trim().to_string();
    if raw.is_empty() { return print_query_output(&safe_unknown_query("", "Consulta vacía"), json_output, token_budget); }
    let tokens = split_query_tokens(&raw);
    let verb = tokens.first().map(|s| s.to_lowercase()).unwrap_or_default();
    let args = tokens.iter().skip(1).cloned().collect::<Vec<_>>();
    let output = match verb.as_str() {
        "grep" | "rg" | "s" | "search" | "buscar" => query_grep_like(connection, &args, limit).await?,
        "find" | "sym" | "symbol" | "simbolo" | "símbolo" => query_find_symbol(connection, &args, limit).await?,
        "ctx" | "context" | "task" | "tarea" => query_context(connection, &args, limit).await?,
        "file" | "f" | "archivo" => query_file(connection, &args).await?,
        "trace" | "impact" | "i" => query_trace_or_tree(connection, &args, true, limit).await?,
        "tree" | "deps" | "dep" => query_trace_or_tree(connection, &args, false, limit).await?,
        "arch" | "architecture" | "arquitectura" => query_architecture(connection, limit).await?,
        "doctor" | "d" => query_doctor(connection).await?,
        "code" | "c" | "doctor-code" => query_code_doctor(connection, limit).await?,
        "skills" | "skill" | "sk" => query_skills(&args, limit),
        _ => if raw.len() >= 3 { query_grep_like(connection, &[raw.clone()], limit).await? } else { safe_unknown_query(&raw, "No pude traducir esta consulta de forma segura") },
    };
    print_query_output(&output, json_output, token_budget)
}

pub fn split_query_tokens(raw: &str) -> Vec<String> {
    let mut tokens = Vec::new(); let mut current = String::new(); let mut in_quotes = false;
    for ch in raw.chars() { match ch { '"' => in_quotes = !in_quotes, c if c.is_whitespace() && !in_quotes => { if !current.is_empty() { tokens.push(current.clone()); current.clear(); } }, c => current.push(c) } }
    if !current.is_empty() { tokens.push(current); }
    tokens
}

pub fn extract_line_range(args: &[String]) -> (Vec<String>, Option<(usize, usize)>) {
    let mut rest = Vec::new(); let mut range = None;
    for arg in args { if let Some((a, b)) = arg.split_once('-') { if let (Ok(start), Ok(end)) = (a.parse::<usize>(), b.parse::<usize>()) { range = Some((start.min(end), start.max(end))); continue; } } rest.push(arg.clone()); }
    (rest, range)
}

pub async fn query_grep_like(connection: &BackendClient, args: &[String], limit: usize) -> anyhow::Result<QueryTranslatorOutput> {
    let (args, range) = extract_line_range(args); let query = args.join(" ").trim().to_string();
    if query.is_empty() { return Ok(safe_unknown_query("grep", "Falta texto para buscar")); }
    let query_lower = query.to_lowercase(); let mut results = Vec::new();
    for symbol in connection.find_symbol(&query, &std::env::current_dir()?.to_string_lossy()).await.unwrap_or_default().into_iter().take(limit) {
        results.push(QueryTranslatorResult { kind: "symbol".to_string(), path: None, line: None, title: symbol, detail: "Coincidencia en símbolos indexados".to_string() });
    }
    for lesson in connection.get_recent_lessons(50, None).await.unwrap_or_default() {
        let haystack = format!("{} {} {}", lesson.file_path, lesson.error_type, lesson.solution).to_lowercase();
        if haystack.contains(&query_lower) && results.len() < limit { results.push(QueryTranslatorResult { kind: "memory".to_string(), path: Some(lesson.file_path), line: None, title: lesson.error_type, detail: compact_text(&lesson.solution, 180) }); }
    }
    let files = connection.get_all_file_paths().await.unwrap_or_default();
    'files: for file_path in files {
        if results.len() >= limit { break; }
        let path = Path::new(&file_path); if !path.exists() || is_binary_file(path) { continue; }
        let Ok(content) = fs::read_to_string(path) else { continue; };
        for (idx, line) in content.lines().enumerate() {
            let line_no = idx + 1; if let Some((start, end)) = range { if line_no < start || line_no > end { continue; } }
            if line.to_lowercase().contains(&query_lower) { results.push(QueryTranslatorResult { kind: "code".to_string(), path: Some(file_path.clone()), line: Some(line_no), title: format!("{}:L{}", short_path(&file_path), line_no), detail: compact_text(line.trim(), 220) }); if results.len() >= limit { break 'files; } break; }
        }
    }
    Ok(QueryTranslatorOutput { mode: "safe", intent: "grep_like_internal_search".to_string(), query, results, suggestions: vec!["ozymem q ctx \"tema\"".to_string(), "ozymem q find NombreSimbolo".to_string()] })
}

pub async fn query_find_symbol(connection: &BackendClient, args: &[String], limit: usize) -> anyhow::Result<QueryTranslatorOutput> {
    let query = args.join(" ").trim().to_string(); if query.is_empty() { return Ok(safe_unknown_query("find", "Falta nombre de símbolo")); }
    let results = connection.find_symbol(&query, &std::env::current_dir()?.to_string_lossy()).await.unwrap_or_default().into_iter().take(limit).map(|s| QueryTranslatorResult { kind: "symbol".to_string(), path: None, line: None, title: s, detail: "Símbolo indexado".to_string() }).collect();
    Ok(QueryTranslatorOutput { mode: "safe", intent: "find_symbol".to_string(), query, results, suggestions: vec!["ozymem q grep texto".to_string()] })
}

pub async fn query_context(connection: &BackendClient, args: &[String], limit: usize) -> anyhow::Result<QueryTranslatorOutput> {
    let query = args.join(" ").trim().to_string(); let mut results = Vec::new();
    for lesson in connection.get_recent_lessons(limit as i64, None).await.unwrap_or_default() { let haystack = format!("{} {} {}", lesson.file_path, lesson.error_type, lesson.solution).to_lowercase(); if query.is_empty() || haystack.contains(&query.to_lowercase()) { results.push(QueryTranslatorResult { kind: "context".to_string(), path: Some(lesson.file_path), line: None, title: lesson.error_type, detail: compact_text(&lesson.solution, 240) }); } }
    if results.is_empty() { let summary = connection.get_graph_summary().await?; results.push(QueryTranslatorResult { kind: "summary".to_string(), path: None, line: None, title: "Project summary".to_string(), detail: format!("files={} functions={} lessons={}", summary.file_count, summary.function_count, summary.engram_count) }); }
    Ok(QueryTranslatorOutput { mode: "safe", intent: "task_context".to_string(), query, results, suggestions: vec!["ozymem q grep texto".to_string(), "ozymem q arch".to_string()] })
}

pub async fn query_file(connection: &BackendClient, args: &[String]) -> anyhow::Result<QueryTranslatorOutput> {
    let file = args.join(" ").trim().to_string(); if file.is_empty() { return Ok(safe_unknown_query("file", "Falta ruta de archivo")); }
    let abs = canonicalize_file(&file).unwrap_or_else(|_| PathBuf::from(&file)); let clean = clean_path(&abs); let context = connection.get_file_context(&clean).await?; let mut results = Vec::new();
    if let Some(ctx) = context { results.push(QueryTranslatorResult { kind: "file".to_string(), path: Some(ctx.file_path.clone()), line: None, title: format!("{} ({})", short_path(&ctx.file_path), ctx.language), detail: format!("functions={}", ctx.functions.len()) }); for f in ctx.functions.iter().take(8) { results.push(QueryTranslatorResult { kind: "function".to_string(), path: Some(ctx.file_path.clone()), line: Some(f.start_line as usize), title: f.name.clone(), detail: format!("{} L{}-L{} via {}", f.kind, f.start_line, f.end_line, f.strategy) }); } }
    Ok(QueryTranslatorOutput { mode: "safe", intent: "file_context".to_string(), query: file, results, suggestions: vec!["ozymem scan .".to_string()] })
}

pub async fn query_trace_or_tree(connection: &BackendClient, args: &[String], incoming: bool, limit: usize) -> anyhow::Result<QueryTranslatorOutput> {
    let file = args.join(" ").trim().to_string(); if file.is_empty() { return Ok(safe_unknown_query(if incoming { "trace" } else { "tree" }, "Falta ruta de archivo")); }
    let abs = canonicalize_file(&file).unwrap_or_else(|_| PathBuf::from(&file)); let clean = clean_path(&abs); let deps = if incoming { connection.get_incoming_dependencies(&clean).await? } else { connection.get_outgoing_dependencies(&clean).await? };
    let results = deps.into_iter().take(limit).map(|p| QueryTranslatorResult { kind: if incoming { "dependent" } else { "dependency" }.to_string(), path: Some(p.clone()), line: None, title: short_path(&p), detail: if incoming { "Depende de este archivo" } else { "Dependencia usada por este archivo" }.to_string() }).collect();
    Ok(QueryTranslatorOutput { mode: "safe", intent: if incoming { "trace_incoming" } else { "tree_outgoing" }.to_string(), query: clean, results, suggestions: vec!["ozymem q arch".to_string()] })
}

pub async fn query_architecture(connection: &BackendClient, limit: usize) -> anyhow::Result<QueryTranslatorOutput> {
    let summary = connection.get_graph_summary().await?; let mut scored = Vec::new();
    for file in connection.get_all_file_paths().await.unwrap_or_default().into_iter().take(500) { let incoming = connection.get_incoming_dependencies(&file).await.unwrap_or_default().len(); let outgoing = connection.get_outgoing_dependencies(&file).await.unwrap_or_default().len(); let score = incoming + outgoing; if score > 0 { scored.push((score, incoming, outgoing, file)); } }
    scored.sort_by(|a, b| b.0.cmp(&a.0));
    let mut results = vec![QueryTranslatorResult { kind: "summary".to_string(), path: None, line: None, title: "Architecture summary".to_string(), detail: format!("files={} functions={} edges={} lessons={}", summary.file_count, summary.function_count, summary.edge_count, summary.engram_count) }];
    for (score, incoming, outgoing, file) in scored.into_iter().take(limit) { results.push(QueryTranslatorResult { kind: "hotspot".to_string(), path: Some(file.clone()), line: None, title: short_path(&file), detail: format!("score={score} incoming={incoming} outgoing={outgoing}") }); }
    Ok(QueryTranslatorOutput { mode: "safe", intent: "architecture_report_compact".to_string(), query: "arch".to_string(), results, suggestions: vec!["ozymem q trace <file>".to_string()] })
}

pub async fn query_doctor(connection: &BackendClient) -> anyhow::Result<QueryTranslatorOutput> {
    let summary = connection.get_graph_summary().await.unwrap_or(GraphSummary { file_count: 0, function_count: 0, engram_count: 0, native_ast_function_count: 0, extension_wasm_function_count: 0, text_heuristic_function_count: 0, vertex_count: 0, edge_count: 0, memory_usage: String::new(), lessons_without_embedding: 0 }); let severity = if summary.file_count == 0 { "warning" } else { "ok" };
    Ok(QueryTranslatorOutput { mode: "safe", intent: "doctor_compact".to_string(), query: "doctor".to_string(), results: vec![QueryTranslatorResult { kind: severity.to_string(), path: None, line: None, title: "Ozymem index".to_string(), detail: format!("files={} functions={} lessons={} missing_embeddings={}", summary.file_count, summary.function_count, summary.engram_count, summary.lessons_without_embedding) }], suggestions: vec!["ozymem scan .".to_string(), "ozymem q arch".to_string()] })
}

pub async fn query_code_doctor(connection: &BackendClient, limit: usize) -> anyhow::Result<QueryTranslatorOutput> { let mut results = query_architecture(connection, limit).await?.results; for r in &mut results { r.kind = format!("code_{}", r.kind); } Ok(QueryTranslatorOutput { mode: "safe", intent: "code_doctor_compact".to_string(), query: "code".to_string(), results, suggestions: vec!["ozymem q grep TODO".to_string(), "ozymem q arch".to_string()] }) }

pub fn query_skills(args: &[String], limit: usize) -> QueryTranslatorOutput {
    let query = args.join(" ").to_lowercase(); let mut skills = vec![("react", "facebook/react", "frontend"), ("vercel-ai", "vercel/ai", "ai-sdk"), ("openai-skills", "openai/skills", "openai"), ("supabase", "supabase/agent-skills", "database"), ("prisma", "prisma/skills", "database"), ("semgrep", "semgrep/skills", "security"), ("sentry", "getsentry/skills", "observability")];
    if !query.is_empty() { skills.retain(|(name, repo, cat)| name.contains(&query) || repo.contains(&query) || cat.contains(&query)); }
    let results = skills.into_iter().take(limit).map(|(name, repo, cat)| QueryTranslatorResult { kind: "official_skill".to_string(), path: None, line: None, title: name.to_string(), detail: format!("repo={repo} category={cat} origin=skills.sh/official review_only=true") }).collect();
    QueryTranslatorOutput { mode: "safe", intent: "skills_official_metadata".to_string(), query, results, suggestions: vec!["ozymem q sk react".to_string()] }
}

pub fn safe_unknown_query(query: &str, reason: &str) -> QueryTranslatorOutput { QueryTranslatorOutput { mode: "safe", intent: "unknown".to_string(), query: query.to_string(), results: vec![QueryTranslatorResult { kind: "safe_reject".to_string(), path: None, line: None, title: reason.to_string(), detail: "No se ejecutó shell externo ni comandos arbitrarios.".to_string() }], suggestions: vec!["ozymem q grep texto".to_string(), "ozymem q find NombreSimbolo".to_string(), "ozymem q ctx \"tarea\"".to_string()] } }

pub fn print_query_output(output: &QueryTranslatorOutput, json_output: bool, token_budget: usize) -> anyhow::Result<()> {
    if json_output { println!("{}", serde_json::to_string(output)?); return Ok(()); }
    let mut rendered = format!("[safe:{}] {}\n", output.intent, output.query);
    if output.results.is_empty() { rendered.push_str("Sin resultados.\n"); }
    for (idx, result) in output.results.iter().enumerate() { let loc = match (&result.path, result.line) { (Some(path), Some(line)) => format!(" {}:L{}", short_path(path), line), (Some(path), None) => format!(" {}", short_path(path)), _ => String::new() }; rendered.push_str(&format!("{}. [{}]{} {} — {}\n", idx + 1, result.kind, loc, result.title, result.detail)); if rendered.len() / 4 >= token_budget { rendered.push_str("... salida truncada por --tokens\n"); break; } }
    if !output.suggestions.is_empty() && rendered.len() / 4 < token_budget { rendered.push_str("Sugerencias: "); rendered.push_str(&output.suggestions.join(" | ")); rendered.push('\n'); }
    print!("{}", rendered); Ok(())
}

pub fn compact_text(value: &str, max_chars: usize) -> String { let mut one_line = value.split_whitespace().collect::<Vec<_>>().join(" "); if one_line.len() > max_chars { one_line.truncate(max_chars.saturating_sub(1)); one_line.push('…'); } one_line }

pub fn short_path(path: &str) -> String { let cwd = std::env::current_dir().ok(); if let Some(cwd) = cwd { if let Ok(rel) = Path::new(path).strip_prefix(cwd) { return rel.to_string_lossy().replace('\\', "/"); } } path.replace('\\', "/") }

pub async fn print_lessons(
    connection: &BackendClient,
    limit: usize,
    file_filter: Option<String>,
) -> anyhow::Result<()> {
    let limit = i64::try_from(limit).context("limit is too large")?;
    let lessons = connection.get_recent_lessons(limit, file_filter).await?;

    println!("HISTORICAL KNOWLEDGE BASE");
    println!("-------------------------");

    if lessons.is_empty() {
        println!("No historical lessons found.");
        return Ok(());
    }

    for lesson in lessons {
        print_lesson_record(&lesson);
    }

    Ok(())
}

pub fn print_lesson_record(lesson: &LessonRecord) {
    println!("[Error: {}] -> {}", lesson.error_type, lesson.file_path);
    println!("Solution: {}", lesson.solution);
    println!();
}

pub async fn print_tree(
    connection: &BackendClient,
    file_path: &str,
    depth: u32,
) -> anyhow::Result<()> {
    let absolute_path = canonicalize_file(file_path)?;
    let absolute_path_text = clean_path(&absolute_path);
    let mut visited = HashSet::new();

    let tree = load_tree_node(connection, &absolute_path_text, depth, &mut visited).await?;
    if tree.context.is_none() {
        println!("No indexed file found for {}", absolute_path_text);
        return Ok(());
    }

    render_tree_node(&tree, "", true, true);
    Ok(())
}

#[derive(Debug, Clone)]
pub struct TreeNode {
    pub path: String,
    pub context: Option<FileGraphContext>,
    pub functions: Vec<StoredFunction>,
    pub dependencies: Vec<TreeNode>,
    pub truncated: bool,
    pub cyclic: bool,
}

pub fn load_tree_node<'a>(
    connection: &'a BackendClient,
    file_path: &'a str,
    remaining_depth: u32,
    visited: &'a mut HashSet<String>,
) -> Pin<Box<dyn Future<Output = anyhow::Result<TreeNode>> + 'a>> {
    Box::pin(async move {
        let context = connection.get_file_context(file_path).await?;
        let functions = context
            .as_ref()
            .map(|context| context.functions.clone())
            .unwrap_or_default();
        let dependencies = connection.get_outgoing_dependencies(file_path).await?;

        let cyclic = !visited.insert(file_path.to_string());
        let truncated = remaining_depth == 0 && !dependencies.is_empty();

        let mut rendered_dependencies = Vec::new();
        if !cyclic && remaining_depth > 0 {
            for dependency in dependencies {
                let child_context = connection.get_file_context(&dependency).await?;
                let child_cyclic = visited.contains(&dependency);

                if child_cyclic {
                    rendered_dependencies.push(TreeNode {
                        path: dependency,
                        context: child_context,
                        functions: Vec::new(),
                        dependencies: Vec::new(),
                        truncated: false,
                        cyclic: true,
                    });
                    continue;
                }

                rendered_dependencies.push(
                    load_tree_node(connection, &dependency, remaining_depth - 1, visited).await?,
                );
            }
        }

        Ok(TreeNode {
            path: file_path.to_string(),
            context,
            functions,
            dependencies: rendered_dependencies,
            truncated,
            cyclic,
        })
    })
}

pub fn render_tree_node(node: &TreeNode, prefix: &str, is_last: bool, is_root: bool) {
    if !is_root && node.cyclic {
        let branch = if is_last { "└──" } else { "├──" };
        println!("{}{} [DEPENDS_ON] File: {} (already listed)", prefix, branch, node.path);
        return;
    }

    if is_root {
        println!("File: {}", node.path);
    } else {
        let branch = if is_last { "└──" } else { "├──" };
        println!("{}{} [DEPENDS_ON] File: {}", prefix, branch, node.path);
    }

    let next_prefix = if is_root {
        String::new()
    } else if is_last {
        format!("{prefix}    ")
    } else {
        format!("{prefix}│   ")
    };

    let has_dependencies = !node.dependencies.is_empty() || node.truncated;
    let functions_branch = if has_dependencies {
        "├──"
    } else {
        "└──"
    };
    println!("{}{} Functions", next_prefix, functions_branch);

    if node.functions.is_empty() {
        let leaf_prefix = if has_dependencies {
            format!("{next_prefix}│   ")
        } else {
            format!("{next_prefix}    ")
        };
        println!("{}└── (none)", leaf_prefix);
    } else {
        let function_prefix = if has_dependencies {
            format!("{next_prefix}│   ")
        } else {
            format!("{next_prefix}    ")
        };

        for (index, function) in node.functions.iter().enumerate() {
            let branch = if index + 1 == node.functions.len() {
                "└──"
            } else {
                "├──"
            };
            println!(
                "{}{} [MEMBER: {}] {} (lines {}-{}) via {}",
                function_prefix,
                branch,
                function.kind.to_uppercase(),
                function.name,
                function.start_line,
                function.end_line,
                function.strategy
            );
        }
    }

    println!("{}└── Dependencies", next_prefix);

    let dependency_prefix = format!("{next_prefix}    ");
    if node.cyclic {
        println!("{}└── (cycle)", dependency_prefix);
        return;
    }

    if node.truncated {
        println!("{}└── (depth limit reached)", dependency_prefix);
        return;
    }

    if node.dependencies.is_empty() {
        println!("{}└── (none)", dependency_prefix);
        return;
    }

    for (index, dependency) in node.dependencies.iter().enumerate() {
        render_tree_node(
            dependency,
            &dependency_prefix,
            index + 1 == node.dependencies.len(),
            false,
        );
    }
}

pub async fn print_trace(
    connection: &BackendClient,
    file_path: &str,
    depth: u32,
) -> anyhow::Result<()> {
    let absolute_path = canonicalize_file(file_path)?;
    let absolute_path_text = clean_path(&absolute_path);
    let mut visited = HashSet::new();

    let trace = load_trace_node(connection, &absolute_path_text, depth, &mut visited).await?;
    if trace.context.is_none() {
        println!("No indexed file found for {}", absolute_path_text);
        return Ok(());
    }

    render_trace_node(&trace, "", true, true);
    Ok(())
}

pub fn load_trace_node<'a>(
    connection: &'a BackendClient,
    file_path: &'a str,
    remaining_depth: u32,
    visited: &'a mut HashSet<String>,
) -> Pin<Box<dyn Future<Output = anyhow::Result<TreeNode>> + 'a>> {
    Box::pin(async move {
        let context = connection.get_file_context(file_path).await?;
        let functions = context
            .as_ref()
            .map(|context| context.functions.clone())
            .unwrap_or_default();
        let incoming = connection.get_incoming_dependencies(file_path).await?;

        let cyclic = !visited.insert(file_path.to_string());
        let truncated = remaining_depth == 0 && !incoming.is_empty();

        let mut rendered_incoming = Vec::new();
        if !cyclic && remaining_depth > 0 {
            for dependent in incoming {
                let child_context = connection.get_file_context(&dependent).await?;
                let child_cyclic = visited.contains(&dependent);

                if child_cyclic {
                    rendered_incoming.push(TreeNode {
                        path: dependent,
                        context: child_context,
                        functions: Vec::new(),
                        dependencies: Vec::new(),
                        truncated: false,
                        cyclic: true,
                      });
                      continue;
                }

                rendered_incoming.push(
                    load_trace_node(connection, &dependent, remaining_depth - 1, visited).await?,
                );
            }
        }

        Ok(TreeNode {
            path: file_path.to_string(),
            context,
            functions,
            dependencies: rendered_incoming,
            truncated,
            cyclic,
        })
    })
}

pub fn render_trace_node(node: &TreeNode, prefix: &str, is_last: bool, is_root: bool) {
    if !is_root && node.cyclic {
        let branch = if is_last { "└──" } else { "├──" };
        println!("{}{} [IMPACTED_BY] File: {} (already listed)", prefix, branch, node.path);
        return;
    }

    if is_root {
        println!("File: {} (Target)", node.path);
    } else {
        let branch = if is_last { "└──" } else { "├──" };
        println!("{}{} [IMPACTED_BY] File: {}", prefix, branch, node.path);
    }

    let next_prefix = if is_root {
        String::new()
    } else if is_last {
        format!("{prefix}    ")
    } else {
        format!("{prefix}│   ")
    };

    let has_incoming = !node.dependencies.is_empty() || node.truncated;
    let functions_branch = if has_incoming {
        "├──"
    } else {
        "└──"
    };
    println!("{}{} Functions", next_prefix, functions_branch);

    if node.functions.is_empty() {
        let leaf_prefix = if has_incoming {
            format!("{next_prefix}│   ")
        } else {
            format!("{next_prefix}    ")
        };
        println!("{}└── (none)", leaf_prefix);
    } else {
        let function_prefix = if has_incoming {
            format!("{next_prefix}│   ")
        } else {
            format!("{next_prefix}    ")
        };

        for (index, function) in node.functions.iter().enumerate() {
            let branch = if index + 1 == node.functions.len() {
                "└──"
            } else {
                "├──"
            };
            println!(
                "{}{} [MEMBER: {}] {} (lines {}-{}) via {}",
                function_prefix,
                branch,
                function.kind.to_uppercase(),
                function.name,
                function.start_line,
                function.end_line,
                function.strategy
            );
        }
    }

    println!("{}└── Incoming Dependencies", next_prefix);

    let incoming_prefix = format!("{next_prefix}    ");
    if node.cyclic {
        println!("{}└── (cycle)", incoming_prefix);
        return;
    }

    if node.truncated {
        println!("{}└── (depth limit reached)", incoming_prefix);
        return;
    }

    if node.dependencies.is_empty() {
        println!("{}└── (none)", incoming_prefix);
        return;
    }

    for (index, dependent) in node.dependencies.iter().enumerate() {
        render_trace_node(
            dependent,
            &incoming_prefix,
            index + 1 == node.dependencies.len(),
            false,
        );
    }
}

