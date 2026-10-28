"""Multi-Repository Workspace Scanner and Live AST Diff Extractor.

Implements biological Synaptic Tagging & Capture (STC) over live working tree changes
and recent commits across consolidated multi-repository architectures (e.g. FastAPI + Next.js).
"""

from __future__ import annotations

import logging
import math
import subprocess
import time
from dataclasses import dataclass, field
from enum import Enum
from pathlib import Path
from typing import Dict, List, Optional, Set, Tuple

logger = logging.getLogger("ozy_brain.scanner.multi_repo")


class RepoRole(str, Enum):
    """Categorized architectural role of a discovered repository."""
    BACKEND_FASTAPI = "BackendFastApi"
    FRONTEND_NEXTJS = "FrontendNextJs"
    RUST_CRATE = "RustCrate"
    FULLSTACK_MONOREPO = "FullstackMonorepo"
    GENERIC_GIT = "GenericGit"


@dataclass
class DiscoveredRepo:
    """Metadata describing a sibling repository inside a consolidated workspace."""
    path: Path
    relative_name: str
    role: RepoRole
    branch: Optional[str] = None
    head_hash: Optional[str] = None


@dataclass
class RepoLiveDiff:
    """In-memory harvest of uncommitted working tree changes and recent commits."""
    repo_path: str
    relative_name: str
    role: RepoRole
    uncommitted_patch: str
    uncommitted_files: List[str]
    recent_commits_patch: str
    recent_commits_files: List[str]

    @property
    def total_affected_files(self) -> List[str]:
        """Unique sorted list of all affected file paths."""
        return sorted(list(set(self.uncommitted_files + self.recent_commits_files)))


@dataclass
class SynapticTag:
    """Biological Synaptic Tagging (STC) mark on a modified file or AST symbol."""
    target_path: str
    symbol_name: Optional[str]
    repo_role: RepoRole
    tagged_at_secs: float
    ttl_secs: float = 1800.0  # 30-minute biological tag window
    weight: float = 1.0

    def is_active(self, now_secs: Optional[float] = None) -> bool:
        """Return True if the tag has not yet decayed."""
        t = time.time() if now_secs is None else now_secs
        return t <= (self.tagged_at_secs + self.ttl_secs)

    def current_weight(self, now_secs: Optional[float] = None) -> float:
        """Compute exponential temporal decay w(t) = w0 * exp(-delta_t / tau)."""
        t = time.time() if now_secs is None else now_secs
        elapsed = max(0.0, t - self.tagged_at_secs)
        tau = self.ttl_secs / 2.0  # Half-life at 15 minutes
        return float(self.weight * math.exp(-elapsed / tau))


