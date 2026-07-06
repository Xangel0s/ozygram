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

        // Blame main.rs — single line
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
}
