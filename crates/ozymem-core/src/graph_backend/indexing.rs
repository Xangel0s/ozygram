use std::sync::Arc;
use crate::StoredFunction;
use std::collections::HashMap;
use crate::sync::{check_noise_or_huge_file, DeltaIndexResult, NoiseFilterDecision, DEFAULT_MAX_DELTA_FILE_BYTES};
use crate::graph_backend::helpers::mark_stale_lessons;
use anyhow::Result;
use petgraph::graph::DiGraph;
use petgraph::visit::EdgeRef;
use rusqlite::params;
use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::time::Instant;
use sha2::{Digest, Sha256};
use ozymem_parser::{
    extract_dependency_hints, is_internal_dependency_hint,
    resolve_dependency_target, parse_source, SymbolKind, is_binary_file,
};
use crate::graph_backend::helpers::{detect_language, file_mtime, is_noise_dir, load_ignore_patterns, path_matches_ignore};
use crate::graph_backend::types::{FileEdge, FileNode, GraphBackend};

impl GraphBackend {
    pub fn reload_if_stale(&self) {
        if self.scanning.load(Ordering::SeqCst) {
            return;
        }

        {
            let mut last = self.last_check.lock().unwrap();
            if last.elapsed() < std::time::Duration::from_millis(500) {
                return;
            }
            *last = Instant::now();
        }

        let proj_path = {
            let inner = self.inner.lock().unwrap();
            inner.project_path.clone()
        };
        let Some(ref proj_path) = proj_path else {
            return;
        };
        if !Path::new(proj_path).exists() {
            return;
        }

        let sqlite_snapshot = {
            let inner = self.inner.lock().unwrap();
            let mut stmt = inner
                .sqlite
                .prepare("SELECT path, mtime FROM files WHERE tenant_id = ?1")
                .ok();
            let file_mtimes: HashMap<String, String> = match stmt {
                Some(ref mut s) => s
                    .query_map(params![self.tenant_id], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                    })
                    .ok()
                    .map(|rows| rows.filter_map(|r| r.ok()).collect())
                    .unwrap_or_default(),
                None => HashMap::new(),
            };
            file_mtimes
        };

        if sqlite_snapshot.is_empty() {
            return;
        }

