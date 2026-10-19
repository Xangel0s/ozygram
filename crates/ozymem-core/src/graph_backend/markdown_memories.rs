use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use crate::graph_backend::types::{GraphBackend, LessonEntry};

/// Metadata extracted from the YAML frontmatter of a Markdown memory file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MarkdownMemoryMetadata {
    pub id: String,
    pub kind: String, // "gotcha" | "convention" | "architecture" | "bugfix" | "decision" | "lesson"
    pub status: Option<String>, // "active" | "resolved" | "archived" (defaults to "active")
    pub topic_key: String,
    pub title: String,
    pub author: Option<String>,
    pub date: Option<String>,
    pub severity: Option<String>, // "critical" | "high" | "medium" | "low"
    pub applies_to: Vec<String>, // relative paths or globs e.g. ["api/database.py", "modules/**"]
    pub tags: Vec<String>,
}

/// Represents an atomic knowledge item stored as Markdown with YAML Frontmatter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MarkdownMemory {
    pub metadata: MarkdownMemoryMetadata,
    pub body: String,
    pub file_path: Option<String>,
}

impl MarkdownMemory {
    /// Creates a new MarkdownMemory instance.
    pub fn new(metadata: MarkdownMemoryMetadata, body: impl Into<String>) -> Self {
        Self {
            metadata,
            body: body.into(),
            file_path: None,
        }
    }

    /// Returns whether this memory is currently active (defaults to true if status is omitted).
    pub fn is_active(&self) -> bool {
        match self.metadata.status.as_deref().map(|s| s.to_lowercase()).as_deref() {
            Some("resolved") | Some("archived") => false,
            _ => true,
        }
    }

    /// Numerical severity weight for ranking in pre-flight alerts (higher = more critical).
    pub fn severity_weight(&self) -> u8 {
        match self.metadata.severity.as_deref().map(|s| s.to_lowercase()).as_deref() {
            Some("critical") => 4,
            Some("high") => 3,
            Some("medium") => 2,
            Some("low") => 1,
            _ => 2,
        }
    }

    /// Parses a Markdown memory file containing YAML frontmatter delimited by `---`.
    pub fn from_markdown(content: &str) -> Result<Self> {
        let trimmed = content.trim_start();
        if !trimmed.starts_with("---") {
            return Err(anyhow!("Missing opening '---' YAML frontmatter delimiter"));
        }

        // Find closing '---'
        let after_opening = &trimmed[3..];
        let end_idx = after_opening
            .find("\n---")
            .ok_or_else(|| anyhow!("Missing closing '---' YAML frontmatter delimiter"))?;

        let frontmatter_str = &after_opening[..end_idx];
        let body_str = after_opening[end_idx + 4..].trim();

        let metadata = parse_yaml_frontmatter(frontmatter_str)?;

        Ok(Self {
            metadata,
            body: body_str.to_string(),
            file_path: None,
        })
    }

    /// Serializes the memory to a clean Markdown document with YAML Frontmatter.
    pub fn to_markdown(&self) -> String {
        let mut out = String::new();
        out.push_str("---\n");
        out.push_str(&format!("id: \"{}\"\n", self.metadata.id));
        out.push_str(&format!("kind: \"{}\"\n", self.metadata.kind));
        if let Some(ref status) = self.metadata.status {
            out.push_str(&format!("status: \"{}\"\n", status));
        }
        out.push_str(&format!("topic_key: \"{}\"\n", self.metadata.topic_key));
        out.push_str(&format!("title: \"{}\"\n", self.metadata.title));

        if let Some(ref author) = self.metadata.author {
            out.push_str(&format!("author: \"{}\"\n", author));
        }
        if let Some(ref date) = self.metadata.date {
            out.push_str(&format!("date: \"{}\"\n", date));
        }
        if let Some(ref severity) = self.metadata.severity {
            out.push_str(&format!("severity: \"{}\"\n", severity));
        }

        if !self.metadata.applies_to.is_empty() {
            out.push_str("applies_to:\n");
            for item in &self.metadata.applies_to {
                out.push_str(&format!("  - \"{}\"\n", item));
            }
        }

        if !self.metadata.tags.is_empty() {
            out.push_str("tags:\n");
            for tag in &self.metadata.tags {
                out.push_str(&format!("  - \"{}\"\n", tag));
            }
        }

        out.push_str("---\n\n");
        out.push_str(&self.body);
        if !self.body.ends_with('\n') {
            out.push('\n');
        }

        out
    }

