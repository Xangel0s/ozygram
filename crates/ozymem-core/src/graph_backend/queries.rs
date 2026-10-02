use std::collections::HashMap;
use anyhow::{Context, Result};
use petgraph::algo::all_simple_paths;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::Bfs;
use rusqlite::params;
use crate::GraphSummary;
use crate::mcp_common::McpBackend;
use crate::graph_backend::types::{FileEdge, FileNode, GraphBackend, ImpactEntry, LiteralMatch, LiteralSearchResult, SymbolReplaceResult, UnifiedSearchResult};

impl GraphBackend {
    pub fn analyze_impact(&self, file_path: &str, depth: u32) -> Vec<ImpactEntry> {
        let resolved = self.resolve_target_path(file_path).unwrap_or_else(|| file_path.to_string());
        let norm_path = crate::normalize_path(file_path);
        let inner = self.inner.lock().unwrap();
        let start_opt = inner.file_index.get(&resolved)
            .or_else(|| inner.file_index.get(&norm_path))
            .or_else(|| inner.file_index.get(file_path));
        let Some(&start) = start_opt else {
            return vec![];
        };

        let mut results = Vec::new();
        let mut bfs = Bfs::new(&inner.graph, start);

        let mut visited: HashMap<NodeIndex, u32> = HashMap::new();
        visited.insert(start, 0);

        // Prepare function query for enriching results
        let mut func_stmt = inner.sqlite.prepare(
            "SELECT name, start_line FROM functions WHERE file_path = ?1 AND tenant_id = ?2 ORDER BY start_line LIMIT 10"
        ).ok();

        // Prepare line-range query
        let mut range_stmt = inner.sqlite.prepare(
            "SELECT COALESCE(MIN(start_line),0), COALESCE(MAX(end_line),0) FROM functions WHERE file_path = ?1 AND tenant_id = ?2"
        ).ok();

        while let Some(node) = bfs.next(&inner.graph) {
            let d = *visited.get(&node).unwrap_or(&0);
            for neighbor in inner.graph.neighbors(node) {
                visited.entry(neighbor).or_insert(d + 1);
            }

            if node == start {
                continue;
            }
            if d > depth {
                continue;
            }
            if let Some(fn_data) = inner.graph.node_weight(node) {
                let path = &fn_data.path;
                let lower = path.to_lowercase();

                // Severity
                let (severity, reason) = if lower.contains("schema")
                    || lower.contains("model")
                    || lower.contains("dto")
                    || lower.contains("entity")
                    || lower.contains("interface")
                    || lower.contains("type")
                {
                    ("breaking", "Contains schema/model/dto/entity/interface/type definitions — changes here affect data contracts".to_string())
                } else if fn_data.lesson_count > 0 {
                    (
                        "warning",
                        format!(
                            "Has {} known lesson(s) registered — previous issues found in this file",
                            fn_data.lesson_count
                        ),
                    )
                } else if fn_data.function_count > 15 {
                    (
                        "warning",
                        format!(
                            "Large file ({} functions) — high risk of cascading changes",
                            fn_data.function_count
                        ),
                    )
                } else if d <= 1 {
                    (
                        "warning",
                        "Direct dependency at depth 1 — changes propagate immediately".to_string(),
                    )
                } else {
                    (
                        "info",
                        "Indirect dependency — unlikely to require changes".to_string(),
                    )
                };

                let suggestion = match severity {
                    "breaking" => "Review and update all imports, type definitions, and data contracts in dependent files".to_string(),
                    "warning" => "Check for cascading changes in consuming code; verify tests still pass".to_string(),
                    _ => "Monitor for indirect effects; no immediate action required".to_string(),
                };

                // Get function names with line numbers
                let functions: Vec<String> = func_stmt
                    .as_mut()
                    .and_then(|stmt| {
                        stmt.query_map(params![path, self.tenant_id], |row| {
                            let name: String = row.get(0)?;
                            let line: i64 = row.get(1)?;
                            Ok(format!("{}:{}", name, line))
                        })
                        .ok()
                    })
                    .map(|rows| rows.filter_map(|r| r.ok()).collect())
                    .unwrap_or_default();

                // Get line range for this file
                let (start_line, end_line) = range_stmt
                    .as_mut()
                    .and_then(|stmt| {
                        stmt.query_row(params![path, self.tenant_id], |row| {
                            let s: i64 = row.get(0)?;
                            let e: i64 = row.get(1)?;
                            Ok((s, e))
                        })
                        .ok()
                    })
                    .unwrap_or((0, 0));

                results.push(ImpactEntry {
                    file_path: path.clone(),
                    depth: d,
                    function_count: fn_data.function_count,
                    lesson_count: fn_data.lesson_count,
                    language: fn_data.language.clone(),
                    severity: severity.to_string(),
                    functions,
                    start_line,
                    end_line,
                    reason,
                    suggestion,
                });
            }
        }
        results
    }

