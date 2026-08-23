use anyhow::Result;
use rusqlite::params;
use crate::{FileGraphContext, GraphSummary, StoredFunction};
use crate::mcp_common::McpBackend;
use crate::graph_backend::types::{GraphBackend, LessonEntry, NeighborInfo};

#[async_trait::async_trait]
impl McpBackend for GraphBackend {
    fn tenant_id(&self) -> String {
        self.tenant_id.clone()
    }

    async fn get_graph_summary(&self) -> Result<GraphSummary> {
        let inner = self.inner.lock().unwrap();

        // Read everything from SQLite (single source of truth).
        // During a background scan, numbers grow incrementally instead of
        // showing stale cross-session data from petgraph.
        let file_count: i64 = inner
            .sqlite
            .query_row(
                "SELECT COUNT(*) FROM files WHERE tenant_id = ?1",
                params![self.tenant_id],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let function_count: i64 = inner
            .sqlite
            .query_row(
                "SELECT COUNT(*) FROM functions WHERE tenant_id = ?1",
                params![self.tenant_id],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let engram_count: i64 = inner
            .sqlite
            .query_row(
                "SELECT COUNT(*) FROM lessons WHERE tenant_id = ?1",
                params![self.tenant_id],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let edge_count: i64 = inner
            .sqlite
            .query_row(
                "SELECT COUNT(*) FROM file_dependencies WHERE tenant_id = ?1",
                params![self.tenant_id],
                |r| r.get(0),
            )
            .unwrap_or(0);

        let mut native = 0i64;
        let mut extension = 0i64;
        let mut text = 0i64;
        if let Ok(mut stmt) = inner.sqlite.prepare(
            "SELECT strategy, COUNT(*) FROM functions WHERE tenant_id = ?1 GROUP BY strategy",
        ) {
            if let Ok(rows) = stmt.query_map(params![self.tenant_id], |row| {
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

        let without_embedding: i64 = inner
            .sqlite
            .query_row(
                "SELECT COUNT(*) FROM lessons WHERE tenant_id = ?1 AND embedding IS NULL",
                params![self.tenant_id],
                |r| r.get(0),
            )
            .unwrap_or(0);

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
            lessons_without_embedding: without_embedding,
        })
    }

    async fn get_file_context(&self, file_path: &str) -> Result<Option<FileGraphContext>> {
        let resolved = self.resolve_target_path(file_path).unwrap_or_else(|| file_path.to_string());
        let norm_path = crate::normalize_path(file_path);
        let inner = self.inner.lock().unwrap();
        let mut stmt = inner
            .sqlite
            .prepare("SELECT language, path FROM files WHERE (path = ?1 OR path = ?2 OR path = ?3) AND tenant_id = ?4 LIMIT 1")?;
        let (language, actual_path): (String, String) =
            match stmt.query_row(params![file_path, norm_path, resolved, self.tenant_id], |row| Ok((row.get(0)?, row.get(1)?))) {
                Ok(v) => v,
                Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
                Err(e) => return Err(e.into()),
            };

        let mut func_stmt = inner.sqlite.prepare(
            "SELECT name, kind, start_line, end_line, strategy FROM functions WHERE (file_path = ?1 OR file_path = ?2) AND tenant_id = ?3 ORDER BY start_line, name"
        )?;
        let funcs: Vec<StoredFunction> = func_stmt
            .query_map(params![file_path, actual_path, self.tenant_id], |row| {
                Ok(StoredFunction {
                    name: row.get(0)?,
                    kind: row.get(1)?,
                    start_line: row.get(2)?,
                    end_line: row.get(3)?,
                    strategy: row.get(4)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        let mut contracts = Vec::new();
        for f in &funcs {
            let symbol_path = format!("{}::{}", actual_path, f.name);
            if let Some(c) = self.engram_store.lookup(&symbol_path) {
                contracts.push(c);
            }
        }

        Ok(Some(FileGraphContext {
            file_path: actual_path,
            language,
            functions: funcs,
            engram_contracts: contracts,
        }))
    }

    async fn get_historical_engram_solutions(&self, file_path: &str) -> Result<Vec<String>> {
        let inner = self.inner.lock().unwrap();
        let mut stmt = match inner.sqlite.prepare(
            "SELECT DISTINCT solution FROM lessons WHERE file_path = ?1 AND tenant_id = ?2 AND solution != ''"
        ) {
            Ok(s) => s,
            Err(_) => return Ok(vec![]),
        };
        let results: Vec<String> = stmt
            .query_map(params![file_path, self.tenant_id], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(results)
    }

    async fn record_lesson(
        &self,
        file_path: &str,
        symbol_name: Option<&str>,
        error_context: &str,
        solution: &str,
    ) -> Result<()> {
        self.record_entry(file_path, symbol_name, error_context, solution, "lesson")
            .await
    }

    async fn record_entry(
        &self,
        file_path: &str,
        symbol_name: Option<&str>,
        error_context: &str,
        solution: &str,
        kind: &str,
    ) -> Result<()> {
        let created_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            .to_string();

        // Generate embedding OUTSIDE the inner lock (CPU-bound)
        let text = format!("{} {}", error_context, solution);
        let (embedding_bytes, _model) = self.embed_text(&[&text]);
        let has_embedding = embedding_bytes.is_some();

        {
            let inner = self.inner.lock().unwrap();
            inner.sqlite.execute(
                "INSERT INTO lessons (file_path, symbol_name, error_context, solution, created_at, tenant_id, kind, workspace_root, embedding, embedding_model) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![file_path, symbol_name.unwrap_or(""), error_context, solution, created_at, self.tenant_id, kind, inner.workspace_root, embedding_bytes, if has_embedding { "all-MiniLM-L6-v2" } else { "" }],
            )?;
        }

        let node_idx = {
            let inner = self.inner.lock().unwrap();
            inner.file_index.get(file_path).copied()
        };

        if let Some(idx) = node_idx {
            let mut inner = self.inner.lock().unwrap();
            if let Some(node) = inner.graph.node_weight_mut(idx) {
                node.lesson_count += 1;
            }
        }

        Ok(())
    }

    async fn search_lessons(
        &self,
        query: &str,
        kind: Option<&str>,
        limit: usize,
    ) -> Result<Vec<LessonEntry>> {
        let inner = self.inner.lock().unwrap();
        let limit = (limit as i64).min(100).max(1);

        // Try FTS5 MATCH first; fall back to LIKE if query syntax is invalid
        let mut results = Vec::new();

        // FTS5 with content-sync: query the lessons table joined with fts
        let tenant_id = &self.tenant_id;
        let root = inner.workspace_root.clone();
        let (tenant_param, workspace_param, kind_clause) = match kind {
            Some(_) => ("?4", "?5", "AND l.kind = ?3".to_string()),
            None => ("?3", "?4", String::new()),
        };
        let sql = format!(
            "SELECT l.id, l.file_path, l.symbol_name, l.error_context, l.solution, l.kind, l.created_at, COALESCE(l.stale,0), l.stale_reason, COALESCE(l.confidence_score, 1.0), COALESCE(l.touch_count, 0), COALESCE(l.last_verified_at, '')
             FROM lessons l
             JOIN lessons_fts fts ON l.id = fts.rowid
             WHERE lessons_fts MATCH ?1 {}
               AND l.stale = 0
               AND l.tenant_id = {}
               AND l.workspace_root = {}
             ORDER BY bm25(lessons_fts, 2.0, 1.0, 3.0, 1.0)
             LIMIT ?2", kind_clause, tenant_param, workspace_param
        );

        if kind.is_none() {
            if let Ok(mut stmt) = inner.sqlite.prepare(&sql) {
                if let Ok(rows) = stmt.query_map(params![query, limit, tenant_id, root], |row| {
                    LessonEntry::from_row(row)
                }) {
                    for row in rows.flatten() {
                        results.push(row);
                    }
                }
            }
        } else if let Some(k) = kind {
            if let Ok(mut stmt) = inner.sqlite.prepare(&sql) {
                if let Ok(rows) = stmt.query_map(params![query, limit, k, tenant_id, root], |row| {
                    LessonEntry::from_row(row)
                }) {
                    for row in rows.flatten() {
                        results.push(row);
                    }
                }
            }
        }

        // If FTS returned nothing, fall back to LIKE on all text columns
        if results.is_empty() && !query.is_empty() {
            let like = format!("%{}%", query);
            let root = inner.workspace_root.clone();
            let (kind_clause, workspace_param) = match kind {
                Some(_) => ("AND kind = ?4".to_string(), "?5"),
                None => (String::new(), "?4"),
            };
            let sql = format!(
             "SELECT id, file_path, symbol_name, error_context, solution, kind, created_at, COALESCE(stale,0), stale_reason, COALESCE(confidence_score, 1.0), COALESCE(touch_count, 0), COALESCE(last_verified_at, '')
              FROM lessons
              WHERE tenant_id = ?1
                AND stale = 0
                AND (file_path LIKE ?2 OR symbol_name LIKE ?2 OR error_context LIKE ?2 OR solution LIKE ?2)
                {}
                AND workspace_root = {}
              ORDER BY id DESC
              LIMIT ?3", kind_clause, workspace_param
            );
            if kind.is_none() {
                if let Ok(mut stmt) = inner.sqlite.prepare(&sql) {
                    if let Ok(rows) =
                        stmt.query_map(params![self.tenant_id, like, limit, root], |row| {
                            LessonEntry::from_row(row)
                        })
                    {
                        for row in rows.flatten() {
                            results.push(row);
                        }
                    }
                }
            } else if let Some(k) = kind {
                if let Ok(mut stmt) = inner.sqlite.prepare(&sql) {
                    if let Ok(rows) =
                        stmt.query_map(params![self.tenant_id, like, limit, k, root], |row| {
                            LessonEntry::from_row(row)
                        })
                    {
                        for row in rows.flatten() {
                            results.push(row);
                        }
                    }
                }
            }
        }

        Ok(results)
    }

    async fn get_file_lessons(&self, file_path: &str) -> Result<Vec<LessonEntry>> {
        let resolved = self.resolve_target_path(file_path).unwrap_or_else(|| file_path.to_string());
        let norm_path = crate::normalize_path(file_path);
        let inner = self.inner.lock().unwrap();
        let mut stmt = inner.sqlite.prepare(
            "SELECT id, file_path, symbol_name, error_context, solution, kind, created_at, COALESCE(stale,0), stale_reason, COALESCE(confidence_score, 1.0), COALESCE(touch_count, 0), COALESCE(last_verified_at, '')
             FROM lessons
             WHERE (file_path = ?1 OR file_path = ?2 OR file_path = ?3) AND tenant_id = ?4 AND (workspace_root = ?5 OR workspace_root = '')
             ORDER BY id DESC"
        )?;
        let results = stmt
            .query_map(
                params![file_path, norm_path, resolved, self.tenant_id, inner.workspace_root],
                |row| LessonEntry::from_row(row),
            )?
            .filter_map(|r| r.ok())
            .collect();
        Ok(results)
    }

    async fn get_symbol_lessons(
        &self,
        file_path: &str,
        symbol_name: &str,
    ) -> Result<Vec<LessonEntry>> {
        let resolved = self.resolve_target_path(file_path).unwrap_or_else(|| file_path.to_string());
        let norm_path = crate::normalize_path(file_path);
        let inner = self.inner.lock().unwrap();
        let mut stmt = inner.sqlite.prepare(
            "SELECT id, file_path, symbol_name, error_context, solution, kind, created_at, COALESCE(stale,0), stale_reason, COALESCE(confidence_score, 1.0), COALESCE(touch_count, 0), COALESCE(last_verified_at, '')
             FROM lessons
             WHERE (file_path = ?1 OR file_path = ?2 OR file_path = ?3) AND symbol_name = ?4 AND tenant_id = ?5 AND (workspace_root = ?6 OR workspace_root = '')
             ORDER BY id DESC"
        )?;
        let results = stmt
            .query_map(
                params![file_path, norm_path, resolved, symbol_name, self.tenant_id, inner.workspace_root],
                |row| LessonEntry::from_row(row),
            )?
            .filter_map(|r| r.ok())
            .collect();
        Ok(results)
    }

    async fn recent_lessons(&self, kind: Option<&str>, limit: usize) -> Result<Vec<LessonEntry>> {
        let inner = self.inner.lock().unwrap();
        let limit = (limit as i64).min(100).max(1);

        let (sql, kind_clause) = match kind {
            Some(_) => (
                "SELECT id, file_path, symbol_name, error_context, solution, kind, created_at, COALESCE(stale,0), stale_reason, COALESCE(confidence_score, 1.0), COALESCE(touch_count, 0), COALESCE(last_verified_at, '')
                 FROM lessons
                 WHERE tenant_id = ?1 AND kind = ?2 AND workspace_root = ?3
                 ORDER BY id DESC
                 LIMIT ?4".to_string(),
                true,
            ),
            None => (
                "SELECT id, file_path, symbol_name, error_context, solution, kind, created_at, COALESCE(stale,0), stale_reason, COALESCE(confidence_score, 1.0), COALESCE(touch_count, 0), COALESCE(last_verified_at, '')
                 FROM lessons
                 WHERE tenant_id = ?1 AND workspace_root = ?2
                 ORDER BY id DESC
                 LIMIT ?3".to_string(),
                false,
            ),
        };

        if kind_clause {
            let mut stmt = inner.sqlite.prepare(&sql)?;
            let k = kind.unwrap();
            let results = stmt
                .query_map(
                    params![self.tenant_id, k, inner.workspace_root, limit],
                    |row| LessonEntry::from_row(row),
                )?
                .filter_map(|r| r.ok())
                .collect();
            Ok(results)
        } else {
            let mut stmt = inner.sqlite.prepare(&sql)?;
            let results = stmt
                .query_map(
                    params![self.tenant_id, inner.workspace_root, limit],
                    |row| LessonEntry::from_row(row),
                )?
                .filter_map(|r| r.ok())
                .collect();
            Ok(results)
        }
    }

    async fn get_graph_neighbors(&self, file_path: &str) -> Result<NeighborInfo> {
        let incoming = self.get_incoming_deps(file_path);
        let outgoing = self.get_outgoing_deps(file_path);
        Ok(NeighborInfo {
            file_path: file_path.to_string(),
            incoming,
            outgoing,
        })
    }

    async fn get_incoming_dependencies(&self, file_path: &str) -> Result<Vec<String>> {
        Ok(self.get_incoming_deps(file_path))
    }

    async fn find_symbol(&self, symbol_name: &str, project_path: &str) -> Result<Vec<String>> {
        let inner = self.inner.lock().unwrap();
        let like = format!("{}%", project_path);
        // Use LIKE for partial matching (supports % wildcards in symbol_name)
        let mut stmt = inner.sqlite.prepare(
            "SELECT f.path, fn.start_line, fn.kind FROM functions fn
             JOIN files f ON f.path = fn.file_path AND f.tenant_id = fn.tenant_id
             WHERE fn.name LIKE ?1 AND fn.tenant_id = ?2 AND f.path LIKE ?3
             ORDER BY fn.name LIMIT 100",
        )?;
        let results: Vec<String> = stmt
            .query_map(params![symbol_name, self.tenant_id, like], |row| {
                let path: String = row.get(0)?;
                let line: i64 = row.get(1)?;
                let kind: String = row.get(2)?;
                Ok(format!("{} [{}] (line {})", path, kind, line))
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(results)
    }

    async fn lookup_engram(&self, symbol_path: &str) -> Result<Option<ozymem_parser::EngramContract>> {
        if let Some(contract) = self.engram_store.lookup(symbol_path) {
            return Ok(Some(contract));
        }
        let matches = self.engram_store.lookup_by_name(symbol_path);
        Ok(matches.into_iter().next())
    }

    async fn get_speculative_engrams(&self, file_path: &str, limit: usize) -> Result<Vec<ozymem_parser::EngramContract>> {
        Ok(GraphBackend::get_speculative_engrams(self, file_path, limit))
    }
}

// ---------------------------------------------------------------------------
// Embedding / Semantic Search
// ---------------------------------------------------------------------------

