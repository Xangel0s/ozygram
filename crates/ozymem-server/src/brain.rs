use ozymem_core::graph_backend::GraphBackend;
use ozymem_core::McpBackend;
use ozymem_core::mcp_common::{self};
use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use sha2::{Digest, Sha256};

#[derive(Debug, serde::Deserialize)]
pub struct OzyBrainResponse {
    pub action: String,
    pub summary: String,
    pub plan: Vec<String>,
    pub risks: Vec<String>,
    pub recommendations: Vec<String>,
    pub memory_updates: Vec<String>,
    pub confidence: f64,
    pub brain_version: Option<String>,
    pub brain_schema_version: Option<String>,
    pub provenance: Option<Vec<BrainProvenance>>,
    pub safe_mode: Option<bool>,
    pub engine: Option<String>,
    pub procedural_rules: Option<Vec<Value>>,
    pub reflection_report: Option<Value>,
}

#[derive(Debug, serde::Deserialize)]
pub struct BrainProvenance {
    pub path: String,
    pub score: Option<i64>,
    pub reasons: Option<Vec<String>>,
    pub commit_hash: Option<String>,
}

pub fn validate_ozy_brain_response_schema(value: &Value) -> anyhow::Result<OzyBrainResponse> {
    if let Some(err) = value.get("error").and_then(Value::as_str) {
        anyhow::bail!("Ozy Brain worker error: {err}");
    }
    serde_json::from_value::<OzyBrainResponse>(value.clone())
        .map_err(|e| anyhow::anyhow!("Ozy Brain response failed strict schema validation: {e}"))
}

pub(crate) async fn handle_ozy_brain(
    backend: &GraphBackend,
    tool_call: &mcp_common::ToolCallParams,
    action: &str,
) -> anyhow::Result<String> {
    let project = tool_call
        .arguments
        .get("project")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            backend.project_path().and_then(|p| {
                Path::new(&p)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(str::to_string)
            })
        })
        .unwrap_or_else(|| "default".to_string());
    let goal = tool_call
        .arguments
        .get("goal")
        .and_then(Value::as_str)
        .or_else(|| tool_call.arguments.get("query").and_then(Value::as_str))
        .unwrap_or("guide the agent safely");
    let limit = tool_call
        .arguments
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(20) as usize;
    let max_tokens = tool_call
        .arguments
        .get("max_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(4000) as usize;

    let summary = backend.get_graph_summary().await?;
    let files = backend
        .list_all_files()
        .unwrap_or_default()
        .into_iter()
        .take(limit.min(50))
        .collect::<Vec<_>>();
    let observations = backend
        .recent_observations(Some(&project), Some("project"), limit.min(30))
        .unwrap_or_default();
    let relevant_observations = backend
        .search_observations(goal, Some(&project), Some("project"), None, limit.min(30))
        .unwrap_or_default();
    let lessons = backend
        .recent_lessons(None, limit.min(30))
        .await
        .unwrap_or_default();
    let relevant_lessons = backend
        .search_lessons(goal, None, limit.min(30))
        .await
        .unwrap_or_default();

    let git_context = backend
        .project_path()
        .map(|p| build_ozy_brain_git_context(Path::new(&p), limit.min(20)))
        .unwrap_or_else(|| json!({ "available": false, "reason": "project path unavailable" }));

    let mut impact = Vec::new();
    if let Some(target) = tool_call.arguments.get("file_path").or_else(|| tool_call.arguments.get("file")).and_then(Value::as_str) {
        impact = backend.analyze_impact(target, 2);
    } else if let Some(first_file) = files.first() {
        impact = backend.analyze_impact(first_file, 2);
    }

    let payload = json!({
        "project": project,
        "goal": goal,
        "query": tool_call.arguments.get("query").cloned().unwrap_or(Value::Null),
        "max_tokens": max_tokens,
        "graph_summary": summary,
        "files": files,
        "memories": observations,
        "relevant_memories": relevant_observations,
        "lessons": lessons,
        "relevant_lessons": relevant_lessons,
        "git_context": git_context,
        "impact": impact,
        "failures": tool_call.arguments.get("failures").cloned().unwrap_or_else(|| json!([])),
        "changes": tool_call.arguments.get("changes").cloned().unwrap_or_else(|| json!([])),
    });
    let req_json = serde_json::to_string(&payload).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(req_json.as_bytes());
    let req_hash = format!("{:x}", hasher.finalize());

    let start_time = std::time::Instant::now();
    let action_owned = action.to_string();
    let payload_for_worker = payload.clone();
    let response =
        tokio::task::spawn_blocking(move || call_ozy_brain_worker_smart(&action_owned, &payload_for_worker, 10_000))
            .await??;

    let validated = validate_ozy_brain_response_schema(&response)?;
    let latency_ms = start_time.elapsed().as_millis() as u64;

    let git_head = git_context
        .get("recent_commits")
        .and_then(Value::as_array)
        .and_then(|arr| arr.get(0))
        .and_then(|c| c.get("id").or(c.get("hash")))
        .and_then(Value::as_str);

    let raw_resp_str = serde_json::to_string(&response).unwrap_or_default();

    let _ = backend.record_brain_audit(
        &req_hash,
        action,
        Some(&project),
        git_head,
        validated.engine.as_deref(),
        validated.brain_version.as_deref(),
        validated.brain_schema_version.as_deref(),
        latency_ms,
        validated.confidence,
        Some(&validated.summary),
        Some(&raw_resp_str),
    );

    Ok(format_ozy_brain_response(&response))
}