    pub fn get_incoming_deps(&self, file_path: &str) -> Vec<String> {
        let resolved = self
            .resolve_target_path(file_path)
            .unwrap_or_else(|| file_path.to_string());
        let norm_path = crate::normalize_path(file_path);
        let inner = self.inner.lock().unwrap();
        let start_opt = inner
            .file_index
            .get(&resolved)
            .or_else(|| inner.file_index.get(&norm_path))
            .or_else(|| inner.file_index.get(file_path));
        let Some(&idx) = start_opt else {
            return vec![];
        };
        inner
            .graph
            .neighbors_directed(idx, petgraph::Direction::Incoming)
            .filter_map(|n| inner.graph.node_weight(n).map(|w| w.path.clone()))
            .collect()
    }

    pub fn get_outgoing_deps(&self, file_path: &str) -> Vec<String> {
        let resolved = self
            .resolve_target_path(file_path)
            .unwrap_or_else(|| file_path.to_string());
        let norm_path = crate::normalize_path(file_path);
        let inner = self.inner.lock().unwrap();
        let start_opt = inner
            .file_index
            .get(&resolved)
            .or_else(|| inner.file_index.get(&norm_path))
            .or_else(|| inner.file_index.get(file_path));
        let Some(&idx) = start_opt else {
            return vec![];
        };
        inner
            .graph
            .neighbors_directed(idx, petgraph::Direction::Outgoing)
            .filter_map(|n| inner.graph.node_weight(n).map(|w| w.path.clone()))
            .collect()
    }

    pub async fn unified_search(
        &self,
        query: &str,
        scope: Option<&str>,
        _mode: Option<&str>,
        limit: usize,
    ) -> anyhow::Result<Vec<UnifiedSearchResult>> {
        let target_scope = scope.unwrap_or("all");
        let mut results = Vec::new();

        // 1. Literal & Symbol Search
        if target_scope == "all" || target_scope == "code" {
            let inner = self.inner.lock().unwrap();
            let mut stmt = inner.sqlite.prepare(
                "SELECT name, kind, file_path, start_line FROM functions WHERE name LIKE ?1 LIMIT ?2"
            )?;
            let query_like = format!("%{query}%");
            let rows = stmt.query_map(params![query_like, limit as i64], |row| {
                Ok(UnifiedSearchResult {
                    category: "symbol".to_string(),
                    title: format!("{}: {}", row.get::<_, String>(1)?, row.get::<_, String>(0)?),
                    path: row.get(2)?,
                    snippet: format!("Línea {}", row.get::<_, i64>(3)?),
                    score: 1.0,
                })
            })?;
            for r in rows.flatten() {
                results.push(r);
            }
        }

        // 2. Lessons & Memory Search
        if target_scope == "all" || target_scope == "lessons" {
            if let Ok(lessons) = self.search_lessons(query, None, limit).await {
                for l in lessons {
                    results.push(UnifiedSearchResult {
                        category: "lesson".to_string(),
                        title: format!("[{}] {}", l.kind, l.file_path),
                        path: l.file_path,
                        snippet: format!("{}: {}", l.error_context, l.solution),
                        score: 0.9,
                    });
                }
            }
        }

        results.truncate(limit);
        Ok(results)
    }


    pub fn find_graph_path(
        &self,
        from: &str,
        to: &str,
        max_paths: usize,
        max_hops: usize,
    ) -> Vec<Vec<String>> {
        let from_res = self.resolve_target_path(from).unwrap_or_else(|| from.to_string());
        let to_res = self.resolve_target_path(to).unwrap_or_else(|| to.to_string());
        let inner = self.inner.lock().unwrap();
        let start = match inner.file_index.get(&from_res).or_else(|| inner.file_index.get(from)) {
            Some(n) => *n,
            None => return Vec::new(),
        };
        let end = match inner.file_index.get(&to_res).or_else(|| inner.file_index.get(to)) {
            Some(n) => *n,
            None => return Vec::new(),
        };

        let paths = all_simple_paths::<Vec<NodeIndex>, &DiGraph<FileNode, FileEdge>>(
            &inner.graph,
            start,
            end,
            0,
            Some(max_hops),
        );

        let mut results = Vec::new();
        for (i, path) in paths.enumerate() {
            if i >= max_paths {
                break;
            }
            let file_paths: Vec<String> = path
                .iter()
                .filter_map(|n| inner.graph.node_weight(*n))
                .map(|n| n.path.clone())
                .collect();
            results.push(file_paths);
        }
        results
    }

