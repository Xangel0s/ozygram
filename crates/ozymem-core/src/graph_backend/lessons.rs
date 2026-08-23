use crate::graph_backend::types::Inner;
use anyhow::Result;
use rusqlite::params;
use std::path::Path;
use crate::graph_backend::helpers::{detect_language, extract_prohibited_terms, now_ts, normalize_scope, normalize_topic_key, normalized_hash, default_project_name};
use crate::graph_backend::types::{DriftAlert, GraphBackend, LessonEntry, ObservationEntry, PruneReport, PrunedLessonInfo};

impl GraphBackend {
    pub fn memory_session_start(&self, id: &str, project: &str, directory: &str) -> Result<()> {
        let now = now_ts();
        let inner = self.inner.lock().unwrap();
        inner.sqlite.execute(
            "INSERT INTO sessions (id, project, directory, started_at, status, tenant_id, workspace_root)
             VALUES (?1, ?2, ?3, ?4, 'active', ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET
                project = CASE WHEN sessions.project = '' THEN excluded.project ELSE sessions.project END,
                directory = CASE WHEN sessions.directory = '' THEN excluded.directory ELSE sessions.directory END,
                status = 'active'",
            params![id, project, directory, now, self.tenant_id, inner.workspace_root],
        )?;
        Ok(())
    }

    pub fn memory_session_end(&self, id: &str, summary: Option<&str>) -> Result<()> {
        let now = now_ts();
        let inner = self.inner.lock().unwrap();
        inner.sqlite.execute(
            "UPDATE sessions SET ended_at = ?1, summary = COALESCE(?2, summary), status = 'completed'
             WHERE id = ?3 AND tenant_id = ?4",
            params![now, summary, id, self.tenant_id],
        )?;
        Ok(())
    }

    pub fn save_user_prompt(
        &self,
        session_id: &str,
        content: &str,
        project: Option<&str>,
    ) -> Result<i64> {
        let now = now_ts();
        let inner = self.inner.lock().unwrap();
        let project = project.unwrap_or_else(|| default_project_name(&inner));
        inner.sqlite.execute(
            "INSERT INTO user_prompts (session_id, content, project, created_at, tenant_id, workspace_root)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![session_id, content, project, now, self.tenant_id, inner.workspace_root],
        )?;
        Ok(inner.sqlite.last_insert_rowid())
    }

    pub fn save_observation(
        &self,
        session_id: &str,
        observation_type: &str,
        title: &str,
        content: &str,
        project: Option<&str>,
        scope: Option<&str>,
        topic_key: Option<&str>,
        tool_name: Option<&str>,
    ) -> Result<ObservationEntry> {
        let now = now_ts();
        let normalized_hash = normalized_hash(&format!("{observation_type}\n{title}\n{content}"));
        let inner = self.inner.lock().unwrap();
        let project = project.unwrap_or_else(|| default_project_name(&inner));
        let scope = normalize_scope(scope);
        let topic = topic_key.map(normalize_topic_key).filter(|s| !s.is_empty());

        if let Some(ref topic) = topic {
            if let Some(id) = inner.sqlite.query_row(
                "SELECT id FROM observations
                 WHERE project = ?1 AND scope = ?2 AND topic_key = ?3 AND tenant_id = ?4 AND deleted_at IS NULL
                 ORDER BY id DESC LIMIT 1",
                params![project, scope, topic, self.tenant_id],
                |row| row.get::<_, i64>(0),
            ).ok() {
                inner.sqlite.execute(
                    "UPDATE observations SET session_id = ?1, type = ?2, title = ?3, content = ?4,
                        tool_name = ?5, normalized_hash = ?6, revision_count = revision_count + 1,
                        last_seen_at = ?7, updated_at = ?7
                     WHERE id = ?8",
                    params![session_id, observation_type, title, content, tool_name, normalized_hash, now, id],
                )?;
                return self.get_observation_locked(&inner, id);
            }
        }

        if let Some(id) = inner
            .sqlite
            .query_row(
                "SELECT id FROM observations
             WHERE normalized_hash = ?1 AND project = ?2 AND scope = ?3 AND type = ?4 AND title = ?5
               AND tenant_id = ?6 AND deleted_at IS NULL
             ORDER BY id DESC LIMIT 1",
                params![
                    normalized_hash,
                    project,
                    scope,
                    observation_type,
                    title,
                    self.tenant_id
                ],
                |row| row.get::<_, i64>(0),
            )
            .ok()
        {
            inner.sqlite.execute(
                "UPDATE observations SET duplicate_count = duplicate_count + 1, last_seen_at = ?1, updated_at = ?1
                 WHERE id = ?2",
                params![now, id],
            )?;
            return self.get_observation_locked(&inner, id);
        }

        inner.sqlite.execute(
            "INSERT INTO observations
             (session_id, type, title, content, tool_name, project, scope, topic_key, normalized_hash,
              last_seen_at, created_at, updated_at, tenant_id, workspace_root)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10, ?10, ?11, ?12)",
            params![session_id, observation_type, title, content, tool_name, project, scope, topic, normalized_hash, now, self.tenant_id, inner.workspace_root],
        )?;
        self.get_observation_locked(&inner, inner.sqlite.last_insert_rowid())
    }