pub(crate) fn build_ozy_brain_git_context(project_path: &Path, limit: usize) -> Value {
    let mut status_files = Vec::new();
    let status_output = Command::new("git")
        .arg("-C")
        .arg(project_path)
        .arg("status")
        .arg("--short")
        .output();

    let mut status_error = None;
    if let Ok(output) = status_output {
        if output.status.success() {
            let text = String::from_utf8_lossy(&output.stdout);
            for line in text.lines().take(limit.max(1)) {
                if line.len() >= 3 {
                    let status = line[..2].trim().to_string();
                    let path = line[3..].trim().to_string();
                    if !path.is_empty() {
                        status_files.push(json!({ "status": status, "path": path }));
                    }
                }
            }
        } else {
            status_error = Some(String::from_utf8_lossy(&output.stderr).trim().to_string());
        }
    } else if let Err(err) = status_output {
        status_error = Some(err.to_string());
    }

    let recent_commits = ozymem_core::git_backend::GitBackend::open(project_path)
        .and_then(|git| git.recent_changes(5))
        .map(|changes| serde_json::to_value(changes).unwrap_or_else(|_| json!([])))
        .unwrap_or_else(|err| json!({ "error": err }));

    json!({
        "available": status_error.is_none(),
        "repo_root": project_path.display().to_string(),
        "dirty": !status_files.is_empty(),
        "status_files": status_files,
        "status_error": status_error,
        "recent_commits": recent_commits,
    })
}

pub fn try_call_ozy_brain_persistent(action: &str, payload: &Value, timeout_ms: u64) -> anyhow::Result<Value> {
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpStream;

    let port = std::env::var("OZY_BRAIN_PORT").unwrap_or_else(|_| "9473".to_string());
    let addr = format!("127.0.0.1:{port}");
    let mut stream = TcpStream::connect_timeout(&addr.parse()?, std::time::Duration::from_millis(timeout_ms.min(800)))?;
    stream.set_read_timeout(Some(std::time::Duration::from_millis(timeout_ms)))?;

    let req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": action,
        "params": payload
    });

    let mut line = serde_json::to_string(&req)?;
    line.push('\n');
    stream.write_all(line.as_bytes())?;
    stream.flush()?;

    let mut reader = BufReader::new(stream);
    let mut resp_line = String::new();
    reader.read_line(&mut resp_line)?;

    let parsed: Value = serde_json::from_str(resp_line.trim())?;
    if let Some(err) = parsed.get("error") {
        anyhow::bail!("Persistent worker error: {err}");
    }
    if let Some(res) = parsed.get("result") {
        Ok(res.clone())
    } else {
        Ok(parsed)
    }
}