    /// Delete all indexed data (files, functions, dependencies) but NOT lessons.
    pub fn clear_data(&self) -> Result<()> {
        let inner = self.inner.lock().unwrap();
        inner.sqlite.execute(
            "DELETE FROM file_dependencies WHERE tenant_id = ?1",
            params![self.tenant_id],
        )?;
        inner.sqlite.execute(
            "DELETE FROM functions WHERE tenant_id = ?1",
            params![self.tenant_id],
        )?;
        inner.sqlite.execute(
            "DELETE FROM files WHERE tenant_id = ?1",
            params![self.tenant_id],
        )?;
        Ok(())
    }

    /// Resolve an input file path in cascade: exact, relative to project_path, normalized, or suffix matching.
    pub fn resolve_target_path(&self, input_path: &str) -> Option<String> {
        let norm = crate::normalize_path(input_path);
        let inner = self.inner.lock().unwrap();

        // 1. Direct match in in-memory file index
        if inner.file_index.contains_key(&norm) {
            return Some(norm);
        }
        if inner.file_index.contains_key(input_path) {
            return Some(input_path.to_string());
        }

        // 1b. Canonicalized path match (resolves symlinks such as /tmp -> /private/tmp on macOS)
        if let Ok(canon) = std::fs::canonicalize(input_path) {
            let canon_norm = crate::normalize_path(&canon.to_string_lossy());
            if inner.file_index.contains_key(&canon_norm) {
                return Some(canon_norm);
            }
        }

        // 2. Direct match in SQLite files table
        let mut stmt = match inner.sqlite.prepare(
            "SELECT path FROM files WHERE (path = ?1 OR path = ?2) AND tenant_id = ?3 LIMIT 1"
        ) {
            Ok(s) => s,
            Err(_) => return None,
        };
        if let Ok(found) = stmt.query_row(params![input_path, norm, self.tenant_id], |row| row.get::<_, String>(0)) {
            return Some(found);
        }

        // 3. Resolve relative path against project_path / workspace_root
        let root_opt = inner.project_path.as_deref().or(if !inner.workspace_root.is_empty() { Some(&inner.workspace_root) } else { None });
        if let Some(root) = root_opt {
            let joined_path = std::path::Path::new(root).join(input_path);
            let joined = crate::normalize_path(&joined_path.to_string_lossy());
            if inner.file_index.contains_key(&joined) {
                return Some(joined);
            }
            if let Ok(canon) = std::fs::canonicalize(&joined_path) {
                let canon_norm = crate::normalize_path(&canon.to_string_lossy());
                if inner.file_index.contains_key(&canon_norm) {
                    return Some(canon_norm);
                }
            }
            if let Ok(found) = stmt.query_row(params![joined, joined, self.tenant_id], |row| row.get::<_, String>(0)) {
                return Some(found);
            }
        }

        // 4. Suffix matching in in-memory index
        let norm_clean = norm.replace('\\', "/");
        for k in inner.file_index.keys() {
            let k_norm = k.replace('\\', "/");
            if k_norm.ends_with(&norm_clean) || k_norm.ends_with(&format!("/{}", norm_clean)) {
                return Some(k.clone());
            }
        }

        // 5. Fallback SQLite LIKE on suffix
        let suffix_like = format!("%{}", input_path.replace('/', "\\"));
        let suffix_like_fwd = format!("%{}", input_path.replace('\\', "/"));
        let mut suffix_stmt = match inner.sqlite.prepare(
            "SELECT path FROM files WHERE (path LIKE ?1 OR path LIKE ?2 OR replace(path, '\\', '/') LIKE ?2) AND tenant_id = ?3 ORDER BY length(path) ASC LIMIT 1"
        ) {
            Ok(s) => s,
            Err(_) => return None,
        };
        if let Ok(found) = suffix_stmt.query_row(params![suffix_like, suffix_like_fwd, self.tenant_id], |row| row.get::<_, String>(0)) {
            return Some(found);
        }

        None
    }

