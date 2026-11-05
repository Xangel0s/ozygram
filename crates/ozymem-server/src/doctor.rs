use ozymem_core::graph_backend::GraphBackend;
use ozymem_core::mcp_common::{self, ContentBlock, ToolCallResult};
use serde_json::Value;
use crate::formatters::build_architecture_report;
use std::io::Read;
use std::path::{Path, PathBuf};
use regex::Regex;
use walkdir::WalkDir;

pub(crate) async fn handle_ozy_doctor(
    backend: Option<&GraphBackend>,
    tool_call: &mcp_common::ToolCallParams,
) -> anyhow::Result<ToolCallResult> {
    let include_projects = tool_call
        .arguments
        .get("include_projects")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let json_format = tool_call
        .arguments
        .get("format")
        .and_then(Value::as_str)
        .unwrap_or("text")
        == "json";
    let home = std::env::var_os("OZYMEM_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(std::path::PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(std::path::PathBuf::from))
        .ok_or_else(|| anyhow::anyhow!("home directory not found"))?;
    let config_path = home.join(".ozymem.toml");
    let registry_path = home.join(".ozymem").join("registry.db");
    let mut checks = Vec::new();
    checks.push(serde_json::json!({"name":"config", "severity": if config_path.exists() {"ok"} else {"warning"}, "detail": config_path.display().to_string()}));
    checks.push(serde_json::json!({"name":"registry_db", "severity": if registry_path.exists() {"ok"} else {"warning"}, "detail": registry_path.display().to_string()}));

    let ast_diags_count = backend.and_then(|b| b.count_ast_diagnostics().ok()).unwrap_or(0);
    checks.push(serde_json::json!({
        "name": "ast_diagnostics",
        "severity": if ast_diags_count == 0 { "ok" } else { "warning" },
        "detail": format!("{} static AST/syntax warning(s) detected during index scan", ast_diags_count)
    }));

    let emb_status = backend
        .map(|b| b.get_embedding_status())
        .unwrap_or_else(ozymem_core::graph_backend::GraphBackend::check_model_files_status);
    let (emb_sev, emb_desc) = match &emb_status {
        ozymem_core::graph_backend::EmbeddingModelStatus::Ready => {
            ("ok", "Embedding model loaded and ready in local cache")
        }
        ozymem_core::graph_backend::EmbeddingModelStatus::Downloading => {
            ("warning", "Embedding model downloading in background")
        }
        ozymem_core::graph_backend::EmbeddingModelStatus::NotDownloaded => (
            "warning",
            "Embedding model not downloaded. Background download available; BM25 lexical search active",
        ),
        ozymem_core::graph_backend::EmbeddingModelStatus::CorruptedOrDeleted => (
            "critical",
            "Embedding model files missing or corrupted. Automatic re-download available",
        ),
        ozymem_core::graph_backend::EmbeddingModelStatus::Failed(msg) => ("critical", msg.as_str()),
    };
    checks.push(serde_json::json!({
        "name": "embedding_model",
        "severity": emb_sev,
        "detail": format!("Status: {:?} - {}", emb_status, emb_desc)
    }));

    // Check Hardware Acceleration & GPU status (strictly optional and configurable)
    let cfg = backend
        .and_then(|b| b.project_path())
        .and_then(|p| ozymem_core::config::OzyConfig::load_resolved(Some(std::path::Path::new(&p))).ok())
        .map(|(c, _)| c)
        .unwrap_or_default();
    let accel_mode = cfg.compute.acceleration.to_lowercase();
    let has_nvidia = std::path::Path::new(r"C:\Windows\System32\nvidia-smi.exe").exists()
        || std::env::var_os("CUDA_PATH").is_some()
        || std::path::Path::new("/usr/bin/nvidia-smi").exists();

    let (accel_sev, accel_detail) = match accel_mode.as_str() {
        "cpu" => (
            "ok",
            "Mode: cpu - Hardware acceleration disabled by user (safe CPU mode, 0% GPU VRAM)"
                .to_string(),
        ),
        "cuda" | "gpu" => {
            if has_nvidia {
                (
                    "ok",
                    "Mode: cuda - Dedicated GPU acceleration active for RAM MCTS and tensor operations"
                        .to_string(),
                )
            } else {
                (
                    "warning",
                    "Mode: cuda - CUDA requested, but no dedicated GPU driver detected; fallback to CPU"
                        .to_string(),
                )
            }
        }
        _ => {
            if has_nvidia {
                (
                    "ok",
                    "Mode: auto - Dedicated GPU detected and ready (auto-switching between CPU and GPU)"
                        .to_string(),
                )
            } else {
                (
                    "ok",
                    "Mode: auto - Safe CPU mode active (Count-Sketch 384D & SQLite WAL; 0% GPU VRAM)"
                        .to_string(),
                )
            }
        }
    };
    checks.push(serde_json::json!({
        "name": "hardware_acceleration",
        "severity": accel_sev,
        "detail": accel_detail
    }));

    if let Some(b) = backend {
        if let Some(proj) = b.project_path() {
            let p = std::path::Path::new(&proj);
            if p.join(".git").exists() {
                let hook_path = p.join(".git").join("hooks").join("post-commit");
                let hook_installed = hook_path.exists()
                    && std::fs::read_to_string(&hook_path)
                        .map(|c| c.contains("ozymem hook run post-commit"))
                        .unwrap_or(false);
                checks.push(serde_json::json!({
                    "name": "git_hook",
                    "severity": if hook_installed { "ok" } else { "warning" },
                    "detail": if hook_installed {
                        "Post-commit knowledge capture hook is active"
                    } else {
                        "Post-commit hook not installed. Run 'ozymem hook install' or MCP 'install_git_hook' for automatic knowledge capture on commit"
                    }
                }));
            }
        }
    }

    let mut projects_json = Vec::new();
    if include_projects {
        match ozymem_core::registry::ProjectRegistry::open().and_then(|r| r.list_projects()) {
            Ok(projects) => {
                for p in projects {
                    let path_exists = std::path::Path::new(&p.path).exists();
                    let db_exists = std::path::Path::new(&p.path).join(".ozymem").join("memory.db").exists();
                    projects_json.push(serde_json::json!({"name":p.name,"path":p.path,"status":format!("{:?}", p.status),"path_exists":path_exists,"memory_db_exists":db_exists}));
                }
            }
            Err(e) => checks.push(serde_json::json!({"name":"project_registry", "severity":"critical", "detail": e.to_string()})),
        }
    }
    let payload = serde_json::json!({
        "doctor":"ozy_doctor",
        "mode":"preview_safe",
        "checks":checks,
        "projects":projects_json,
        "recommended_actions":[
            {"kind":"safe","action":"Run 'ozymem hook install' or MCP 'install_git_hook' to capture knowledge automatically on commit."},
            {"kind":"safe","action":"Run ozy_project action=refresh for projects with stale or empty indexes."},
            {"kind":"needs_confirmation","action":"Remove or archive broken/stale projects only after user confirmation."},
            {"kind":"manual","action":"Review skills metadata before applying best-practice guidance."}
        ]
    });
    let text = if json_format {
        serde_json::to_string_pretty(&payload)?
    } else {
        format_doctor_payload(&payload)
    };
    Ok(ToolCallResult {
        content: vec![ContentBlock { kind: "text", text }],
        is_error: None,
    })
}


pub(crate) fn classify_duplicate_block(block: &str) -> (&'static str, &'static str) {
    let lower = block.to_lowercase();
    let is_procedural = lower.contains("if ")
        || lower.contains("for ")
        || lower.contains("while ")
        || lower.contains("match ")
        || lower.contains("switch ")
        || lower.contains("try ")
        || lower.contains("catch ")
        || lower.contains("except ")
        || lower.contains("return ")
        || lower.contains("def ")
        || lower.contains("fn ");

    let is_structural = lower.contains("class ")
        || lower.contains("basemodel")
        || lower.contains("interface ")
        || lower.contains("struct ")
        || lower.contains("schema")
        || lower.contains("type ")
        || lower.contains("enum ");

    if is_structural && !is_procedural {
        (
            "[STRUCTURAL BOILERPLATE]",
            "DTO/Model/Schema structure repeated by design. Safe to keep modular or unify if domain contracts match.",
        )
    } else {
        (
            "[HIGH-PRIORITY REFACTOR CANDIDATE]",
            "Procedural logic or utility repeated across modules. Consider consolidating into a shared helper function.",
        )
    }
}

pub(crate) async fn build_code_doctor_report(
    backend: &GraphBackend,
    scope: Option<&str>,
    min_lines: usize,
    max_findings: usize,
) -> anyhow::Result<String> {
    let files = backend.list_all_files().unwrap_or_default();
    let scope_norm = scope.unwrap_or("").replace('\\', "/");
    let mut seen: std::collections::HashMap<String, Vec<(String, usize)>> =
        std::collections::HashMap::new();
    for fp in files {
        let fp_norm = fp.replace('\\', "/");
        if !scope_norm.is_empty() && !fp_norm.contains(&scope_norm) {
            continue;
        }
        let path = std::path::Path::new(&fp);
        if !path.exists() {
            continue;
        }
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !matches!(
            ext,
            "rs" | "ts" | "tsx" | "js" | "jsx" | "php" | "py" | "go" | "sql"
        ) {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(path) else {
            continue;
        };
        let lines: Vec<String> = content.lines().map(normalize_code_line).collect();
        if lines.len() < min_lines {
            continue;
        }
        for i in 0..=lines.len().saturating_sub(min_lines) {
            let block = lines[i..i + min_lines]
                .iter()
                .filter(|l| !l.is_empty())
                .cloned()
                .collect::<Vec<_>>()
                .join("\n");
            if block.lines().count() >= min_lines / 2 && block.len() > 80 {
                seen.entry(block).or_default().push((fp.clone(), i + 1));
            }
        }
    }
    let mut duplicates: Vec<_> = seen
        .into_iter()
        .filter(|(_, locs)| locs.len() > 1)
        .collect();
    duplicates.sort_by(|a, b| b.1.len().cmp(&a.1.len()));

    let mut refactor_candidates = Vec::new();
    let mut boilerplate_findings = Vec::new();
    for (block, locs) in duplicates {
        let (category, suggestion) = classify_duplicate_block(&block);
        if category == "[HIGH-PRIORITY REFACTOR CANDIDATE]" {
            refactor_candidates.push((category, suggestion, locs));
        } else {
            boilerplate_findings.push((category, suggestion, locs));
        }
    }

    let arch = build_architecture_report(backend, scope).await?;
    let mut body = format!(
        "# Ozy Doctor Code\n\nMode: preview_safe (no files modified)\n\n## Duplicate findings (Refactor Candidates: {}, Structural Boilerplate: {})\n\n",
        refactor_candidates.len(),
        boilerplate_findings.len()
    );

    if refactor_candidates.is_empty() && boilerplate_findings.is_empty() {
        body.push_str("No duplicate blocks found with current threshold.\n");
    } else {
        if !refactor_candidates.is_empty() {
            body.push_str("### [High-Priority Refactor Candidates] (Procedural / Utility Logic)\n");
            for (idx, (cat, sugg, locs)) in refactor_candidates.iter().take(max_findings).enumerate() {
                body.push_str(&format!(
                    "{}. {} Duplicate block in {} locations:\n",
                    idx + 1,
                    cat,
                    locs.len()
                ));
                for (fp, line) in locs.iter().take(6) {
                    body.push_str(&format!("   - {fp}:L{line}\n"));
                }
                body.push_str(&format!("   suggestion: {sugg}\n\n"));
            }
        }

        if !boilerplate_findings.is_empty() {
            body.push_str("### [Structural Boilerplate] (Schemas / DTOs / Types)\n");
            for (idx, (cat, sugg, locs)) in boilerplate_findings.iter().take(max_findings).enumerate() {
                body.push_str(&format!(
                    "{}. {} Duplicate structure in {} locations:\n",
                    idx + 1,
                    cat,
                    locs.len()
                ));
                for (fp, line) in locs.iter().take(6) {
                    body.push_str(&format!("   - {fp}:L{line}\n"));
                }
                body.push_str(&format!("   suggestion: {sugg}\n\n"));
            }
        }
    }

    body.push_str("\n## Architecture and best-practice feedback\n");
    body.push_str(&arch);
    body.push_str("\n## Autosanado preview\n- safe: refresh stale indexes before making decisions.\n- needs_confirmation: remove redundant code only after tests and user approval.\n- manual: review official skills.sh guidance before applying framework-specific changes.\n");
    Ok(body)
}

pub(crate) fn normalize_code_line(line: &str) -> String {
    let t = line.trim();
    if t.is_empty() || t.starts_with("//") || t.starts_with('#') || t.starts_with("*") {
        return String::new();
    }
    t.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(crate) fn format_doctor_payload(payload: &Value) -> String {
    let mut body = String::from("# Ozy Doctor\n\nMode: preview_safe\n\nChecks:\n");
    if let Some(checks) = payload.get("checks").and_then(Value::as_array) {
        for c in checks {
            body.push_str(&format!(
                "- [{}] {}: {}\n",
                c.get("severity")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown"),
                c.get("name").and_then(Value::as_str).unwrap_or("check"),
                c.get("detail").and_then(Value::as_str).unwrap_or("")
            ));
        }
    }
    if let Some(projects) = payload.get("projects").and_then(Value::as_array) {
        body.push_str(&format!("\nProjects: {}\n", projects.len()));
        for p in projects.iter().take(20) {
            body.push_str(&format!(
                "- {} ({}) path={} db={}\n",
                p.get("name").and_then(Value::as_str).unwrap_or("unknown"),
                p.get("status").and_then(Value::as_str).unwrap_or("unknown"),
                p.get("path_exists")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                p.get("memory_db_exists")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
            ));
        }
    }
    body.push_str(
        "\nRecommended actions are preview-only; no autosanado is executed without confirmation.\n",
    );
    body
}

/// Audit OpenXML (.xlsx) templates for XML namespace corruption, undeclared mc:Ignorable prefixes, and archive integrity.
pub(crate) async fn audit_excel_templates(
    backend: Option<&GraphBackend>,
    target_path: Option<&str>,
) -> anyhow::Result<String> {
    let mut files_to_check: Vec<PathBuf> = Vec::new();

    if let Some(tp) = target_path {
        let p = Path::new(tp);
        if p.is_file() {
            files_to_check.push(p.to_path_buf());
        } else if p.is_dir() {
            for entry in WalkDir::new(p).into_iter().filter_map(|e| e.ok()) {
                let ep = entry.path();
                if ep.is_file() && ep.extension().and_then(|s| s.to_str()) == Some("xlsx") {
                    files_to_check.push(ep.to_path_buf());
                }
            }
        }
    } else {
        let root = backend
            .and_then(|b| b.project_path())
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

        let search_dirs = [
            root.join("app").join("templates"),
            root.join("api-geofal-crm").join("app").join("templates"),
            root.join("templates"),
            root.clone(),
        ];

        let mut seen = std::collections::HashSet::new();
        for dir in &search_dirs {
            if dir.exists() {
                for entry in WalkDir::new(dir)
                    .max_depth(8)
                    .into_iter()
                    .filter_entry(|e| {
                        let name = e.file_name().to_string_lossy();
                        !name.starts_with('.')
                            && name != "node_modules"
                            && name != "target"
                            && name != ".git"
                            && name != ".venv"
                            && name != "dist"
                            && name != "build"
                    })
                    .filter_map(|e| e.ok())
                {
                    let p = entry.path();
                    if p.is_file()
                        && p.extension().and_then(|s| s.to_str()) == Some("xlsx")
                        && seen.insert(p.to_path_buf())
                    {
                        files_to_check.push(p.to_path_buf());
                    }
                }
                if !files_to_check.is_empty() && dir != &root {
                    break;
                }
            }
        }
    }

    if files_to_check.is_empty() {
        return Ok("No .xlsx templates found in scope to audit.\n".to_string());
    }

    let mut body = String::from("# OpenXML Excel Template Audit Report\n\n");
    let mut corrupted_count = 0usize;
    let mut healthy_count = 0usize;

    let ignorable_regex = Regex::new(r#"(?:mc:Ignorable|ns\d+:Ignorable|Ignorable)\s*=\s*"([^"]+)""#)
        .map_err(|e| anyhow::anyhow!("Regex compilation error: {e}"))?;

    for file_path in &files_to_check {
        let display_path = file_path.display().to_string();
        let file = match std::fs::File::open(file_path) {
            Ok(f) => f,
            Err(e) => {
                corrupted_count += 1;
                body.push_str(&format!(
                    "[FAIL] {}\n  - [CRITICAL: IO_ERROR] Unable to open file: {}\n\n",
                    display_path, e
                ));
                continue;
            }
        };

        let mut archive = match zip::ZipArchive::new(std::io::BufReader::new(file)) {
            Ok(a) => a,
            Err(e) => {
                corrupted_count += 1;
                body.push_str(&format!(
                    "[FAIL] {}\n  - [CRITICAL: INVALID_ZIP_ARCHIVE] Corrupted ZIP structure: {}\n\n",
                    display_path, e
                ));
                continue;
            }
        };

        let mut issues = Vec::new();
        let mut sheet_count = 0usize;

        for i in 0..archive.len() {
            let mut entry = match archive.by_index(i) {
                Ok(e) => e,
                Err(e) => {
                    issues.push(format!("[CORRUPTED ENTRY #{i}] {}", e));
                    continue;
                }
            };

            let name = entry.name().to_string();
            if name.starts_with("xl/worksheets/sheet") && name.ends_with(".xml") {
                sheet_count += 1;
                let mut content = String::new();
                if let Err(e) = entry.read_to_string(&mut content) {
                    issues.push(format!(
                        "[CRITICAL: UNREADABLE_XML] {}: failed to read UTF-8 stream: {}",
                        name, e
                    ));
                    continue;
                }

                if let Some(caps) = ignorable_regex.captures(&content) {
                    if let Some(val) = caps.get(1) {
                        let prefixes: Vec<&str> = val.as_str().split_whitespace().collect();
                        for prefix in prefixes {
                            let decl_pattern_1 = format!("xmlns:{}=", prefix);
                            let decl_pattern_2 = format!("xmlns:{} =", prefix);
                            if !content.contains(&decl_pattern_1) && !content.contains(&decl_pattern_2) {
                                issues.push(format!(
                                    "[CRITICAL: UNDECLARED_NAMESPACE] {}: undeclared prefix '{}' referenced in Ignorable attribute.\n    Explanation: Violates ISO/IEC 29500 (OpenXML). Causes Excel repair error 0x808c0002 on file open.\n    Remediation: Declare xmlns:{} in root <worksheet> element or strip '{}' from Ignorable.",
                                    name, prefix, prefix, prefix
                                ));
                            }
                        }
                    }
                }

                if !content.contains("</worksheet>") && !content.ends_with("/>") {
                    issues.push(format!(
                        "[CRITICAL: MALFORMED_XML] {}: missing closing </worksheet> root tag.",
                        name
                    ));
                }
            } else if name == "xl/workbook.xml" {
                let mut content = String::new();
                if let Err(e) = entry.read_to_string(&mut content) {
                    issues.push(format!(
                        "[CRITICAL: UNREADABLE_XML] xl/workbook.xml: failed to read UTF-8 stream: {}",
                        e
                    ));
                } else if !content.contains("</workbook>") && !content.ends_with("/>") {
                    issues.push(
                        "[CRITICAL: MALFORMED_XML] xl/workbook.xml: missing closing </workbook> root tag."
                            .to_string(),
                    );
                }
            }
        }

        if issues.is_empty() {
            healthy_count += 1;
            body.push_str(&format!(
                "[PASS] {} (valid OpenXML structure, {} worksheet(s) verified)\n",
                display_path, sheet_count
            ));
        } else {
            corrupted_count += 1;
            body.push_str(&format!("[FAIL] {}\n", display_path));
            for issue in issues {
                body.push_str(&format!("  - {}\n", issue));
            }
            body.push('\n');
        }
    }

    body.push_str(&format!(
        "\nSummary: {} file(s) checked - {} healthy, {} corrupted.\nStatus: {}\n",
        files_to_check.len(),
        healthy_count,
        corrupted_count,
        if corrupted_count == 0 { "HEALTHY" } else { "ACTION_REQUIRED" }
    ));

    Ok(body)
}

fn extract_attr<'a>(tag: &'a str, attr: &str) -> Option<&'a str> {
    let pattern = format!("{attr}=\"");
    let start = tag.find(&pattern)? + pattern.len();
    let end = tag[start..].find('"')? + start;
    Some(&tag[start..end])
}

fn parse_cell_coord(coord: &str) -> (String, usize) {
    let mut col = String::new();
    let mut row_str = String::new();
    for c in coord.chars() {
        if c.is_ascii_alphabetic() {
            col.push(c.to_ascii_uppercase());
        } else if c.is_ascii_digit() {
            row_str.push(c);
        }
    }
    let row_num = row_str.parse::<usize>().unwrap_or(0);
    (col, row_num)
}

fn decode_xml_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}

/// Inspects an Excel (.xlsx) data file for cell anomalies: unformatted float times,
/// string length overflows, formula errors, and provides a structured preview of worksheets.
pub(crate) async fn inspect_excel_data(
    backend: Option<&GraphBackend>,
    path_arg: Option<&str>,
    max_rows: usize,
    max_text_len: usize,
) -> anyhow::Result<String> {
    let target_path = if let Some(p) = path_arg {
        let clean = p.trim().trim_matches(|c| c == '"' || c == '\'');
        let pb = PathBuf::from(clean);
        if pb.is_absolute() && pb.exists() {
            pb
        } else if let Some(b) = backend {
            if let Some(proj) = b.project_path() {
                let candidate = Path::new(&proj).join(clean);
                if candidate.exists() {
                    candidate
                } else {
                    pb
                }
            } else {
                pb
            }
        } else {
            pb
        }
    } else {
        let root = backend
            .and_then(|b| b.project_path())
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        let mut found = None;
        for entry in WalkDir::new(&root).max_depth(3).into_iter().filter_map(|e| e.ok()) {
            let p = entry.path();
            if p.is_file() && p.extension().and_then(|s| s.to_str()) == Some("xlsx") {
                found = Some(p.to_path_buf());
                break;
            }
        }
        match found {
            Some(f) => f,
            None => return Ok("No .xlsx file found to inspect. Specify path=\"path/to/file.xlsx\".\n".to_string()),
        }
    };

    if !target_path.exists() {
        return Ok(format!("File not found: {}\n", target_path.display()));
    }

    let file = match std::fs::File::open(&target_path) {
        Ok(f) => f,
        Err(e) => return Ok(format!("[FAIL] Could not open file {}: {}\n", target_path.display(), e)),
    };

    let mut archive = match zip::ZipArchive::new(std::io::BufReader::new(file)) {
        Ok(a) => a,
        Err(e) => return Ok(format!("[FAIL] Corrupted or invalid .xlsx (ZIP) archive {}: {}\n", target_path.display(), e)),
    };

    // Find all archive entry names
    let all_names: Vec<String> = (0..archive.len())
        .filter_map(|i| archive.by_index(i).ok().map(|e| e.name().to_string()))
        .collect();

    // 1. Extract shared strings
    let mut shared_strings = Vec::new();
    let sst_entry = all_names
        .iter()
        .find(|n| *n == "xl/sharedStrings.xml" || *n == "sharedStrings.xml" || n.ends_with("/sharedStrings.xml"))
        .cloned();
    if let Some(sst_name) = sst_entry {
        if let Ok(mut entry) = archive.by_name(&sst_name) {
            let mut xml = String::new();
            if entry.read_to_string(&mut xml).is_ok() {
                let mut pos = 0;
                while let Some(si_start) = xml[pos..].find("<si") {
                    let abs_si = pos + si_start;
                    let Some(si_end) = xml[abs_si..].find("</si>") else { break };
                    let si_slice = &xml[abs_si..abs_si + si_end];
                    let mut s = String::new();
                    let mut t_pos = 0;
                    while let Some(t_idx) = si_slice[t_pos..].find("<t") {
                        let abs_t = t_pos + t_idx;
                        let Some(tag_close) = si_slice[abs_t..].find('>') else { break };
                        let content_start = abs_t + tag_close + 1;
                        let Some(t_end) = si_slice[content_start..].find("</t>") else { break };
                        s.push_str(&si_slice[content_start..content_start + t_end]);
                        t_pos = content_start + t_end + 4;
                    }
                    shared_strings.push(decode_xml_entities(&s));
                    pos = abs_si + si_end + 5;
                }
            }
        }
    }

    // 2. Extract sheets list from workbook.xml
    let mut sheet_names: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let wb_entry = all_names
        .iter()
        .find(|n| *n == "xl/workbook.xml" || *n == "workbook.xml" || n.ends_with("/workbook.xml"))
        .cloned();
    if let Some(wb_name) = wb_entry {
        if let Ok(mut entry) = archive.by_name(&wb_name) {
            let mut xml = String::new();
            if entry.read_to_string(&mut xml).is_ok() {
                let sheet_re = Regex::new(r#"<sheet\s+[^>]*name="([^"]+)"[^>]*sheetId="([^"]+)""#)?;
                for cap in sheet_re.captures_iter(&xml) {
                    let name = cap.get(1).map(|m| m.as_str().to_string()).unwrap_or_default();
                    let id = cap.get(2).map(|m| m.as_str().to_string()).unwrap_or_default();
                    sheet_names.insert(format!("sheet{}.xml", id), name.clone());
                    sheet_names.insert(format!("xl/worksheets/sheet{}.xml", id), name);
                }
            }
        }
    }

    // 3. Collect sheet entry names
    let sheet_entries: Vec<String> = all_names
        .into_iter()
        .filter(|n| {
            (n.starts_with("xl/worksheets/sheet") || n.starts_with("sheet") || n.contains("/sheet"))
                && n.ends_with(".xml")
                && !n.ends_with(".rels")
        })
        .collect();

    let mut body = format!(
        "# Ozy Doctor Excel Data Inspector\n\nFile: `{}`\nTotal Shared Strings: {}\nWorksheets Found: {}\n\n",
        target_path.display(),
        shared_strings.len(),
        sheet_entries.len()
    );

    let mut total_anomalies = 0usize;

    for sheet_entry_name in &sheet_entries {
        let base_name = sheet_entry_name
            .trim_start_matches("xl/worksheets/")
            .split('/')
            .last()
            .unwrap_or(sheet_entry_name);
        let display_name = sheet_names
            .get(base_name)
            .or_else(|| sheet_names.get(sheet_entry_name))
            .cloned()
            .unwrap_or_else(|| base_name.to_string());

        let mut content = String::new();
        if let Ok(mut entry) = archive.by_name(sheet_entry_name) {
            let _ = entry.read_to_string(&mut content);
        }

        // Fast dimension extraction without regex
        let dimension = if let Some(dim_pos) = content.find("<dimension") {
            let max_look = content.len().min(dim_pos + 120);
            extract_attr(&content[dim_pos..max_look], "ref")
                .map(|s| s.to_string())
                .unwrap_or_else(|| "Unknown".to_string())
        } else {
            "Unknown".to_string()
        };

        struct ExtractedCell {
            pos: usize,
            coord: String,
            value: String,
            is_formula_err: bool,
        }

        let mut extracted: Vec<ExtractedCell> = Vec::new();

        // Pass A: Find all <v> tags in document order
        let mut v_search = 0;
        while let Some(rel) = content[v_search..].find("<v") {
            let v_idx = v_search + rel;
            let after_v = &content[v_idx + 2..];
            if after_v.starts_with('>') || after_v.starts_with(' ') {
                if let Some(v_close) = after_v.find("</v>") {
                    if let Some(tag_close) = after_v.find('>') {
                        let inner_val = &after_v[tag_close + 1..v_close];
                        let back_limit = v_idx.saturating_sub(1024);
                        if let Some(rel_c) = content[back_limit..v_idx].rfind("<c ") {
                            let c_start = back_limit + rel_c;
                            if let Some(rel_c_close) = content[c_start..v_idx].find('>') {
                                let cell_tag = &content[c_start..c_start + rel_c_close + 1];
                                if !cell_tag.ends_with("/>") {
                                    if let Some(coord) = extract_attr(cell_tag, "r") {
                                        let cell_type = extract_attr(cell_tag, "t").map(|s| s.to_string());
                                        let is_err = cell_type.as_deref() == Some("e");
                                        let resolved = if cell_type.as_deref() == Some("s") {
                                            if let Ok(idx) = inner_val.trim().parse::<usize>() {
                                                if idx < shared_strings.len() {
                                                    shared_strings[idx].clone()
                                                } else {
                                                    format!("[SST#{idx}]")
                                                }
                                            } else {
                                                inner_val.to_string()
                                            }
                                        } else {
                                            decode_xml_entities(inner_val.trim())
                                        };

                                        extracted.push(ExtractedCell {
                                            pos: c_start,
                                            coord: coord.to_string(),
                                            value: resolved,
                                            is_formula_err: is_err,
                                        });
                                    }
                                }
                            }
                        }
                    }
                    v_search = v_idx + 2 + v_close + 4;
                    continue;
                }
            }
            v_search = v_idx + 2;
        }

        // Pass B: Find all <is> tags (inline strings)
        let mut is_search = 0;
        while let Some(rel) = content[is_search..].find("<is") {
            let is_idx = is_search + rel;
            let after_is = &content[is_idx + 3..];
            if after_is.starts_with('>') || after_is.starts_with(' ') {
                if let Some(is_close) = after_is.find("</is>") {
                    let is_inner = &after_is[..is_close];
                    let mut s = String::new();
                    let mut t_pos = 0;
                    while let Some(t_idx) = is_inner[t_pos..].find("<t") {
                        let abs_t = t_pos + t_idx;
                        let Some(tag_close) = is_inner[abs_t..].find('>') else { break };
                        let content_start = abs_t + tag_close + 1;
                        let Some(t_end) = is_inner[content_start..].find("</t>") else { break };
                        s.push_str(&is_inner[content_start..content_start + t_end]);
                        t_pos = content_start + t_end + 4;
                    }

                    let back_limit = is_idx.saturating_sub(1024);
                    if let Some(rel_c) = content[back_limit..is_idx].rfind("<c ") {
                        let c_start = back_limit + rel_c;
                        if let Some(rel_c_close) = content[c_start..is_idx].find('>') {
                            let cell_tag = &content[c_start..c_start + rel_c_close + 1];
                            if !cell_tag.ends_with("/>") {
                                if let Some(coord) = extract_attr(cell_tag, "r") {
                                    extracted.push(ExtractedCell {
                                        pos: c_start,
                                        coord: coord.to_string(),
                                        value: decode_xml_entities(&s),
                                        is_formula_err: false,
                                    });
                                }
                            }
                        }
                    }
                    is_search = is_idx + 3 + is_close + 5;
                    continue;
                }
            }
            is_search = is_idx + 3;
        }

        // Sort extracted cells by document position
        extracted.sort_by_key(|c| c.pos);

        let mut anomalies = Vec::new();
        let mut row_cells: std::collections::BTreeMap<usize, std::collections::BTreeMap<String, String>> = std::collections::BTreeMap::new();
        let non_empty_count = extracted.len();

        for cell in extracted {
            let coord = cell.coord;
            let resolved_val = cell.value;
            let is_formula_err = cell.is_formula_err;

            if resolved_val.is_empty() {
                continue;
            }

            let (col, row_num) = parse_cell_coord(&coord);

            // Anomaly 1: Formula errors (#REF!, #VALUE!, etc.)
            if is_formula_err
                || resolved_val.starts_with("#REF!")
                || resolved_val.starts_with("#VALUE!")
                || resolved_val.starts_with("#DIV/0!")
                || resolved_val.starts_with("#N/A")
                || resolved_val.starts_with("#NAME?")
                || resolved_val.starts_with("#NUM!")
                || resolved_val.starts_with("#NULL!")
            {
                anomalies.push(format!(
                    "- [FORMULA_ERROR] Celda {coord}: contiene error de fórmula '{resolved_val}'."
                ));
            }

            // Anomaly 2: Unformatted float time (e.g. 0.41666666666666669 -> 10:00:00)
            if (resolved_val.starts_with("0.") && resolved_val.len() >= 7)
                || (resolved_val.starts_with('0') && resolved_val.contains('.') && resolved_val.len() >= 7)
            {
                if let Ok(f) = resolved_val.parse::<f64>() {
                    if f > 0.0 && f < 1.0 {
                        let total_seconds = (f * 86400.0).round() as u64 % 86400;
                        let h = total_seconds / 3600;
                        let m = (total_seconds % 3600) / 60;
                        let s = total_seconds % 60;
                        let time_str = format!("{:02}:{:02}:{:02}", h, m, s);
                        anomalies.push(format!(
                            "- [CRITICAL: UNFORMATTED_FLOAT_TIME] Celda {coord}: valor '{resolved_val}' (longitud: {} chars) equivale a hora '{time_str}'.\n  Alerta: Excede límites de columna VARCHAR(10) en PostgreSQL y causa DataError / StringDataRightTruncation.",
                            resolved_val.len()
                        ));
                    }
                }
            }

            // Anomaly 3: String Length Overflow
            if resolved_val.len() > max_text_len {
                let preview = if resolved_val.len() > 60 {
                    format!("{}...", &resolved_val[..57])
                } else {
                    resolved_val.clone()
                };
                anomalies.push(format!(
                    "- [TEXT_LENGTH_OVERFLOW] Celda {coord}: longitud {} caracteres (> {}): '{preview}'",
                    resolved_val.len(),
                    max_text_len
                ));
            }

            // Anomaly 4: Unformatted serial datetime (e.g. 46302.41666)
            if resolved_val.contains('.') && resolved_val.len() >= 8 {
                if let Ok(f) = resolved_val.parse::<f64>() {
                    if (40000.0..=60000.0).contains(&f) {
                        let frac = f - (f as i64 as f64);
                        let total_seconds = (frac * 86400.0).round() as u64 % 86400;
                        let h = total_seconds / 3600;
                        let m = (total_seconds % 3600) / 60;
                        let s = total_seconds % 60;
                        let time_str = format!("{:02}:{:02}:{:02}", h, m, s);
                        anomalies.push(format!(
                            "- [SUSPECT_SERIAL_DATETIME] Celda {coord}: float '{resolved_val}' representa fecha serial Excel con hora no formateada ({time_str})."
                        ));
                    }
                }
            }

            if row_num > 0 && row_cells.len() < max_rows + 20 {
                row_cells.entry(row_num).or_default().insert(col, resolved_val);
            }
        }

        total_anomalies += anomalies.len();

        body.push_str(&format!(
            "## Worksheet: `{}` (`{}`)\n- Dimension: {}\n- Celdas no vacías: {}\n- Anomalías detectadas: {}\n\n",
            display_name,
            sheet_entry_name,
            dimension,
            non_empty_count,
            anomalies.len()
        ));

        if anomalies.is_empty() {
            body.push_str("Sin anomalías críticas detectadas en esta hoja.\n\n");
        } else {
            body.push_str("### Anomalías Detectadas:\n");
            for a in anomalies.iter().take(30) {
                body.push_str(&format!("{}\n", a));
            }
            if anomalies.len() > 30 {
                body.push_str(&format!("... y {} anomalías adicionales omitidas.\n", anomalies.len() - 30));
            }
            body.push('\n');
        }

        // Preview sample rows
        if !row_cells.is_empty() {
            body.push_str("### Vista Previa de Filas (Muestreo):\n");
            let mut sample_rows: Vec<usize> = row_cells.keys().cloned().take(max_rows).collect();
            sample_rows.sort();

            // Collect all unique columns in these sample rows
            let mut cols_set = std::collections::BTreeSet::new();
            for r in &sample_rows {
                if let Some(cols) = row_cells.get(r) {
                    for c in cols.keys() {
                        cols_set.insert(c.clone());
                    }
                }
            }
            let cols_list: Vec<String> = cols_set.into_iter().take(12).collect();

            // Table header
            body.push_str("| Fila |");
            for c in &cols_list {
                body.push_str(&format!(" {} |", c));
            }
            body.push('\n');

            body.push_str("| --- |");
            for _ in &cols_list {
                body.push_str(" --- |");
            }
            body.push('\n');

            // Table rows
            for r in &sample_rows {
                body.push_str(&format!("| {} |", r));
                if let Some(cells) = row_cells.get(r) {
                    for c in &cols_list {
                        let val = cells.get(c).map(|s| s.replace('|', "\\|")).unwrap_or_default();
                        let preview = if val.len() > 25 {
                            format!("{}...", &val[..22])
                        } else {
                            val
                        };
                        body.push_str(&format!(" {} |", preview));
                    }
                } else {
                    for _ in &cols_list {
                        body.push_str(" |");
                    }
                }
                body.push('\n');
            }
            body.push('\n');
        }
    }

    body.push_str(&format!(
        "## Resumen General\nTotal de Hojas Auditadas: {}\nTotal de Anomalías: {}\nEstado: {}\n",
        sheet_entries.len(),
        total_anomalies,
        if total_anomalies == 0 { "CLEAN" } else { "ANOMALIES_FOUND" }
    ));

    Ok(body)
}

