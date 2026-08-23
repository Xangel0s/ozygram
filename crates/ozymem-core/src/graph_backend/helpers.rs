use rusqlite::Connection;
use crate::graph_backend::types::{OZYMEM_DIR, MEMORY_DB};
use anyhow::{Context, Result};
use rusqlite::params;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use sha2::{Digest, Sha256};
use ozymem_parser::SupportedLanguage;
use crate::graph_backend::types::Inner;

pub fn strip_unc_prefix(path: PathBuf) -> PathBuf {
    let s = path.to_string_lossy();
    if let Some(stripped) = s.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{}", stripped))
    } else if let Some(stripped) = s.strip_prefix(r"\\?\") {
        PathBuf::from(stripped)
    } else {
        path
    }
}

/// Resolve the project-scoped database path.
///
/// Canonicalizes `project_path`, creates `.ozymem/` inside it,
/// and returns the path to `memory.db`.
pub fn resolve_project_db_path(project_path: &Path) -> Result<PathBuf> {
    if !project_path.exists() {
        let _ = std::fs::create_dir_all(project_path);
    }
    let canonical = project_path
        .canonicalize()
        .map(strip_unc_prefix)
        .unwrap_or_else(|_| strip_unc_prefix(project_path.to_path_buf()));
    let db_dir = canonical.join(OZYMEM_DIR);
    std::fs::create_dir_all(&db_dir)
        .with_context(|| format!("failed to create `{}`", db_dir.display()))?;
    let db_path = db_dir.join(MEMORY_DB);
    eprintln!("[ozymem] DB path: {}", db_path.display());
    Ok(db_path)
}

pub fn extract_prohibited_terms(rule_text: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let lower = rule_text.to_lowercase();
    for trigger in &["never use ", "never ", "do not use ", "do not ", "prohibited ", "evitar ", "forbidden "] {
        if let Some(pos) = lower.find(trigger) {
            let rest = &lower[pos + trigger.len()..];
            let clause = rest.split(|c| c == ',' || c == '.' || c == ';').next().unwrap_or(rest);
            let clause = clause.split("always").next().unwrap_or(clause);
            let clause = clause.split("instead").next().unwrap_or(clause);
            for word in clause.split(|c: char| !c.is_alphanumeric() && c != '_') {
                let w = word.trim();
                if w.len() >= 2 && !matches!(w, "the" | "a" | "an" | "for" | "to" | "in" | "on" | "with" | "and" | "or" | "use" | "using" | "rule" | "standard" | "point") {
                    terms.push(w.to_string());
                }
            }
        }
    }
    terms
}

/// Ensure `.ozymem/` is listed in the project's `.gitignore`.
///
/// Returns `true` if an entry was added, `false` if already present
/// or if the project root has no `.git` repo.
pub fn auto_manage_gitignore(project_path: &Path) -> Result<bool> {
    let gitignore_path = project_path.join(".gitignore");
    let entry = format!("/{}", OZYMEM_DIR);

    // Don't create .gitignore if no .git repo exists
    if !project_path.join(".git").exists() {
        return Ok(false);
    }

    if !gitignore_path.exists() {
        std::fs::write(&gitignore_path, format!("{entry}\n"))?;
        return Ok(true);
    }

    let content = std::fs::read_to_string(&gitignore_path)?;
    if content.lines().any(|l| {
        let t = l.trim();
        t == entry || t == OZYMEM_DIR || t == format!("{}/", OZYMEM_DIR)
    }) {
        return Ok(false);
    }

    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&gitignore_path)?;
    use std::io::Write;
    writeln!(file, "\n# Ozymem project memory\n{entry}")?;
    eprintln!("[ozymem] added `{entry}` to .gitignore");
    Ok(true)
}

/// Path to the legacy global DB at `~/.ozymem/memory.db`.
pub fn legacy_global_db_path() -> PathBuf {
    let home = home::home_dir().expect("cannot find home dir");
    home.join(OZYMEM_DIR).join(MEMORY_DB)
}

