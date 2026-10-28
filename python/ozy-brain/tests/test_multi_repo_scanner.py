"""Unit and integration tests for Multi-Repo Workspace Scanner and Synaptic Tagger."""

import subprocess
import tempfile
import time
from pathlib import Path

import pytest
from ozy_brain.scanner.multi_repo import (
    DiscoveredRepo,
    LiveDiffExtractor,
    MultiRepoScanner,
    RepoLiveDiff,
    RepoRole,
    SynapticTag,
    SynapticTagger,
)


def _init_git_repo(path: Path) -> None:
    """Helper to initialize a git repo for testing."""
    path.mkdir(parents=True, exist_ok=True)
    subprocess.run(["git", "init", "--initial-branch=main"], cwd=path, capture_output=True, check=True)
    subprocess.run(["git", "config", "user.email", "test@test.local"], cwd=path, capture_output=True, check=True)
    subprocess.run(["git", "config", "user.name", "Test Agent"], cwd=path, capture_output=True, check=True)


def test_classify_repo_markers():
    """Verify classification of different frameworks based on directory markers."""
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)

        # Backend FastAPI
        backend_dir = root / "backend"
        backend_dir.mkdir()
        (backend_dir / "main.py").write_text("from fastapi import FastAPI\napp = FastAPI()\n", encoding="utf-8")
        assert MultiRepoScanner.classify_repo(backend_dir) == RepoRole.BACKEND_FASTAPI

        # Frontend Next.js
        frontend_dir = root / "frontend"
        frontend_dir.mkdir()
        (frontend_dir / "package.json").write_text('{"dependencies": {"next": "16.0.0"}}\n', encoding="utf-8")
        assert MultiRepoScanner.classify_repo(frontend_dir) == RepoRole.FRONTEND_NEXTJS

        # Rust Crate
        rust_dir = root / "rust_crate"
        rust_dir.mkdir()
        (rust_dir / "Cargo.toml").write_text('[package]\nname = "test"\n', encoding="utf-8")
        assert MultiRepoScanner.classify_repo(rust_dir) == RepoRole.RUST_CRATE


def test_discover_sibling_repos_mock():
    """Verify discovery of multiple sibling git repos within a consolidated workspace."""
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)

        api_dir = root / "api-geofal-crm"
        _init_git_repo(api_dir)
        (api_dir / "main.py").write_text("from fastapi import FastAPI\n", encoding="utf-8")
        subprocess.run(["git", "add", "."], cwd=api_dir, capture_output=True, check=True)
        subprocess.run(["git", "commit", "-m", "init backend"], cwd=api_dir, capture_output=True, check=True)

        fe_dir = root / "crm-geofal"
        _init_git_repo(fe_dir)
        (fe_dir / "package.json").write_text('{"dependencies": {"next": "16.0.0"}}\n', encoding="utf-8")
        subprocess.run(["git", "add", "."], cwd=fe_dir, capture_output=True, check=True)
        subprocess.run(["git", "commit", "-m", "init frontend"], cwd=fe_dir, capture_output=True, check=True)

        discovered = MultiRepoScanner.discover_sibling_repos(root)
        assert len(discovered) == 2

        roles = {d.relative_name: d.role for d in discovered}
        assert roles["api-geofal-crm"] == RepoRole.BACKEND_FASTAPI
        assert roles["crm-geofal"] == RepoRole.FRONTEND_NEXTJS


def test_live_diff_extractor_in_memory():
    """Verify live extraction of uncommitted changes and recent commits in memory."""
    with tempfile.TemporaryDirectory() as tmp:
        repo_dir = Path(tmp)
        _init_git_repo(repo_dir)

        # Initial commit
        (repo_dir / "README.md").write_text("# Test\n", encoding="utf-8")
        subprocess.run(["git", "add", "."], cwd=repo_dir, capture_output=True, check=True)
        subprocess.run(["git", "commit", "-m", "initial commit"], cwd=repo_dir, capture_output=True, check=True)

        # Uncommitted change in working tree
        (repo_dir / "README.md").write_text("# Test\n\nNew live uncommitted line\n", encoding="utf-8")
        (repo_dir / "new_scratch.py").write_text("print('hotspot')\n", encoding="utf-8")

        diff = LiveDiffExtractor.extract_repo_diff(repo_dir, hours=24)
        assert "README.md" in diff.uncommitted_files
        assert "+New live uncommitted line" in diff.uncommitted_patch
        assert len(diff.recent_commits_files) >= 1


def test_synaptic_tagger_stc_decay():
    """Verify Synaptic Tagging & Capture (STC) lifecycle, decay, and pruning."""
    tagger = SynapticTagger(default_ttl_secs=10.0)
    now = 1000.0

    tagger.tag_file("api/routes.py", RepoRole.BACKEND_FASTAPI, symbol_name="auto_sync", weight=1.0, now_secs=now)
    tagger.tag_file("ui/Dock.tsx", RepoRole.FRONTEND_NEXTJS, weight=0.8, now_secs=now)

    assert tagger.count() == 2
    assert tagger.is_tagged("api/routes.py", symbol_name="auto_sync", now_secs=now + 5.0)
    assert tagger.is_tagged("ui/Dock.tsx", now_secs=now + 9.9)

    # After TTL, tag is expired
    assert not tagger.is_tagged("api/routes.py", symbol_name="auto_sync", now_secs=now + 10.1)

    # Decay calculation
    active = tagger.active_tags(now_secs=now + 5.0)
    assert len(active) == 2
    decayed_weight = active[0].current_weight(now_secs=now + 5.0)
    assert decayed_weight < 1.0  # Weight decays exponentially

    # Pruning
    pruned = tagger.prune_decayed(now_secs=now + 15.0)
    assert pruned == 2
    assert tagger.count() == 0


def test_crmnew_real_workspace_discovery():
    """Verify discovery against real crmnew workspace if present."""
    # Look for crmnew in parent or current directory
    crmnew_path = Path("crmnew").resolve()
    if not crmnew_path.is_dir():
        crmnew_path = Path("../../crmnew").resolve()

    if crmnew_path.is_dir() and (crmnew_path / "api-geofal-crm").is_dir():
        discovered = MultiRepoScanner.discover_sibling_repos(crmnew_path)
        names = [d.relative_name for d in discovered]
        assert "api-geofal-crm" in names
        assert "crm-geofal" in names

        diffs = LiveDiffExtractor.extract_workspace_diffs(discovered, hours=24)
        assert len(diffs) >= 2
        for d in diffs:
            assert isinstance(d, RepoLiveDiff)

