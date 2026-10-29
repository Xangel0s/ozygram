use anyhow::{anyhow, Context, Result};
use clap::Subcommand;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use ozymem_core::graph_backend::GraphBackend;
use ozymem_core::McpBackend;

#[derive(Subcommand, Debug, Clone)]
pub enum HookSubcommand {
    /// Install git post-commit hook for automated knowledge capture
    Install {
        /// Optional path to the repository (defaults to current directory)
        path: Option<PathBuf>,
    },
    /// Uninstall git post-commit hook
    Uninstall {
        /// Optional path to the repository (defaults to current directory)
        path: Option<PathBuf>,
    },
    /// Check whether the git hook is installed and active
    Status {
        /// Optional path to the repository (defaults to current directory)
        path: Option<PathBuf>,
    },
    /// Execute the hook handler (called automatically by git)
    Run {
        /// Name of the hook, e.g. post-commit
        #[arg(default_value = "post-commit")]
        hook_type: String,
        /// Optional path to the repository (defaults to current directory)
        #[arg(long)]
        project: Option<PathBuf>,
    },
}

const POST_COMMIT_SNIPPET: &str = r#"
# --- Ozymem automated knowledge capture ---
if command -v ozymem >/dev/null 2>&1; then
    ozymem hook run post-commit 2>/dev/null || true
fi
# --- End Ozymem ---
"#;

const PRE_COMMIT_SNIPPET: &str = r#"
# --- Ozymem Centinela contract drift gatekeeper ---
if command -v ozymem >/dev/null 2>&1; then
    ozymem hook run pre-commit || exit 1
fi
# --- End Ozymem Centinela ---
"#;

pub fn find_git_dir(start: &Path) -> Option<PathBuf> {
    for ancestor in start.ancestors() {
        let git = ancestor.join(".git");
        if git.is_dir() {
            return Some(git);
        }
        if git.is_file() {
            // Worktree: .git is a text file containing "gitdir: <path>"
            if let Ok(content) = fs::read_to_string(&git) {
                if let Some(target) = content.strip_prefix("gitdir:") {
                    let trimmed = target.trim();
                    let path = PathBuf::from(trimmed);
                    if path.is_absolute() {
                        return Some(path);
                    } else {
                        return Some(ancestor.join(path));
                    }
                }
            }
        }
    }
    None
}

pub fn install_hook(target_dir: Option<&Path>) -> Result<String> {
    let cwd = std::env::current_dir()?;
    let base = target_dir.unwrap_or(&cwd);
    let git_dir = find_git_dir(base)
        .ok_or_else(|| anyhow!("Not a git repository: {}", base.display()))?;

    let hooks_dir = git_dir.join("hooks");
    fs::create_dir_all(&hooks_dir)
        .with_context(|| format!("Failed to create hooks directory at {}", hooks_dir.display()))?;

    // 1. Install pre-commit hook (Centinela gatekeeper)
    let pre_hook_file = hooks_dir.join("pre-commit");
    if pre_hook_file.exists() {
        let content = fs::read_to_string(&pre_hook_file)?;
        if !content.contains("ozymem hook run pre-commit") {
            let updated = format!("{}\n{}", content.trim_end(), PRE_COMMIT_SNIPPET);
            fs::write(&pre_hook_file, updated)?;
        }
    } else {
        let new_content = format!("#!/bin/sh\n{}", PRE_COMMIT_SNIPPET);
        fs::write(&pre_hook_file, new_content)?;
    }

    // 2. Install post-commit hook (Knowledge capture)
    let post_hook_file = hooks_dir.join("post-commit");
    if post_hook_file.exists() {
        let content = fs::read_to_string(&post_hook_file)?;
        if !content.contains("ozymem hook run post-commit") {
            let updated = format!("{}\n{}", content.trim_end(), POST_COMMIT_SNIPPET);
            fs::write(&post_hook_file, updated)?;
        }
    } else {
        let new_content = format!("#!/bin/sh\n{}", POST_COMMIT_SNIPPET);
        fs::write(&post_hook_file, new_content)?;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for file in [&pre_hook_file, &post_hook_file] {
            if let Ok(meta) = fs::metadata(file) {
                let mut perms = meta.permissions();
                perms.set_mode(0o755);
                let _ = fs::set_permissions(file, perms);
            }
        }
    }

    Ok(format!(
        "Installed Centinela pre-commit & post-commit hooks at {}",
        hooks_dir.display()
    ))
}

