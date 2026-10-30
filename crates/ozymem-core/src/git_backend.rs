use git2::{Repository, Sort};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RecentChange {
    pub hash: String,
    pub author: String,
    pub message: String,
    pub timestamp: String,
    pub files: Vec<String>,
    pub is_merge: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DiffSummary {
    pub files_changed: usize,
    pub insertions: usize,
    pub deletions: usize,
    pub files: Vec<DiffFileEntry>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DiffFileEntry {
    pub path: String,
    pub status: String,
    pub insertions: usize,
    pub deletions: usize,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BlameEntry {
    pub hash: String,
    pub author: String,
    pub timestamp: String,
    pub start_line: usize,
    pub end_line: usize,
}

pub struct GitBackend {
    repo: Repository,
    repo_root: PathBuf,
}

impl std::fmt::Debug for GitBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GitBackend")
            .field("repo_root", &self.repo_root)
            .finish()
    }
}

impl GitBackend {
    /// Open a git repository, discovering from `project_path` upward.
    pub fn open(project_path: &Path) -> Result<Self, String> {
        let repo = Repository::discover(project_path)
            .map_err(|e| format!("No git repository found at or above `{}`: {}", project_path.display(), e))?;
        let repo_root = repo.workdir()
            .map(|p| p.to_path_buf())
            .ok_or_else(|| format!("Repository at {} has no working directory", project_path.display()))?;
        // Canonicalize so paths match the format used by GraphBackend's file_index
        let repo_root = repo_root.canonicalize().unwrap_or(repo_root);
        Ok(GitBackend { repo, repo_root })
    }

    /// Return the absolute root path of the repository.
    pub fn repo_path(&self) -> &Path {
        &self.repo_root
    }

    /// Return the most recent commits (up to `limit`) with their changed files.
    ///
    /// Root commits (no parents) diff against an empty tree.
    /// Merge commits diff against the first parent only.
    pub fn recent_changes(&self, limit: usize) -> Result<Vec<RecentChange>, String> {
        let mut revwalk = self.repo.revwalk().map_err(|e| format!("revwalk: {e}"))?;
        revwalk.set_sorting(Sort::TIME).map_err(|e| format!("sort: {e}"))?;
        revwalk.push_head().map_err(|e| format!("push_head: {e}"))?;

        let mut results = Vec::new();
        for oid_result in revwalk {
            if results.len() >= limit {
                break;
            }
            let oid = oid_result.map_err(|e| format!("revwalk iteration: {e}"))?;
            let commit = self.repo.find_commit(oid)
                .map_err(|e| format!("find_commit {oid}: {e}"))?;

            let tree = commit.tree().map_err(|e| format!("tree: {e}"))?;
            let parent_count = commit.parent_count();
            let is_merge = parent_count > 1;

            let files = if parent_count > 0 {
                let parent_tree = commit.parent(0).and_then(|p| p.tree())
                    .map_err(|e| format!("parent tree: {e}"))?;
                let diff = self.repo.diff_tree_to_tree(Some(&parent_tree), Some(&tree), None)
                    .map_err(|e| format!("diff: {e}"))?;
                collect_diff_paths(&diff)
            } else {
                let diff = self.repo.diff_tree_to_tree(None, Some(&tree), None)
                    .map_err(|e| format!("diff root: {e}"))?;
                collect_diff_paths(&diff)
            };

            let author = commit.author();
            let secs = commit.time().seconds();

            results.push(RecentChange {
                hash: oid.to_string(),
                author: author.name().unwrap_or("unknown").to_string(),
                message: commit.message().unwrap_or("").trim().to_string(),
                timestamp: format_timestamp(secs),
                files,
                is_merge,
            });
        }

        Ok(results)
    }

    /// Diff statistics between two refs (default `to` is HEAD).
    ///
    /// `from` and `to` can be any refspec: branch name, tag, `HEAD~3`, commit hash, etc.
    pub fn diff_summary(&self, from: &str, to: Option<&str>) -> Result<DiffSummary, String> {
        let from_commit = resolve_commit(&self.repo, from)?;
        let to_commit = resolve_commit(&self.repo, to.unwrap_or("HEAD"))?;

        let from_tree = from_commit.tree().map_err(|e| format!("from tree: {e}"))?;
        let to_tree = to_commit.tree().map_err(|e| format!("to tree: {e}"))?;

        let diff = self.repo.diff_tree_to_tree(Some(&from_tree), Some(&to_tree), None)
            .map_err(|e| format!("diff: {e}"))?;

        let stats = diff.stats().map_err(|e| format!("stats: {e}"))?;
        let files = collect_diff_files(&diff);

        Ok(DiffSummary {
            files_changed: stats.files_changed() as usize,
            insertions: stats.insertions() as usize,
            deletions: stats.deletions() as usize,
            files,
        })
    }

    /// Get the unified diff of a single file between two refs.
    pub fn diff_file(&self, from: &str, to: Option<&str>, file_path: &str) -> Result<String, String> {
        let from_commit = resolve_commit(&self.repo, from)?;
        let to_commit = resolve_commit(&self.repo, to.unwrap_or("HEAD"))?;

        let from_tree = from_commit.tree().map_err(|e| format!("from tree: {e}"))?;
        let to_tree = to_commit.tree().map_err(|e| format!("to tree: {e}"))?;

        let mut opts = git2::DiffOptions::new();
        opts.pathspec(file_path);

        let diff = self.repo.diff_tree_to_tree(Some(&from_tree), Some(&to_tree), Some(&mut opts))
            .map_err(|e| format!("diff: {e}"))?;

        if diff.deltas().len() == 0 {
            return Ok(format!("No changes for {file_path} between {from} and {}", to.unwrap_or("HEAD")));
        }

        let mut buf = String::new();
        diff.print(git2::DiffFormat::Patch, |_delta, _hunk, line| {
            let origin = line.origin();
            if let Ok(content) = std::str::from_utf8(line.content()) {
                if origin == 'H' || origin == 'F' || origin == 'B' || origin == '\n' || origin == '\0' {
                    buf.push_str(content);
                } else {
                    buf.push(origin);
                    buf.push_str(content);
                }
            }
            true
        }).map_err(|e| format!("print diff: {e}"))?;

        Ok(buf)
    }

    /// Get blame annotations for a range of lines in a file.
    pub fn blame_file(&self, file_path: &str, start_line: Option<usize>, end_line: Option<usize>) -> Result<Vec<BlameEntry>, String> {
        let mut opts = git2::BlameOptions::new();

        let blame = self.repo.blame_file(Path::new(file_path), Some(&mut opts))
            .map_err(|e| format!("blame error for {file_path}: {e}"))?;

        let start = start_line.unwrap_or(1);
        let end = end_line.unwrap_or(usize::MAX);

        let mut entries = Vec::new();
        for hunk in blame.iter() {
            let hunk_start = hunk.final_start_line() as usize;
            let hunk_end = hunk_start + hunk.lines_in_hunk() as usize - 1;

            if hunk_end < start || hunk_start > end {
                continue;
            }

            let commit = match self.repo.find_commit(hunk.orig_commit_id()) {
                Ok(c) => c,
                Err(_) => continue,
            };

            let line_start = std::cmp::max(hunk_start, start);
            let line_end = std::cmp::min(hunk_end, end);

            entries.push(BlameEntry {
                hash: hunk.orig_commit_id().to_string(),
                author: commit.author().name().unwrap_or("unknown").to_string(),
                timestamp: format_timestamp(commit.time().seconds()),
                start_line: line_start,
                end_line: line_end,
            });
        }

        Ok(entries)
    }

    /// Retorna la rama actual si estÃ¡ disponible.
    pub fn current_branch(&self) -> Option<String> {
        self.repo.head().ok().and_then(|h| h.shorthand().map(str::to_string))
    }

    /// Retorna el commit hash actual del HEAD.
    pub fn head_commit_hash(&self) -> Option<String> {
        self.repo.head().ok().and_then(|h| h.peel_to_commit().ok()).map(|c| c.id().to_string())
    }

    /// Escribe una nota en `refs/notes/ozymem` vinculada a un commit.
    pub fn write_note(
        &self,
        commit_ref: Option<&str>,
        note_ref: Option<&str>,
        message: &str,
        force: bool,
    ) -> Result<String, String> {
        let note_ref_name = note_ref.unwrap_or("refs/notes/ozymem");
        let target_oid = if let Some(cr) = commit_ref {
            self.repo.revparse_single(cr)
                .map_err(|e| format!("Invalid commit ref `{}`: {}", cr, e))?
                .id()
        } else {
            self.repo.head()
                .and_then(|h| h.peel_to_commit())
                .map(|c| c.id())
                .map_err(|e| format!("Cannot resolve HEAD commit: {}", e))?
        };

        let sig = self.repo.signature().unwrap_or_else(|_| {
            git2::Signature::now("Ozymem Agent", "agent@ozymem.local").unwrap()
        });

        let note_oid = self.repo.note(&sig, &sig, Some(note_ref_name), target_oid, message, force)
            .map_err(|e| format!("Failed to write git note on `{}` (ref `{}`): {}", target_oid, note_ref_name, e))?;

        Ok(note_oid.to_string())
    }

    /// Lee la nota asociada a un commit desde `refs/notes/ozymem`.
    pub fn read_note(
        &self,
        commit_ref: Option<&str>,
        note_ref: Option<&str>,
    ) -> Result<Option<String>, String> {
        let note_ref_name = note_ref.unwrap_or("refs/notes/ozymem");
        let target_oid = if let Some(cr) = commit_ref {
            self.repo.revparse_single(cr)
                .map_err(|e| format!("Invalid commit ref `{}`: {}", cr, e))?
                .id()
        } else {
            match self.repo.head().and_then(|h| h.peel_to_commit()) {
                Ok(c) => c.id(),
                Err(_) => return Ok(None),
            }
        };

        match self.repo.find_note(Some(note_ref_name), target_oid) {
            Ok(note) => Ok(note.message().map(|s| s.to_string())),
            Err(e) if e.code() == git2::ErrorCode::NotFound => Ok(None),
            Err(e) => Err(format!("Failed to find note on `{}`: {}", target_oid, e)),
        }
    }

    /// Lista todas las notas registradas en `refs/notes/ozymem`.
    pub fn list_notes(
        &self,
        note_ref: Option<&str>,
    ) -> Result<Vec<(String, String)>, String> {
        let note_ref_name = note_ref.unwrap_or("refs/notes/ozymem");
        let mut notes = Vec::new();

        let iter = match self.repo.notes(Some(note_ref_name)) {
            Ok(it) => it,
            Err(e) if e.code() == git2::ErrorCode::NotFound => return Ok(vec![]),
            Err(e) => return Err(format!("Failed to list notes for `{}`: {}", note_ref_name, e)),
        };

        for item in iter {
            if let Ok((_note_oid, commit_oid)) = item {
                if let Ok(note) = self.repo.find_note(Some(note_ref_name), commit_oid) {
                    if let Some(msg) = note.message() {
                        notes.push((commit_oid.to_string(), msg.to_string()));
                    }
                }
            }
        }

        Ok(notes)
    }

    /// Return in-memory patch and changed files for uncommitted working directory changes.
    pub fn live_uncommitted_diff(&self) -> Result<(String, Vec<String>), String> {
        let head_tree = self.repo.head().ok().and_then(|h| h.peel_to_tree().ok());
        let diff = self.repo.diff_tree_to_workdir_with_index(head_tree.as_ref(), None)
            .map_err(|e| format!("diff_tree_to_workdir_with_index: {e}"))?;

        let files = collect_diff_paths(&diff);
        let mut patch = String::new();
        diff.print(git2::DiffFormat::Patch, |_delta, _hunk, line| {
            let origin = line.origin();
            if let Ok(content) = std::str::from_utf8(line.content()) {
                if origin == '+' || origin == '-' || origin == ' ' {
                    patch.push(origin);
                }
                patch.push_str(content);
            }
            true
        }).map_err(|e| format!("print diff: {e}"))?;

        Ok((patch, files))
    }

    /// Return in-memory patch and changed files for commits made in the last `hours` hours.
    pub fn recent_commits_diff(&self, hours: u32) -> Result<(String, Vec<String>), String> {
        let now = chrono::Utc::now().timestamp();
        let threshold = now - (hours as i64 * 3600);

        let mut revwalk = self.repo.revwalk().map_err(|e| format!("revwalk: {e}"))?;
        revwalk.set_sorting(Sort::TIME).map_err(|e| format!("sort: {e}"))?;
        if revwalk.push_head().is_err() {
            return Ok((String::new(), Vec::new()));
        }

        let mut matching_commits = Vec::new();
        for oid_res in revwalk {
            if let Ok(oid) = oid_res {
                if let Ok(commit) = self.repo.find_commit(oid) {
                    if commit.time().seconds() >= threshold {
                        matching_commits.push(commit);
                    } else {
                        break;
                    }
                }
            }
        }

        if matching_commits.is_empty() {
            return Ok((String::new(), Vec::new()));
        }

        let newest_tree = matching_commits.first().unwrap().tree().map_err(|e| format!("tree: {e}"))?;
        let oldest_commit = matching_commits.last().unwrap();
        let oldest_tree = if oldest_commit.parent_count() > 0 {
            oldest_commit.parent(0).and_then(|p| p.tree()).ok()
        } else {
            None
        };

        let diff = self.repo.diff_tree_to_tree(oldest_tree.as_ref(), Some(&newest_tree), None)
            .map_err(|e| format!("diff_tree_to_tree: {e}"))?;

        let files = collect_diff_paths(&diff);
        let mut patch = String::new();
        diff.print(git2::DiffFormat::Patch, |_delta, _hunk, line| {
            let origin = line.origin();
            if let Ok(content) = std::str::from_utf8(line.content()) {
                if origin == '+' || origin == '-' || origin == ' ' {
                    patch.push(origin);
                }
                patch.push_str(content);
            }
            true
        }).map_err(|e| format!("print diff: {e}"))?;

        Ok((patch, files))
    }
}

fn resolve_commit<'a>(repo: &'a Repository, name: &str) -> Result<git2::Commit<'a>, String> {
    if let Ok(oid) = name.parse::<git2::Oid>() {
        return repo.find_commit(oid).map_err(|e| format!("commit `{name}`: {e}"));
    }
    let obj = repo.revparse_single(name).map_err(|e| format!("revparse `{name}`: {e}"))?;
    obj.peel_to_commit().map_err(|e| format!("peel `{name}`: {e}"))
}

