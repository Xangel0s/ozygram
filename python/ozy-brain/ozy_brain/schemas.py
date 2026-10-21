from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Iterable


BRAIN_VERSION = "0.2.0"
BRAIN_SCHEMA_VERSION = "v1"


@dataclass
class BrainResponse:
    action: str
    summary: str
    plan: list[str]
    risks: list[str]
    recommendations: list[str]
    memory_updates: list[str]
    confidence: float
    suggested_mcp_calls: list[dict[str, Any]] | None = None
    structured_plan: dict[str, Any] | None = None
    execution_policy: dict[str, Any] | None = None
    brain_context_pack: dict[str, Any] | None = None
    reflection_report: dict[str, Any] | None = None
    risk_assessment: dict[str, Any] | None = None
    mental_model: dict[str, Any] | None = None
    provenance: list[dict[str, Any]] | None = None
    procedural_rules: list[dict[str, Any]] | None = None
    brain_version: str = BRAIN_VERSION
    brain_schema_version: str = BRAIN_SCHEMA_VERSION

    def to_dict(self) -> dict[str, Any]:
        return {
            "action": self.action,
            "summary": self.summary,
            "plan": self.plan,
            "risks": self.risks,
            "recommendations": self.recommendations,
            "memory_updates": self.memory_updates,
            "confidence": self.confidence,
            "suggested_mcp_calls": self.suggested_mcp_calls or [],
            "structured_plan": self.structured_plan or {},
            "execution_policy": self.execution_policy or {},
            "brain_context_pack": self.brain_context_pack or {},
            "reflection_report": self.reflection_report or {},
            "risk_assessment": self.risk_assessment or {},
            "mental_model": self.mental_model or {},
            "provenance": self.provenance or [],
            "procedural_rules": self.procedural_rules or [],
            "brain_version": self.brain_version,
            "brain_schema_version": self.brain_schema_version,
        }



def _items(payload: dict[str, Any], key: str) -> list[Any]:
    value = payload.get(key)
    return value if isinstance(value, list) else []


def _text(payload: dict[str, Any], key: str, default: str = "") -> str:
    value = payload.get(key)
    return value if isinstance(value, str) else default


def _project(payload: dict[str, Any]) -> str:
    return _text(payload, "project", "unknown-project")


def _goal(payload: dict[str, Any]) -> str:
    return _text(payload, "goal") or _text(payload, "query") or "understand and guide the project safely"


def _memory_titles(memories: Iterable[Any], limit: int = 5) -> list[str]:
    titles: list[str] = []
    for mem in memories:
        if isinstance(mem, dict):
            title = mem.get("title") or mem.get("error_context") or mem.get("content")
            if title:
                titles.append(str(title))
        elif mem:
            titles.append(str(mem))
        if len(titles) >= limit:
            break
    return titles


def _combined_memory_titles(payload: dict[str, Any], limit: int = 10) -> list[str]:
    combined: list[Any] = []
    combined.extend(_items(payload, "memories"))
    combined.extend(_items(payload, "relevant_memories"))
    combined.extend(_items(payload, "lessons"))
    combined.extend(_items(payload, "relevant_lessons"))
    seen: set[str] = set()
    titles: list[str] = []
    for title in _memory_titles(combined, limit=limit * 2):
        key = title.strip().lower()
        if key and key not in seen:
            seen.add(key)
            titles.append(title)
        if len(titles) >= limit:
            break
    return titles


def _safe_mcp_calls(payload: dict[str, Any], include_graph: bool = True) -> list[dict[str, Any]]:
    goal = _goal(payload)
    calls: list[dict[str, Any]] = [
        {"tool": "ozy_context", "arguments": {"action": "task", "query": goal, "max_tokens": 3000}},
        {"tool": "ozy_memory", "arguments": {"action": "search", "query": goal, "project": _project(payload), "scope": "project", "limit": 10}},
    ]
    if include_graph:
        calls.append({"tool": "ozy_graph", "arguments": {"action": "summary"}})
    calls.append({"tool": "ozy_project", "arguments": {"action": "list"}})
    return calls


