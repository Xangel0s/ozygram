use ozymem_core::graph_backend::GraphBackend;
use ozymem_core::mcp_common::{self, ContentBlock, ToolCallResult};
use serde_json::Value;
use crate::formatters::build_architecture_report;

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

    let arch = build_architecture_report(backend).await?;
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