/// Mark lessons as stale when their file or symbol no longer exists.
///
/// Only lessons whose `file_path` is in `scanned_files` are evaluated.
/// - File deleted: `stale_reason = "file_deleted"`
/// - Symbol removed (file exists but `symbol_name` not in `functions`): `stale_reason = "symbol_removed"`
pub fn mark_stale_lessons(
    conn: &Connection,
    tenant_id: &str,
    scanned_files: &HashSet<String>,
    workspace_root: &str,
) -> Result<u64> {
    let now = chrono::Utc::now()
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string();
    let mut marked: u64 = 0;

    if scanned_files.is_empty() {
        return Ok(0);
    }

    // Process each scanned file individually to avoid dynamic IN-clause
    // parameter juggling that rusqlite handles poorly in bulk.
    let mut check_path = conn.prepare(
        "SELECT id, file_path FROM lessons
         WHERE tenant_id = ?1 AND stale = 0 AND file_path = ?2 AND workspace_root = ?3",
    )?;
    let mut check_symbol = conn.prepare(
        "SELECT id, file_path, symbol_name FROM lessons
         WHERE tenant_id = ?1 AND stale = 0 AND symbol_name != '' AND file_path = ?2 AND workspace_root = ?3"
    )?;
    let mut count_func = conn.prepare(
        "SELECT COUNT(*) FROM functions WHERE file_path = ?1 AND name = ?2 AND tenant_id = ?3",
    )?;
    let mut update = conn.prepare(
        "UPDATE lessons SET stale = 1, stale_reason = ?1, stale_since = ?2 WHERE id = ?3",
    )?;

    for file_path in scanned_files {
        // Step 1: file_deleted
        let rows: Vec<(i64, String)> = check_path
            .query_map(params![tenant_id, file_path, workspace_root], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })?
            .filter_map(|r| r.ok())
            .collect();

        for (id, stored_path) in &rows {
            if !Path::new(&stored_path).exists() {
                update.execute(params!["file_deleted", now, id])?;
                marked += 1;
            }
        }

        // Step 2: symbol_removed (only for lessons whose file still exists)
        let rows: Vec<(i64, String, String)> = check_symbol
            .query_map(params![tenant_id, file_path, workspace_root], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })?
            .filter_map(|r| r.ok())
            .collect();

        for (id, stored_path, symbol_name) in &rows {
            if !Path::new(stored_path).exists() {
                continue;
            }
            let exists: bool = count_func
                .query_row(params![stored_path, symbol_name, tenant_id], |row| {
                    row.get::<_, i64>(0)
                })
                .map(|c| c > 0)
                .unwrap_or(false);

            if !exists {
                update.execute(params!["symbol_removed", now, id])?;
                marked += 1;
            }
        }
    }

    Ok(marked)
}


pub fn is_noise_dir(path: &Path) -> bool {
    if !path.is_dir() {
        return false;
    }
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        matches!(
            name,
            "target"
                | "node_modules"
                | ".git"
                | ".svn"
                | "__pycache__"
                | ".ozymem"
                | "build"
                | "dist"
                | ".next"
                | ".cache"
                | "vendor"
                | "bower_components"
                | ".idea"
                | ".vscode"
        )
    } else {
        false
    }
}

/// Load ignore patterns from `.ozymemignore` and `.gitignore` in the project root.
pub fn load_ignore_patterns(project_root: &Path) -> Vec<String> {
    let mut patterns = Vec::new();
    for filename in &[".ozymemignore", ".gitignore"] {
        let path = project_root.join(filename);
        if let Ok(content) = std::fs::read_to_string(&path) {
            for line in content.lines() {
                let t = line.trim();
                if !t.is_empty() && !t.starts_with('#') {
                    patterns.push(t.to_string());
                }
            }
        }
    }
    patterns
}

/// Check if a path matches any ignore pattern.
pub fn path_matches_ignore(path: &Path, patterns: &[String], project_root: &Path) -> bool {
    if patterns.is_empty() {
        return false;
    }
    let relative = path.strip_prefix(project_root).unwrap_or(path);
    let rel = relative.to_string_lossy().replace('\\', "/");
    let rel_lower = rel.to_lowercase();

    for p in patterns {
        let pat = p.trim().replace('\\', "/");
        if pat.is_empty() {
            continue;
        }
        let pat_lower = pat.to_lowercase();

        // Exact match or directory prefix
        if rel_lower == pat_lower || rel_lower.starts_with(&format!("{}/", pat_lower)) {
            return true;
        }
        // Match any path component (e.g. "coverage" matches "src/coverage/index.html")
        for component in relative.components() {
            if let Some(comp) = component.as_os_str().to_str() {
                if comp.to_lowercase() == pat_lower {
                    return true;
                }
            }
        }
    }
    false
}

pub fn detect_language(path: &Path) -> SupportedLanguage {
    match path.extension().and_then(|e| e.to_str()) {
        Some("py") => SupportedLanguage::Python,
        Some("go") => SupportedLanguage::Go,
        Some("rs") => SupportedLanguage::Rust,
        Some("js") | Some("jsx") => SupportedLanguage::JavaScript,
        Some("ts") | Some("tsx") => SupportedLanguage::TypeScriptReact,
        Some("sql") => SupportedLanguage::SQL,
        _ => SupportedLanguage::Unknown,
    }
}

pub fn file_mtime(path: &Path) -> Option<String> {
    let metadata = std::fs::metadata(path).ok()?;
    let modified = metadata.modified().ok()?;
    let duration = modified.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some(duration.as_secs().to_string())
}

pub fn now_ts() -> String {
    chrono::Utc::now().to_rfc3339()
}

pub fn normalize_scope(scope: Option<&str>) -> &str {
    match scope {
        Some("personal") => "personal",
        _ => "project",
    }
}

pub fn normalize_topic_key(topic: &str) -> String {
    topic
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '/' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

pub fn normalized_hash(content: &str) -> String {
    let normalized = content
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    let mut hasher = Sha256::new();
    hasher.update(normalized.as_bytes());
    hex::encode(hasher.finalize())
}

pub(crate) fn default_project_name(inner: &Inner) -> &str {
    inner
        .project_path
        .as_deref()
        .and_then(|p| Path::new(p).file_name().and_then(|n| n.to_str()))
        .unwrap_or("default")
}