pub fn uninstall_hook(target_dir: Option<&Path>) -> Result<String> {
    let cwd = std::env::current_dir()?;
    let base = target_dir.unwrap_or(&cwd);
    let git_dir = find_git_dir(base)
        .ok_or_else(|| anyhow!("Not a git repository: {}", base.display()))?;

    let hooks_dir = git_dir.join("hooks");
    let mut removed_count = 0;

    for (name, snippet_marker, clean_target) in [
        ("pre-commit", "# --- Ozymem Centinela", "ozymem hook run pre-commit"),
        ("post-commit", "# --- Ozymem automated knowledge capture", "ozymem hook run post-commit"),
    ] {
        let file = hooks_dir.join(name);
        if !file.exists() {
            continue;
        }
        let content = fs::read_to_string(&file)?;
        if !content.contains(clean_target) {
            continue;
        }

        let cleaned = if let (Some(start), Some(end)) = (content.find(snippet_marker), content.find("# --- End Ozymem")) {
            let after_end = if let Some(end_offset) = content[end..].find('\n') {
                end + end_offset + 1
            } else {
                content.len()
            };
            format!("{}{}", &content[..start], &content[after_end..])
        } else {
            content.lines().filter(|l| !l.contains(clean_target)).collect::<Vec<_>>().join("\n")
        };

        let new_content = cleaned.trim().to_string();
        if new_content.is_empty() || new_content == "#!/bin/sh" {
            let _ = fs::remove_file(&file);
        } else {
            let _ = fs::write(&file, format!("{new_content}\n"));
        }
        removed_count += 1;
    }

    if removed_count == 0 {
        Ok("No ozymem hooks were installed".to_string())
    } else {
        Ok(format!("Removed ozymem hooks from {}", hooks_dir.display()))
    }
}

pub fn hook_status(target_dir: Option<&Path>) -> Result<String> {
    let cwd = std::env::current_dir()?;
    let base = target_dir.unwrap_or(&cwd);
    let git_dir = match find_git_dir(base) {
        Some(d) => d,
        None => return Ok("Not a git repository".to_string()),
    };

    let hooks_dir = git_dir.join("hooks");
    let pre_active = hooks_dir.join("pre-commit").exists()
        && fs::read_to_string(hooks_dir.join("pre-commit")).map(|c| c.contains("ozymem hook run pre-commit")).unwrap_or(false);
    let post_active = hooks_dir.join("post-commit").exists()
        && fs::read_to_string(hooks_dir.join("post-commit")).map(|c| c.contains("ozymem hook run post-commit")).unwrap_or(false);

    match (pre_active, post_active) {
        (true, true) => Ok(format!("Centinela pre-commit and post-commit hooks are ACTIVE at {}", hooks_dir.display())),
        (true, false) => Ok(format!("Centinela pre-commit is ACTIVE, post-commit is NOT installed at {}", hooks_dir.display())),
        (false, true) => Ok(format!("Post-commit is ACTIVE, Centinela pre-commit is NOT installed at {}", hooks_dir.display())),
        (false, false) => Ok("Git hooks are NOT installed".to_string()),
    }
}