    pub fn get_observation(&self, id: i64) -> Result<ObservationEntry> {
        let inner = self.inner.lock().unwrap();
        self.get_observation_locked(&inner, id)
    }

    pub fn soft_delete_observation(&self, id: i64) -> Result<bool> {
        let inner = self.inner.lock().unwrap();
        let affected = inner.sqlite.execute(
            "UPDATE observations SET deleted_at = ?1, updated_at = ?1 WHERE id = ?2 AND tenant_id = ?3 AND deleted_at IS NULL",
            params![now_ts(), id, self.tenant_id],
        )?;
        Ok(affected > 0)
    }

    pub fn search_observations(
        &self,
        query: &str,
        project: Option<&str>,
        scope: Option<&str>,
        observation_type: Option<&str>,
        limit: usize,
    ) -> Result<Vec<ObservationEntry>> {
        let inner = self.inner.lock().unwrap();
        let limit = (limit as i64).min(100).max(1);
        let project = project.unwrap_or_else(|| default_project_name(&inner));
        let scope = normalize_scope(scope);
        let mut out = Vec::new();
        let like = format!("%{}%", query);
        let type_filter = observation_type.unwrap_or("");
        let sql = "SELECT id FROM observations
            WHERE tenant_id = ?1 AND project = ?2 AND scope = ?3 AND deleted_at IS NULL
              AND (?4 = '' OR type = ?4)
              AND (?5 = '' OR title LIKE ?6 OR content LIKE ?6 OR COALESCE(tool_name,'') LIKE ?6)
            ORDER BY id DESC LIMIT ?7";
        let mut stmt = inner.sqlite.prepare(sql)?;
        for id in stmt
            .query_map(
                params![
                    self.tenant_id,
                    project,
                    scope,
                    type_filter,
                    query,
                    like,
                    limit
                ],
                |row| row.get::<_, i64>(0),
            )?
            .flatten()
        {
            out.push(self.get_observation_locked(&inner, id)?);
        }
        Ok(out)
    }

    pub fn recent_observations(
        &self,
        project: Option<&str>,
        scope: Option<&str>,
        limit: usize,
    ) -> Result<Vec<ObservationEntry>> {
        self.search_observations("", project, scope, None, limit)
    }