    /// Search AST indexed symbols (functions, classes, structs) when explicit lessons are empty.

    pub fn get_subpath_graph_summary(&self, subpath: &str) -> Result<GraphSummary> {
        let norm_sub = crate::normalize_path(subpath).replace('\\', "/");
        let like1 = format!("%{}%", norm_sub);
        let like2 = format!("%{}%", norm_sub.replace('/', "\\"));
        let inner = self.inner.lock().unwrap();

        let file_count: i64 = inner
            .sqlite
            .query_row(
                "SELECT COUNT(*) FROM files WHERE tenant_id = ?1 AND (path LIKE ?2 OR replace(path, '\\', '/') LIKE ?3)",
                params![self.tenant_id, like2, like1],
                |r| r.get(0),
            )
            .unwrap_or(0);

        let function_count: i64 = inner
            .sqlite
            .query_row(
                "SELECT COUNT(*) FROM functions WHERE tenant_id = ?1 AND (file_path LIKE ?2 OR replace(file_path, '\\', '/') LIKE ?3)",
                params![self.tenant_id, like2, like1],
                |r| r.get(0),
            )
            .unwrap_or(0);

        let engram_count: i64 = inner
            .sqlite
            .query_row(
                "SELECT COUNT(*) FROM lessons WHERE tenant_id = ?1 AND (file_path LIKE ?2 OR replace(file_path, '\\', '/') LIKE ?3)",
                params![self.tenant_id, like2, like1],
                |r| r.get(0),
            )
            .unwrap_or(0);

        let edge_count: i64 = inner
            .sqlite
            .query_row(
                "SELECT COUNT(*) FROM file_dependencies WHERE tenant_id = ?1 AND (origin_path LIKE ?2 OR replace(origin_path, '\\', '/') LIKE ?3)",
                params![self.tenant_id, like2, like1],
                |r| r.get(0),
            )
            .unwrap_or(0);

        let mut native = 0i64;
        let mut extension = 0i64;
        let mut text = 0i64;
        if let Ok(mut stmt) = inner.sqlite.prepare(
            "SELECT strategy, COUNT(*) FROM functions WHERE tenant_id = ?1 AND (file_path LIKE ?2 OR replace(file_path, '\\', '/') LIKE ?3) GROUP BY strategy",
        ) {
            if let Ok(rows) = stmt.query_map(params![self.tenant_id, like2, like1], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            }) {
                for row in rows.flatten() {
                    match row.0.as_str() {
                        "NativeAST" => native += row.1,
                        "ExtensionWASM" => extension += row.1,
                        _ => text += row.1,
                    }
                }
            }
        }