    /// Checks whether this memory applies to a given target file.
    /// Supports exact matching, relative suffix matching, and `/**` prefix globbing.
    pub fn matches_file(&self, target_file: &str) -> bool {
        if self.metadata.applies_to.is_empty() {
            return false;
        }

        let norm_target = target_file.replace('\\', "/").trim_matches('/').to_lowercase();

        for pattern in &self.metadata.applies_to {
            let norm_pat = pattern.replace('\\', "/").trim_matches('/').to_lowercase();
            if norm_pat.is_empty() {
                continue;
            }

            // 1. Exact match
            if norm_target == norm_pat {
                return true;
            }

            // 2. Glob directory match (e.g. "app/modules/**" or "app/modules/*")
            if let Some(prefix) = norm_pat.strip_suffix("/**") {
                if norm_target.starts_with(prefix) {
                    return true;
                }
            } else if let Some(prefix) = norm_pat.strip_suffix("/*") {
                if norm_target.starts_with(prefix) {
                    return true;
                }
            }

            // 3. Suffix match (e.g. pattern is "database.py" or "app/database.py" matching full path)
            if norm_target.ends_with(&norm_pat) {
                // Ensure boundary at a path separator or exact match
                if norm_target.len() == norm_pat.len()
                    || norm_target.as_bytes()[norm_target.len() - norm_pat.len() - 1] == b'/'
                {
                    return true;
                }
            }
        }

        false
    }

    /// Suggests a canonical filename for saving this memory to disk.
    /// Format: `<date>_<slug>.md` or `<slug>.md`.
    pub fn suggested_filename(&self) -> String {
        let slug = self
            .metadata
            .topic_key
            .chars()
            .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
            .collect::<String>();
        let clean_slug = slug.trim_matches('-');

        if let Some(ref date) = self.metadata.date {
            let clean_date = date.trim();
            if !clean_date.is_empty() {
                return format!("{clean_date}_{clean_slug}.md");
            }
        }
        format!("{clean_slug}.md")
    }
}

/// Recursively scans a directory for `.md` memory files and parses them into `MarkdownMemory`.
pub fn scan_markdown_memories(base_dir: &Path) -> Result<Vec<MarkdownMemory>> {
    let mut memories = Vec::new();
    if !base_dir.exists() {
        return Ok(memories);
    }

    for entry in walkdir::WalkDir::new(base_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("md") {
            if let Ok(content) = std::fs::read_to_string(path) {
                if let Ok(mut mem) = MarkdownMemory::from_markdown(&content) {
                    mem.file_path = Some(path.to_string_lossy().replace('\\', "/"));
                    memories.push(mem);
                }
            }
        }
    }

    Ok(memories)
}