class MultiRepoScanner:
    """Scans and classifies sibling repositories in a multi-repo workspace."""

    @staticmethod
    def classify_repo(repo_path: Path) -> RepoRole:
        """Inspect directory markers and files to classify architectural role."""
        has_fastapi = False
        has_nextjs = False
        has_cargo = (repo_path / "Cargo.toml").is_file()

        # Backend FastAPI heuristics
        if (repo_path / "main.py").is_file() or (repo_path / "app" / "main.py").is_file() or (repo_path / "api").is_dir():
            has_fastapi = True
        req_file = repo_path / "requirements.txt"
        if req_file.is_file():
            try:
                content = req_file.read_text(encoding="utf-8", errors="ignore").lower()
                if "fastapi" in content:
                    has_fastapi = True
            except Exception:
                pass
        pyp_file = repo_path / "pyproject.toml"
        if pyp_file.is_file():
            try:
                content = pyp_file.read_text(encoding="utf-8", errors="ignore").lower()
                if "fastapi" in content:
                    has_fastapi = True
            except Exception:
                pass

        # Frontend Next.js heuristics
        if (
            (repo_path / "next.config.js").is_file()
            or (repo_path / "next.config.mjs").is_file()
            or (repo_path / "next.config.ts").is_file()
        ):
            has_nextjs = True
        pkg_file = repo_path / "package.json"
        if pkg_file.is_file():
            try:
                content = pkg_file.read_text(encoding="utf-8", errors="ignore")
                if '"next"' in content:
                    has_nextjs = True
            except Exception:
                pass

        if has_fastapi and has_nextjs:
            return RepoRole.FULLSTACK_MONOREPO
        if has_fastapi:
            return RepoRole.BACKEND_FASTAPI
        if has_nextjs:
            return RepoRole.FRONTEND_NEXTJS
        if has_cargo:
            return RepoRole.RUST_CRATE
        return RepoRole.GENERIC_GIT

    @classmethod
    def discover_sibling_repos(cls, root_dir: Path) -> List[DiscoveredRepo]:
        """Discover all sibling git repos inside root_dir."""
        discovered: List[DiscoveredRepo] = []
        if not root_dir.is_dir():
            return discovered

        for item in sorted(root_dir.iterdir()):
            if item.is_dir() and (item / ".git").exists():
                role = cls.classify_repo(item)
                branch, head_hash = cls._read_git_head_info(item)
                discovered.append(
                    DiscoveredRepo(
                        path=item,
                        relative_name=item.name,
                        role=role,
                        branch=branch,
                        head_hash=head_hash,
                    )
                )

        # If root_dir itself is a git repo and no sub-repos found, include it
        if not discovered and (root_dir / ".git").exists():
            role = cls.classify_repo(root_dir)
            branch, head_hash = cls._read_git_head_info(root_dir)
            discovered.append(
                DiscoveredRepo(
                    path=root_dir,
                    relative_name=".",
                    role=role,
                    branch=branch,
                    head_hash=head_hash,
                )
            )

        return discovered

    @staticmethod
    def _read_git_head_info(repo_path: Path) -> Tuple[Optional[str], Optional[str]]:
        """Read branch and commit hash cleanly via git CLI."""
        branch = None
        head_hash = None
        try:
            res_branch = subprocess.run(
                ["git", "rev-parse", "--abbrev-ref", "HEAD"],
                cwd=repo_path,
                capture_output=True,
                encoding="utf-8",
                errors="replace",
                timeout=2,
                check=False,
            )
            if res_branch.returncode == 0:
                branch = res_branch.stdout.strip()

            res_hash = subprocess.run(
                ["git", "rev-parse", "HEAD"],
                cwd=repo_path,
                capture_output=True,
                encoding="utf-8",
                errors="replace",
                timeout=2,
                check=False,
            )
            if res_hash.returncode == 0:
                head_hash = res_hash.stdout.strip()
        except Exception as e:
            logger.debug("Failed to read git head for %s: %s", repo_path, e)
        return branch, head_hash


class LiveDiffExtractor:
    """Harvests working tree uncommitted changes and recent commits in memory."""

    @staticmethod
    def extract_repo_diff(repo_path: Path, hours: int = 24) -> RepoLiveDiff:
        """Extract uncommitted patch and recent commits patch for a single repository."""
        role = MultiRepoScanner.classify_repo(repo_path)
        rel_name = repo_path.name

        # 1. Working tree uncommitted changes (HEAD diff)
        uncommitted_patch = ""
        uncommitted_files: List[str] = []
        try:
            res = subprocess.run(
                ["git", "diff", "HEAD"],
                cwd=repo_path,
                capture_output=True,
                encoding="utf-8",
                errors="replace",
                timeout=5,
                check=False,
            )
            if res.returncode == 0:
                uncommitted_patch = res.stdout

            res_names = subprocess.run(
                ["git", "diff", "--name-only", "HEAD"],
                cwd=repo_path,
                capture_output=True,
                encoding="utf-8",
                errors="replace",
                timeout=3,
                check=False,
            )
            if res_names.returncode == 0:
                uncommitted_files = [f.strip() for f in res_names.stdout.splitlines() if f.strip()]
        except Exception as e:
            logger.error("Error reading uncommitted diff for %s: %s", repo_path, e)

        # 2. Recent commits in the last N hours
        recent_patch = ""
        recent_files: List[str] = []
        try:
            since_arg = f"--since={hours} hours ago"
            res_log = subprocess.run(
                ["git", "log", since_arg, "-p", "-n", "20"],
                cwd=repo_path,
                capture_output=True,
                encoding="utf-8",
                errors="replace",
                timeout=5,
                check=False,
            )
            if res_log.returncode == 0:
                recent_patch = res_log.stdout

            res_log_files = subprocess.run(
                ["git", "log", since_arg, "--name-only", "--pretty=format:", "-n", "20"],
                cwd=repo_path,
                capture_output=True,
                encoding="utf-8",
                errors="replace",
                timeout=3,
                check=False,
            )
            if res_log_files.returncode == 0:
                recent_files = sorted(
                    list(set(f.strip() for f in res_log_files.stdout.splitlines() if f.strip()))
                )
        except Exception as e:
            logger.error("Error reading recent commits for %s: %s", repo_path, e)

        return RepoLiveDiff(
            repo_path=str(repo_path.resolve()),
            relative_name=rel_name,
            role=role,
            uncommitted_patch=uncommitted_patch,
            uncommitted_files=uncommitted_files,
            recent_commits_patch=recent_patch,
            recent_commits_files=recent_files,
        )

    @classmethod
    def extract_workspace_diffs(
        cls, repos: List[DiscoveredRepo], hours: int = 24
    ) -> List[RepoLiveDiff]:
        """Harvest live diffs across all discovered sibling repos."""
        return [cls.extract_repo_diff(r.path, hours=hours) for r in repos]


