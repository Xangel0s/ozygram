use ozymem_core::graph_backend::GraphBackend;
use ozymem_core::registry::ProjectRegistry;
use ozymem_core::mcp_common::{self, ContentBlock, ToolCallResult};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use crate::state::{Notifier, error_response, ok_response};

pub(crate) async fn handle_package_tool(
    id: Value,
    tool_call: &mcp_common::ToolCallParams,
    notifier: Option<&Notifier>,
) -> anyhow::Result<mcp_common::JsonRpcResponse> {
    let log = |level: &str, msg: String| {
        if let Some(ref n) = notifier {
            n.log(level, msg);
        }
    };

    match tool_call.name.as_str() {
        "create_project" => {
            let name = match tool_call.arguments.get("name").and_then(Value::as_str) {
                Some(n) => n,
                None => {
                    return Ok(error_response(
                        id,
                        -32602,
                        "Missing required parameter: name",
                    ));
                }
            };
            let parent = tool_call
                .arguments
                .get("path")
                .and_then(Value::as_str)
                .map(|s| PathBuf::from(s))
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
            let proj_type = tool_call
                .arguments
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("node");
            let packages = tool_call
                .arguments
                .get("packages")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();

            let project_dir = parent.join(name);
            if project_dir.exists() {
                return Ok(error_response(
                    id,
                    -32602,
                    &format!("Directory already exists: {}", project_dir.display()),
                ));
            }

            std::fs::create_dir_all(&project_dir)?;
            log(
                "info",
                format!(
                    "[ozymem-server] created directory {}",
                    project_dir.display()
                ),
            );

            // Run package manager init
            match proj_type {
                "node" => {
                    let init_cmd = if cfg!(target_os = "windows") {
                        std::process::Command::new("cmd")
                            .args(["/C", "pnpm", "init", "-y"])
                            .current_dir(&project_dir)
                            .output()
                            .map(|o| o.status.success())
                            .unwrap_or(false)
                    } else {
                        std::process::Command::new("pnpm")
                            .args(["init", "-y"])
                            .current_dir(&project_dir)
                            .output()
                            .map(|o| o.status.success())
                            .unwrap_or(false)
                    };
                    if !init_cmd {
                        let fallback = if cfg!(target_os = "windows") {
                            std::process::Command::new("cmd")
                                .args(["/C", "npm", "init", "-y"])
                                .current_dir(&project_dir)
                                .output()
                        } else {
                            std::process::Command::new("npm")
                                .args(["init", "-y"])
                                .current_dir(&project_dir)
                                .output()
                        };
                        fallback?;
                    }
                }
                "rust" => {
                    std::process::Command::new("cargo")
                        .args(["init"])
                        .current_dir(&project_dir)
                        .output()?;
                }
                _ => {}
            }

            // Install packages
            let mut installed = Vec::new();
            if !packages.is_empty() {
                let cmd = if cfg!(target_os = "windows") {
                    "cmd"
                } else {
                    "sh"
                };
                let args: Vec<&str> = if cfg!(target_os = "windows") {
                    let mut a = vec!["/C", "pnpm", "add"];
                    a.extend(packages.iter().map(|s| s.as_str()));
                    a
                } else {
                    let mut a = vec!["pnpm", "add"];
                    a.extend(packages.iter().map(|s| s.as_str()));
                    a
                };
                let output = std::process::Command::new(cmd)
                    .args(&args)
                    .current_dir(&project_dir)
                    .output()?;
                if output.status.success() {
                    installed = packages.clone();
                    log(
                        "info",
                        format!(
                            "[ozymem-server] installed {} packages in {}",
                            installed.len(),
                            project_dir.display()
                        ),
                    );
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    log(
                        "warn",
                        format!("[ozymem-server] pnpm install warning: {stderr}"),
                    );
                }
            }

            // Register in ProjectRegistry
            let reg = ProjectRegistry::open()?;
            let path_str = ozymem_core::normalize_path(&project_dir.to_string_lossy());
            let _project = reg.register(name, &path_str)?;
            log(
                "info",
                format!("[ozymem-server] registered project '{}'", name),
            );

            // Open backend and scan
            let gb = GraphBackend::open_for_project(&project_dir)?;
            let resolved_str = project_dir.to_string_lossy().to_string();
            gb.full_scan(&resolved_str, None)?;

            let body = format!(
                "Project '{}' created at {}.\n  Type: {}\n  Packages installed: {}\n  Project registered and scanned.",
                name,
                path_str,
                proj_type,
                if installed.is_empty() {
                    "none".to_string()
                } else {
                    installed.join(", ")
                },
            );
            Ok(ok_response(
                id,
                serde_json::to_value(ToolCallResult {
                    content: vec![ContentBlock {
                        kind: "text",
                        text: body,
                    }],
                    is_error: None,
                })?,
            ))
        }

        "add_package" => {
            let project_name = match tool_call
                .arguments
                .get("project_name")
                .and_then(Value::as_str)
            {
                Some(n) => n,
                None => {
                    return Ok(error_response(
                        id,
                        -32602,
                        "Missing required parameter: project_name",
                    ));
                }
            };
            let packages = match tool_call
                .arguments
                .get("packages")
                .and_then(Value::as_array)
            {
                Some(arr) => arr
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect::<Vec<_>>(),
                None => {
                    return Ok(error_response(
                        id,
                        -32602,
                        "Missing required parameter: packages",
                    ));
                }
            };
            let dev = tool_call
                .arguments
                .get("dev")
                .and_then(Value::as_bool)
                .unwrap_or(false);

            let reg = ProjectRegistry::open()?;
            let project = match reg.get_project_by_name(project_name)? {
                Some(p) => p,
                None => {
                    return Ok(error_response(
                        id,
                        -32602,
                        &format!("Project '{}' not found", project_name),
                    ));
                }
            };

            let project_dir = PathBuf::from(&project.path);
            let cmd = if cfg!(target_os = "windows") {
                "cmd"
            } else {
                "sh"
            };
            let mut shell_args = vec!["/C", "pnpm", "add"];
            if dev {
                shell_args.push("-D");
            }
            for p in &packages {
                shell_args.push(p.as_str());
            }
            let output = std::process::Command::new(cmd)
                .args(&shell_args)
                .current_dir(&project_dir)
                .output()?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Ok(error_response(
                    id,
                    -32603,
                    &format!("pnpm install failed: {stderr}"),
                ));
            }

            // Re-scan project to update package.json in the graph
            let gb = GraphBackend::open_for_project(&project_dir)?;
            let resolved = project_dir.to_string_lossy().to_string();
            gb.full_scan(&resolved, None)?;

            let body = format!(
                "Installed {} into '{}'{}.\nProject re-scanned.",
                packages.join(", "),
                project_name,
                if dev { " (dev dependency)" } else { "" },
            );
            log("info", format!("[ozymem-server] {body}"));
            Ok(ok_response(
                id,
                serde_json::to_value(ToolCallResult {
                    content: vec![ContentBlock {
                        kind: "text",
                        text: body,
                    }],
                    is_error: None,
                })?,
            ))
        }

        "remove_package" => {
            let project_name = match tool_call
                .arguments
                .get("project_name")
                .and_then(Value::as_str)
            {
                Some(n) => n,
                None => {
                    return Ok(error_response(
                        id,
                        -32602,
                        "Missing required parameter: project_name",
                    ));
                }
            };
            let packages = match tool_call
                .arguments
                .get("packages")
                .and_then(Value::as_array)
            {
                Some(arr) => arr
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect::<Vec<_>>(),
                None => {
                    return Ok(error_response(
                        id,
                        -32602,
                        "Missing required parameter: packages",
                    ));
                }
            };

            let reg = ProjectRegistry::open()?;
            let project = match reg.get_project_by_name(project_name)? {
                Some(p) => p,
                None => {
                    return Ok(error_response(
                        id,
                        -32602,
                        &format!("Project '{}' not found", project_name),
                    ));
                }
            };

            let project_dir = PathBuf::from(&project.path);
            let cmd = if cfg!(target_os = "windows") {
                "cmd"
            } else {
                "sh"
            };
            let mut shell_args = vec!["/C", "pnpm", "remove"];
            for p in &packages {
                shell_args.push(p.as_str());
            }
            let output = std::process::Command::new(cmd)
                .args(&shell_args)
                .current_dir(&project_dir)
                .output()?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Ok(error_response(
                    id,
                    -32603,
                    &format!("pnpm remove failed: {stderr}"),
                ));
            }

            // Re-scan
            let gb = GraphBackend::open_for_project(&project_dir)?;
            let resolved = project_dir.to_string_lossy().to_string();
            gb.full_scan(&resolved, None)?;

            let body = format!(
                "Removed {} from '{}'.\nProject re-scanned.",
                packages.join(", "),
                project_name,
            );
            log("info", format!("[ozymem-server] {body}"));
            Ok(ok_response(
                id,
                serde_json::to_value(ToolCallResult {
                    content: vec![ContentBlock {
                        kind: "text",
                        text: body,
                    }],
                    is_error: None,
                })?,
            ))
        }

        "get_dependencies" => {
            let project_name = match tool_call
                .arguments
                .get("project_name")
                .and_then(Value::as_str)
            {
                Some(n) => n,
                None => {
                    return Ok(error_response(
                        id,
                        -32602,
                        "Missing required parameter: project_name",
                    ));
                }
            };

            let reg = ProjectRegistry::open()?;
            let project = match reg.get_project_by_name(project_name)? {
                Some(p) => p,
                None => {
                    return Ok(error_response(
                        id,
                        -32602,
                        &format!("Project '{}' not found", project_name),
                    ));
                }
            };

            let pkg_path = Path::new(&project.path).join("package.json");
            if !pkg_path.exists() {
                return Ok(error_response(
                    id,
                    -32602,
                    "No package.json found in project",
                ));
            }

            let content = std::fs::read_to_string(&pkg_path)?;
            let pkg: serde_json::Value = serde_json::from_str(&content)?;

            let deps = pkg.get("dependencies").cloned().unwrap_or(json!({}));
            let dev_deps = pkg.get("devDependencies").cloned().unwrap_or(json!({}));
            let scripts = pkg.get("scripts").cloned().unwrap_or(json!({}));

            let body = format!(
                "Project: {}\n\nDependencies ({})",
                project_name,
                deps.as_object().map(|o| o.len()).unwrap_or(0),
            );
            let deps_text = if let Some(obj) = deps.as_object() {
                let mut lines: Vec<String> = obj
                    .iter()
                    .map(|(k, v)| format!("  {}: {}", k, v.as_str().unwrap_or("?")))
                    .collect();
                lines.sort();
                lines.join("\n")
            } else {
                String::new()
            };
            let dev_text = if let Some(obj) = dev_deps.as_object() {
                let mut lines: Vec<String> = obj
                    .iter()
                    .map(|(k, v)| format!("  {}: {}", k, v.as_str().unwrap_or("?")))
                    .collect();
                lines.sort();
                if lines.is_empty() {
                    String::new()
                } else {
                    format!(
                        "\n\nDev Dependencies ({}):\n{}",
                        obj.len(),
                        lines.join("\n")
                    )
                }
            } else {
                String::new()
            };
            let scripts_text = if let Some(obj) = scripts.as_object() {
                let mut lines: Vec<String> = obj
                    .iter()
                    .map(|(k, v)| format!("  {}: {}", k, v.as_str().unwrap_or("?")))
                    .collect();
                lines.sort();
                if lines.is_empty() {
                    String::new()
                } else {
                    format!("\n\nScripts:\n{}", lines.join("\n"))
                }
            } else {
                String::new()
            };

            Ok(ok_response(
                id,
                serde_json::to_value(ToolCallResult {
                    content: vec![ContentBlock {
                        kind: "text",
                        text: format!("{}\n{}\n{}{}", body, deps_text, dev_text, scripts_text),
                    }],
                    is_error: None,
                })?,
            ))
        }

        "run_script" => {
            let project_name = match tool_call
                .arguments
                .get("project_name")
                .and_then(Value::as_str)
            {
                Some(n) => n,
                None => {
                    return Ok(error_response(
                        id,
                        -32602,
                        "Missing required parameter: project_name",
                    ));
                }
            };
            let script = match tool_call.arguments.get("script").and_then(Value::as_str) {
                Some(s) => s,
                None => {
                    return Ok(error_response(
                        id,
                        -32602,
                        "Missing required parameter: script",
                    ));
                }
            };
            let extra_args: Vec<&str> = tool_call
                .arguments
                .get("args")
                .and_then(Value::as_array)
                .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
                .unwrap_or_default();

            let reg = ProjectRegistry::open()?;
            let project = match reg.get_project_by_name(project_name)? {
                Some(p) => p,
                None => {
                    return Ok(error_response(
                        id,
                        -32602,
                        &format!("Project '{}' not found", project_name),
                    ));
                }
            };

            let project_dir = PathBuf::from(&project.path);
            let cmd = if cfg!(target_os = "windows") {
                "cmd"
            } else {
                "sh"
            };
            let mut shell_args = vec!["/C", "pnpm", "run", script];
            shell_args.extend(extra_args);

            let output = match std::process::Command::new(cmd)
                .args(&shell_args)
                .current_dir(&project_dir)
                .output()
            {
                Ok(o) => o,
                Err(e) => {
                    return Ok(error_response(
                        id,
                        -32603,
                        &format!("Failed to run script: {e}"),
                    ));
                }
            };

            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let exit_code = output.status.code().unwrap_or(-1);

            let mut body = format!(
                "Script '{}' in '{}' (exit code: {})",
                script, project_name, exit_code
            );
            if !stdout.is_empty() {
                body.push_str("\n\n--- stdout ---\n");
                body.push_str(&stdout);
            }
            if !stderr.is_empty() {
                body.push_str("\n\n--- stderr ---\n");
                body.push_str(&stderr);
            }

            Ok(ok_response(
                id,
                serde_json::to_value(ToolCallResult {
                    content: vec![ContentBlock {
                        kind: "text",
                        text: body,
                    }],
                    is_error: if output.status.success() {
                        None
                    } else {
                        Some(true)
                    },
                })?,
            ))
        }

        "analyze_package" => {
            let project_name = match tool_call
                .arguments
                .get("project_name")
                .and_then(Value::as_str)
            {
                Some(n) => n,
                None => return Ok(error_response(id, -32602, "Missing required: project_name")),
            };
            let package_name = match tool_call
                .arguments
                .get("package_name")
                .and_then(Value::as_str)
            {
                Some(n) => n,
                None => return Ok(error_response(id, -32602, "Missing required: package_name")),
            };

            let reg = ProjectRegistry::open()?;
            let project = match reg.get_project_by_name(project_name)? {
                Some(p) => p,
                None => {
                    return Ok(error_response(
                        id,
                        -32602,
                        &format!("Project '{}' not found", project_name),
                    ));
                }
            };

            let pkg_path = Path::new(&project.path)
                .join("node_modules")
                .join(package_name)
                .join("package.json");
            if !pkg_path.exists() {
                return Ok(error_response(
                    id,
                    -32602,
                    &format!("Package '{}' not found in node_modules", package_name),
                ));
            }

            let content = std::fs::read_to_string(&pkg_path)?;
            let pkg: serde_json::Value = serde_json::from_str(&content)?;

            let name = pkg
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or(package_name);
            let version = pkg
                .get("version")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let description = pkg.get("description").and_then(Value::as_str).unwrap_or("");
            let license = pkg
                .get("license")
                .and_then(Value::as_str)
                .unwrap_or("unknown");

            let deps = pkg
                .get("dependencies")
                .and_then(|d| d.as_object())
                .map(|o| {
                    let mut v: Vec<String> = o
                        .iter()
                        .map(|(k, v)| format!("    {}: {}", k, v.as_str().unwrap_or("?")))
                        .collect();
                    v.sort();
                    v.join("\n")
                })
                .unwrap_or_default();
            let dev_deps = pkg
                .get("devDependencies")
                .and_then(|d| d.as_object())
                .map(|o| {
                    let mut v: Vec<String> = o
                        .iter()
                        .map(|(k, v)| format!("    {}: {}", k, v.as_str().unwrap_or("?")))
                        .collect();
                    v.sort();
                    v.join("\n")
                })
                .unwrap_or_default();

            let mut body = format!(
                "Package: {}\nVersion: {}\nLicense: {}\n",
                name, version, license,
            );
            if !description.is_empty() {
                body.push_str(&format!("Description: {}\n", description));
            }
            if !deps.is_empty() {
                let count = deps.lines().count();
                body.push_str(&format!("\nDependencies ({}):\n{}", count, deps));
            }
            if !dev_deps.is_empty() {
                let count = dev_deps.lines().count();
                body.push_str(&format!("\n\nDev Dependencies ({}):\n{}", count, dev_deps));
            }

            Ok(ok_response(
                id,
                serde_json::to_value(ToolCallResult {
                    content: vec![ContentBlock {
                        kind: "text",
                        text: body,
                    }],
                    is_error: None,
                })?,
            ))
        }

        "verify_dependencies" => {
            let project_name = match tool_call
                .arguments
                .get("project_name")
                .and_then(Value::as_str)
            {
                Some(n) => n,
                None => return Ok(error_response(id, -32602, "Missing required: project_name")),
            };

            let reg = ProjectRegistry::open()?;
            let project = match reg.get_project_by_name(project_name)? {
                Some(p) => p,
                None => {
                    return Ok(error_response(
                        id,
                        -32602,
                        &format!("Project '{}' not found", project_name),
                    ));
                }
            };

            let project_dir = Path::new(&project.path);

            // Read package.json
            let pkg_path = project_dir.join("package.json");
            if !pkg_path.exists() {
                return Ok(error_response(id, -32602, "No package.json found"));
            }
            let pkg_content = std::fs::read_to_string(&pkg_path)?;
            let pkg: serde_json::Value = serde_json::from_str(&pkg_content)?;

            let mut declared: std::collections::HashSet<String> = std::collections::HashSet::new();
            for key in &["dependencies", "devDependencies", "peerDependencies"] {
                if let Some(obj) = pkg.get(key).and_then(|v| v.as_object()) {
                    for name in obj.keys() {
                        declared.insert(name.clone());
                    }
                }
            }

            // Scan source files for imports
            let mut used: std::collections::HashSet<String> = std::collections::HashSet::new();
            // Regex to extract module names from import statements
            let import_re =
                regex::Regex::new(r#"(?:from\s+['"]|require\s*\(\s*['"]|import\s+['"])([^'"]+)"#)
                    .map_err(|e| anyhow::anyhow!("regex error: {e}"))?;

            // Walk source files, ignoring noise and .ozymignore/.gitignore patterns
            let ignore_patterns = ozymem_core::graph_backend::load_ignore_patterns(project_dir);
            let has_ignores = !ignore_patterns.is_empty();

            for entry in walkdir::WalkDir::new(project_dir)
                .into_iter()
                .filter_entry(|e: &walkdir::DirEntry| {
                    if ozymem_core::graph_backend::is_noise_dir(e.path()) {
                        return false;
                    }
                    if has_ignores
                        && ozymem_core::graph_backend::path_matches_ignore(
                            e.path(),
                            &ignore_patterns,
                            project_dir,
                        )
                    {
                        return false;
                    }
                    true
                })
                .filter_map(|e: Result<walkdir::DirEntry, walkdir::Error>| e.ok())
            {
                let path = entry.path();
                if !path.is_file() {
                    continue;
                }
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                if !matches!(ext, "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs") {
                    continue;
                }

                let source = match std::fs::read_to_string(path) {
                    Ok(s) => s,
                    Err(_) => continue,
                };

                for cap in import_re.captures_iter(&source) {
                    let module = cap.get(1).map(|m| m.as_str()).unwrap_or("");
                    // Skip relative and bare-specifier imports
                    if module.starts_with('.') || module.starts_with('/') {
                        continue;
                    }
                    // Extract package name (handle @scoped/packages)
                    let pkg_name = if module.starts_with('@') {
                        let parts: Vec<&str> = module.splitn(3, '/').collect();
                        if parts.len() >= 2 {
                            format!("{}/{}", parts[0], parts[1])
                        } else {
                            module.to_string()
                        }
                    } else {
                        module.split('/').next().unwrap_or(module).to_string()
                    };
                    used.insert(pkg_name);
                }
            }

            // Compare
            let mut missing = Vec::new();
            let mut unused = Vec::new();
            for p in &used {
                if !declared.contains(p) {
                    missing.push(p.clone());
                }
            }
            for p in &declared {
                if !used.contains(p) {
                    unused.push(p.clone());
                }
            }
            missing.sort();
            unused.sort();

            let mut body = format!("Dependency verification for '{}':\n", project_name);
            body.push_str(&format!("  Declared: {}\n", declared.len()));
            body.push_str(&format!("  Used in imports: {}\n\n", used.len()));

            if missing.is_empty() {
                body.push_str("[OK] All used dependencies are declared in package.json\n");
            } else {
                body.push_str(&format!(
                    "[Missing] Missing from package.json (used but not declared, {})",
                    missing.len()
                ));
                for p in &missing {
                    body.push_str(&format!("\n    - {}", p));
                }
                body.push('\n');
            }
            if unused.is_empty() {
                body.push_str("[OK] All declared dependencies are used\n");
            } else {
                body.push_str(&format!(
                    "[Unused] Unused in source (declared but not imported, {})",
                    unused.len()
                ));
                for p in &unused {
                    body.push_str(&format!("\n    - {}", p));
                }
                body.push('\n');
            }

            Ok(ok_response(
                id,
                serde_json::to_value(ToolCallResult {
                    content: vec![ContentBlock {
                        kind: "text",
                        text: body,
                    }],
                    is_error: None,
                })?,
            ))
        }

        _ => Ok(error_response(
            id,
            -32601,
            "Unknown package management tool",
        )),
    }
}