fn collect_diff_paths(diff: &git2::Diff) -> Vec<String> {
    let mut paths = Vec::new();
    let _ = diff.foreach(
        &mut |delta, _| {
            let path = delta.new_file().path()
                .or_else(|| delta.old_file().path())
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();
            if !path.is_empty() {
                paths.push(path);
            }
            true
        },
        None, None, None,
    );
    paths
}

fn collect_diff_files(diff: &git2::Diff) -> Vec<DiffFileEntry> {
    let mut files = Vec::new();
    let _ = diff.foreach(
        &mut |delta, _| {
            let path = delta.new_file().path()
                .or_else(|| delta.old_file().path())
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();
            let status = match delta.status() {
                git2::Delta::Added => "added",
                git2::Delta::Deleted => "deleted",
                git2::Delta::Modified => "modified",
                git2::Delta::Renamed => "renamed",
                git2::Delta::Copied => "copied",
                _ => "unknown",
            };
            files.push(DiffFileEntry {
                path,
                status: status.to_string(),
                insertions: 0,
                deletions: 0,
            });
            true
        },
        None, None, None,
    );
    files
}

fn format_timestamp(secs: i64) -> String {
    use chrono::DateTime;
    match DateTime::from_timestamp(secs, 0) {
        Some(dt) => dt.format("%Y-%m-%d %H:%M:%S").to_string(),
        None => secs.to_string(),
    }
}


