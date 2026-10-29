"""
Zero-I/O Speculative AST Patch Validator for OzyBrain.
Validates code replacements in volatile RAM (< 2ms) using native AST parsing
before touching disk or staging Git modifications.
Detects indentation errors, unbalanced brackets, broken scope, and invalid syntax.
"""

from __future__ import annotations

import ast
from dataclasses import dataclass
from typing import Any, Dict, Optional


@dataclass
class AstPreflightResult:
    """Result of volatile memory AST pre-flight verification."""
    is_valid: bool
    error_type: Optional[str] = None
    error_line: Optional[int] = None
    error_message: Optional[str] = None
    veto_directive: Optional[str] = None

    def to_dict(self) -> Dict[str, Any]:
        return {
            "is_valid": self.is_valid,
            "error_type": self.error_type,
            "error_line": self.error_line,
            "error_message": self.error_message,
            "veto_directive": self.veto_directive,
        }


def validate_python_patch_in_memory(
    original_source: str,
    target: str,
    replacement: str,
) -> AstPreflightResult:
    """
    Simulates string replacement in volatile RAM and compiles with ast.parse.
    Returns AstPreflightResult with line number and syntax diagnostics.
    """
    if target not in original_source:
        return AstPreflightResult(
            is_valid=False,
            error_type="TargetNotFoundError",
            error_message="The target text block was not found in original source.",
            veto_directive="[CRITICAL_VETO: REPLACEMENT_TARGET_NOT_FOUND]",
        )

    # Speculative volatile splice
    simulated_source = original_source.replace(target, replacement, 1)

    try:
        ast.parse(simulated_source)
        return AstPreflightResult(is_valid=True)
    except IndentationError as e:
        return AstPreflightResult(
            is_valid=False,
            error_type="IndentationError",
            error_line=e.lineno,
            error_message=f"{e.msg} at line {e.lineno}",
            veto_directive=f"[CRITICAL_VETO: AST_INDENTATION_CORRUPTION] Proposed patch corrupts indentation: {e.msg} (line {e.lineno})",
        )
    except SyntaxError as e:
        return AstPreflightResult(
            is_valid=False,
            error_type="SyntaxError",
            error_line=e.lineno,
            error_message=f"{e.msg} at line {e.lineno}, offset {getattr(e, 'offset', 0)}",
            veto_directive=f"[CRITICAL_VETO: AST_SYNTAX_CORRUPTION] Proposed patch breaks AST syntax: {e.msg} (line {e.lineno})",
        )
    except Exception as e:
        return AstPreflightResult(
            is_valid=False,
            error_type=type(e).__name__,
            error_message=str(e),
            veto_directive=f"[CRITICAL_VETO: AST_PARSER_EXCEPTION] {e}",
        )



import os
import re
import time
from pathlib import Path

_CONTRACT_SKELETON_CACHE: dict[str, tuple[float, list[str]]] = {}
_CACHE_TTL_SECS = 60.0


def extract_file_contract_skeleton(
    file_path: str,
    max_symbols: int = 4,
    workspace_root: str | None = None,
) -> list[str]:
    """
    Extracts exported symbols, interfaces, and function contracts from code files in < 1ms.
    Uses RAM caching with TTL to prevent redundant file system reads during planning.
    Supports Python (.py), TypeScript/JavaScript (.ts, .tsx, .js, .jsx), and Rust (.rs).
    """
    clean_path = str(file_path).strip().replace("\\", "/")
    if not clean_path:
        return []

    now = time.time()
    if clean_path in _CONTRACT_SKELETON_CACHE:
        cached_time, cached_symbols = _CONTRACT_SKELETON_CACHE[clean_path]
        if now - cached_time < _CACHE_TTL_SECS:
            return cached_symbols[:max_symbols]

    # Resolve file on disk
    target_path = Path(clean_path)
    if not target_path.is_file():
        candidates = [
            Path(workspace_root) / clean_path if workspace_root else None,
            Path("ozygram") / clean_path,
            Path.cwd() / clean_path,
        ]
        for base in [Path.cwd()] + list(Path.cwd().parents)[:3]:
            candidates.append(base / clean_path)
            if "python/ozy-brain/" in clean_path:
                candidates.append(base / clean_path.split("python/ozy-brain/")[-1])
        for cand in candidates:
            if cand and cand.is_file():
                target_path = cand
                break

    if not target_path.is_file():
        return []

    try:
        # Read at most 64KB to ensure sub-millisecond execution
        with open(target_path, "r", encoding="utf-8", errors="ignore") as f:
            file_content = f.read(65536)
    except Exception:
        return []

    symbols: list[str] = []
    ext = target_path.suffix.lower()

    if ext == ".py":
        try:
            tree = ast.parse(file_content)
            for node in tree.body:
                if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef, ast.ClassDef)):
                    if not node.name.startswith("_"):
                        symbols.append(node.name)
        except Exception:
            for match in re.finditer(r"^(?:async\s+)?def\s+([a-zA-Z0-9_]+)\b|^class\s+([a-zA-Z0-9_]+)\b", file_content, re.MULTILINE):
                fn_name = match.group(1) or match.group(2)
                if fn_name and not fn_name.startswith("_") and fn_name not in symbols:
                    symbols.append(fn_name)

    elif ext in (".ts", ".tsx", ".js", ".jsx"):
        ts_patterns = [
            r"export\s+(?:async\s+)?function\s+([a-zA-Z0-9_$]+)\b",
            r"export\s+const\s+([a-zA-Z0-9_$]+)\s*=",
            r"export\s+(?:default\s+)?class\s+([a-zA-Z0-9_$]+)\b",
            r"export\s+interface\s+([a-zA-Z0-9_$]+)\b",
            r"export\s+type\s+([a-zA-Z0-9_$]+)\b",
            r"export\s+default\s+function\s+([a-zA-Z0-9_$]+)\b",
        ]
        for pat in ts_patterns:
            for match in re.finditer(pat, file_content):
                sym = match.group(1)
                if sym and sym not in symbols:
                    symbols.append(sym)

    elif ext == ".rs":
        rs_patterns = [
            r"pub\s+(?:async\s+)?fn\s+([a-zA-Z0-9_]+)\b",
            r"pub\s+struct\s+([a-zA-Z0-9_]+)\b",
            r"pub\s+enum\s+([a-zA-Z0-9_]+)\b",
            r"pub\s+trait\s+([a-zA-Z0-9_]+)\b",
        ]
        for pat in rs_patterns:
            for match in re.finditer(pat, file_content):
                sym = match.group(1)
                if sym and sym not in symbols:
                    symbols.append(sym)

    _CONTRACT_SKELETON_CACHE[clean_path] = (now, symbols)
    return symbols[:max_symbols]