    pub fn observation_timeline(
        &self,
        id: i64,
        before: usize,
        after: usize,
    ) -> Result<Vec<ObservationEntry>> {
        let inner = self.inner.lock().unwrap();
        let session_id: String = inner.sqlite.query_row(
            "SELECT session_id FROM observations WHERE id = ?1 AND tenant_id = ?2 AND deleted_at IS NULL",
            params![id, self.tenant_id],
            |row| row.get(0),
        )?;
        let mut stmt = inner.sqlite.prepare(
            "SELECT id FROM observations
             WHERE session_id = ?1 AND tenant_id = ?2 AND deleted_at IS NULL
             ORDER BY id ASC",
        )?;
        let ids: Vec<i64> = stmt
            .query_map(params![session_id, self.tenant_id], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();
        let pos = ids.iter().position(|x| *x == id).unwrap_or(0);
        let start = pos.saturating_sub(before);
        let end = (pos + after + 1).min(ids.len());
        ids[start..end]
            .iter()
            .map(|id| self.get_observation_locked(&inner, *id))
            .collect()
    }

    pub fn passive_capture(
        &self,
        session_id: &str,
        text: &str,
        project: Option<&str>,
    ) -> Result<Vec<ObservationEntry>> {
        let mut saved = Vec::new();
        let mut in_section = false;
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.eq_ignore_ascii_case("## key learnings:")
                || trimmed.eq_ignore_ascii_case("## key learnings")
                || trimmed.eq_ignore_ascii_case("## aprendizajes clave:")
                || trimmed.eq_ignore_ascii_case("## aprendizajes clave")
            {
                in_section = true;
                continue;
            }
            if in_section && trimmed.starts_with("## ") {
                break;
            }
            if !in_section {
                continue;
            }
            let item = trimmed
                .trim_start_matches(|c: char| {
                    c == '-' || c == '*' || c.is_ascii_digit() || c == '.' || c == ')'
                })
                .trim();
            if item.len() >= 8 {
                saved.push(self.save_observation(
                    session_id,
                    "learning",
                    &item.chars().take(80).collect::<String>(),
                    item,
                    project,
                    Some("project"),
                    None,
                    Some("passive_capture"),
                )?);
            }
        }
        Ok(saved)
    }

    fn get_observation_locked(&self, inner: &Inner, id: i64) -> Result<ObservationEntry> {
        inner
            .sqlite
            .query_row(
                "SELECT id, session_id, type, title, content, project, scope, topic_key,
                    revision_count, duplicate_count, created_at, updated_at
             FROM observations
             WHERE id = ?1 AND tenant_id = ?2 AND deleted_at IS NULL",
                params![id, self.tenant_id],
                |row| {
                    Ok(ObservationEntry {
                        id: row.get(0)?,
                        session_id: row.get(1)?,
                        observation_type: row.get(2)?,
                        title: row.get(3)?,
                        content: row.get(4)?,
                        project: row.get(5)?,
                        scope: row.get(6)?,
                        topic_key: row.get(7)?,
                        revision_count: row.get(8)?,
                        duplicate_count: row.get(9)?,
                        created_at: row.get(10)?,
                        updated_at: row.get(11)?,
                    })
                },
            )
            .map_err(Into::into)
    }