pub async fn run_hook(hook_type: &str, target_dir: Option<&Path>) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let base = target_dir.unwrap_or(&cwd);
    let git_dir = match find_git_dir(base) {
        Some(d) => d,
        None => return Ok(()),
    };
    let repo_root = git_dir.parent().unwrap_or(base);

    if hook_type == "pre-commit" {
        return run_pre_commit_centinela(repo_root);
    }
    if hook_type != "post-commit" {
        return Ok(());
    }

    // 1. Get latest commit hash and message
    let rev_out = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .current_dir(repo_root)
        .output();
    let commit_hash = match rev_out {
        Ok(ref o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => return Ok(()),
    };

    let title_out = Command::new("git")
        .args(["log", "-1", "--pretty=%s"])
        .current_dir(repo_root)
        .output();
    let commit_title = match title_out {
        Ok(ref o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => String::new(),
    };

    let body_out = Command::new("git")
        .args(["log", "-1", "--pretty=%b"])
        .current_dir(repo_root)
        .output();
    let commit_body = match body_out {
        Ok(ref o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => String::new(),
    };

    // 2. Get changed files from commit using git diff-tree (works for root commits too)
    let diff_out = Command::new("git")
        .args(["diff-tree", "--no-commit-id", "--name-status", "-r", "HEAD"])
        .current_dir(repo_root)
        .output();

    let changed_lines = match diff_out {
        Ok(ref o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
        _ => String::new(),
    };

    let mut indexed_count = 0;
    let mut primary_file: Option<String> = None;

    if let Ok(backend) = GraphBackend::open_for_project(repo_root) {
        for line in changed_lines.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 2 {
                continue;
            }
            let status = parts[0];
            let rel_path = parts[1];
            let abs_path = repo_root.join(rel_path);

            if status.starts_with('D') {
                let _ = backend.remove_file_delta(&abs_path, repo_root);
            } else {
                if primary_file.is_none() {
                    primary_file = Some(rel_path.to_string());
                }
                if let Ok(ozymem_core::sync::DeltaIndexResult::Indexed { .. }) =
                    backend.index_file_delta(&abs_path, repo_root)
                {
                    indexed_count += 1;
                }
            }
        }

        // 3. Save commit knowledge observation
        let proj_name = repo_root
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("project");

        let content = if commit_body.is_empty() {
            format!("Commit: {}\nFiles:\n{}", commit_title, changed_lines)
        } else {
            format!("Commit: {}\n{}\nFiles:\n{}", commit_title, commit_body, changed_lines)
        };

        let _ = backend.save_observation(
            &format!("git_commit_{commit_hash}"),
            "commit_knowledge",
            &commit_title,
            &content,
            Some(proj_name),
            Some("git_commit"),
            Some(&format!("commit_{commit_hash}")),
            Some("git_hook"),
        );

        // 4. If commit is a fix/bugfix, also record as lesson
        let lower_title = commit_title.to_lowercase();
        if lower_title.starts_with("fix") || lower_title.contains("bug") || lower_title.contains("solve") {
            if let Some(target_file) = primary_file {
                let _ = backend
                    .record_lesson(
                        &target_file,
                        None,
                        &commit_title,
                        &format!("Commit {commit_hash}: {commit_body}"),
                    )
                    .await;
            }
        }

        eprintln!("[ozymem] Auto-captured commit {commit_hash}: {indexed_count} file(s) indexed.");
    }

    Ok(())
}

pub fn run_pre_commit_centinela(repo_root: &Path) -> Result<()> {
    let t0 = std::time::Instant::now();
    // 1. Get staged files: git diff --cached --name-status
    let diff_out = Command::new("git")
        .args(["diff", "--cached", "--name-status"])
        .current_dir(repo_root)
        .output();

    let staged_lines = match diff_out {
        Ok(ref o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
        _ => return Ok(()),
    };

    if staged_lines.trim().is_empty() {
        return Ok(());
    }

    let mut staged_api_files = Vec::new();
    let mut staged_frontend_files = Vec::new();

    for line in staged_lines.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 2 {
            continue;
        }
        let rel_path = parts[1];
        if rel_path.ends_with(".py") || rel_path.ends_with(".rs") {
            staged_api_files.push(rel_path.to_string());
        } else if rel_path.ends_with(".ts") || rel_path.ends_with(".tsx") || rel_path.ends_with(".js") {
            staged_frontend_files.push(rel_path.to_string());
        }
    }

    if staged_api_files.is_empty() {
        println!(" [STATUS: CENTINELA_VERIFIED] Staged contracts clean (no API endpoints staged, {} ms).", t0.elapsed().as_millis());
        return Ok(());
    }

    // 2. Check staged API files for endpoint declarations using ozymem_parser
    let mut modified_endpoints = Vec::new();

    for api_file in &staged_api_files {
        let abs = repo_root.join(api_file);
        if let Ok(content) = fs::read_to_string(&abs) {
            let lang = if api_file.ends_with(".py") {
                ozymem_parser::SupportedLanguage::Python
            } else if api_file.ends_with(".rs") {
                ozymem_parser::SupportedLanguage::Rust
            } else {
                ozymem_parser::SupportedLanguage::Unknown
            };
            let routes = ozymem_parser::api_routes::parse_api_routes(&content, api_file, lang);
            for r in routes {
                if r.path.starts_with("/api/") {
                    modified_endpoints.push((api_file.clone(), r.path));
                }
            }
        }
    }

    if modified_endpoints.is_empty() {
        println!(" [STATUS: CENTINELA_VERIFIED] Staged files clean ({} ms).", t0.elapsed().as_millis());
        return Ok(());
    }

    // 3. Search for potential frontend consumers across repo
    let mut drift_alerts = Vec::new();
    for (api_file, ep) in &modified_endpoints {
        let grep_out = Command::new("git")
            .args(["grep", "-l", ep, "--", "*.ts", "*.tsx", "*.js"])
            .current_dir(repo_root)
            .output();

        if let Ok(ref g) = grep_out {
            if g.status.success() {
                let callers = String::from_utf8_lossy(&g.stdout);
                let broken_callers: Vec<&str> = callers
                    .lines()
                    .filter(|caller| !staged_frontend_files.iter().any(|staged| staged == caller))
                    .collect();

                if !broken_callers.is_empty() {
                    drift_alerts.push((api_file.clone(), ep.clone(), broken_callers.iter().map(|s| s.to_string()).collect::<Vec<_>>()));
                }
            }
        }
    }

    if !drift_alerts.is_empty() {
        eprintln!();
        eprintln!("========================================================================");
        eprintln!("   [CRITICAL_VETO: CROSS_REPO_CONTRACT_DRIFT]                          ");
        eprintln!("   Centinela detected API endpoint changes without frontend sync:      ");
        eprintln!("========================================================================");
        for (api_file, ep, callers) in &drift_alerts {
            eprintln!(" -> API file: {} (Endpoint: '{}')", api_file, ep);
            eprintln!("    Unsynced frontend callers: [{}]", callers.join(", "));
        }
        eprintln!("------------------------------------------------------------------------");
        eprintln!(" Action required: Update the frontend callers or bypass via:");
        eprintln!("   git commit --no-verify");
        eprintln!("========================================================================");
        eprintln!();
        return Err(anyhow!("[CRITICAL_VETO: CROSS_REPO_CONTRACT_DRIFT] Staged endpoint drift detected"));
    }

    println!(" [STATUS: CENTINELA_VERIFIED] Staged contracts verified clean ({} ms).", t0.elapsed().as_millis());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_install_status_and_uninstall_hook() {
        let root = std::env::temp_dir().join(format!("ozymem-hook-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".git")).unwrap();

        // 1. Initial status: not installed
        let s0 = hook_status(Some(&root)).unwrap();
        assert!(s0.contains("NOT installed"), "expected not installed, got: {s0}");

        // 2. Install hook (Centinela pre-commit & post-commit)
        let res = install_hook(Some(&root)).unwrap();
        assert!(res.contains("Installed Centinela"), "got: {res}");

        let pre_hook_file = root.join(".git").join("hooks").join("pre-commit");
        let post_hook_file = root.join(".git").join("hooks").join("post-commit");
        assert!(pre_hook_file.exists());
        assert!(post_hook_file.exists());

        let pre_content = fs::read_to_string(&pre_hook_file).unwrap();
        assert!(pre_content.contains("ozymem hook run pre-commit"));

        let post_content = fs::read_to_string(&post_hook_file).unwrap();
        assert!(post_content.contains("ozymem hook run post-commit"));

        // 3. Status: active
        let s1 = hook_status(Some(&root)).unwrap();
        assert!(s1.contains("ACTIVE"), "expected active, got: {s1}");

        // 4. Idempotent install
        let res2 = install_hook(Some(&root)).unwrap();
        assert!(res2.contains("Installed Centinela"), "got: {res2}");

        // 5. Uninstall hook
        let un_res = uninstall_hook(Some(&root)).unwrap();
        assert!(un_res.contains("Removed"), "got: {un_res}");
        assert!(!pre_hook_file.exists());
        assert!(!post_hook_file.exists());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn test_pre_commit_centinela_empty_staging_succeeds() {
        let root = std::env::temp_dir().join(format!("ozymem-hook-centinela-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".git")).unwrap();

        // Should return Ok(()) on empty or non-git staging
        let res = run_pre_commit_centinela(&root);
        assert!(res.is_ok());

        let _ = fs::remove_dir_all(&root);
    }
}