pub fn call_ozy_brain_worker_smart(action: &str, payload: &Value, timeout_ms: u64) -> anyhow::Result<Value> {
    let mode = std::env::var("OZY_BRAIN_MODE").unwrap_or_else(|_| "auto".to_string());
    if mode != "oneshot" {
        if let Ok(res) = try_call_ozy_brain_persistent(action, payload, timeout_ms) {
            return Ok(res);
        }
    }
    call_ozy_brain_worker(action, payload, timeout_ms)
}

pub fn call_ozy_brain_worker(action: &str, payload: &Value, timeout_ms: u64) -> anyhow::Result<Value> {
    let brain_dir = resolve_ozy_brain_dir().ok_or_else(|| {
        anyhow::anyhow!(
            "Ozy Brain worker not found. Set OZY_BRAIN_PATH or keep python/ozy-brain in the repository."
        )
    })?;
    let payload_text = serde_json::to_string(payload)?;
    let mut last_error = String::new();
    for candidate in python_candidates() {
        let mut command = if cfg!(target_os = "windows") {
            let mut c = Command::new("cmd");
            let mut args = vec!["/C"];
            args.extend(candidate.split_whitespace());
            args.extend(["-m", "ozy_brain", "--action", action]);
            c.args(args);
            c
        } else {
            let mut c = Command::new(candidate);
            c.args(["-m", "ozy_brain", "--action", action]);
            c
        };
        let mut child = match command
            .current_dir(&brain_dir)
            .env("PYTHONPATH", &brain_dir)
            .env("PYTHONIOENCODING", "utf-8")
            .env("PYTHONUTF8", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(child) => child,
            Err(e) => {
                last_error = e.to_string();
                continue;
            }
        };
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(payload_text.as_bytes())?;
        }

        let start = std::time::Instant::now();
        loop {
            if child.try_wait()?.is_some() {
                let output = child.wait_with_output()?;
                if output.status.success() {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    return serde_json::from_str(stdout.trim()).map_err(Into::into);
                }
                last_error = String::from_utf8_lossy(&output.stderr).to_string();
                if last_error.trim().is_empty() {
                    last_error = String::from_utf8_lossy(&output.stdout).to_string();
                }
                break;
            }
            if start.elapsed() > std::time::Duration::from_millis(timeout_ms) {
                let _ = child.kill();
                return Err(anyhow::anyhow!(
                    "Ozy Brain worker timed out after {}ms",
                    timeout_ms
                ));
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
    }
    Err(anyhow::anyhow!("Ozy Brain worker failed: {}", last_error))
}

pub(crate) fn python_candidates() -> Vec<String> {
    if let Ok(explicit) = std::env::var("OZY_BRAIN_PYTHON") {
        return vec![explicit];
    }
    if cfg!(target_os = "windows") {
        vec!["python".to_string(), "py -3".to_string()]
    } else {
        vec!["python3".to_string(), "python".to_string()]
    }
}

pub fn resolve_ozy_brain_dir() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("OZY_BRAIN_PATH") {
        let p = PathBuf::from(path);
        if p.join("ozy_brain").join("__main__.py").exists() {
            return Some(p);
        }
    }
    let mut candidates = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.clone());
        for ancestor in cwd.ancestors() {
            candidates.push(ancestor.to_path_buf());
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            for ancestor in parent.ancestors() {
                candidates.push(ancestor.to_path_buf());
            }
        }
    }
    for base in candidates {
        let p = base.join("python").join("ozy-brain");
        if p.join("ozy_brain").join("__main__.py").exists() {
            return Some(p);
        }
    }
    None
}