        Ok(GraphSummary {
            file_count,
            function_count,
            engram_count,
            native_ast_function_count: native,
            extension_wasm_function_count: extension,
            text_heuristic_function_count: text,
            vertex_count: file_count,
            edge_count,
            memory_usage: format!("{} MB", (file_count * 200) / 1024 / 1024 + 1),
            lessons_without_embedding: 0,
        })
    }

    /// Speculative Context Decoding: Obtiene contratos Engram predictivos de archivos vecinos
    /// inmediatos (dependencias y dependientes) en tiempo O(1) usando el grafo AST y MemTable.
    pub fn get_speculative_engrams(&self, file_path: &str, limit: usize) -> Vec<ozymem_parser::EngramContract> {
        let resolved = self.resolve_target_path(file_path).unwrap_or_else(|| file_path.to_string());
        let norm_path = crate::normalize_path(file_path);

        // 1. Obtener archivos vecinos desde el grafo petgraph
        let mut candidate_neighbor_files = Vec::new();
        {
            let inner = self.inner.lock().unwrap();
            let start_opt = inner.file_index.get(&resolved)
                .or_else(|| inner.file_index.get(&norm_path))
                .or_else(|| inner.file_index.get(file_path));

            if let Some(&start) = start_opt {
                for neighbor in inner.graph.neighbors(start) {
                    if let Some(node_data) = inner.graph.node_weight(neighbor) {
                        candidate_neighbor_files.push(node_data.path.clone());
                    }
                }
                for edge in inner.graph.raw_edges() {
                    if edge.target() == start {
                        if let Some(node_data) = inner.graph.node_weight(edge.source()) {
                            if !candidate_neighbor_files.contains(&node_data.path) {
                                candidate_neighbor_files.push(node_data.path.clone());
                            }
                        }
                    }
                }
            }
        }

        // Si no hay vecinos explícitos en grafo, buscar en el mismo subdirectorio
        if candidate_neighbor_files.is_empty() {
            let parent_dir = std::path::Path::new(&resolved)
                .parent()
                .and_then(|p| p.to_str())
                .unwrap_or("");
            if !parent_dir.is_empty() {
                let inner = self.inner.lock().unwrap();
                for (path, _) in &inner.file_index {
                    if path != &resolved && path != &norm_path && path.starts_with(parent_dir) {
                        candidate_neighbor_files.push(path.clone());
                        if candidate_neighbor_files.len() >= 4 {
                            break;
                        }
                    }
                }
            }
        }

        // 2. Resolver funciones y contratos Engram de los vecinos
        let mut speculative_contracts = Vec::new();
        let inner = self.inner.lock().unwrap();
        for neighbor_path in candidate_neighbor_files {
            if let Ok(mut stmt) = inner.sqlite.prepare(
                "SELECT name FROM functions WHERE (file_path = ?1 OR file_path = ?2) AND tenant_id = ?3 ORDER BY start_line LIMIT 5"
            ) {
                let norm_neighbor = crate::normalize_path(&neighbor_path);
                if let Ok(rows) = stmt.query_map(params![neighbor_path, norm_neighbor, self.tenant_id], |r| r.get::<_, String>(0)) {
                    for sym in rows.flatten() {
                        let symbol_path = format!("{}::{}", neighbor_path, sym);
                        if let Some(contract) = self.engram_store.lookup(&symbol_path) {
                            speculative_contracts.push(contract);
                            if speculative_contracts.len() >= limit {
                                return speculative_contracts;
                            }
                        }
                    }
                }
            }
        }

        speculative_contracts
    }

    /// Surgical symbol extraction: returns exact symbol source code slice and metadata.
    pub fn get_symbol(&self, file_path: &str, symbol_name: &str) -> Result<Option<ozymem_parser::SymbolExtract>> {
        let resolved = self.resolve_target_path(file_path).unwrap_or_else(|| file_path.to_string());
        
        let path_obj = std::path::Path::new(&resolved);
        let effective_path = if path_obj.exists() {
            path_obj.to_path_buf()
        } else if let Some(root) = self.project_path() {
            std::path::Path::new(&root).join(&resolved)
        } else {
            std::path::PathBuf::from(&resolved)
        };

        if !effective_path.exists() {
            return Ok(None);
        }

        let source = std::fs::read_to_string(&effective_path)?;
        let lang = ozymem_parser::SupportedLanguage::from_path(&effective_path);

        Ok(ozymem_parser::extract_symbol_source(&resolved, symbol_name, &source, lang))
    }

    /// Returns skeletal AST file member tree and outgoing dependencies.
    pub fn get_file_tree(&self, file_path: &str, depth: u32, format: &str) -> Result<String> {
        let resolved = self.resolve_target_path(file_path).unwrap_or_else(|| file_path.to_string());
        let norm_path = crate::normalize_path(file_path);

        let inner = self.inner.lock().unwrap();
        let mut functions = Vec::new();
        if let Ok(mut stmt) = inner.sqlite.prepare(
            "SELECT name, kind, start_line, end_line, strategy FROM functions WHERE (file_path = ?1 OR file_path = ?2) AND tenant_id = ?3 ORDER BY start_line, name"
        ) {
            if let Ok(rows) = stmt.query_map(params![resolved, norm_path, self.tenant_id], |r| {
                Ok(crate::StoredFunction {
                    name: r.get(0)?,
                    kind: r.get(1)?,
                    start_line: r.get(2)?,
                    end_line: r.get(3)?,
                    strategy: r.get(4)?,
                })
            }) {
                functions = rows.filter_map(|r| r.ok()).collect();
            }
        }

        let mut outgoing = Vec::new();
        if let Ok(mut stmt) = inner.sqlite.prepare(
            "SELECT target_path FROM dependencies WHERE (source_path = ?1 OR source_path = ?2) AND tenant_id = ?3 ORDER BY target_path"
        ) {
            if let Ok(rows) = stmt.query_map(params![resolved, norm_path, self.tenant_id], |r| r.get::<_, String>(0)) {
                outgoing = rows.filter_map(|r| r.ok()).collect();
            }
        }
        drop(inner);

        // Fallback: If not yet indexed in database, parse source on the fly
        if functions.is_empty() {
            let path_obj = std::path::Path::new(&resolved);
            let effective = if path_obj.exists() {
                path_obj.to_path_buf()
            } else if let Some(root) = self.project_path() {
                std::path::Path::new(&root).join(&resolved)
            } else {
                std::path::PathBuf::from(&resolved)
            };
            if effective.exists() {
                if let Ok(source) = std::fs::read_to_string(&effective) {
                    let lang = ozymem_parser::SupportedLanguage::from_path(&effective);
                    if let Ok(map) = ozymem_parser::parse_source(&resolved, lang, &source) {
                        for f in map.functions {
                            functions.push(crate::StoredFunction {
                                name: f.name,
                                kind: f.kind.as_str().to_string(),
                                start_line: f.start_line as i64,
                                end_line: f.end_line as i64,
                                strategy: map.strategy.as_str().to_string(),
                            });
                        }
                    }
                    if let Ok(hints) = ozymem_parser::extract_dependency_hints(&resolved, lang, &source) {
                        for h in hints {
                            if ozymem_parser::is_internal_dependency_hint(&h) {
                                if let Some(target) = ozymem_parser::resolve_dependency_target(&h, &resolved) {
                                    outgoing.push(target.to_string_lossy().to_string());
                                }
                            }
                        }
                    }
                }
            }
        }

        if format == "json" {
            return Ok(serde_json::to_string_pretty(&serde_json::json!({
                "file_path": resolved,
                "functions": functions,
                "dependencies": outgoing,
                "depth": depth
            }))?);
        }

        let mut out = String::new();
        out.push_str(&format!("File: {}\n", resolved));
        let has_deps = !outgoing.is_empty();
        let fn_branch = if has_deps { "├──" } else { "└──" };
        out.push_str(&format!("{} Functions\n", fn_branch));

        let fn_prefix = if has_deps { "│   " } else { "    " };
        if functions.is_empty() {
            out.push_str(&format!("{}└── (none)\n", fn_prefix));
        } else {
            for (idx, f) in functions.iter().enumerate() {
                let branch = if idx + 1 == functions.len() { "└──" } else { "├──" };
                out.push_str(&format!(
                    "{}{} [MEMBER: {}] {} (lines {}-{}) via {}\n",
                    fn_prefix,
                    branch,
                    f.kind.to_uppercase(),
                    f.name,
                    f.start_line,
                    f.end_line,
                    f.strategy
                ));
            }
        }

        out.push_str("└── Dependencies\n");
        if outgoing.is_empty() {
            out.push_str("    └── (none)\n");
        } else {
            for (idx, dep) in outgoing.iter().enumerate() {
                let branch = if idx + 1 == outgoing.len() { "└──" } else { "├──" };
                out.push_str(&format!("    {} [DEPENDS_ON] File: {}\n", branch, dep));
            }
        }

        Ok(out)
    }

    /// Surgical AST symbol replacement with pre-flight syntax verification and auto-reindexing.
    pub fn replace_symbol(
        &self,
        file_path: &str,
        symbol_name: &str,
        new_code: &str,
        dry_run: bool,
    ) -> Result<SymbolReplaceResult> {
        let resolved = self.resolve_target_path(file_path).unwrap_or_else(|| file_path.to_string());
        let path_obj = std::path::Path::new(&resolved);
        let effective_path = if path_obj.exists() {
            path_obj.to_path_buf()
        } else if let Some(root) = self.project_path() {
            std::path::Path::new(&root).join(&resolved)
        } else {
            std::path::PathBuf::from(&resolved)
        };

        if !effective_path.exists() {
            return Ok(SymbolReplaceResult {
                success: false,
                file_path: resolved.clone(),
                symbol_name: symbol_name.to_string(),
                old_start_line: 0,
                old_end_line: 0,
                new_start_line: 0,
                new_end_line: 0,
                lines_diff: 0,
                preview_diff: String::new(),
                dry_run,
                ast_verified: false,
                error: Some(format!("[ALERT: FILE_NOT_FOUND] File `{}` does not exist", effective_path.display())),
            });
        }

        let source = std::fs::read_to_string(&effective_path)?;
        let lang = ozymem_parser::SupportedLanguage::from_path(&effective_path);

        let is_crlf = source.contains("\r\n");
        let newline = if is_crlf { "\r\n" } else { "\n" };

        let map = ozymem_parser::parse_source(&resolved, lang, &source)?;
        let Some(target_fn) = map.functions.iter().find(|f| f.name == symbol_name || f.name.eq_ignore_ascii_case(symbol_name)) else {
            let available: Vec<String> = map.functions.iter().map(|f| format!("{} ({})", f.name, f.kind.as_str())).collect();
            return Ok(SymbolReplaceResult {
                success: false,
                file_path: resolved.clone(),
                symbol_name: symbol_name.to_string(),
                old_start_line: 0,
                old_end_line: 0,
                new_start_line: 0,
                new_end_line: 0,
                lines_diff: 0,
                preview_diff: String::new(),
                dry_run,
                ast_verified: false,
                error: Some(format!("[ALERT: SYMBOL_NOT_FOUND] Symbol `{}` not found in `{}`. Available symbols: [{}]", symbol_name, resolved, available.join(", "))),
            });
        };

        let old_start_line = target_fn.start_line;
        let old_end_line = target_fn.end_line;

        let raw_lines: Vec<&str> = if is_crlf {
            source.split("\r\n").collect()
        } else {
            source.split('\n').collect()
        };

        if old_start_line == 0 || old_start_line > raw_lines.len() || old_end_line > raw_lines.len() {
            return Ok(SymbolReplaceResult {
                success: false,
                file_path: resolved.clone(),
                symbol_name: symbol_name.to_string(),
                old_start_line,
                old_end_line,
                new_start_line: 0,
                new_end_line: 0,
                lines_diff: 0,
                preview_diff: String::new(),
                dry_run,
                ast_verified: false,
                error: Some(format!("[ALERT: INVALID_LINE_RANGE] Lines {}-{} are out of bounds (total lines: {})", old_start_line, old_end_line, raw_lines.len())),
            });
        }

        let before_lines = &raw_lines[..(old_start_line - 1)];
        let old_symbol_lines = &raw_lines[(old_start_line - 1)..old_end_line];
        let after_lines = &raw_lines[old_end_line..];

        let new_code_lines: Vec<&str> = if is_crlf {
            new_code.split("\r\n").collect()
        } else {
            new_code.split('\n').collect()
        };

        let new_lines_count = new_code_lines.len();
        let old_lines_count = old_symbol_lines.len();
        let lines_diff = new_lines_count as i64 - old_lines_count as i64;
        let new_start_line = old_start_line;
        let new_end_line = old_start_line + new_lines_count.saturating_sub(1);

        let mut final_lines: Vec<&str> = Vec::with_capacity(before_lines.len() + new_lines_count + after_lines.len());
        final_lines.extend_from_slice(before_lines);
        final_lines.extend_from_slice(&new_code_lines);
        final_lines.extend_from_slice(after_lines);

        let candidate_source = final_lines.join(newline);

        // Pre-flight AST syntax verification
        let diagnostics = ozymem_parser::extract_ast_diagnostics(&resolved, lang, &candidate_source);
        if !diagnostics.is_empty() {
            let diag_messages: Vec<String> = diagnostics
                .iter()
                .map(|d| format!("line {}: {}", d.line_number, d.message))
                .collect();
            return Ok(SymbolReplaceResult {
                success: false,
                file_path: resolved.clone(),
                symbol_name: symbol_name.to_string(),
                old_start_line,
                old_end_line,
                new_start_line,
                new_end_line,
                lines_diff,
                preview_diff: String::new(),
                dry_run,
                ast_verified: false,
                error: Some(format!(
                    "[ALERT: AST_SYNTAX_ERROR] The proposed code introduces syntax errors in `{}`: {}. File unmodified.",
                    resolved,
                    diag_messages.join("; ")
                )),
            });
        }

        let mut diff_preview = format!("--- {}\n+++ {}\n@@ -{},{} +{},{} @@ [SYMBOL: {}]\n", resolved, resolved, old_start_line, old_lines_count, new_start_line, new_lines_count, symbol_name);
        for line in old_symbol_lines {
            diff_preview.push_str(&format!("-{}\n", line));
        }
        for line in &new_code_lines {
            diff_preview.push_str(&format!("+{}\n", line));
        }

        if !dry_run {
            std::fs::write(&effective_path, &candidate_source)?;
            if let Some(root) = self.project_path() {
                let _ = self.index_file_delta(&effective_path, std::path::Path::new(&root));
            }
        }

        Ok(SymbolReplaceResult {
            success: true,
            file_path: resolved,
            symbol_name: symbol_name.to_string(),
            old_start_line,
            old_end_line,
            new_start_line,
            new_end_line,
            lines_diff,
            preview_diff: diff_preview,
            dry_run,
            ast_verified: true,
            error: None,
        })
    }

    /// High-performance literal/regex search across workspace with token budgeting.
    pub fn search_literal(
        &self,
        query: &str,
        path_prefix: Option<&str>,
        is_regex: bool,
        case_sensitive: bool,
        token_budget: usize,
        max_matches: usize,
        context_lines: usize,
    ) -> Result<LiteralSearchResult> {
        let root_dir = if let Some(p) = path_prefix {
            let resolved = self.resolve_target_path(p).unwrap_or_else(|| p.to_string());
            std::path::PathBuf::from(resolved)
        } else if let Some(root) = self.project_path() {
            std::path::PathBuf::from(root)
        } else {
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
        };

        let re = if is_regex {
            regex::RegexBuilder::new(query)
                .case_insensitive(!case_sensitive)
                .build()
                .with_context(|| format!("Invalid regex pattern: '{query}'"))?
        } else {
            let escaped = regex::escape(query);
            regex::RegexBuilder::new(&escaped)
                .case_insensitive(!case_sensitive)
                .build()
                .with_context(|| format!("Failed to build literal matcher for '{query}'"))?
        };

        let mut matches = Vec::new();
        let mut estimated_tokens: usize = 0;
        let mut status = "success".to_string();
        let mut total_scanned = 0;

        let walker = walkdir::WalkDir::new(&root_dir)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| {
                if e.file_type().is_dir() {
                    !crate::graph_backend::helpers::is_noise_dir(e.path())
                } else {
                    true
                }
            });

        for entry in walker {
            let Ok(entry) = entry else { continue };
            if !entry.file_type().is_file() { continue };
            let path = entry.path();

            if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
                let ext_lower = ext.to_lowercase();
                if matches!(ext_lower.as_str(), "exe" | "dll" | "so" | "png" | "jpg" | "jpeg" | "gif" | "webp" | "ico" | "wasm" | "zip" | "tar" | "gz" | "parquet" | "duckdb" | "db" | "lock" | "onnx" | "bin" | "pyc") {
                    continue;
                }
            }

            if let Ok(meta) = entry.metadata() {
                if meta.len() > 512 * 1024 {
                    continue;
                }
            }

            total_scanned += 1;
            let Ok(content) = std::fs::read_to_string(path) else { continue };
            let lines: Vec<&str> = content.lines().collect();

            for (idx, line) in lines.iter().enumerate() {
                if re.is_match(line) {
                    let line_number = idx + 1;
                    let trimmed = line.trim();
                    let snippet = if trimmed.len() > 250 {
                        format!("{}...", &trimmed[..247])
                    } else {
                        trimmed.to_string()
                    };
                    let before_ctx = if context_lines > 0 {
                        let s = idx.saturating_sub(context_lines);
                        lines[s..idx].iter().map(|l| l.trim().to_string()).collect()
                    } else {
                        Vec::new()
                    };
                    let after_ctx = if context_lines > 0 {
                        let e = (idx + 1 + context_lines).min(lines.len());
                        lines[(idx + 1)..e].iter().map(|l| l.trim().to_string()).collect()
                    } else {
                        Vec::new()
                    };

                    let snippet_token_cost = (snippet.len() + 10) / 4 + 1
                        + before_ctx.iter().map(|s| (s.len() + 10) / 4).sum::<usize>()
                        + after_ctx.iter().map(|s| (s.len() + 10) / 4).sum::<usize>();

                    if estimated_tokens + snippet_token_cost > token_budget && !matches.is_empty() {
                        status = "budget_reached".to_string();
                        break;
                    }

                    estimated_tokens += snippet_token_cost;
                    let rel_path = path
                        .strip_prefix(&root_dir)
                        .map(|p| p.to_string_lossy().replace('\\', "/"))
                        .unwrap_or_else(|_| path.to_string_lossy().replace('\\', "/"));

                    matches.push(LiteralMatch {
                        file_path: rel_path,
                        line_number,
                        line_content: snippet,
                        before_context: before_ctx,
                        after_context: after_ctx,
                    });

                    if matches.len() >= max_matches {
                        status = "max_matches_reached".to_string();
                        break;
                    }
                }
            }
            if status != "success" {
                break;
            }
        }

        Ok(LiteralSearchResult {
            status,
            query: query.to_string(),
            matches_count: matches.len(),
            total_scanned_files: total_scanned,
            estimated_tokens,
            matches,
        })
    }
}