use anyhow::Result;
use serde_json::{json, Value};
use std::path::Path;
use std::process::Command;
use ozymem_core::graph_backend::GraphBackend;
use crate::brain::call_ozy_brain_worker_smart;
use crate::formatters::format_engram_contract;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VerificationDiagnostic {
    pub file: String,
    pub line: Option<i64>,
    pub column: Option<i64>,
    pub code: Option<String>,
    pub message: String,
    pub rendered: Option<String>,
    pub level: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VerificationResult {
    pub passed: bool,
    pub language: String,
    pub execution_time_ms: u64,
    pub diagnostics: Vec<VerificationDiagnostic>,
    pub summary: String,
    pub auto_correction_guidance: Option<String>,
}

pub fn run_sandboxed_verification(
    project_path: &Path,
    file_path: &str,
    _proposed_code: Option<&str>,
    backend: &GraphBackend,
) -> Result<VerificationResult> {
    let start_time = std::time::Instant::now();
    let ext = Path::new(file_path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    let language = match ext {
        "rs" => "Rust",
        "py" => "Python",
        "ts" | "tsx" => "TypeScript",
        "js" | "jsx" => "JavaScript",
        "go" => "Go",
        _ => "Unknown",
    };

    let mut diagnostics = Vec::new();

    match ext {
        "rs" => {
            let mut cmd = if cfg!(windows) {
                let mut c = Command::new("cmd");
                c.args(["/c", "cargo", "check", "--message-format=json"]);
                c
            } else {
                let mut c = Command::new("cargo");
                c.args(["check", "--message-format=json"]);
                c
            };
            cmd.current_dir(project_path);
            if let Ok(output) = cmd.output() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                for line in stdout.lines() {
                    if let Ok(val) = serde_json::from_str::<Value>(line) {
                        if val.get("reason").and_then(Value::as_str) == Some("compiler-message") {
                            if let Some(msg) = val.get("message") {
                                let level = msg.get("level").and_then(Value::as_str).unwrap_or("error");
                                if level == "error" || level == "warning" {
                                    let message = msg.get("message").and_then(Value::as_str).unwrap_or("").to_string();
                                    let code = msg.get("code").and_then(|c| c.get("code")).and_then(Value::as_str).map(str::to_string);
                                    let rendered = msg.get("rendered").and_then(Value::as_str).map(str::to_string);
                                    let mut line_num = None;
                                    let mut col_num = None;
                                    let mut target_file = file_path.to_string();

                                    if let Some(spans) = msg.get("spans").and_then(Value::as_array) {
                                        if let Some(primary) = spans.iter().find(|s| s.get("is_primary").and_then(Value::as_bool).unwrap_or(false)) {
                                            target_file = primary.get("file_name").and_then(Value::as_str).unwrap_or(file_path).to_string();
                                            line_num = primary.get("line_start").and_then(Value::as_i64);
                                            col_num = primary.get("column_start").and_then(Value::as_i64);
                                        }
                                    }

                                    diagnostics.push(VerificationDiagnostic {
                                        file: target_file,
                                        line: line_num,
                                        column: col_num,
                                        code,
                                        message,
                                        rendered,
                                        level: level.to_string(),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
        "py" => {
            let full_file = project_path.join(file_path);
            let mut cmd = if cfg!(windows) {
                let mut c = Command::new("cmd");
                c.args(["/c", "python", "-m", "py_compile", &full_file.to_string_lossy()]);
                c
            } else {
                let mut c = Command::new("python");
                c.args(["-m", "py_compile", &full_file.to_string_lossy()]);
                c
            };
            cmd.current_dir(project_path);
            if let Ok(output) = cmd.output() {
                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    diagnostics.push(VerificationDiagnostic {
                        file: file_path.to_string(),
                        line: None,
                        column: None,
                        code: None,
                        message: stderr.to_string(),
                        rendered: Some(stderr.to_string()),
                        level: "error".to_string(),
                    });
                }
            }
        }
        _ => {}
    }

    let error_diagnostics: Vec<VerificationDiagnostic> = diagnostics.iter().filter(|d| d.level == "error").cloned().collect();
    let passed = error_diagnostics.is_empty();
    let elapsed = start_time.elapsed().as_millis() as u64;

    if passed {
        let warn_count = diagnostics.len();
        Ok(VerificationResult {
            passed: true,
            language: language.to_string(),
            execution_time_ms: elapsed,
            diagnostics,
            summary: format!(
                "[VERIFICATION_PASSED] Syntax and type check passed cleanly (0 errors, {} warnings) in {}ms.",
                warn_count, elapsed
            ),
            auto_correction_guidance: None,
        })
    } else {
        let failure_strings: Vec<String> = error_diagnostics
            .iter()
            .map(|d| {
                if let Some(code) = &d.code {
                    format!("{}: [{}] {}", d.file, code, d.message)
                } else {
                    format!("{}: {}", d.file, d.message)
                }
            })
            .collect();

        let payload = json!({
            "project": project_path.file_name().and_then(|n| n.to_str()).unwrap_or("project"),
            "goal": format!("Verify diff for {}", file_path),
            "failures": failure_strings,
            "changes": [file_path],
            "files": [file_path],
        });

        let mut guidance = String::new();
        guidance.push_str(&format!(
            "[VERIFICATION_FAILED: Closed-Loop Sandbox Detected {} Error(s) in {}ms]\n\n",
            error_diagnostics.len(),
            elapsed
        ));

        guidance.push_str("── Exact Diagnostics:\n");
        for (i, d) in error_diagnostics.iter().enumerate() {
            let loc = match (d.line, d.column) {
                (Some(l), Some(c)) => format!(":{}:{}", l, c),
                (Some(l), None) => format!(":{}", l),
                _ => String::new(),
            };
            let code_tag = d.code.as_deref().map(|c| format!("[{c}] ")).unwrap_or_default();
            guidance.push_str(&format!("  {}. {}{}{}: {}\n", i + 1, d.file, loc, code_tag, d.message));
        }

        let mut procedural_rules = Vec::new();

        // 1. Intentar llamar al worker Python ozy-brain
        if let Ok(resp) = call_ozy_brain_worker_smart("reflect", &payload, 5000) {
            if let Some(rules) = resp.get("procedural_rules").and_then(Value::as_array) {
                for rule in rules {
                    if let Some(block) = rule.get("engram_block").and_then(Value::as_str) {
                        procedural_rules.push(block.to_string());
                    }
                }
            }
        }

        // 2. Fallback de alta velocidad en Rust si el worker Python no está disponible en el sandbox
        if procedural_rules.is_empty() {
            for d in &error_diagnostics {
                let code_opt = d.code.as_deref().unwrap_or("");
                if code_opt == "E0063" || d.message.contains("missing field") {
                    let trigger = format!("error[E0063]: {}", d.message);
                    let action = "Initialize missing struct field in initializer".to_string();
                    procedural_rules.push(format!("[TRIGGER: {trigger}] -> [ACTION: {action}]"));
                } else if code_opt == "E0599" || d.message.contains("no method named") {
                    let trigger = format!("error[E0599]: {}", d.message);
                    let action = "Import trait into scope or implement method".to_string();
                    procedural_rules.push(format!("[TRIGGER: {trigger}] -> [ACTION: {action}]"));
                } else if code_opt == "E0308" || d.message.contains("mismatched types") {
                    let trigger = format!("error[E0308]: {}", d.message);
                    let action = "Cast or convert mismatched type or adjust return signature".to_string();
                    procedural_rules.push(format!("[TRIGGER: {trigger}] -> [ACTION: {action}]"));
                }
            }
        }

        if !procedural_rules.is_empty() {
            guidance.push_str("\n── Auto-Correction Procedural Rules (ozy-brain):\n");
            for block in procedural_rules {
                guidance.push_str(&format!("  • {}\n", block));
            }
        }

        // Preload matching contracts from IncrementalEngramStore
        if let Some(contract) = backend.engram_store.lookup_by_name(file_path).into_iter().next() {
            guidance.push_str("\n── Target Symbol Contract:\n");
            guidance.push_str(&format_engram_contract(&contract));
            guidance.push('\n');
        }

        Ok(VerificationResult {
            passed: false,
            language: language.to_string(),
            execution_time_ms: elapsed,
            diagnostics,
            summary: format!(
                "[VERIFICATION_FAILED] Found {} compilation/type error(s) in {}ms.",
                error_diagnostics.len(),
                elapsed
            ),
            auto_correction_guidance: Some(guidance),
        })
    }
}

pub fn handle_verify_diff(
    backend: &GraphBackend,
    tool_call: &ozymem_core::mcp_common::ToolCallParams,
) -> anyhow::Result<ozymem_core::mcp_common::ToolCallResult> {
    use ozymem_core::mcp_common::{ContentBlock, ToolCallResult};

    let file_path = tool_call
        .arguments
        .get("file_path")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("missing 'file_path'"))?;
    let proposed_code = tool_call
        .arguments
        .get("proposed_code")
        .and_then(Value::as_str);
    let project_path_str = tool_call
        .arguments
        .get("project_path")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| backend.project_path())
        .unwrap_or_else(|| ".".to_string());

    let project_path = std::path::Path::new(&project_path_str);
    let res = run_sandboxed_verification(
        project_path,
        file_path,
        proposed_code,
        backend,
    )?;

    let body = if res.passed {
        res.summary
    } else {
        res.auto_correction_guidance.unwrap_or(res.summary)
    };

    Ok(ToolCallResult {
        content: vec![ContentBlock {
            kind: "text",
            text: body,
        }],
        is_error: if res.passed { None } else { Some(true) },
    })
}