class SynapticTagger:
    """Manages ephemeral synaptic tags on affected AST targets with biological STC decay."""

    def __init__(self, default_ttl_secs: float = 1800.0) -> None:
        self.default_ttl_secs = default_ttl_secs
        self._tags: Dict[str, SynapticTag] = {}

    def _key(self, target_path: str, symbol_name: Optional[str] = None) -> str:
        return f"{target_path}:{symbol_name}" if symbol_name else target_path

    def tag_file(
        self,
        target_path: str,
        repo_role: RepoRole,
        symbol_name: Optional[str] = None,
        weight: float = 1.0,
        now_secs: Optional[float] = None,
    ) -> SynapticTag:
        """Register or reinforce a synaptic tag on a target path."""
        t = time.time() if now_secs is None else now_secs
        key = self._key(target_path, symbol_name)
        tag = SynapticTag(
            target_path=target_path,
            symbol_name=symbol_name,
            repo_role=repo_role,
            tagged_at_secs=t,
            ttl_secs=self.default_ttl_secs,
            weight=weight,
        )
        self._tags[key] = tag
        return tag

    def tag_from_live_diff(
        self, live_diff: RepoLiveDiff, now_secs: Optional[float] = None
    ) -> int:
        """Tag all affected files from a live diff harvest."""
        count = 0
        for f in live_diff.total_affected_files:
            # Full path relative to repo or workspace
            target = f"{live_diff.relative_name}/{f}"
            self.tag_file(target, live_diff.role, now_secs=now_secs)
            count += 1
        return count

    def is_tagged(
        self,
        target_path: str,
        symbol_name: Optional[str] = None,
        now_secs: Optional[float] = None,
    ) -> bool:
        """Check if target path or symbol is currently actively tagged."""
        t = time.time() if now_secs is None else now_secs
        if symbol_name:
            key = self._key(target_path, symbol_name)
            tag = self._tags.get(key)
            return tag.is_active(t) if tag else False

        # Direct file tag
        file_tag = self._tags.get(target_path)
        if file_tag and file_tag.is_active(t):
            return True

        # Any symbol in file tagged
        prefix = f"{target_path}:"
        for k, tag in self._tags.items():
            if k.startswith(prefix) and tag.is_active(t):
                return True
        return False

    def active_tags(self, now_secs: Optional[float] = None) -> List[SynapticTag]:
        """Return all non-decayed active synaptic tags."""
        t = time.time() if now_secs is None else now_secs
        return [tag for tag in self._tags.values() if tag.is_active(t)]

    def prune_decayed(self, now_secs: Optional[float] = None) -> int:
        """Purge all expired tags and return count of pruned tags."""
        t = time.time() if now_secs is None else now_secs
        before = len(self._tags)
        self._tags = {k: v for k, v in self._tags.items() if v.is_active(t)}
        return before - len(self._tags)

    def count(self) -> int:
        """Return total tracked tags in memory."""
        return len(self._tags)
