from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Iterable
import re


BRAIN_VERSION = "0.2.0"
BRAIN_SCHEMA_VERSION = "v1"

DOMAIN_SYNONYMS: dict[str, list[str]] = {
    # Informes & Documentos
    "informe": ["report", "reports"],
    "informes": ["report", "reports"],
    "report": ["informe", "informes"],
    "reports": ["informe", "informes"],
    "documento": ["document", "doc", "pdf"],
    "documentos": ["documents", "docs", "pdfs"],
    "pdf": ["pdf", "document", "doc", "report"],
    "doc": ["pdf", "document", "docqr"],
    "numero": ["number", "num", "code", "cod"],
    "número": ["number", "num", "code", "cod"],
    "codigo": ["code", "cod"],
    "código": ["code", "cod"],
    # Búsqueda y Seguimiento
    "busqueda": ["search", "tracing"],
    "búsqueda": ["search", "tracing"],
    "seguimiento": ["tracing", "trace", "tracking"],
    "trazabilidad": ["tracing", "trace"],
    "tracing": ["seguimiento", "trazabilidad"],
    "search": ["busqueda", "buscar"],
    # Acciones con Archivos
    "descarga": ["download", "export"],
    "descargar": ["download", "export"],
    "download": ["descarga", "descargar"],
    "subir": ["upload", "attach", "drop", "drag"],
    "subida": ["upload", "attach"],
    "upload": ["subir", "adjuntar", "attach"],
    "attach": ["adjuntar", "subir", "upload"],
    "adjuntar": ["attach", "subir", "upload"],
    "cambiar": ["replace", "change", "upload", "edit", "custom"],
    "reemplazar": ["replace", "change"],
    "modificar": ["modify", "edit", "change"],
    "editar": ["edit", "custom", "update"],
    "edit": ["editar", "modificar"],
    "custom": ["personalizado", "editar", "custom"],
    "drag": ["drag", "drop", "uploader", "file", "upload"],
    "drop": ["drag", "drop", "uploader", "upload"],
    # UI y Tablas
    "tabla": ["table", "grid"],
    "table": ["tabla"],
    "grid": ["tabla"],
    "modal": ["modal", "dialog"],
    "dialog": ["modal", "dialog"],
    "cuadro": ["modal", "dialog"],
    # Dominio CRM / Geofal
    "recepcion": ["reception"],
    "recepción": ["reception"],
    "reception": ["recepcion"],
    "probeta": ["sample", "specimen", "probeta"],
    "probetas": ["samples", "specimens", "probetas"],
    "muestra": ["sample"],
    "muestras": ["samples"],
    "concreto": ["concrete", "compresion"],
    "rotura": ["break", "failure"],
    "ensayo": ["test", "assay"],
    "ensayos": ["tests", "assays"],
    "firmado": ["signed", "signature", "stamp"],
    "firma": ["sign", "signature"],
}


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


def _completed_phases(payload: dict[str, Any]) -> list[str]:
    raw = payload.get("completed_phases") or payload.get("done_phases") or []
    return [str(p) for p in raw if p]


def _current_phase(payload: dict[str, Any]) -> str:
    return _text(payload, "current_phase") or _text(payload, "active_phase") or "context"


def _working_edits(payload: dict[str, Any]) -> list[dict[str, Any]]:
    raw = payload.get("working_edits") or payload.get("in_flight_edits") or payload.get("diffs") or []
    return [e for e in raw if isinstance(e, dict)]


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


def _clean_path(path: str) -> str:
    p = str(path).strip()
    if (p.startswith('"') and p.endswith('"')) or (p.startswith("'") and p.endswith("'")):
        p = p[1:-1].strip()
    return p.replace("\\", "/")


CODE_EXTENSIONS = (
    ".py", ".rs", ".ts", ".tsx", ".js", ".jsx", ".mjs", ".cjs",
    ".sql", ".toml", ".yaml", ".yml", ".prisma", ".sh", ".bash", ".ps1",
    ".html", ".css", ".scss", ".json",
)