/// Lightweight parser for YAML frontmatter key-value pairs and arrays without external dependencies.
fn parse_yaml_frontmatter(raw: &str) -> Result<MarkdownMemoryMetadata> {
    let mut id = String::new();
    let mut kind = "gotcha".to_string();
    let mut status: Option<String> = None;
    let mut topic_key = String::new();
    let mut title = String::new();
    let mut author: Option<String> = None;
    let mut date: Option<String> = None;
    let mut severity: Option<String> = None;
    let mut applies_to = Vec::new();
    let mut tags = Vec::new();

    let mut current_array_target: Option<&mut Vec<String>> = None;

    for line in raw.lines() {
        let line = line.trim_end();
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }

        // Check for multiline list element (indented "- ...")
        if line.starts_with("  -") || line.starts_with(" -") || line.starts_with("\t-") || (line.starts_with("- ") && current_array_target.is_some()) {
            if let Some(ref mut arr) = current_array_target {
                let val = line.trim_start().trim_start_matches('-').trim();
                let clean = strip_quotes(val);
                if !clean.is_empty() {
                    arr.push(clean.to_string());
                }
                continue;
            }
        }

        // We encountered a new key, reset current array target
        current_array_target = None;

        let Some((key, val_part)) = line.split_once(':') else {
            continue;
        };

        let key = key.trim();
        let val_part = val_part.trim();

        match key {
            "id" => id = strip_quotes(val_part).to_string(),
            "kind" => kind = strip_quotes(val_part).to_string(),
            "status" => status = Some(strip_quotes(val_part).to_string()),
            "topic_key" => topic_key = strip_quotes(val_part).to_string(),
            "title" => title = strip_quotes(val_part).to_string(),
            "author" => author = Some(strip_quotes(val_part).to_string()),
            "date" => date = Some(strip_quotes(val_part).to_string()),
            "severity" => severity = Some(strip_quotes(val_part).to_string()),
            "applies_to" => {
                if val_part.starts_with('[') && val_part.ends_with(']') {
                    applies_to = parse_inline_array(val_part);
                } else {
                    current_array_target = Some(&mut applies_to);
                }
            }
            "tags" => {
                if val_part.starts_with('[') && val_part.ends_with(']') {
                    tags = parse_inline_array(val_part);
                } else {
                    current_array_target = Some(&mut tags);
                }
            }
            _ => {}
        }
    }

    if id.is_empty() {
        if !topic_key.is_empty() {
            id = format!("mem_{}", topic_key.replace('-', "_"));
        } else {
            id = format!("mem_{}", chrono::Utc::now().format("%Y%m%d%H%M%S"));
        }
    }

    if title.is_empty() {
        title = topic_key.clone();
    }

    Ok(MarkdownMemoryMetadata {
        id,
        kind,
        status,
        topic_key,
        title,
        author,
        date,
        severity,
        applies_to,
        tags,
    })
}

fn strip_quotes(s: &str) -> &str {
    let trimmed = s.trim();
    if (trimmed.starts_with('"') && trimmed.ends_with('"'))
        || (trimmed.starts_with('\'') && trimmed.ends_with('\''))
    {
        if trimmed.len() >= 2 {
            return &trimmed[1..trimmed.len() - 1];
        }
    }
    trimmed
}