    pub fn map_api_routes(&self, file_path: Option<&str>) -> Result<Vec<ozymem_parser::ApiRouteDefinition>> {
        let mut routes = Vec::new();
        let target_files: Vec<(String, String)> = {
            let inner = self.inner.lock().unwrap();
            if let Some(target) = file_path {
                let norm = crate::normalize_path(target);
                let mut stmt = inner.sqlite.prepare(
                    "SELECT path, language FROM files WHERE path = ?1 AND tenant_id = ?2",
                )?;
                let rows = stmt.query_map(params![norm, self.tenant_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
                rows.filter_map(|r| r.ok()).collect()
            } else {
                let mut stmt = inner.sqlite.prepare(
                    "SELECT path, language FROM files WHERE tenant_id = ?1 ORDER BY path ASC",
                )?;
                let rows = stmt.query_map(params![self.tenant_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
                rows.filter_map(|r| r.ok()).collect()
            }
        };

        for (path_str, _lang_str) in target_files {
            let p = Path::new(&path_str);
            if p.exists() {
                if let Ok(source) = std::fs::read_to_string(p) {
                    let lang = detect_language(p);
                    let file_routes = ozymem_parser::parse_api_routes(&source, &path_str, lang);
                    routes.extend(file_routes);
                }
            }
        }

        Ok(routes)
    }

    /// Marks a lesson as recently verified / used, boosting its confidence score.
    pub fn touch_lesson(&self, lesson_id: i64) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        let inner = self.inner.lock().unwrap();
        inner.sqlite.execute(
            "UPDATE lessons SET touch_count = touch_count + 1, last_verified_at = ?1, confidence_score = MIN(1.0, confidence_score + 0.1) WHERE id = ?2 AND tenant_id = ?3",
            params![now, lesson_id, self.tenant_id],
        )?;
        Ok(())
    }

    /// Evaluates staleness and confidence score of stored lessons.
    /// Returns a PruneReport summarizing active, stale, and low-confidence memories.
    pub fn rank_and_prune_lessons(&self, min_confidence: f64) -> Result<PruneReport> {
        let now_str = chrono::Utc::now().to_rfc3339();
        let mut active_count = 0;
        let mut stale_count = 0;
        let mut pruned_count = 0;
        let mut pruned_lessons = Vec::new();

        let lessons_to_evaluate: Vec<(i64, String, String, String, i64, Option<String>, f64, i64)> = {
            let inner = self.inner.lock().unwrap();
            let mut stmt = inner.sqlite.prepare(
                "SELECT id, file_path, symbol_name, solution, stale, stale_reason, confidence_score, touch_count FROM lessons WHERE tenant_id = ?1",
            )?;
            let rows = stmt.query_map(params![self.tenant_id], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                ))
            })?;
            rows.filter_map(|r| r.ok()).collect()
        };

        let inner = self.inner.lock().unwrap();
        for (id, file_path, symbol_name, solution, stale, stale_reason, mut confidence, touches) in lessons_to_evaluate {
            let mut reasons = Vec::new();
            if stale != 0 {
                confidence *= 0.5;
                if let Some(r) = stale_reason {
                    reasons.push(r);
                } else {
                    reasons.push("stale_marked".to_string());
                }
            }

            // Check if file still exists on disk
            let file_exists = Path::new(&file_path).exists();
            if !file_exists {
                confidence *= 0.3;
                reasons.push("file_not_found".to_string());
            } else if !symbol_name.is_empty() {
                // Check if symbol exists in functions
                let fn_exists: i64 = inner.sqlite.query_row(
                    "SELECT COUNT(*) FROM functions WHERE file_path = ?1 AND name = ?2 AND tenant_id = ?3",
                    params![file_path, symbol_name, self.tenant_id],
                    |r| r.get(0),
                ).unwrap_or(0);
                if fn_exists == 0 {
                    confidence *= 0.6;
                    reasons.push("symbol_removed".to_string());
                }
            }

            // Add touch bonus
            if touches > 0 {
                confidence = (confidence + (touches as f64 * 0.05)).min(1.0);
            }

            confidence = (confidence * 100.0).round() / 100.0;

            inner.sqlite.execute(
                "UPDATE lessons SET confidence_score = ?1, last_verified_at = ?2 WHERE id = ?3",
                params![confidence, now_str, id],
            )?;

            if confidence >= min_confidence && stale == 0 && file_exists {
                active_count += 1;
            } else {
                stale_count += 1;
                if confidence < min_confidence {
                    pruned_count += 1;
                    pruned_lessons.push(PrunedLessonInfo {
                        id,
                        file_path,
                        symbol_name,
                        solution,
                        confidence_score: confidence,
                        reason: if reasons.is_empty() { "low_confidence".to_string() } else { reasons.join(", ") },
                    });
                }
            }
        }

        Ok(PruneReport {
            active_count,
            stale_count,
            pruned_count,
            pruned_lessons,
        })
    }