def _is_noise_path(path: str) -> bool:
    clean = path.replace("\\", "/").lower()
    noise_tokens = [
        ".fastembed_cache",
        "node_modules",
        "/.git/",
        "/target/",
        "/backups/",
        "/.backup/",
        "/.agents/",
        "/.cache/",
        "/build/",
        "/dist/",
        "/.venv/",
        "/venv/",
    ]
    if any(tok in clean for tok in noise_tokens):
        return True
    noise_exts = (
        ".lock",
        ".onnx",
        ".bin",
        ".db",
        ".sqlite",
        ".sqlite3",
        ".duckdb",
        ".parquet",
        ".pyc",
        ".wasm",
        ".so",
        ".dll",
        ".dylib",
        ".png",
        ".jpg",
        ".jpeg",
        ".gif",
        ".ico",
        ".pdf",
        ".zip",
        ".tar",
        ".gz",
    )
    if clean.endswith(noise_exts):
        return True
    parts = clean.split("/")
    last = parts[-1]
    if len(last) >= 32 and all(c in "0123456789abcdef" for c in last):
        return True
    return False


def _is_directory_path(path: str) -> bool:
    clean = path.replace("\\", "/").strip()
    if clean.endswith("/"):
        return True
    last = clean.split("/")[-1]
    if not last or ("." not in last and last.lower() not in ("dockerfile", "makefile", "license", "procfile")):
        return True
    return False