pub(crate) fn format_ozy_brain_response(value: &Value) -> String {
    if let Some(error) = value.get("error").and_then(Value::as_str) {
        return format!("Ozy Brain error: {error}");
    }
    let action = value
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or("brain");
    let summary = value.get("summary").and_then(Value::as_str).unwrap_or("");
    let confidence = value
        .get("confidence")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    let mut body = format!("# Ozy Brain: {action}\n\n{summary}\n\nConfidence: {confidence:.2}\n");
    for (title, key) in [
        ("Plan", "plan"),
        ("Risks", "risks"),
        ("Recommendations", "recommendations"),
        ("Memory Updates", "memory_updates"),
    ] {
        if let Some(items) = value.get(key).and_then(Value::as_array) {
            if !items.is_empty() {
                body.push_str(&format!("\n## {title}\n"));
                for item in items {
                    if let Some(text) = item.as_str() {
                        body.push_str(&format!("- {text}\n"));
                    }
                }
            }
        }
    }
    if let Some(structured) = value.get("structured_plan").and_then(Value::as_object) {
        if !structured.is_empty() {
            body.push_str("\n## Structured Plan\n");
            if let Some(level) = structured.get("autonomy_level").and_then(Value::as_str) {
                body.push_str(&format!("- autonomy_level: {level}\n"));
            }
            if let Some(goal) = structured.get("goal").and_then(Value::as_str) {
                body.push_str(&format!("- goal: {goal}\n"));
            }
            if let Some(blast) = structured.get("blast_radius_analysis").and_then(Value::as_object) {
                body.push_str("\n### Blast Radius Analysis\n");
                if let Some(tier) = blast.get("risk_tier").and_then(Value::as_str) {
                    body.push_str(&format!("- risk_tier: {tier}\n"));
                }
                if let Some(total) = blast.get("total_blast_radius").and_then(Value::as_i64) {
                    body.push_str(&format!("- total_impacted_files: {total}\n"));
                }
                if let Some(high) = blast.get("high_severity_files").and_then(Value::as_array) {
                    if !high.is_empty() {
                        let list = high.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ");
                        body.push_str(&format!("- high_severity_nodes: [{list}]\n"));
                    }
                }
            }
            if let Some(seq) = structured.get("topological_edit_sequence").and_then(Value::as_array) {
                if !seq.is_empty() {
                    body.push_str("\n### Topological Edit Sequence (Leaves to Root)\n");
                    for item in seq {
                        let step = item.get("step").and_then(Value::as_i64).unwrap_or(0);
                        let file = item.get("file").and_then(Value::as_str).unwrap_or("");
                        let phase = item.get("phase").and_then(Value::as_str).unwrap_or("");
                        let action = item.get("action").and_then(Value::as_str).unwrap_or("");
                        body.push_str(&format!("{step}. [{phase}] {file} → {action}\n"));
                    }
                }
            }
            if let Some(phases) = structured.get("phases").and_then(Value::as_array) {
                body.push_str("\n### Phases\n");
                for phase in phases {
                    let name = phase.get("name").and_then(Value::as_str).unwrap_or("phase");
                    let objective = phase.get("objective").and_then(Value::as_str).unwrap_or("");
                    let exit = phase
                        .get("exit_condition")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    body.push_str(&format!("- {name}: {objective} | exit: {exit}\n"));
                }
            }
            for (title, key) in [
                ("Candidate Files", "candidate_files"),
                ("Suggested Commands", "suggested_commands"),
                ("Validation Checks", "validation_checks"),
                ("Stop Conditions", "stop_conditions"),
                ("What Not To Touch", "what_not_to_touch"),
            ] {
                if let Some(items) = structured.get(key).and_then(Value::as_array) {
                    if !items.is_empty() {
                        body.push_str(&format!("\n### {title}\n"));
                        for item in items {
                            if let Some(text) = item.as_str() {
                                body.push_str(&format!("- {text}\n"));
                            }
                        }
                    }
                }
            }
        }
    }
    if let Some(pack) = value.get("brain_context_pack").and_then(Value::as_object) {
        if !pack.is_empty() {
            body.push_str("\n## Brain Context Pack\n");
            if let Some(risk) = pack.get("risk_level").and_then(Value::as_str) {
                body.push_str(&format!("- risk_level: {risk}\n"));
            }
            if let Some(dirty) = pack.get("dirty").and_then(Value::as_bool) {
                body.push_str(&format!("- dirty: {dirty}\n"));
            }
            if let Some(files) = pack.get("candidate_file_scores").and_then(Value::as_array) {
                if !files.is_empty() {
                    body.push_str("\n### Candidate File Scores\n");
                    for file in files.iter().take(10) {
                        let path = file
                            .get("path")
                            .and_then(Value::as_str)
                            .unwrap_or("unknown");
                        let score = file.get("score").and_then(Value::as_i64).unwrap_or(0);
                        let reasons = file
                            .get("reasons")
                            .and_then(Value::as_array)
                            .map(|items| {
                                items
                                    .iter()
                                    .filter_map(Value::as_str)
                                    .take(3)
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            })
                            .unwrap_or_default();
                        body.push_str(&format!("- {path} (score {score}) {reasons}\n"));
                    }
                }
            }
            for (title, key) in [
                ("Recommended Validation", "recommended_validation"),
                ("Persistible Recommendations", "persistible_recommendations"),
            ] {
                if let Some(items) = pack.get(key).and_then(Value::as_array) {
                    if !items.is_empty() {
                        body.push_str(&format!("\n### {title}\n"));
                        for item in items {
                            if let Some(text) = item.as_str() {
                                body.push_str(&format!("- {text}\n"));
                            }
                        }
                    }
                }
            }
        }
    }
    if let Some(policy) = value.get("execution_policy").and_then(Value::as_object) {
        if !policy.is_empty() {
            body.push_str("\n## Execution Policy\n");
            if let Some(mode) = policy.get("mode").and_then(Value::as_str) {
                body.push_str(&format!("- mode: {mode}\n"));
            }
            if let Some(authority) = policy.get("rust_authority").and_then(Value::as_bool) {
                body.push_str(&format!("- rust_authority: {authority}\n"));
            }
            if let Some(role) = policy.get("python_role").and_then(Value::as_str) {
                body.push_str(&format!("- python_role: {role}\n"));
            }
            for (title, key) in [
                ("Can Do Without Confirmation", "can_do_without_confirmation"),
                ("Requires Confirmation", "requires_confirmation"),
                ("Forbidden For Python Worker", "forbidden_for_python_worker"),
            ] {
                if let Some(items) = policy.get(key).and_then(Value::as_array) {
                    if !items.is_empty() {
                        body.push_str(&format!("\n### {title}\n"));
                        for item in items {
                            if let Some(text) = item.as_str() {
                                body.push_str(&format!("- {text}\n"));
                            }
                        }
                    }
                }
            }
        }
    }
    if let Some(report) = value.get("reflection_report").and_then(Value::as_object) {
        if !report.is_empty() {
            body.push_str("\n## Reflection Report\n");
            if let Some(creep) = report.get("scope_creep_detected").and_then(Value::as_bool) {
                body.push_str(&format!("- scope_creep_detected: {creep}\n"));
            }
            if let Some(rules) = value.get("procedural_rules").or_else(|| report.get("procedural_rules")).and_then(Value::as_array) {
                if !rules.is_empty() {
                    body.push_str("\n### Procedural Rules (Trigger -> Action)\n");
                    for rule in rules {
                        if let Some(block) = rule.get("engram_block").and_then(Value::as_str) {
                            body.push_str(&format!("- {}\n", block));
                        } else if let (Some(trigger), Some(action)) = (
                            rule.get("trigger").and_then(Value::as_str),
                            rule.get("action").and_then(Value::as_str),
                        ) {
                            body.push_str(&format!("- [TRIGGER: {}] -> [ACTION: {}]\n", trigger, action));
                        }
                    }
                }
            }
            for (title, key) in [
                ("Root Causes", "root_causes"),
                ("Out of Scope Files", "out_of_scope_files"),
                ("Extracted Gotchas", "extracted_gotchas"),
                ("Recommended Memory Actions", "recommended_memory_actions"),
            ] {
                if let Some(items) = report.get(key).and_then(Value::as_array) {
                    if !items.is_empty() {
                        body.push_str(&format!("\n### {title}\n"));
                        for item in items {
                            if let Some(text) = item.as_str() {
                                body.push_str(&format!("- {text}\n"));
                            }
                        }
                    }
                }
            }
        }
    }
    if let Some(assessment) = value.get("risk_assessment").and_then(Value::as_object) {
        if !assessment.is_empty() {
            body.push_str("\n## Risk Assessment\n");
            if let Some(level) = assessment.get("risk_level").and_then(Value::as_str) {
                body.push_str(&format!("- risk_level: {level}\n"));
            }
            if let Some(confirm) = assessment.get("requires_user_confirmation").and_then(Value::as_bool) {
                body.push_str(&format!("- requires_user_confirmation: {confirm}\n"));
            }
            for (title, key) in [
                ("Risk Categories", "risk_categories"),
                ("Critical Paths", "critical_paths"),
                ("Verification Checklist", "verification_checklist"),
            ] {
                if let Some(items) = assessment.get(key).and_then(Value::as_array) {
                    if !items.is_empty() {
                        body.push_str(&format!("\n### {title}\n"));
                        for item in items {
                            if let Some(text) = item.as_str() {
                                body.push_str(&format!("- {text}\n"));
                            }
                        }
                    }
                }
            }
        }
    }
    if let Some(model) = value.get("mental_model").and_then(Value::as_object) {
        if !model.is_empty() {
            body.push_str("\n## Project Mental Model\n");
            if let Some(arch) = model.get("architecture_type").and_then(Value::as_str) {
                body.push_str(&format!("- architecture_type: {arch}\n"));
            }
            if let Some(purpose) = model.get("purpose").and_then(Value::as_str) {
                body.push_str(&format!("- purpose: {purpose}\n"));
            }
            for (title, key) in [
                ("Core Modules", "core_modules"),
                ("Critical Data Flows", "critical_data_flows"),
                ("User Rules", "user_rules"),
                ("Validation Playbook", "validation_playbook"),
                ("Where To Look First", "where_to_look_first"),
            ] {
                if let Some(items) = model.get(key).and_then(Value::as_array) {
                    if !items.is_empty() {
                        body.push_str(&format!("\n### {title}\n"));
                        for item in items {
                            if let Some(text) = item.as_str() {
                                body.push_str(&format!("- {text}\n"));
                            }
                        }
                    }
                }
            }
        }
    }
    if let Some(calls) = value.get("suggested_mcp_calls").and_then(Value::as_array) {
        if !calls.is_empty() {
            body.push_str("\n## Suggested MCP Calls\n");
            for call in calls {
                let tool = call
                    .get("tool")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                let args = call
                    .get("arguments")
                    .map(|v| serde_json::to_string(v).unwrap_or_else(|_| "{}".to_string()))
                    .unwrap_or_else(|| "{}".to_string());
                body.push_str(&format!("- {tool} {args}\n"));
            }
        }
    }
    if let Some(prov_items) = value.get("provenance").and_then(Value::as_array) {
        if !prov_items.is_empty() {
            body.push_str("\n## Provenance & Source Locations\n");
            for item in prov_items {
                if let Some(path) = item.get("path").and_then(Value::as_str) {
                    let score = item.get("score").and_then(Value::as_i64).unwrap_or(0);
                    let commit = item.get("commit_hash").and_then(Value::as_str).unwrap_or("");
                    body.push_str(&format!("- {path} (score {score}) commit: {commit}\n"));
                }
            }
        }
    }
    body.push_str("\nSafe mode: advisory only; no files or commands were modified by Python.\n");
    body
}