    /// Audits diff content or changed files against stored conventions and rules.
    pub fn detect_code_drift(&self, changed_files: &[String], diff_content: &str) -> Result<Vec<DriftAlert>> {
        let mut alerts = Vec::new();
        let conventions: Vec<(i64, String, String, String, String)> = {
            let inner = self.inner.lock().unwrap();
            let mut stmt = inner.sqlite.prepare(
                "SELECT id, file_path, symbol_name, error_context, solution FROM lessons WHERE tenant_id = ?1 AND kind IN ('convention', 'module_rule') AND stale = 0",
            )?;
            let rows = stmt.query_map(params![self.tenant_id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })?;
            rows.filter_map(|r| r.ok()).collect()
        };

        if conventions.is_empty() {
            return Ok(alerts);
        }

        for (id, file_path, _symbol, error_context, solution) in &conventions {
            let rule_text = format!("{} {}", error_context, solution).to_lowercase();
            let norm_file_path = crate::normalize_path(file_path);
            let matches_file = changed_files.is_empty() || changed_files.iter().any(|f| {
                let norm = crate::normalize_path(f);
                norm == norm_file_path || norm_file_path.is_empty() || norm.contains(&norm_file_path) || norm_file_path.contains(&norm)
            });

            if matches_file && !diff_content.is_empty() {
                let prohibited_terms = extract_prohibited_terms(&rule_text);
                for line in diff_content.lines() {
                    if line.starts_with('+') && !line.starts_with("+++") {
                        let line_lower = line.to_lowercase();

                        let is_violation = if !prohibited_terms.is_empty() {
                            prohibited_terms.iter().any(|term| line_lower.contains(term))
                        } else {
                            let has_enforcement = rule_text.contains("never")
                                || rule_text.contains("do not")
                                || rule_text.contains("prohibited")
                                || rule_text.contains("deprecated")
                                || rule_text.contains("siempre")
                                || rule_text.contains("always")
                                || rule_text.contains("evitar")
                                || rule_text.contains("forbidden");

                            let matches_keyword = rule_text.split(|c: char| !c.is_alphanumeric() && c != '_').any(|word| {
                                word.len() >= 3 && !matches!(word, "never" | "always" | "with" | "from" | "that" | "this" | "have" | "para" | "siempre" | "rule" | "standard" | "cents" | "price" | "prices") && line_lower.contains(word)
                            });
                            has_enforcement && matches_keyword
                        };

                        if is_violation {
                            alerts.push(DriftAlert {
                                file_path: file_path.clone(),
                                convention_id: *id,
                                rule_snippet: solution.clone(),
                                diff_snippet: line.to_string(),
                                severity: "warning".into(),
                                description: format!("Diff line potentially conflicts with convention #{id}: {}", solution),
                            });
                            break;
                        }
                    }
                }
            }
        }

        Ok(alerts)
    }

    /// Synchronous version of recent_lessons for fast bundle export / tooling.
    pub fn recent_lessons_sync(&self, kind: Option<&str>, limit: usize) -> Result<Vec<LessonEntry>> {
        let inner = self.inner.lock().unwrap();
        let limit = (limit as i64).min(10000).max(1);

        let (sql, kind_clause) = match kind {
            Some(_) => (
                "SELECT id, file_path, symbol_name, error_context, solution, kind, created_at, COALESCE(stale,0), stale_reason, COALESCE(confidence_score, 1.0), COALESCE(touch_count, 0), COALESCE(last_verified_at, '')
                 FROM lessons
                 WHERE tenant_id = ?1 AND kind = ?2 AND (workspace_root = ?3 OR workspace_root = '')
                 ORDER BY id DESC
                 LIMIT ?4".to_string(),
                true,
            ),
            None => (
                "SELECT id, file_path, symbol_name, error_context, solution, kind, created_at, COALESCE(stale,0), stale_reason, COALESCE(confidence_score, 1.0), COALESCE(touch_count, 0), COALESCE(last_verified_at, '')
                 FROM lessons
                 WHERE tenant_id = ?1 AND (workspace_root = ?2 OR workspace_root = '')
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

    /// Synchronous record_entry for importing bundles and fast synchronous scripting.
    pub fn record_entry_sync(
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


}