EXCLUDED_LOCKFILES_AND_SCHEMAS = (
    "package-lock.json", "cargo.lock", "yarn.lock", "pnpm-lock.yaml", "poetry.lock", "composer.lock",
)


def _is_code_file(path: str, allow_markdown: bool = False) -> bool:
    clean = _clean_path(path).lower()
    last = clean.split("/")[-1]

    # Excluir schemas de herramientas de Ozygram (ozy_*.json)
    if last.startswith("ozy_") and last.endswith(".json"):
        return False

    # Excluir lockfiles
    if last in EXCLUDED_LOCKFILES_AND_SCHEMAS:
        return False

    # Exclude dotfiles, environment templates, and ignore files from code candidates
    if last.startswith(".") and not clean.endswith((".py", ".rs", ".ts", ".tsx", ".js", ".jsx")):
        return False
    if last.startswith(".env") or last.startswith(".git") or last in (".dockerignore", ".editorconfig", ".prettierrc", ".gitattributes", ".gitignore"):
        return False

    # Archivos de configuración especiales en raíz
    if last in ("dockerfile", "makefile", "procfile"):
        return True

    if allow_markdown and clean.endswith((".md", ".txt", ".rst")):
        return True

    return clean.endswith(CODE_EXTENSIONS)


def _is_noise_path(path: str) -> bool:
    clean = _clean_path(path).lower()
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
        "/downloads/",
        "/recordings/",
        "/grabaciones/",
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
        ".7z",
        ".rar",
        # Multimedia
        ".mp4",
        ".mov",
        ".avi",
        ".mkv",
        ".webm",
        ".flv",
        ".mp3",
        ".wav",
        ".aac",
        ".ogg",
        # Ofimática no plana
        ".docx",
        ".xlsx",
        ".pptx",
        ".csv",
    )
    if clean.endswith(noise_exts):
        return True
    parts = clean.split("/")
    last = parts[-1]
    if len(last) >= 32 and all(c in "0123456789abcdef" for c in last):
        return True
    return False


def _is_directory_path(path: str) -> bool:
    import os
    clean = _clean_path(path)
    if clean.endswith("/"):
        return True
    if os.path.isdir(clean):
        return True
    last = clean.split("/")[-1]
    if not last or ("." not in last and last.lower() not in ("dockerfile", "makefile", "license", "procfile")):
        return True
    return False


def _candidate_files(payload: dict[str, Any], limit: int = 8) -> list[str]:
    files: list[str] = []
    goal = _goal(payload).lower()
    is_doc_goal = any(tok in goal for tok in ("docs", "documentation", "readme", "guide", "rule", "markdown", "agent"))
    for item in _items(payload, "files"):
        text = _clean_path(str(item))
        if _is_noise_path(text) or _is_directory_path(text) or not _is_code_file(text, allow_markdown=is_doc_goal):
            continue
        lower = text.lower()
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


def _vector_semantic_scores(goal: str) -> list[dict[str, Any]]:
    """Queries local ChromaDB vector store for semantic code AST and heuristic matches."""
    if not goal or len(goal.strip()) < 3:
        return []
    try:
        from ozy_brain.vector_store import VectorMemoryStore, HAS_CHROMADB
        if not HAS_CHROMADB:
            return []
        store = VectorMemoryStore()
        if not store.is_available():
            return []

        results: list[dict[str, Any]] = []
        for coll_name in ("code_graph_ast", "semantic_heuristics"):
            matches = store.search_similar(goal, limit=6, collection_name=coll_name)
            for m in matches:
                meta = m.get("metadata") or {}
                fpath = meta.get("file_path") or meta.get("path") or meta.get("target_files")
                sim = float(m.get("similarity_score", 0.0))
                if fpath and sim >= 0.65:
                    results.append({
                        "file_path": str(fpath),
                        "score": max(5, int(round(sim * 8))),
                        "similarity": sim,
                        "reason": f"vector similarity in {coll_name} ({sim:.2f})",
                    })
        store.close()
        return results
    except Exception:
        return []