/// Result of running validation inside an ephemeral worktree sandbox.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ShellPolicy {
    /// Whitelist of allowed executable commands (base names without path or extension)
    pub allowed_binaries: std::collections::HashSet<String>,
    /// Blacklist of forbidden command patterns or arguments (e.g. "rm", "DROP", "TRUNCATE")
    pub forbidden_patterns: Vec<String>,
    /// Environment variable names to scrub from subprocess execution (preventing secret exfiltration)
    pub scrubbed_env_vars: Vec<String>,
    /// Execution timeout in seconds
    pub timeout_secs: u64,
}

impl Default for ShellPolicy {
    fn default() -> Self {
        let allowed = [
            "cargo", "rustc", "git", "npm", "npx", "pnpm", "yarn", "bun",
            "node", "tsc", "python", "python3", "pytest", "ruff", "mypy",
            "grep", "rg", "find", "ls", "cat", "head", "tail"
        ].iter().map(|s| s.to_string()).collect();

        let forbidden = vec![
            "rm -rf".to_string(), "rmdir".to_string(), "del /f".to_string(),
            "drop table".to_string(), "truncate table".to_string(),
            ":(){ :|:& };:".to_string(), "format ".to_string(),
            "remove-item".to_string(), "mkfs".to_string(),
        ];

        let scrubbed = vec![
            "AWS_SECRET_ACCESS_KEY".to_string(), "OPENAI_API_KEY".to_string(),
            "ANTHROPIC_API_KEY".to_string(), "GEMINI_API_KEY".to_string(),
            "DATABASE_URL".to_string(), "SUPABASE_SERVICE_ROLE_KEY".to_string(),
            "GITHUB_TOKEN".to_string(), "OZY_SECRET".to_string(),
        ];

        Self {
            allowed_binaries: allowed,
            forbidden_patterns: forbidden,
            scrubbed_env_vars: scrubbed,
            timeout_secs: 30,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ValidationResult {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub passed: bool,
    pub duration_ms: u64,
}

/// An isolated ephemeral Git worktree sandbox in volatile/temp space.
/// Uses RAII (Drop) to guarantee cleanup of both the worktree registration and temp directory.
pub struct EphemeralWorktree {
    repo_root: PathBuf,
    worktree_path: PathBuf,
    cleaned_up: bool,
}

impl EphemeralWorktree {
    /// Create a new detached worktree from HEAD into an ephemeral directory.
    pub fn create(repo_root: &Path) -> Result<Self, String> {
        let temp_dir = tempfile::Builder::new()
            .prefix("ozysandbox_")
            .tempdir()
            .map_err(|e| format!("Failed to create temporary sandbox directory: {}", e))?;

        let worktree_path = temp_dir.path().to_path_buf();
        drop(temp_dir);
        let _ = std::fs::remove_dir_all(&worktree_path);

        let output = std::process::Command::new("git")
            .args(["worktree", "add", "--detach"])
            .arg(&worktree_path)
            .arg("HEAD")
            .current_dir(repo_root)
            .output()
            .map_err(|e| format!("Failed to spawn git worktree add: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("git worktree add failed: {}", stderr));
        }

        Ok(Self {
            repo_root: repo_root.to_path_buf(),
            worktree_path,
            cleaned_up: false,
        })
    }

    /// Path to the isolated sandbox directory.
    pub fn path(&self) -> &Path {
        &self.worktree_path
    }

    /// Write or overwrite a file in the ephemeral sandbox.
    pub fn apply_file(&self, relative_path: &str, content: &str) -> Result<(), String> {
        let dest = self.worktree_path.join(relative_path);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create parent dirs for {}: {}", relative_path, e))?;
        }
        std::fs::write(&dest, content)
            .map_err(|e| format!("Failed to write file {}: {}", relative_path, e))?;
        Ok(())
    }

    /// Verify command safety against Default-Deny whitelist and forbidden patterns (Strands Shell-style).
    pub fn verify_command_safety(cmd: &str, args: &[&str], policy: &ShellPolicy) -> Result<(), String> {
        let cmd_clean = std::path::Path::new(cmd)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(cmd)
            .to_lowercase();

        // 1. Default-Deny whitelist check
        if !policy.allowed_binaries.contains(&cmd_clean) {
            return Err(format!(
                "[CRITICAL_VETO: COMMAND_DENIED] Binary '{}' is not in sandbox whitelist (Default-Deny policy).",
                cmd
            ));
        }

        // 2. Destructive pattern blacklist check
        let full_cmd_line = format!("{} {}", cmd, args.join(" ")).to_lowercase();
        for forbidden in &policy.forbidden_patterns {
            if full_cmd_line.contains(forbidden) {
                return Err(format!(
                    "[CRITICAL_VETO: DESTRUCTIVE_PATTERN_DETECTED] Command line matches forbidden pattern '{}'.",
                    forbidden
                ));
            }
        }

        Ok(())
    }

    /// Executes a command in the isolated worktree under strict Default-Deny security policies.
    /// Scrubs sensitive environment variables and enforces whitelisted execution.
    pub fn run_sandboxed_command(&self, cmd: &str, args: &[&str]) -> Result<ValidationResult, String> {
        self.run_sandboxed_command_with_policy(cmd, args, &ShellPolicy::default())
    }

    /// Executes a sandboxed command with a custom ShellPolicy.
    pub fn run_sandboxed_command_with_policy(&self, cmd: &str, args: &[&str], policy: &ShellPolicy) -> Result<ValidationResult, String> {
        Self::verify_command_safety(cmd, args, policy)?;

        let start = std::time::Instant::now();
        let mut command = std::process::Command::new(cmd);
        command.args(args)
            .current_dir(&self.worktree_path);

        // Scrub sensitive environment variables
        for var in &policy.scrubbed_env_vars {
            command.env_remove(var);
        }

        let output = command.output()
            .map_err(|e| format!("Failed to execute sandboxed command '{}': {}", cmd, e))?;

        let duration_ms = start.elapsed().as_millis() as u64;
        let exit_code = output.status.code().unwrap_or(-1);
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let passed = output.status.success();

        Ok(ValidationResult {
            exit_code,
            stdout,
            stderr,
            passed,
            duration_ms,
        })
    }

    /// Run a validation command inside the isolated worktree with execution timing.
    pub fn run_validation(&self, cmd: &str, args: &[&str]) -> Result<ValidationResult, String> {
        self.run_sandboxed_command(cmd, args)
    }

    /// Explicitly remove the worktree and clean up references.
    pub fn cleanup(&mut self) {
        if !self.cleaned_up {
            let _ = std::process::Command::new("git")
                .args(["worktree", "remove", "--force"])
                .arg(&self.worktree_path)
                .current_dir(&self.repo_root)
                .output();

            let _ = std::process::Command::new("git")
                .args(["worktree", "prune"])
                .current_dir(&self.repo_root)
                .output();

            if self.worktree_path.exists() {
                let _ = std::fs::remove_dir_all(&self.worktree_path);
            }
            self.cleaned_up = true;
        }
    }
}

impl Drop for EphemeralWorktree {
    fn drop(&mut self) {
        self.cleanup();
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub enum RepoRole {
    BackendFastApi,
    FrontendNextJs,
    RustCrate,
    FullstackMonorepo,
    GenericGit,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DiscoveredRepo {
    pub path: PathBuf,
    pub relative_name: String,
    pub role: RepoRole,
    pub branch: Option<String>,
    pub head_hash: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RepoLiveDiff {
    pub repo_path: String,
    pub relative_name: String,
    pub role: RepoRole,
    pub uncommitted_patch: String,
    pub uncommitted_files: Vec<String>,
    pub recent_commits_patch: String,
    pub recent_commits_files: Vec<String>,
}

pub struct MultiRepoScanner;

impl MultiRepoScanner {
    pub fn classify_repo(repo_path: &Path) -> RepoRole {
        let has_fastapi = repo_path.join("main.py").exists()
            || repo_path.join("app").join("main.py").exists()
            || repo_path.join("api").exists()
            || if let Ok(c) = std::fs::read_to_string(repo_path.join("requirements.txt")) {
                c.to_lowercase().contains("fastapi")
            } else {
                false
            };

        let has_nextjs = repo_path.join("next.config.js").exists()
            || repo_path.join("next.config.mjs").exists()
            || repo_path.join("next.config.ts").exists()
            || if let Ok(c) = std::fs::read_to_string(repo_path.join("package.json")) {
                c.contains("\"next\"")
            } else {
                false
            };

        let has_cargo = repo_path.join("Cargo.toml").exists();

        match (has_fastapi, has_nextjs, has_cargo) {
            (true, true, _) => RepoRole::FullstackMonorepo,
            (true, false, _) => RepoRole::BackendFastApi,
            (false, true, _) => RepoRole::FrontendNextJs,
            (false, false, true) => RepoRole::RustCrate,
            _ => RepoRole::GenericGit,
        }
    }

    pub fn discover_sibling_repos(root_dir: &Path) -> Result<Vec<DiscoveredRepo>, String> {
        let mut discovered = Vec::new();
        let entries = std::fs::read_dir(root_dir)
            .map_err(|e| format!("Failed to read directory {}: {}", root_dir.display(), e))?;

        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if path.join(".git").exists() {
                    let relative_name = path.file_name()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    let role = Self::classify_repo(&path);

                    let (branch, head_hash) = if let Ok(backend) = GitBackend::open(&path) {
                        (backend.current_branch(), backend.head_commit_hash())
                    } else {
                        (None, None)
                    };

                    discovered.push(DiscoveredRepo {
                        path,
                        relative_name,
                        role,
                        branch,
                        head_hash,
                    });
                }
            }
        }

        if discovered.is_empty() && root_dir.join(".git").exists() {
            let role = Self::classify_repo(root_dir);
            let (branch, head_hash) = if let Ok(backend) = GitBackend::open(root_dir) {
                (backend.current_branch(), backend.head_commit_hash())
            } else {
                (None, None)
            };
            discovered.push(DiscoveredRepo {
                path: root_dir.to_path_buf(),
                relative_name: ".".to_string(),
                role,
                branch,
                head_hash,
            });
        }

        Ok(discovered)
    }

    pub fn extract_live_diffs(
        repos: &[DiscoveredRepo],
        recent_hours: u32,
    ) -> Result<Vec<RepoLiveDiff>, String> {
        let mut diffs = Vec::new();
        for r in repos {
            if let Ok(backend) = GitBackend::open(&r.path) {
                let (uncommitted_patch, uncommitted_files) = backend.live_uncommitted_diff()
                    .unwrap_or_else(|_| (String::new(), Vec::new()));
                let (recent_commits_patch, recent_commits_files) = backend.recent_commits_diff(recent_hours)
                    .unwrap_or_else(|_| (String::new(), Vec::new()));

                diffs.push(RepoLiveDiff {
                    repo_path: r.path.to_string_lossy().to_string(),
                    relative_name: r.relative_name.clone(),
                    role: r.role.clone(),
                    uncommitted_patch,
                    uncommitted_files,
                    recent_commits_patch,
                    recent_commits_files,
                });
            }
        }
        Ok(diffs)
    }
}

/// Biological Synaptic Tagging & Capture (STC) substrate.
/// Manages ephemeral memory marks on AST files/symbols with temporal decay.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SynapticTag {
    pub target_path: String,
    pub symbol_name: Option<String>,
    pub repo_role: RepoRole,
    pub tagged_at_secs: i64,
    pub ttl_secs: i64,
    pub weight: f32,
}

pub struct SynapticTagger {
    default_ttl_secs: i64,
    tags: std::collections::HashMap<String, SynapticTag>,
}

impl SynapticTagger {
    pub fn new(default_ttl_secs: i64) -> Self {
        Self {
            default_ttl_secs: if default_ttl_secs <= 0 { 1800 } else { default_ttl_secs },
            tags: std::collections::HashMap::new(),
        }
    }

    pub fn tag_file(
        &mut self,
        target_path: &str,
        repo_role: RepoRole,
        symbol_name: Option<&str>,
        weight: f32,
        now_secs: i64,
    ) {
        let key = match symbol_name {
            Some(sym) => format!("{}:{}", target_path, sym),
            None => target_path.to_string(),
        };
        self.tags.insert(
            key,
            SynapticTag {
                target_path: target_path.to_string(),
                symbol_name: symbol_name.map(|s| s.to_string()),
                repo_role,
                tagged_at_secs: now_secs,
                ttl_secs: self.default_ttl_secs,
                weight,
            },
        );
    }

    pub fn is_tagged(&self, target_path: &str, symbol_name: Option<&str>, now_secs: i64) -> bool {
        let key = match symbol_name {
            Some(sym) => format!("{}:{}", target_path, sym),
            None => target_path.to_string(),
        };
        if let Some(tag) = self.tags.get(&key) {
            now_secs <= tag.tagged_at_secs + tag.ttl_secs
        } else {
            false
        }
    }

    pub fn active_tags(&self, now_secs: i64) -> Vec<SynapticTag> {
        self.tags
            .values()
            .filter(|tag| now_secs <= tag.tagged_at_secs + tag.ttl_secs)
            .cloned()
            .collect()
    }

    pub fn prune_decayed(&mut self, now_secs: i64) -> usize {
        let before = self.tags.len();
        self.tags.retain(|_, tag| now_secs <= tag.tagged_at_secs + tag.ttl_secs);
        before - self.tags.len()
    }

    pub fn count(&self) -> usize {
        self.tags.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use tempfile::TempDir;

    fn create_repo(dir: &Path) {
        Command::new("git").args(["init", "--initial-branch=main"]).arg(dir).output().ok();
        let cwd = || std::env::set_current_dir(dir).ok();
        cwd();
        Command::new("git").args(["config", "user.email", "test@test.com"]).current_dir(dir).output().ok();
        Command::new("git").args(["config", "user.name", "Test User"]).current_dir(dir).output().ok();
    }

    fn git_commit(dir: &Path, msg: &str) {
        Command::new("git").args(["add", "."]).current_dir(dir).output().ok();
        Command::new("git").args(["commit", "-m", msg]).current_dir(dir).output().ok();
    }

    fn init_repo(dir: &Path) -> GitBackend {
        create_repo(dir);
        std::fs::write(dir.join("README.md"), "# Hello\n").unwrap();
        git_commit(dir, "Initial commit");
        std::fs::write(dir.join("main.rs"), "fn main() {}\n").unwrap();
        std::fs::write(dir.join("lib.rs"), "pub fn helper() {}\n").unwrap();
        git_commit(dir, "Add main and lib");
        GitBackend::open(dir).unwrap()
    }

    #[test]
    fn test_recent_changes_limit() {
        let dir = TempDir::new().unwrap();
        let backend = init_repo(dir.path());
        let changes = backend.recent_changes(1).unwrap();
        assert_eq!(changes.len(), 1, "limit=1");
        assert!(changes[0].message.contains("main and lib"));
    }

    #[test]
    fn test_recent_changes_all() {
        let dir = TempDir::new().unwrap();
        let backend = init_repo(dir.path());
        let changes = backend.recent_changes(10).unwrap();
        assert_eq!(changes.len(), 2, "2 commits");
        assert!(changes[0].files.contains(&"main.rs".to_string()));
        assert!(changes[1].message.contains("Initial"));
    }

    #[test]
    fn test_diff_summary() {
        let dir = TempDir::new().unwrap();
        let backend = init_repo(dir.path());
        let summary = backend.diff_summary("HEAD~1", Some("HEAD")).unwrap();
        assert_eq!(summary.files_changed, 2);
    }

    #[test]
    fn test_open_from_subdir() {
        let dir = TempDir::new().unwrap();
        let _backend = init_repo(dir.path());
        let sub = dir.path().join("src");
        std::fs::create_dir_all(&sub).unwrap();
        let backend = GitBackend::open(&sub).unwrap();
        let changes = backend.recent_changes(1).unwrap();
        assert_eq!(changes.len(), 1, "discover from subdirectory");
    }

    #[test]
    fn test_diff_file() {
        let dir = TempDir::new().unwrap();
        let backend = init_repo(dir.path());

        let diff = backend.diff_file("HEAD~1", Some("HEAD"), "main.rs").unwrap();
        assert!(diff.contains("+fn main()"), "diff should have '+fn main()':\n{diff}");
        assert!(diff.contains("--- "), "diff should have file header:\n{diff}");
    }

    #[test]
    fn test_diff_file_no_changes() {
        let dir = TempDir::new().unwrap();
        let backend = init_repo(dir.path());

        let diff = backend.diff_file("HEAD", Some("HEAD"), "main.rs").unwrap();
        assert!(diff.contains("No changes"), "no changes between same ref: {diff}");
    }

    #[test]
    fn test_blame_file() {
        let dir = TempDir::new().unwrap();
        let backend = init_repo(dir.path());

        // Blame main.rs â€” single line
        let entries = backend.blame_file("main.rs", None, None).unwrap();
        assert!(!entries.is_empty(), "blame should return at least one entry");
        for e in &entries {
            assert!(!e.hash.is_empty(), "blame entry must have hash");
            assert!(!e.author.is_empty(), "blame entry must have author");
        }
    }

    #[test]
    fn test_blame_file_range() {
        let dir = TempDir::new().unwrap();
        let backend = init_repo(dir.path());

        // Blame with range
        let entries = backend.blame_file("main.rs", Some(1), Some(1)).unwrap();
        assert!(!entries.is_empty(), "line 1 should have blame");
        assert_eq!(entries[0].start_line, 1);
    }

    #[test]
    fn test_open_no_repo() {
        let dir = TempDir::new().unwrap();
        let result = GitBackend::open(dir.path());
        assert!(result.is_err(), "non-repo should fail");
        let err = result.unwrap_err();
        assert!(err.contains("No git repository"), "got: {err}");
    }

    #[test]
    fn test_root_commit() {
        let dir = TempDir::new().unwrap();
        create_repo(dir.path());
        std::fs::write(dir.path().join("README.md"), "# Hello\n").unwrap();
        git_commit(dir.path(), "Initial commit");

        let backend = GitBackend::open(dir.path()).unwrap();
        let changes = backend.recent_changes(10).unwrap();
        assert_eq!(changes.len(), 1, "single commit repo");
        assert_eq!(changes[0].message, "Initial commit");
        assert!(!changes[0].files.is_empty(), "root commit must have files");
        assert!(changes[0].files.contains(&"README.md".to_string()));
        assert!(!changes[0].is_merge, "root commit is not a merge");
    }

    #[test]
    fn test_merge_commit() {
        let dir = TempDir::new().unwrap();
        create_repo(dir.path());

        // Initial commit on main
        std::fs::write(dir.path().join("base.txt"), "base\n").unwrap();
        git_commit(dir.path(), "base");

        // Create feature branch and commit
        Command::new("git").args(["checkout", "-b", "feature"]).current_dir(dir.path()).output().ok();
        std::fs::write(dir.path().join("feature.txt"), "feature\n").unwrap();
        git_commit(dir.path(), "feature work");

        // Back to main, merge feature with --no-ff to force a merge commit
        Command::new("git").args(["checkout", "main"]).current_dir(dir.path()).output().ok();
        Command::new("git").args(["merge", "--no-ff", "-m", "Merge feature branch", "feature"])
            .current_dir(dir.path()).output().ok();

        let backend = GitBackend::open(dir.path()).unwrap();
        let changes = backend.recent_changes(10).unwrap();
        assert_eq!(changes.len(), 3, "3 commits: base, feature, merge");

        // Newest commit is the merge
        let merge = &changes[0];
        assert!(merge.is_merge, "merge commit must be marked as merge");
        assert!(merge.message.contains("Merge feature"), "merge message: {}", merge.message);

        // Merge commit should show files from the merged branch
        assert!(merge.files.contains(&"feature.txt".to_string()),
            "merge commit must include merged file; got: {:?}", merge.files);

        // Older commits are not merges
        assert!(!changes[2].is_merge, "root commit should not be merge");
        assert!(!changes[1].is_merge, "feature commit should not be merge");
    }

    #[test]
    fn test_ephemeral_worktree_sandbox() {
        let dir = TempDir::new().unwrap();
        let backend = init_repo(dir.path());

        // Create ephemeral worktree
        let worktree = EphemeralWorktree::create(backend.repo_path()).unwrap();
        assert!(worktree.path().exists(), "Worktree dir must exist");
        assert!(worktree.path().join("main.rs").exists(), "HEAD files must be present");

        // Apply a mutated file in sandbox
        worktree.apply_file("sandbox_test.txt", "sandbox content\n").unwrap();
        assert!(worktree.path().join("sandbox_test.txt").exists());
        assert!(!dir.path().join("sandbox_test.txt").exists(), "Real repo must NOT have sandbox file");

        // Run validation command
        let res = worktree.run_validation("git", &["status"]).unwrap();
        assert!(res.passed, "git status in sandbox should pass");

        // Drop / cleanup worktree
        let wt_path = worktree.path().to_path_buf();
        drop(worktree);
        assert!(!wt_path.exists(), "Worktree path must be cleaned up on drop");
    }

    #[test]
    fn test_ephemeral_worktree_default_deny_mediation() {
        let dir = TempDir::new().unwrap();
        let backend = init_repo(dir.path());
        let worktree = EphemeralWorktree::create(backend.repo_path()).unwrap();

        // 1. Whitelisted command: git status must pass
        let ok_res = worktree.run_sandboxed_command("git", &["status"]);
        assert!(ok_res.is_ok(), "git status must be allowed");
        assert!(ok_res.unwrap().passed);

        // 2. Unwhitelisted binary: must trigger [CRITICAL_VETO: COMMAND_DENIED]
        let denied_res = worktree.run_sandboxed_command("untrusted_evil_script", &["arg1"]);
        assert!(denied_res.is_err());
        assert!(denied_res.unwrap_err().contains("[CRITICAL_VETO: COMMAND_DENIED]"));

        // 3. Destructive pattern: must trigger [CRITICAL_VETO: DESTRUCTIVE_PATTERN_DETECTED]
        let veto_res = worktree.run_sandboxed_command("git", &["rm -rf", "src"]);
        assert!(veto_res.is_err());
        assert!(veto_res.unwrap_err().contains("[CRITICAL_VETO: DESTRUCTIVE_PATTERN_DETECTED]"));
    }

    #[test]
    fn test_multi_repo_scanner_and_classification() {
        let dir = TempDir::new().unwrap();
        let api_dir = dir.path().join("api-geofal-crm");
        let frontend_dir = dir.path().join("crm-geofal");
        let rust_dir = dir.path().join("rust-worker");

        std::fs::create_dir_all(&api_dir).unwrap();
        std::fs::create_dir_all(&frontend_dir).unwrap();
        std::fs::create_dir_all(&rust_dir).unwrap();

        create_repo(&api_dir);
        std::fs::write(api_dir.join("main.py"), "from fastapi import FastAPI\n").unwrap();
        git_commit(&api_dir, "initial backend");

        create_repo(&frontend_dir);
        std::fs::write(frontend_dir.join("package.json"), "{\"dependencies\": {\"next\": \"16.0.0\"}}\n").unwrap();
        git_commit(&frontend_dir, "initial frontend");

        create_repo(&rust_dir);
        std::fs::write(rust_dir.join("Cargo.toml"), "[package]\nname=\"worker\"\n").unwrap();
        git_commit(&rust_dir, "initial rust");

        let repos = MultiRepoScanner::discover_sibling_repos(dir.path()).unwrap();
        assert_eq!(repos.len(), 3);

        let api_repo = repos.iter().find(|r| r.relative_name == "api-geofal-crm").unwrap();
        assert_eq!(api_repo.role, RepoRole::BackendFastApi);

        let fe_repo = repos.iter().find(|r| r.relative_name == "crm-geofal").unwrap();
        assert_eq!(fe_repo.role, RepoRole::FrontendNextJs);

        let rs_repo = repos.iter().find(|r| r.relative_name == "rust-worker").unwrap();
        assert_eq!(rs_repo.role, RepoRole::RustCrate);
    }

    #[test]
    fn test_synaptic_tagger_lifecycle_and_decay() {
        let mut tagger = SynapticTagger::new(100);
        let now = 1000;

        tagger.tag_file("src/api.py", RepoRole::BackendFastApi, Some("auto_sync"), 0.9, now);
        tagger.tag_file("src/ui.tsx", RepoRole::FrontendNextJs, None, 0.5, now);

        assert_eq!(tagger.count(), 2);
        assert!(tagger.is_tagged("src/api.py", Some("auto_sync"), now + 50));
        assert!(tagger.is_tagged("src/ui.tsx", None, now + 100));

        // After TTL passes, is_tagged should return false
        assert!(!tagger.is_tagged("src/api.py", Some("auto_sync"), now + 101));

        let active = tagger.active_tags(now + 50);
        assert_eq!(active.len(), 2);

        let active_later = tagger.active_tags(now + 101);
        assert_eq!(active_later.len(), 0);

        let pruned = tagger.prune_decayed(now + 101);
        assert_eq!(pruned, 2);
        assert_eq!(tagger.count(), 0);
    }

    #[test]
    fn test_live_uncommitted_diff_in_memory() {
        let dir = TempDir::new().unwrap();
        let backend = init_repo(dir.path());

        // Modify a file in working tree without committing
        std::fs::write(dir.path().join("main.rs"), "fn main() { println!(\"live\"); }\n").unwrap();

        let (patch, files) = backend.live_uncommitted_diff().unwrap();
        assert!(files.contains(&"main.rs".to_string()));
        assert!(patch.contains("+fn main() { println!(\"live\"); }"));

        // Extract live diffs via MultiRepoScanner
        let repos = MultiRepoScanner::discover_sibling_repos(dir.path()).unwrap();
        let live_diffs = MultiRepoScanner::extract_live_diffs(&repos, 24).unwrap();
        assert_eq!(live_diffs.len(), 1);
        assert!(live_diffs[0].uncommitted_files.contains(&"main.rs".to_string()));
    }
}