fn parse_inline_array(raw: &str) -> Vec<String> {
    let inner = raw.trim_start_matches('[').trim_end_matches(']');
    inner
        .split(',')
        .map(|item| strip_quotes(item).trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

impl GraphBackend {
    /// Resolves the canonical `.ozy/memories` root path for the current project.
    pub fn resolve_ozy_memories_dir(&self) -> Option<std::path::PathBuf> {
        let inner = self.inner.lock().unwrap();
        let root = inner.project_path.clone().or_else(|| {
            if !inner.workspace_root.is_empty() {
                Some(inner.workspace_root.clone())
            } else {
                None
            }
        })?;
        Some(std::path::PathBuf::from(root).join(".ozy").join("memories"))
    }

    /// Scans `.ozy/memories` in the active project and parses all markdown files.
    pub fn scan_project_markdown_memories(&self) -> Result<Vec<MarkdownMemory>> {
        let Some(dir) = self.resolve_ozy_memories_dir() else {
            return Ok(Vec::new());
        };
        scan_markdown_memories(&dir)
    }

    /// Retrieves all markdown memories that apply to the specified file path.
    pub fn get_applicable_markdown_memories(&self, file_path: &str) -> Vec<MarkdownMemory> {
        let Ok(memories) = self.scan_project_markdown_memories() else {
            return Vec::new();
        };
        memories
            .into_iter()
            .filter(|mem| mem.matches_file(file_path))
            .collect()
    }

    /// Formats a zero-emoji alert badge block if there are active gotchas or conventions
    /// applicable to the specified file path, protected by a Token Budget Guard (top 3 by severity/date).
    pub fn format_applicable_gotchas_alert(&self, file_path: &str) -> Option<String> {
        let all_applicable = self.get_applicable_markdown_memories(file_path);
        let mut active_memories: Vec<MarkdownMemory> = all_applicable
            .into_iter()
            .filter(|mem| mem.is_active())
            .collect();

        if active_memories.is_empty() {
            return None;
        }

        // Rank by severity_weight (descending), then by date (descending)
        active_memories.sort_by(|a, b| {
            b.severity_weight()
                .cmp(&a.severity_weight())
                .then_with(|| {
                    let date_a = a.metadata.date.as_deref().unwrap_or("");
                    let date_b = b.metadata.date.as_deref().unwrap_or("");
                    date_b.cmp(date_a)
                })
        });

        let total_active = active_memories.len();
        const MAX_INJECTED_GOTCHAS: usize = 3;

        let mut out = String::new();
        out.push_str(&format!(
            "[ALERT: GOTCHA_PREVENTION: {} ACTIVE MEMORIES DETECTED FOR '{}']\n",
            total_active,
            file_path
        ));

        for mem in active_memories.iter().take(MAX_INJECTED_GOTCHAS) {
            let sev = mem.metadata.severity.as_deref().unwrap_or("HIGH").to_uppercase();
            let author = mem.metadata.author.as_deref().unwrap_or("team");
            let date = mem.metadata.date.as_deref().unwrap_or("unknown");
            out.push_str(&format!(
                "* [SEVERITY: {}] [{}] {} (by {}, {})\n",
                sev,
                mem.metadata.kind.to_uppercase(),
                mem.metadata.title,
                author,
                date
            ));
            let lines: Vec<&str> = mem
                .body
                .lines()
                .filter(|l| !l.trim().is_empty())
                .take(3)
                .collect();
            for l in lines {
                out.push_str(&format!("    {}\n", l.trim()));
            }
        }

        if total_active > MAX_INJECTED_GOTCHAS {
            let omitted = total_active - MAX_INJECTED_GOTCHAS;
            out.push_str(&format!(
                "  [... {} older or lower-priority gotchas omitted to save tokens. Use ozy_memory(action=\"file\", file_path=\"{}\") to view all]\n",
                omitted,
                file_path
            ));
        }

        Some(out)
    }

    /// Synchronizes and indexes all Markdown memories from `.ozy/memories` into SQLite.
    /// Deduplicates by `error_context` containing the memory ID or matching topic_key.
    pub fn sync_markdown_memories_to_sqlite(&self) -> Result<usize> {
        let memories = self.scan_project_markdown_memories()?;
        let mut count = 0;

        for mem in &memories {
            let primary_file = mem
                .metadata
                .applies_to
                .first()
                .cloned()
                .unwrap_or_else(|| "global".to_string());
            let symbol_name = mem.metadata.topic_key.clone();
            let error_context = format!("[ID: {}] {}", mem.metadata.id, mem.metadata.title);
            let solution = mem.body.clone();
            let kind = match mem.metadata.kind.as_str() {
                "gotcha" | "convention" | "decision" | "module_rule" => &mem.metadata.kind,
                _ => "lesson",
            };

            let already_exists = {
                let inner = self.inner.lock().unwrap();
                let exists: bool = inner
                    .sqlite
                    .query_row(
                        "SELECT 1 FROM lessons WHERE tenant_id = ?1 AND error_context = ?2 LIMIT 1",
                        rusqlite::params![self.tenant_id, error_context],
                        |_| Ok(true),
                    )
                    .unwrap_or(false);
                exists
            };

            if !already_exists {
                self.record_entry_sync(
                    &primary_file,
                    Some(&symbol_name),
                    &error_context,
                    &solution,
                    kind,
                )?;
                count += 1;
            }
        }

        Ok(count)
    }

    /// Exports a LessonEntry into an atomic Markdown memory file under `.ozy/memories/<kind>/`.
    pub fn export_lesson_to_markdown(&self, lesson: &LessonEntry) -> Result<std::path::PathBuf> {
        let dir = self
            .resolve_ozy_memories_dir()
            .ok_or_else(|| anyhow::anyhow!("No active project root to export markdown memory"))?;
        let kind_dir = dir.join(&lesson.kind);
        std::fs::create_dir_all(&kind_dir)?;

        let id = format!("mem_{}", lesson.id);
        let topic_key = if !lesson.symbol_name.is_empty() {
            lesson.symbol_name.clone()
        } else {
            format!("lesson-{}", lesson.id)
        };

        let title = if let Some((_, rest)) = lesson.error_context.split_once(']') {
            rest.trim().to_string()
        } else {
            lesson
                .error_context
                .lines()
                .next()
                .unwrap_or("Untitled Lesson")
                .to_string()
        };

        let mut applies_to = Vec::new();
        if !lesson.file_path.is_empty()
            && lesson.file_path != "global"
            && lesson.file_path != "unknown"
        {
            applies_to.push(lesson.file_path.clone());
        }

        let meta = MarkdownMemoryMetadata {
            id,
            kind: lesson.kind.clone(),
            status: Some("active".to_string()),
            topic_key,
            title,
            author: Some("agent".to_string()),
            date: Some(chrono::Utc::now().format("%Y-%m-%d").to_string()),
            severity: Some("high".to_string()),
            applies_to,
            tags: vec![lesson.kind.clone()],
        };

        let memory = MarkdownMemory::new(meta, &lesson.solution);
        let filename = memory.suggested_filename();
        let full_path = kind_dir.join(filename);

        std::fs::write(&full_path, memory.to_markdown())?;
        Ok(full_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_and_serialize_roundtrip() {
        let md = r#"---
id: "mem_20261019_a1b2c3"
kind: "gotcha"
topic_key: "fastapi-pool-exhaustion"
title: "Agotamiento de pool SQLAlchemy"
author: "diego"
date: "2026-10-19"
severity: "critical"
applies_to:
  - "backend/api/database.py"
  - "backend/api/modules/**"
tags:
  - "database"
  - "sqlalchemy"
  - "pool"
---

### [Contexto y Tropiezo]
Al ejecutar eliminaciones masivas en bucle con Promise.all, el pool colapsa.

### [Regla de Oro]
Usar siempre endpoint batch.

```python
db.query(Modelo).filter(Modelo.id.in_(deleted_ids)).delete()
```
"#;

        let memory = MarkdownMemory::from_markdown(md).expect("Parsing must succeed");
        assert_eq!(memory.metadata.id, "mem_20261019_a1b2c3");
        assert_eq!(memory.metadata.kind, "gotcha");
        assert_eq!(memory.metadata.topic_key, "fastapi-pool-exhaustion");
        assert_eq!(memory.metadata.title, "Agotamiento de pool SQLAlchemy");
        assert_eq!(memory.metadata.author.as_deref(), Some("diego"));
        assert_eq!(memory.metadata.severity.as_deref(), Some("critical"));
        assert_eq!(memory.metadata.applies_to.len(), 2);
        assert_eq!(memory.metadata.tags.len(), 3);
        assert!(memory.body.contains("```python"));

        // Roundtrip serialization
        let serialized = memory.to_markdown();
        let parsed_again = MarkdownMemory::from_markdown(&serialized).expect("Roundtrip parsing must succeed");
        assert_eq!(memory.metadata, parsed_again.metadata);
        assert_eq!(memory.body.trim(), parsed_again.body.trim());
    }

    #[test]
    fn test_inline_arrays() {
        let md = r#"---
id: "mem_001"
kind: "convention"
topic_key: "api-endpoints"
title: "Batch delete contract"
applies_to: [api/routes.py, api/models.py]
tags: [api, rest, conventions]
---

Always use batch deletion endpoints.
"#;
        let memory = MarkdownMemory::from_markdown(md).unwrap();
        assert_eq!(memory.metadata.applies_to, vec!["api/routes.py", "api/models.py"]);
        assert_eq!(memory.metadata.tags, vec!["api", "rest", "conventions"]);
        assert_eq!(memory.body.trim(), "Always use batch deletion endpoints.");
    }

    #[test]
    fn test_matches_file_logic() {
        let meta = MarkdownMemoryMetadata {
            id: "mem_test".to_string(),
            kind: "gotcha".to_string(),
            status: Some("active".to_string()),
            topic_key: "pool-bug".to_string(),
            title: "Pool bug".to_string(),
            author: None,
            date: None,
            severity: Some("critical".to_string()),
            applies_to: vec![
                "backend/api/database.py".to_string(),
                "backend/api/modules/**".to_string(),
                "utils/helpers.py".to_string(),
            ],
            tags: vec![],
        };
        let memory = MarkdownMemory::new(meta, "body");

        // Exact match
        assert!(memory.matches_file("backend/api/database.py"));
        // Windows backslash normalized match
        assert!(memory.matches_file(r"backend\api\database.py"));
        // Glob match
        assert!(memory.matches_file("backend/api/modules/auth/service.py"));
        // Suffix match
        assert!(memory.matches_file("c:/repo/utils/helpers.py"));
        // Negative match
        assert!(!memory.matches_file("frontend/src/index.tsx"));
    }

    #[test]
    fn test_suggested_filename() {
        let meta = MarkdownMemoryMetadata {
            id: "mem_1".to_string(),
            kind: "gotcha".to_string(),
            status: Some("active".to_string()),
            topic_key: "fastapi-pool-exhaustion".to_string(),
            title: "Pool".to_string(),
            author: None,
            date: Some("2026-10-19".to_string()),
            severity: None,
            applies_to: vec![],
            tags: vec![],
        };
        let mem = MarkdownMemory::new(meta, "");
        assert_eq!(mem.suggested_filename(), "2026-10-19_fastapi-pool-exhaustion.md");
    }

    #[test]
    fn test_graph_backend_markdown_lifecycle() {
        let temp_dir = tempfile::tempdir().unwrap();
        let db_path = temp_dir.path().join("test_memory.db");
        let backend = GraphBackend::open(Some(&db_path.to_string_lossy())).unwrap();
        backend.set_project_path(Some(&temp_dir.path().to_string_lossy()));

        // 1. Create a markdown gotcha in .ozy/memories/gotcha/
        let ozy_dir = temp_dir.path().join(".ozy").join("memories").join("gotcha");
        std::fs::create_dir_all(&ozy_dir).unwrap();

        let md_content = r#"---
id: "mem_test_db_leak"
kind: "gotcha"
topic_key: "db-connection-leak"
title: "SQLAlchemy connection leak on unclosed sessions"
author: "maria"
date: "2026-10-19"
severity: "critical"
applies_to:
  - "app/database.py"
tags:
  - "database"
---

Always use context manager `with SessionLocal() as db:` to prevent leak.
"#;
        std::fs::write(ozy_dir.join("db_leak.md"), md_content).unwrap();

        // 2. Scan and verify applicable memory
        let applicable = backend.get_applicable_markdown_memories("app/database.py");
        assert_eq!(applicable.len(), 1);
        assert_eq!(applicable[0].metadata.title, "SQLAlchemy connection leak on unclosed sessions");

        // 3. Format gotcha alert
        let alert = backend.format_applicable_gotchas_alert("app/database.py");
        assert!(alert.is_some());
        let alert_text = alert.unwrap();
        assert!(alert_text.contains("[ALERT: GOTCHA_PREVENTION"));
        assert!(alert_text.contains("SQLAlchemy connection leak"));

        // 4. Sync into SQLite
        let synced = backend.sync_markdown_memories_to_sqlite().unwrap();
        assert_eq!(synced, 1);

        // 5. Duplicate sync is idempotent
        let synced_again = backend.sync_markdown_memories_to_sqlite().unwrap();
        assert_eq!(synced_again, 0);

        // 6. Export lesson to markdown
        let lesson = LessonEntry {
            id: 42,
            file_path: "src/utils.ts".to_string(),
            symbol_name: "date-formatter".to_string(),
            error_context: "Invalid date format on Safari".to_string(),
            solution: "Use ISO-8601 strings".to_string(),
            kind: "convention".to_string(),
            created_at: "2026-10-19".to_string(),
            stale: 0,
            stale_reason: None,
            confidence_score: 1.0,
            touch_count: 0,
            last_verified_at: String::new(),
            freshness_warning: None,
        };
        let exported_path = backend.export_lesson_to_markdown(&lesson).unwrap();
        assert!(exported_path.exists());
        let read_back = std::fs::read_to_string(&exported_path).unwrap();
        assert!(read_back.contains("date-formatter"));
        assert!(read_back.contains("Use ISO-8601 strings"));
    }

    #[test]
    fn test_lifecycle_status_and_token_budget_guard() {
        let temp_dir = tempfile::tempdir().unwrap();
        let db_path = temp_dir.path().join("test_budget.db");
        let backend = GraphBackend::open(Some(&db_path.to_string_lossy())).unwrap();
        backend.set_project_path(Some(&temp_dir.path().to_string_lossy()));

        let gotcha_dir = temp_dir.path().join(".ozy").join("memories").join("gotcha");
        std::fs::create_dir_all(&gotcha_dir).unwrap();

        // 1. Create a resolved memory (should be excluded from pre-flight alert)
        let resolved_md = r#"---
id: "mem_resolved_01"
kind: "gotcha"
status: "resolved"
topic_key: "old-migration-bug"
title: "Old Alembic deadlock"
severity: "critical"
applies_to: ["app/database.py"]
---
Fixed in v2.0
"#;
        std::fs::write(gotcha_dir.join("01_resolved.md"), resolved_md).unwrap();

        // 2. Create 4 active memories with different severities
        let active1 = r#"---
id: "mem_act_01"
kind: "gotcha"
status: "active"
topic_key: "low-severity-rule"
title: "Docstring formatting"
severity: "low"
date: "2026-01-01"
applies_to: ["app/database.py"]
---
Use PEP 257.
"#;
        std::fs::write(gotcha_dir.join("02_low.md"), active1).unwrap();

        let active2 = r#"---
id: "mem_act_02"
kind: "gotcha"
status: "active"
topic_key: "medium-severity-rule"
title: "Timeout config"
severity: "medium"
date: "2026-02-01"
applies_to: ["app/database.py"]
---
Set timeout to 30s.
"#;
        std::fs::write(gotcha_dir.join("03_medium.md"), active2).unwrap();

        let active3 = r#"---
id: "mem_act_03"
kind: "gotcha"
topic_key: "high-severity-rule"
title: "N+1 query hazard"
severity: "high"
date: "2026-03-01"
applies_to: ["app/database.py"]
---
Always use joinedload.
"#;
        std::fs::write(gotcha_dir.join("04_high.md"), active3).unwrap();

        let active4 = r#"---
id: "mem_act_04"
kind: "gotcha"
topic_key: "critical-severity-rule"
title: "Connection pool exhaustion"
severity: "critical"
date: "2026-04-01"
applies_to: ["app/database.py"]
---
Close all sessions.
"#;
        std::fs::write(gotcha_dir.join("05_critical.md"), active4).unwrap();

        // 3. Format alert: resolved should be skipped, 4 active detected, top 3 shown, 1 omitted
        let alert = backend.format_applicable_gotchas_alert("app/database.py").expect("Alert must exist");
        assert!(alert.contains("[ALERT: GOTCHA_PREVENTION: 4 ACTIVE MEMORIES DETECTED FOR 'app/database.py']"));
        // Critical must be present
        assert!(alert.contains("Connection pool exhaustion"));
        // High must be present
        assert!(alert.contains("N+1 query hazard"));
        // Medium must be present
        assert!(alert.contains("Timeout config"));
        // Low should be omitted due to Top 3 Token Budget Guard
        assert!(!alert.contains("Docstring formatting"));
        // Resolved must NOT be present
        assert!(!alert.contains("Old Alembic deadlock"));
        // Omission message must be present
        assert!(alert.contains("1 older or lower-priority gotchas omitted to save tokens"));
    }
}
