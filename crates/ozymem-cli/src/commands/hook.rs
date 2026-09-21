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

    let hook_file = hooks_dir.join("post-commit");
    if hook_file.exists() {
        let content = fs::read_to_string(&hook_file)?;
        if content.contains("ozymem hook run post-commit") {
            return Ok(format!("Hook already installed at {}", hook_file.display()));
        }
        let updated = format!("{}\n{}", content.trim_end(), POST_COMMIT_SNIPPET);
        fs::write(&hook_file, updated)?;
    } else {
        let new_content = format!("#!/bin/sh\n{}", POST_COMMIT_SNIPPET);
        fs::write(&hook_file, new_content)?;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = fs::metadata(&hook_file) {
            let mut perms = meta.permissions();
            perms.set_mode(0o755);
            let _ = fs::set_permissions(&hook_file, perms);
        }
    }

    Ok(format!("Installed post-commit hook at {}", hook_file.display()))
}

pub fn uninstall_hook(target_dir: Option<&Path>) -> Result<String> {
    let cwd = std::env::current_dir()?;
    let base = target_dir.unwrap_or(&cwd);
    let git_dir = find_git_dir(base)
        .ok_or_else(|| anyhow!("Not a git repository: {}", base.display()))?;

    let hook_file = git_dir.join("hooks").join("post-commit");
    if !hook_file.exists() {
        return Ok("Post-commit hook was not installed".to_string());
    }

    let content = fs::read_to_string(&hook_file)?;
    if !content.contains("ozymem hook run post-commit") {
        return Ok("No ozymem hook found in post-commit".to_string());
    }

    let cleaned = if let (Some(start), Some(end)) = (
        content.find("# --- Ozymem automated knowledge capture ---"),
        content.find("# --- End Ozymem ---"),
    ) {
        let after_end = end + "# --- End Ozymem ---".len();
        format!("{}{}", &content[..start], &content[after_end..])
    } else {
        content
            .lines()
            .filter(|l| !l.contains("ozymem hook run post-commit"))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let new_content = cleaned.trim().to_string();
    if new_content.is_empty() || new_content == "#!/bin/sh" {
        fs::remove_file(&hook_file)?;
        Ok(format!("Removed empty post-commit hook at {}", hook_file.display()))
    } else {
        fs::write(&hook_file, format!("{new_content}\n"))?;
        Ok(format!("Removed ozymem entry from {}", hook_file.display()))
    }
}

pub fn hook_status(target_dir: Option<&Path>) -> Result<String> {
    let cwd = std::env::current_dir()?;
    let base = target_dir.unwrap_or(&cwd);
    let git_dir = match find_git_dir(base) {
        Some(d) => d,
        None => return Ok("Not a git repository".to_string()),
    };

    let hook_file = git_dir.join("hooks").join("post-commit");
    if !hook_file.exists() {
        return Ok("Post-commit hook is NOT installed".to_string());
    }

    let content = fs::read_to_string(&hook_file)?;
    if content.contains("ozymem hook run post-commit") {
        Ok(format!("Post-commit hook is ACTIVE at {}", hook_file.display()))
    } else {
        Ok(format!("Post-commit hook exists at {}, but does not contain ozymem", hook_file.display()))
    }
}

pub async fn run_hook(hook_type: &str, target_dir: Option<&Path>) -> Result<()> {
    if hook_type != "post-commit" {
        return Ok(());
    }

    let cwd = std::env::current_dir()?;
    let base = target_dir.unwrap_or(&cwd);
    let git_dir = match find_git_dir(base) {
        Some(d) => d,
        None => return Ok(()),
    };

    let repo_root = git_dir.parent().unwrap_or(base);

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

        // 2. Install hook
        let res = install_hook(Some(&root)).unwrap();
        assert!(res.contains("Installed post-commit hook"), "got: {res}");

        let hook_file = root.join(".git").join("hooks").join("post-commit");
        assert!(hook_file.exists());
        let content = fs::read_to_string(&hook_file).unwrap();
        assert!(content.contains("ozymem hook run post-commit"));

        // 3. Status: active
        let s1 = hook_status(Some(&root)).unwrap();
        assert!(s1.contains("ACTIVE"), "expected active, got: {s1}");

        // 4. Idempotent install
        let res2 = install_hook(Some(&root)).unwrap();
        assert!(res2.contains("already installed"), "got: {res2}");

        // 5. Uninstall hook
        let un_res = uninstall_hook(Some(&root)).unwrap();
        assert!(un_res.contains("Removed"), "got: {un_res}");
        assert!(!hook_file.exists());

        let _ = fs::remove_dir_all(&root);
    }
}