def _candidate_file_scores(payload: dict[str, Any], limit: int = 10) -> list[dict[str, Any]]:
    stop_words = {
        "de", "en", "la", "el", "al", "un", "una", "los", "las", "del", "por", "para", "con",
        "to", "in", "on", "at", "is", "by", "or", "if", "an", "the", "and", "for", "with", "from",
        "se", "ya", "es", "su", "sus", "tu", "tus", "mi", "mis", "as"
    }
    raw_tokens = {
        token
        for token in _goal(payload).lower().replace("/", " ").replace("-", " ").replace("_", " ").split()
        if len(token) >= 2 and token not in stop_words
    }
    for ct in re.findall(r"[A-Z]?[a-z]+|[A-Z]+(?=[A-Z][a-z]|\b)", _goal(payload)):
        if len(ct) >= 3 and ct.lower() not in stop_words:
            raw_tokens.add(ct.lower())

    def _stems(tok: str) -> set[str]:
        res = {tok}
        if tok.endswith("es") and len(tok) > 4:
            res.add(tok[:-2])
        elif tok.endswith("s") and len(tok) > 3:
            res.add(tok[:-1])
        if tok.endswith("cion") and len(tok) > 5:
            res.add(tok[:-4])
        elif tok.endswith("tion") and len(tok) > 5:
            res.add(tok[:-4])
        return res

    goal_tokens: set[str] = set()
    for token in raw_tokens:
        goal_tokens.update(_stems(token))
        if token in DOMAIN_SYNONYMS:
            for syn in DOMAIN_SYNONYMS[token]:
                goal_tokens.update(_stems(syn))

    is_doc_goal = any(tok in goal_tokens for tok in ("docs", "documentation", "readme", "guide", "rule", "markdown", "agent"))
    scores: dict[str, dict[str, Any]] = {}

    def bump(path: str, points: int, reason: str) -> None:
        clean = _clean_path(path)
        entry = scores.setdefault(clean, {"path": clean, "score": 0, "reasons": []})
        entry["score"] += points
        if reason not in entry["reasons"]:
            entry["reasons"].append(reason)

    for path in _items(payload, "files"):
        text = _clean_path(str(path))
        if _is_noise_path(text) or _is_directory_path(text) or not _is_code_file(text, allow_markdown=is_doc_goal):
            continue
        clean = text
        if not is_doc_goal and (clean.endswith((".md", ".txt", ".rst")) or "/.agents/" in clean):
            continue
        lower = clean.lower()
        overlap = sorted(token for token in goal_tokens if token in lower)
        if overlap:
            bump(text, 3 + min(len(overlap), 4), f"matches {', '.join(overlap[:3])}")

    # Topological graph impact neighbors
    for entry in _items(payload, "impact"):
        if isinstance(entry, dict):
            fpath = entry.get("file_path") or entry.get("target") or entry.get("path")
            if fpath:
                clean_fpath = _clean_path(str(fpath))
                if clean_fpath and _is_code_file(clean_fpath, allow_markdown=is_doc_goal) and not _is_noise_path(clean_fpath) and not _is_directory_path(clean_fpath):
                    if not is_doc_goal and (clean_fpath.endswith((".md", ".txt", ".rst")) or "/.agents/" in clean_fpath):
                        continue
                    depth = int(entry.get("depth", 1))
                    reason = str(entry.get("reason", "graph neighbor"))
                    pts = 6 if depth <= 1 else 4
                    bump(clean_fpath, pts, f"graph connected depth {depth} ({reason})")

    # Vector semantic matches from ChromaDB (concept & intent similarity)
    for v_match in _vector_semantic_scores(_goal(payload)):
        fpath = _clean_path(v_match["file_path"])
        if fpath and _is_code_file(fpath, allow_markdown=is_doc_goal) and not _is_noise_path(fpath) and not _is_directory_path(fpath):
            if not is_doc_goal and (fpath.endswith((".md", ".txt", ".rst")) or "/.agents/" in fpath):
                continue
            bump(fpath, v_match["score"], v_match["reason"])

    # Semantic lesson candidates sent from Rust
    for cand in _items(payload, "candidates"):
        if isinstance(cand, dict):
            fpath = cand.get("file_path")
            if fpath:
                clean_fpath = _clean_path(str(fpath))
                if clean_fpath and _is_code_file(clean_fpath, allow_markdown=is_doc_goal) and not _is_noise_path(clean_fpath) and not _is_directory_path(clean_fpath):
                    sim = float(cand.get("similarity_score", 0.7))
                    pts = max(5, int(round(sim * 7)))
                    bump(clean_fpath, pts, f"semantic candidate ({sim:.2f})")

    project_name = str(payload.get("project", "")).lower()
    project_tokens = {project_name} if project_name else set()
    if len(project_name) >= 3:
        project_tokens.add(project_name[:3])

    for item in _git_status_files(payload):
        raw_path = str(item.get("path", "")).strip()
        status = str(item.get("status", "")).strip()
        path = _clean_path(raw_path)
        if path and _is_code_file(path, allow_markdown=is_doc_goal) and not _is_noise_path(path) and not _is_directory_path(path):
            if not is_doc_goal and (path.endswith((".md", ".txt", ".rst")) or "/.agents/" in path):
                continue
            lower = path.lower()
            overlap = sorted(token for token in goal_tokens if token in lower)
            overlap_specific = [t for t in overlap if t not in project_tokens]
            if overlap_specific:
                base_pts = 5 if status in ("M", "A") else 2
                bump(path, base_pts + min(len(overlap_specific), 3) * 2, f"dirty git status {status} with goal overlap ({', '.join(overlap_specific)})")
            elif overlap:
                bump(path, 1, f"dirty git status {status} weak/project overlap ({', '.join(overlap)})")
            elif any(c.lower() in lower for c in _items(payload, "changes")):
                bump(path, 5, f"dirty git status {status} in explicit changes")

    for change in _items(payload, "changes"):
        text = _clean_path(str(change))
        if ("/" in text or "." in text) and not _is_directory_path(text) and _is_code_file(text, allow_markdown=is_doc_goal):
            bump(text, 5, "explicit change input")

    for memory in _items(payload, "relevant_memories") + _items(payload, "relevant_lessons"):
        if isinstance(memory, dict):
            # 1. Extraer linked_files y file_path explícitos
            for k in ("linked_files", "file_path", "target_files"):
                val = memory.get(k)
                items_to_check = val if isinstance(val, list) else [val] if val else []
                for item in items_to_check:
                    clean_item = _clean_path(str(item))
                    if clean_item and _is_code_file(clean_item, allow_markdown=is_doc_goal) and not _is_noise_path(clean_item) and not _is_directory_path(clean_item):
                        bump(clean_item, 6, "linked file from relevant memory")

            # 2. Escaneo de menciones textuales en contenido de memoria
            haystack = " ".join(str(memory.get(k, "")) for k in ("title", "content", "file_path", "error_context", "solution", "tags")).lower()
            for path in list(scores.keys()):
                name = path.split("/")[-1].lower()
                if name and len(name) >= 5 and name in haystack:
                    bump(path, 4, f"referenced in memory ({name})")

            # 3. Verificar si algún candidato explícito aparece mencionado en el texto de la memoria
            for path in _items(payload, "files"):
                clean_path_str = _clean_path(str(path))
                name = clean_path_str.split("/")[-1].lower()
                if name and len(name) >= 5 and name in haystack and not _is_noise_path(clean_path_str) and not _is_directory_path(clean_path_str) and _is_code_file(clean_path_str, allow_markdown=is_doc_goal):
                    bump(clean_path_str, 5, f"candidate mentioned in memory text ({name})")

    # Ensure we include primary files if fewer than 3 scored candidates
    if len(scores) < min(3, len(_items(payload, "files"))):
        for path in _items(payload, "files"):
            text = _clean_path(str(path))
            if text not in scores and not _is_noise_path(text) and not _is_directory_path(text) and _is_code_file(text, allow_markdown=is_doc_goal):
                bump(text, 1, "indexed fallback candidate")
                if len(scores) >= 3:
                    break

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

