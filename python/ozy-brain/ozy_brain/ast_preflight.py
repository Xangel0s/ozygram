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