        let mut changed = false;
        let proj_ref = Path::new(proj_path);
        let ignore_patterns = load_ignore_patterns(proj_ref);
        let has_ignores = !ignore_patterns.is_empty();
        for entry in walkdir::WalkDir::new(proj_path)
            .into_iter()
            .filter_entry(|e| {
                if is_noise_dir(e.path()) {
                    return false;
                }
                if has_ignores && path_matches_ignore(e.path(), &ignore_patterns, proj_ref) {
                    return false;
                }
                true
            })
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if !path.is_file() || is_binary_file(path) {
                continue;
            }
            let abs_path = path.to_string_lossy().to_string();
            let current_mtime = file_mtime(path).unwrap_or_default();
            let stored = sqlite_snapshot.get(&abs_path);
            if stored.map_or(true, |s| *s != current_mtime) {
                changed = true;
                break;
            }
        }

        if changed {
            eprintln!("[graph] changes detected, rescanning...");
            self.scanning.store(true, Ordering::SeqCst);
            if let Err(e) = self.full_scan(proj_path, None) {
                eprintln!("[graph] scan error: {e}");
            }
            self.scanning.store(false, Ordering::SeqCst);
        }
    }

    pub fn full_scan(&self, project_path: &str, progress: Option<&dyn Fn(u64, u64)>) -> Result<()> {
        self.scanning.store(true, Ordering::SeqCst);

        // Collect scanned file paths for staleness comparison
        let mut scanned_files: HashSet<String> = HashSet::new();

        // Load ignore patterns from .ozymignore and .gitignore
        let ignore_patterns = load_ignore_patterns(Path::new(project_path));
        let has_ignores = !ignore_patterns.is_empty();

        // Clear stale data for this tenant before re-scan
        {
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
            inner.sqlite.execute(
                "DELETE FROM ast_diagnostics WHERE tenant_id = ?1",
                params![self.tenant_id],
            )?;
        }

        let proj_root = Path::new(project_path);

        // Pre-count total files for progress reporting
        let total_file_count: u64 = walkdir::WalkDir::new(project_path)
            .into_iter()
            .filter_entry(|e| {
                if is_noise_dir(e.path()) {
                    return false;
                }
                if has_ignores && path_matches_ignore(e.path(), &ignore_patterns, proj_root) {
                    return false;
                }
                true
            })
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_file() && !is_binary_file(e.path()))
            .count() as u64;

        let mut file_count = 0i64;
        let mut skipped_noise = 0u64;
        let mut skipped_ignore = 0u64;
        let mut skipped_binary = 0u64;
        let mut skipped_read = 0u64;
        let mut skipped_parse = 0u64;
        let mut processed: u64 = 0;
        for entry in walkdir::WalkDir::new(project_path)
            .into_iter()
            .filter_entry(|e| {
                let noise = is_noise_dir(e.path());
                if noise && e.path().is_dir() {
                    skipped_noise += 1;
                }
                if !noise
                    && has_ignores
                    && path_matches_ignore(e.path(), &ignore_patterns, proj_root)
                {
                    if e.path().is_dir() {
                        skipped_ignore += 1;
                    }
                    return false;
                }
                !noise
            })
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            if is_binary_file(path) {
            let abs_path = {
                let raw = crate::normalize_path(&path.to_string_lossy());
                std::fs::canonicalize(path)
                    .map(|c| crate::normalize_path(&c.to_string_lossy()))
                    .unwrap_or(raw)
            };
            if ozymem_parser::is_excel_template_candidate(&abs_path) {
                    if let Ok(Some(meta)) = ozymem_parser::parse_excel_template(path, &abs_path) {
                        let _ = self.record_excel_template(&meta);
                    }
                }
                skipped_binary += 1;
                if let Some(cb) = progress {
                    processed += 1;
                    cb(processed, total_file_count);
                }
                continue;
            }
            let abs_path = {
                let raw = crate::normalize_path(&path.to_string_lossy());
                std::fs::canonicalize(path)
                    .map(|c| crate::normalize_path(&c.to_string_lossy()))
                    .unwrap_or(raw)
            };
            scanned_files.insert(abs_path.clone());
            let mtime = file_mtime(path).unwrap_or_default();

            let source = match std::fs::read_to_string(path) {
                Ok(s) => s,
                Err(_) => {
                    skipped_read += 1;
                    if let Some(cb) = progress {
                        processed += 1;
                        cb(processed, total_file_count);
                    }
                    continue;
                }
            };
            let lang = detect_language(path);
            let parsed = match parse_source(&abs_path, lang, &source) {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("[graph] parse error {abs_path}: {e}");
                    skipped_parse += 1;
                    if let Some(cb) = progress {
                        processed += 1;
                        cb(processed, total_file_count);
                    }
                    continue;
                }
            };

            let engram_contracts = ozymem_parser::extract_engram_contracts(&abs_path, lang, &source);
            self.engram_store.update_file_incremental(&abs_path, engram_contracts);

            let sha256_hash = {
                let mut hasher = Sha256::new();
                hasher.update(source.as_bytes());
                hex::encode(hasher.finalize())
            };

            let inner = self.inner.lock().unwrap();
            inner.sqlite.execute(
                "INSERT OR REPLACE INTO files (path, language, strategy, mtime, tenant_id, workspace_root, sha256) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![abs_path, parsed.language, parsed.strategy.as_str(), mtime, self.tenant_id, inner.workspace_root, sha256_hash],
            )?;

            for fn_data in &parsed.functions {
                let kind_str = match fn_data.kind {
                    SymbolKind::Function => "Function",
                    SymbolKind::Class => "Class",
                };
                inner.sqlite.execute(
                    "INSERT OR REPLACE INTO functions (name, kind, start_line, end_line, strategy, file_path, tenant_id, workspace_root) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![fn_data.name, kind_str, fn_data.start_line as i64, fn_data.end_line as i64, parsed.strategy.as_str(), abs_path, self.tenant_id, inner.workspace_root],
                )?;
            }

            let hints = extract_dependency_hints(&abs_path, lang, &source).unwrap_or_default();
            for hint in &hints {
                if !is_internal_dependency_hint(hint) {
                    continue;
                }
                if let Some(target) = resolve_dependency_target(hint, &abs_path) {
                    let target_str = crate::normalize_path(&target.to_string_lossy());
                    inner.sqlite.execute(
                        "INSERT OR IGNORE INTO file_dependencies (origin_path, destination_path, tenant_id, workspace_root) VALUES (?1, ?2, ?3, ?4)",
                        params![abs_path, target_str, self.tenant_id, inner.workspace_root],
                    )?;
                }
            }

            let diags = ozymem_parser::extract_ast_diagnostics(&abs_path, lang, &source);
            for d in &diags {
                inner.sqlite.execute(
                    "INSERT INTO ast_diagnostics (file_path, line_number, severity, message, tenant_id, workspace_root) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![abs_path, d.line_number as i64, d.severity, d.message, self.tenant_id, inner.workspace_root],
                )?;
            }

            file_count += 1;
            if let Some(cb) = progress {
                processed += 1;
                cb(processed, total_file_count);
            }
        }

        // Mark stale lessons — only for files that were part of this scan
        if !scanned_files.is_empty() {
            let inner = self.inner.lock().unwrap();
            let marked = mark_stale_lessons(
                &inner.sqlite,
                &self.tenant_id,
                &scanned_files,
                &inner.workspace_root,
            )?;
            if marked > 0 {
                eprintln!("[graph] marked {marked} lessons as stale");
            }
        }

        self.rebuild_graph()?;

        self.scanning.store(false, Ordering::SeqCst);
        let inner = self.inner.lock().unwrap();
        eprintln!(
            "[graph] scan complete: indexed={file_count}, noise_dirs={skipped_noise}, ignore_pat={skipped_ignore}, binary={skipped_binary}, read_err={skipped_read}, parse_err={skipped_parse}; graph: {} nodes, {} edges",
            inner.graph.node_count(),
            inner.graph.edge_count()
        );
        Ok(())
    }

    pub fn rebuild_graph(&self) -> Result<()> {
        let rows: Vec<(String, String, String, i64, i64)>;
        let deps: Vec<(String, String)>;

        {
            let inner = self.inner.lock().unwrap();

            let mut stmt = inner.sqlite.prepare(
                 "SELECT f.path, f.language, f.strategy,
                        (SELECT COUNT(*) FROM functions fn WHERE fn.file_path = f.path AND fn.tenant_id = f.tenant_id) AS fc,
                        (SELECT COUNT(*) FROM lessons l WHERE l.file_path = f.path AND l.tenant_id = f.tenant_id) AS lc
                 FROM files f WHERE f.tenant_id = ?1 ORDER BY f.path ASC"
            )?;
            rows = stmt
                .query_map(params![self.tenant_id], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, i64>(4)?,
                    ))
                })?
                .filter_map(|r| r.ok())
                .collect();

            let mut dep_stmt = inner.sqlite.prepare(
                "SELECT origin_path, destination_path FROM file_dependencies WHERE tenant_id = ?1",
            )?;
            deps = dep_stmt
                .query_map(params![self.tenant_id], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?
                .filter_map(|r| r.ok())
                .collect();
        }

        let mut graph = DiGraph::new();
        let mut file_index = HashMap::new();

        for (path, language, strategy, fc, lc) in &rows {
            let idx = graph.add_node(FileNode {
                path: path.clone(),
                language: language.clone(),
                strategy: strategy.clone(),
                function_count: *fc,
                lesson_count: *lc,
            });
            file_index.insert(path.clone(), idx);
        }

        for (origin, destination) in &deps {
            if let (Some(&from_idx), Some(&to_idx)) =
                (file_index.get(origin), file_index.get(destination))
            {
                graph.add_edge(from_idx, to_idx, FileEdge);
            }
        }

        let mut inner_w = self.inner.lock().unwrap();
        inner_w.graph = graph;
        inner_w.file_index = file_index;
        drop(inner_w);

        let _ = self.engram_store.compact_and_persist();

        Ok(())
    }

    /// Incrementally indexes a single file if modified (Live Delta Indexing).
    /// Skips files that match ignore patterns, noise dirs, minified/lockfiles, or unchanged SHA-256.
    pub fn index_file_delta(&self, file_path: &Path, project_root: &Path) -> Result<DeltaIndexResult> {
        // 1. Noise, Lockfile, Minified, Size limit check
        match check_noise_or_huge_file(file_path, DEFAULT_MAX_DELTA_FILE_BYTES) {
            NoiseFilterDecision::Process => {}
            NoiseFilterDecision::NoiseDir
            | NoiseFilterDecision::MinifiedOrBundle
            | NoiseFilterDecision::LockFile
            | NoiseFilterDecision::LargeOrBinaryData => {
                return Ok(DeltaIndexResult::SkippedNoise);
            }
            NoiseFilterDecision::ExceedsSizeLimit(size_bytes) => {
                return Ok(DeltaIndexResult::SkippedTooLarge { size_bytes });
            }
        }

        // 2. Binary file check
        if is_binary_file(file_path) {
            let abs_path = crate::normalize_path(&file_path.to_string_lossy());
            if ozymem_parser::is_excel_template_candidate(&abs_path) {
                if let Ok(Some(meta)) = ozymem_parser::parse_excel_template(file_path, &abs_path) {
                    let _ = self.record_excel_template(&meta);
                }
            }
            return Ok(DeltaIndexResult::SkippedBinary);
        }

        // 3. Ignore patterns check
        let ignore_patterns = load_ignore_patterns(project_root);
        if !ignore_patterns.is_empty() && path_matches_ignore(file_path, &ignore_patterns, project_root) {
            return Ok(DeltaIndexResult::SkippedNoise);
        }

        // 4. Read source content and calculate SHA-256 (with Windows lock backoff)
        let source = match crate::sync::read_file_with_backoff(file_path, 3) {
            Ok(s) => s,
            Err(_) => return Ok(DeltaIndexResult::SkippedNoise),
        };

        let sha256_hash = {
            let mut hasher = Sha256::new();
            hasher.update(source.as_bytes());
            hex::encode(hasher.finalize())
        };

        let raw_path = crate::normalize_path(&file_path.to_string_lossy());
        let abs_path = std::fs::canonicalize(file_path)
            .map(|c| crate::normalize_path(&c.to_string_lossy()))
            .unwrap_or_else(|_| raw_path.clone());
        let mtime = file_mtime(file_path).unwrap_or_default();

        // 5. Compare with existing SHA-256 in SQLite
        {
            let inner = self.inner.lock().unwrap();
            let mut stmt = inner.sqlite.prepare(
                "SELECT sha256 FROM files WHERE (path = ?1 OR path = ?2) AND tenant_id = ?3",
            )?;
            let mut rows = stmt.query(params![abs_path, raw_path, self.tenant_id])?;
            if let Some(row) = rows.next()? {
                let existing_sha: String = row.get(0)?;
                if !existing_sha.is_empty() && existing_sha == sha256_hash {
                    return Ok(DeltaIndexResult::Unchanged);
                }
            }
        }

        // 6. Parse AST
        let lang = detect_language(file_path);
        let parsed = match parse_source(&abs_path, lang, &source) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("[delta] parse error for {abs_path}: {e}");
                return Ok(DeltaIndexResult::SkippedNoise);
            }
        };

        let hints = extract_dependency_hints(&abs_path, lang, &source).unwrap_or_default();
        let mut resolved_deps: Vec<String> = Vec::new();
        for hint in &hints {
            if is_internal_dependency_hint(hint) {
                if let Some(target) = resolve_dependency_target(hint, &abs_path) {
                    resolved_deps.push(crate::normalize_path(&target.to_string_lossy()));
                }
            }
        }

        // 7. Update SQLite atomically
        {
            let inner = self.inner.lock().unwrap();
            inner.sqlite.execute(
                "DELETE FROM functions WHERE (file_path = ?1 OR file_path = ?2) AND tenant_id = ?3",
                params![abs_path, raw_path, self.tenant_id],
            )?;
            inner.sqlite.execute(
                "DELETE FROM file_dependencies WHERE (origin_path = ?1 OR origin_path = ?2) AND tenant_id = ?3",
                params![abs_path, raw_path, self.tenant_id],
            )?;
            inner.sqlite.execute(
                "DELETE FROM ast_diagnostics WHERE (file_path = ?1 OR file_path = ?2) AND tenant_id = ?3",
                params![abs_path, raw_path, self.tenant_id],
            )?;

            inner.sqlite.execute(
                "INSERT OR REPLACE INTO files (path, language, strategy, mtime, tenant_id, workspace_root, sha256) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![abs_path, parsed.language, parsed.strategy.as_str(), mtime, self.tenant_id, inner.workspace_root, sha256_hash],
            )?;

            for fn_data in &parsed.functions {
                let kind_str = match fn_data.kind {
                    SymbolKind::Function => "Function",
                    SymbolKind::Class => "Class",
                };
                inner.sqlite.execute(
                    "INSERT OR REPLACE INTO functions (name, kind, start_line, end_line, strategy, file_path, tenant_id, workspace_root) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![fn_data.name, kind_str, fn_data.start_line as i64, fn_data.end_line as i64, parsed.strategy.as_str(), abs_path, self.tenant_id, inner.workspace_root],
                )?;
            }

            for dep in &resolved_deps {
                inner.sqlite.execute(
                    "INSERT OR IGNORE INTO file_dependencies (origin_path, destination_path, tenant_id, workspace_root) VALUES (?1, ?2, ?3, ?4)",
                    params![abs_path, dep, self.tenant_id, inner.workspace_root],
                )?;
            }

            let diags = ozymem_parser::extract_ast_diagnostics(&abs_path, lang, &source);
            for d in &diags {
                inner.sqlite.execute(
                    "INSERT INTO ast_diagnostics (file_path, line_number, severity, message, tenant_id, workspace_root) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![abs_path, d.line_number as i64, d.severity, d.message, self.tenant_id, inner.workspace_root],
                )?;
            }

            for dep in &resolved_deps {
                inner.sqlite.execute(
                    "INSERT OR IGNORE INTO file_dependencies (origin_path, destination_path, tenant_id, workspace_root) VALUES (?1, ?2, ?3, ?4)",
                    params![abs_path, dep, self.tenant_id, inner.workspace_root],
                )?;
            }
        }

        let engram_contracts = ozymem_parser::extract_engram_contracts(&abs_path, lang, &source);
        self.engram_store.update_file_incremental(&abs_path, engram_contracts);

        // 8. Update In-Memory Graph and File Index directly
        {
            let mut inner = self.inner.lock().unwrap();
            let strategy_str = parsed.strategy.as_str().to_string();
            let func_len = parsed.functions.len() as i64;
            let current_tenant = self.tenant_id.clone();
            let mut lesson_count = 0i64;
            if let Ok(mut lstmt) = inner.sqlite.prepare("SELECT COUNT(*) FROM lessons WHERE file_path = ?1 AND tenant_id = ?2") {
                if let Ok(mut lrows) = lstmt.query(params![abs_path, current_tenant]) {
                    if let Ok(Some(lrow)) = lrows.next() {
                        lesson_count = lrow.get(0).unwrap_or(0);
                    }
                }
            }

            let node_idx = if let Some(&existing_idx) = inner.file_index.get(&abs_path) {
                if let Some(weight) = inner.graph.node_weight_mut(existing_idx) {
                    weight.language = lang.as_str().to_string();
                    weight.strategy = strategy_str;
                    weight.function_count = func_len;
                    weight.lesson_count = lesson_count;
                }
                existing_idx
            } else {
                let new_idx = inner.graph.add_node(FileNode {
                    path: abs_path.clone(),
                    language: lang.as_str().to_string(),
                    strategy: strategy_str,
                    function_count: func_len,
                    lesson_count,
                });
                inner.file_index.insert(abs_path.clone(), new_idx);
                new_idx
            };

            let edges_to_remove: Vec<_> = inner.graph.edges_directed(node_idx, petgraph::Direction::Outgoing)
                .map(|e| e.id())
                .collect();
            for edge_id in edges_to_remove {
                inner.graph.remove_edge(edge_id);
            }

            for dep in &resolved_deps {
                if let Some(&dest_idx) = inner.file_index.get(dep) {
                    inner.graph.add_edge(node_idx, dest_idx, FileEdge);
                }
            }
        }

        Ok(DeltaIndexResult::Indexed {
            symbols: parsed.functions.len(),
            dependencies: resolved_deps.len(),
        })
    }

    /// Removes a file and its relations from SQLite and in-memory graph (Delta Indexing).
    pub fn remove_file_delta(&self, file_path: &Path, _project_root: &Path) -> Result<bool> {
        let raw_path = crate::normalize_path(&file_path.to_string_lossy());
        let abs_path = std::fs::canonicalize(file_path)
            .map(|c| crate::normalize_path(&c.to_string_lossy()))
            .unwrap_or_else(|_| raw_path.clone());

        self.engram_store.remove_file_incremental(&abs_path);
        self.engram_store.remove_file_incremental(&raw_path);

        let affected = {
            let inner = self.inner.lock().unwrap();

            inner.sqlite.execute(
                "DELETE FROM file_dependencies WHERE (origin_path = ?1 OR origin_path = ?2 OR destination_path = ?1 OR destination_path = ?2) AND tenant_id = ?3",
                params![abs_path, raw_path, self.tenant_id],
            )?;
            inner.sqlite.execute(
                "DELETE FROM functions WHERE (file_path = ?1 OR file_path = ?2) AND tenant_id = ?3",
                params![abs_path, raw_path, self.tenant_id],
            )?;
            inner.sqlite.execute(
                "DELETE FROM ast_diagnostics WHERE (file_path = ?1 OR file_path = ?2) AND tenant_id = ?3",
                params![abs_path, raw_path, self.tenant_id],
            )?;
            inner.sqlite.execute(
                "DELETE FROM files WHERE (path = ?1 OR path = ?2) AND tenant_id = ?3",
                params![abs_path, raw_path, self.tenant_id],
            )?
        };

        if affected > 0 {
            self.rebuild_graph()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Extracts API route definitions across project source files or a specific file.

    pub async fn scan_project_background(self: Arc<Self>, project_path: String) {
        self.set_project_path(Some(&project_path));
        self.full_scan(&project_path, None).ok();
        eprintln!("[scan] complete for {project_path}");
    }

    /// Autocomplete a file path fragment against the indexed files table.
    /// Returns up to `limit` matching paths.
    pub fn complete_file_path(&self, fragment: &str, limit: usize) -> Result<Vec<String>> {
        let inner = self.inner.lock().unwrap();
        let pattern = format!("%{}%", fragment);
        let mut stmt = inner.sqlite.prepare(
            "SELECT path FROM files WHERE tenant_id = ?1 AND path LIKE ?2 ORDER BY path ASC LIMIT ?3"
        )?;
        let rows = stmt.query_map(params![self.tenant_id, pattern, limit as i64], |row| {
            row.get::<_, String>(0)
        })?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    /// Return all indexed file paths for the current tenant.
    pub fn list_all_files(&self) -> Result<Vec<String>> {
        let inner = self.inner.lock().unwrap();
        let mut stmt = inner
            .sqlite
            .prepare("SELECT path FROM files WHERE tenant_id = ?1 ORDER BY path")?;
        let rows = stmt.query_map(params![self.tenant_id], |row| row.get::<_, String>(0))?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    /// Find simple paths between two files in the dependency graph.
    /// Returns up to `max_paths` paths, each as a list of file paths from `from` to `to`.
    /// Path length is limited to `max_hops` intermediate nodes.

    pub fn search_ast_symbols(&self, query: &str, limit: usize) -> Result<Vec<StoredFunction>> {
        let inner = self.inner.lock().unwrap();
        let limit = (limit as i64).min(50).max(1);
        let like_pattern = format!("%{}%", query);

        let mut stmt = inner.sqlite.prepare(
            "SELECT name, kind, start_line, end_line, strategy, file_path 
             FROM functions 
             WHERE (name LIKE ?1 OR file_path LIKE ?1) AND tenant_id = ?2 
             ORDER BY CASE WHEN name LIKE ?1 THEN 0 ELSE 1 END, name ASC 
             LIMIT ?3"
        )?;

        let funcs: Vec<StoredFunction> = stmt
            .query_map(params![like_pattern, self.tenant_id, limit], |row| {
                Ok(StoredFunction {
                    name: row.get(0)?,
                    kind: row.get(1)?,
                    start_line: row.get(2)?,
                    end_line: row.get(3)?,
                    strategy: format!("{} in {}", row.get::<_, String>(4)?, row.get::<_, String>(5)?),
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(funcs)
    }

    /// Filter graph summary by a subpath or monorepo sub-scope.

    pub fn get_ast_diagnostics(&self, limit: usize) -> Result<Vec<ozymem_parser::AstDiagnostic>> {
        let inner = self.inner.lock().unwrap();
        let limit = (limit as i64).min(100).max(1);
        let mut stmt = inner.sqlite.prepare(
            "SELECT file_path, line_number, severity, message FROM ast_diagnostics WHERE tenant_id = ?1 ORDER BY id DESC LIMIT ?2"
        )?;
        let diags = stmt.query_map(params![self.tenant_id, limit], |row| {
            Ok(ozymem_parser::AstDiagnostic {
                file_path: row.get(0)?,
                line_number: row.get::<_, i64>(1)? as usize,
                severity: row.get(2)?,
                message: row.get(3)?,
            })
        })?.filter_map(|r| r.ok()).collect();
        Ok(diags)
    }

    /// Count total static AST diagnostics.
    pub fn count_ast_diagnostics(&self) -> Result<i64> {
        let inner = self.inner.lock().unwrap();
        inner.sqlite.query_row(
            "SELECT COUNT(*) FROM ast_diagnostics WHERE tenant_id = ?1",
            params![self.tenant_id],
            |r| r.get(0),
        ).map_err(Into::into)
    }

    /// Count lessons for the current tenant.
    pub fn lesson_count(&self) -> Result<i64> {
        let inner = self.inner.lock().unwrap();
        inner
            .sqlite
            .query_row(
                "SELECT COUNT(*) FROM lessons WHERE tenant_id = ?1",
                params![self.tenant_id],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }
}