def _candidate_files(payload: dict[str, Any], limit: int = 8) -> list[str]:
    files: list[str] = []
    goal = _goal(payload).lower()
    for item in _items(payload, "files"):
        text = str(item)
        if _is_noise_path(text) or _is_directory_path(text):
            continue
        lower = text.lower().replace("\\", "/")
        if any(token in lower for token in goal.replace("/", " ").replace("-", " ").split() if len(token) >= 4):
            files.append(text)
        elif len(files) < max(3, limit // 2):
            files.append(text)
        if len(files) >= limit:
            break
    return files


def _git_status_files(payload: dict[str, Any]) -> list[dict[str, str]]:
    git_context = payload.get("git_context")
    if not isinstance(git_context, dict):
        return []
    files = git_context.get("status_files")
    return [item for item in files if isinstance(item, dict)] if isinstance(files, list) else []


def _candidate_file_scores(payload: dict[str, Any], limit: int = 10) -> list[dict[str, Any]]:
    goal_tokens = {token for token in _goal(payload).lower().replace("/", " ").replace("-", " ").replace("_", " ").split() if len(token) >= 4}
    is_doc_goal = any(tok in goal_tokens for tok in ("docs", "documentation", "readme", "guide", "rule", "markdown", "agent"))
    scores: dict[str, dict[str, Any]] = {}

    def bump(path: str, points: int, reason: str) -> None:
        clean = path.replace("\\", "/")
        entry = scores.setdefault(clean, {"path": clean, "score": 0, "reasons": []})
        entry["score"] += points
        if reason not in entry["reasons"]:
            entry["reasons"].append(reason)

    for path in _items(payload, "files"):
        text = str(path)
        if _is_noise_path(text) or _is_directory_path(text):
            continue
        clean = text.replace("\\", "/")
        if not is_doc_goal and (clean.endswith((".md", ".txt", ".rst")) or "/.agents/" in clean):
            continue
        lower = clean.lower()
        overlap = sorted(token for token in goal_tokens if token in lower)
        bump(text, 2 + min(len(overlap), 4), "indexed candidate" + (f" matches {', '.join(overlap[:3])}" if overlap else ""))

    for item in _git_status_files(payload):
        path = str(item.get("path", "")).strip()
        status = str(item.get("status", "")).strip()
        if path and not _is_noise_path(path) and not _is_directory_path(path):
            clean = path.replace("\\", "/")
            if not is_doc_goal and (clean.endswith((".md", ".txt", ".rst")) or "/.agents/" in clean):
                continue
            lower = clean.lower()
            overlap = sorted(token for token in goal_tokens if token in lower)
            if overlap:
                bump(path, 6 + min(len(overlap), 3), f"dirty git status {status} with goal overlap ({', '.join(overlap)})")
            elif any(c.lower() in lower for c in _items(payload, "changes")):
                bump(path, 5, f"dirty git status {status} in explicit changes")
            else:
                bump(path, 1, f"dirty git status {status} unrelated to goal")

    for change in _items(payload, "changes"):
        text = str(change)
        if ("/" in text or "\\" in text or "." in text) and not _is_directory_path(text):
            bump(text, 5, "explicit change input")

    for memory in _items(payload, "relevant_memories") + _items(payload, "relevant_lessons"):
        if isinstance(memory, dict):
            haystack = " ".join(str(memory.get(k, "")) for k in ("title", "content", "file_path", "error_context"))
            for path in list(scores.keys()):
                name = path.split("/")[-1].lower()
                if name and name in haystack.lower():
                    bump(path, 3, "referenced by relevant memory")

    ranked = sorted(scores.values(), key=lambda item: (-int(item["score"]), str(item["path"])))
    return ranked[:limit]


def _risk_level(payload: dict[str, Any], dirty_files: list[dict[str, str]], candidate_scores: list[dict[str, Any]]) -> str:
    goal = _goal(payload).lower()
    high_risk_words = (
        "auth", "security", "database", "migration", "delete", "deploy", "drop", "truncate", "secret", "credential", "production",
        "seguridad", "autenticacion", "autenticación", "base de datos", "migracion", "migración", "eliminar", "desplegar", "borrar", "secreto", "credencial", "produccion", "producción"
    )
    if any(word in goal for word in high_risk_words):
        return "high"
    relevant_candidates = [c for c in candidate_scores if int(c.get("score", 0)) >= 4]
    if len(relevant_candidates) >= 8 or any(word in goal for word in ["refactor", "architecture", "cross"]):
        return "medium"
    return "low"


def _detect_stack(payload: dict[str, Any]) -> dict[str, bool]:
    target_files = [str(f).lower().replace("\\", "/") for f in _items(payload, "files")]
    for c in _items(payload, "changes"):
        target_files.append(str(c).lower().replace("\\", "/"))

    cand_scores = _candidate_file_scores(payload)
    for cs in cand_scores:
        if cs.get("score", 0) >= 4:
            target_files.append(str(cs.get("path", "")).lower().replace("\\", "/"))

    target_files = [f for f in target_files if not _is_directory_path(f)]

    if not target_files:
        goal_tokens = {token for token in _goal(payload).lower().replace("/", " ").replace("-", " ").replace("_", " ").split() if len(token) >= 4}
        for item in _git_status_files(payload):
            if p := item.get("path"):
                clean = str(p).lower().replace("\\", "/")
                if any(t in clean for t in goal_tokens) and not _is_directory_path(clean):
                    target_files.append(clean)

    file_str = " ".join(target_files)
    project = str(payload.get("project", "")).lower()

    has_rust = (
        "cargo.toml" in file_str
        or any(f.endswith(".rs") for f in target_files)
        or project == "ozygram"
    )
    has_node = (
        "package.json" in file_str
        or "tsconfig.json" in file_str
        or any(f.endswith((".ts", ".tsx", ".js", ".jsx", ".mjs", ".cjs")) for f in target_files)
    )
    has_python = (
        "pyproject.toml" in file_str
        or "requirements.txt" in file_str
        or "setup.py" in file_str
        or any(f.endswith(".py") for f in target_files)
    )
    has_go = (
        "go.mod" in file_str
        or any(f.endswith(".go") for f in target_files)
    )
    return {
        "rust": has_rust,
        "node": has_node,
        "python": has_python,
        "go": has_go,
    }


def _validation_commands(payload: dict[str, Any]) -> list[str]:
    stack = _detect_stack(payload)
    commands: list[str] = []
    if stack["rust"]:
        commands.append("cargo test")
    if stack["node"]:
        commands.append("npm test (or pnpm test / bun test)")
        commands.append("npm run build (typecheck & asset verification)")
    if stack["python"]:
        commands.append("pytest (or python -m pytest)")
    if stack["go"]:
        commands.append("go test ./...")
    if not commands:
        commands.append("Run project test suite and static typecheck")
    commands.append("Run focused validation for changed files before broad validation")
    return commands


def _authority_recommendation(payload: dict[str, Any]) -> str:
    stack = _detect_stack(payload)
    if stack["rust"] and not (stack["node"] or stack["python"]):
        return "Maintain Rust as the validation authority and test runner."
    if stack["node"] and stack["python"]:
        return "Maintain TypeScript compiler (npm run build) and pytest as the validation authorities."
    if stack["node"]:
        return "Maintain TypeScript compiler and test runner (npm/pnpm test) as the validation authority."
    if stack["python"]:
        return "Maintain Python test suite (pytest) and type checker as the validation authority."
    if stack["rust"]:
        return "Maintain Rust as the validation authority and Python as consultative advisory."
    return "Maintain project toolchain and automated test suite as the validation authority."


def _validation_summary_label(payload: dict[str, Any]) -> str:
    commands = _validation_commands(payload)
    valid_cmds = [cmd.split()[0] for cmd in commands if not cmd.startswith("Run")]
    return " / ".join(valid_cmds) if valid_cmds else "automated test suite"


def _execution_policy(payload: dict[str, Any], autonomy: str = "advisory") -> dict[str, Any]:
    return {
        "mode": autonomy,
        "safe_mode": True,
        "can_do_without_confirmation": [
            "read indexed context and graph summary",
            "rank memories and cluster past lessons",
            "suggest safe MCP tool calls",
            "draft phased plans, risk reviews, and validation checklists",
            "identify and score candidate files",
        ],
        "requires_confirmation": [
            "modify files in workspace",
            "execute destructive shell or git operations",
            "commit or push changes",
            "delete or rewrite project data",
            "execute database migrations",
        ],
        "forbidden_for_python_worker": [
            "delete files directly",
            "make git commits directly",
            "git push to remote directly",
            "execute arbitrary shell commands",
            "modify SQLite DB without Rust authority",
            "perform unvalidated network calls",
            "bypass Rust MCP safety checks",
        ],
        "rust_authority": True,
        "python_role": "advisory_reasoning_worker",
    }



def _brain_context_pack(payload: dict[str, Any]) -> dict[str, Any]:
    git_context = payload.get("git_context") if isinstance(payload.get("git_context"), dict) else {}
    dirty_files = _git_status_files(payload)
    candidate_scores = _candidate_file_scores(payload)
    validation = _validation_commands(payload)
    persistible = [
        "Save final decision or architecture change with ozy_memory action=save and a stable topic_key.",
        "Use ozy_memory action=passive for ## Key Learnings sections after validation.",
    ]
    if dirty_files:
        persistible.append("Record dirty-file baseline if it affects future agent handoff.")
    return {
        "project": _project(payload),
        "goal": _goal(payload),
        "risk_level": _risk_level(payload, dirty_files, candidate_scores),
        "dirty": bool(git_context.get("dirty")) if isinstance(git_context, dict) else bool(dirty_files),
        "dirty_files": dirty_files[:12],
        "candidate_file_scores": candidate_scores,
        "recommended_context_calls": _safe_mcp_calls(payload),
        "recommended_validation": validation,
        "persistible_recommendations": persistible,
    }


def _base_summary(action: str, payload: dict[str, Any]) -> str:
    project = _project(payload)
    goal = _goal(payload)
    files = len(_items(payload, "files"))
    memories = len(_items(payload, "memories"))
    return f"Ozy Brain {action} for {project}: goal='{goal}', files={files}, memories={memories}."


def _base_risks(payload: dict[str, Any]) -> list[str]:
    risks: list[str] = []
    goal = _goal(payload).lower()
    if any(word in goal for word in ["auth", "login", "token", "permission", "security"]):
        risks.append("Security/auth flow may be impacted; require focused tests and no credential logging.")
    if any(word in goal for word in ["migration", "database", "schema", "delete", "bulk"]):
        risks.append("Data or schema change risk; require backup/transaction/verification plan before execution.")
    if any(word in goal for word in ["refactor", "architecture", "module", "cross"]):
        risks.append("Cross-module change risk; run graph impact and limit edits to confirmed files.")
    if not risks:
        risks.append("Context drift risk; verify current repo state before changing files.")
    return risks


def _extract_provenance(payload: dict[str, Any]) -> list[dict[str, Any]]:
    provenance: list[dict[str, Any]] = []
    git_context = payload.get("git_context") if isinstance(payload.get("git_context"), dict) else {}
    recent_commits = git_context.get("recent_commits")
    commit_hash = ""
    if isinstance(recent_commits, list) and len(recent_commits) > 0 and isinstance(recent_commits[0], dict):
        commit_hash = str(recent_commits[0].get("id") or recent_commits[0].get("hash") or "")[:7]

    candidate_scores = _candidate_file_scores(payload, limit=5)
    for cs in candidate_scores:
        provenance.append({
            "path": cs["path"],
            "score": cs["score"],
            "reasons": cs["reasons"],
            "commit_hash": commit_hash,
        })
    return provenance

