// Copyright (c) 2026 Ozygram Project. All rights reserved.
// Unit and integration tests for Fuzzy AST Block Patching Engine.
// Zero-Emoji Standard compliant.

use ozymem_parser::patching::{apply_fuzzy_ast_patch, find_target_block, is_dominant_crlf, normalize_fuzzy_line};

#[test]
fn test_normalize_fuzzy_line() {
    assert_eq!(normalize_fuzzy_line("   def  foo( x,   y ):  "), "def foo(x,y):");
    assert_eq!(normalize_fuzzy_line("\t\tconst   a = 1;\r"), "const a=1;");
    assert_eq!(normalize_fuzzy_line(""), "");
}

#[test]
fn test_exact_and_fuzzy_block_finding() {
    let source = "def main():\n    print('start')\n    val = calculate(10)\n    return val\n";

    // 1. Exact match
    let target_exact = "    val = calculate(10)\n    return val";
    let (s, e, kind) = find_target_block(source, target_exact).expect("Must find exact block");
    assert_eq!(s, 3);
    assert_eq!(e, 4);
    assert_eq!(kind, "exact");

    // 2. Fuzzy match with altered whitespace and indentation
    let target_fuzzy = "val  =   calculate( 10 )\nreturn   val";
    let (s, e, kind) = find_target_block(source, target_fuzzy).expect("Must find fuzzy block");
    assert_eq!(s, 3);
    assert_eq!(e, 4);
    assert_eq!(kind, "fuzzy_whitespace");
}

#[test]
fn test_auto_indentation_alignment() {
    let source = "class UserService:\n    def get_user(self, user_id):\n        user = db.find(user_id)\n        return user\n";
    let target = "user = db.find(user_id)\nreturn user";
    
    // Replacement has NO indentation (0 spaces)
    let replacement = "user = db.find(user_id)\nif not user:\n    raise NotFound()\nreturn user";

    let res = apply_fuzzy_ast_patch("service.py", source, target, replacement, false)
        .expect("Patch must apply successfully");

    assert!(res.success);
    assert!(res.ast_verified);
    assert_eq!(res.matched_by, "fuzzy_whitespace");

    let patched = res.patched_content.expect("Patched content must exist");
    // Verify that replacement received 8 spaces base indentation
    assert!(patched.contains("        user = db.find(user_id)"));
    assert!(patched.contains("        if not user:"));
    assert!(patched.contains("            raise NotFound()"));
    assert!(patched.contains("        return user"));
}

#[test]
fn test_crlf_preservation() {
    let source = "function greet() {\r\n    console.log('hi');\r\n}\r\n";
    let target = "console.log('hi');";
    let replacement = "console.log('hello world');";

    let res = apply_fuzzy_ast_patch("greet.ts", source, target, replacement, false)
        .expect("Patch must apply successfully");

    assert!(res.success);
    let patched = res.patched_content.expect("Patched content must exist");
    assert!(patched.contains("\r\n"), "Must preserve CRLF line endings");
    assert!(patched.contains("console.log('hello world');"));
}

#[test]
fn test_multiline_crlf_source_with_lf_target_and_replacement() {
    let source = "def process_data(items):\r\n    total = 0\r\n    for item in items:\r\n        total += item\r\n    return total\r\n";
    let target = "    for item in items:\n        total += item";
    let replacement = "    for item in items:\n        if item > 0:\n            total += item";

    let res = apply_fuzzy_ast_patch("process.py", source, target, replacement, false)
        .expect("Patch must apply across CRLF/LF boundary");

    assert!(res.success);
    assert_eq!(res.matched_by, "exact");
    assert_eq!(res.start_line, 3);
    assert_eq!(res.end_line, 4);

    let patched = res.patched_content.expect("Patched content must exist");
    assert!(patched.contains("\r\n"), "Output must preserve dominant CRLF");
    assert!(!patched.contains("\r\r\n"), "Must never create double CR carriage returns");
    assert!(patched.contains("        if item > 0:\r\n            total += item"));
}

#[test]
fn test_mixed_line_endings_matching_and_normalization() {
    let source = "header = True\r\nstep1 = 'done'\nstep2 = 'done'\r\nreturn step2\n";
    let target = "step1 = 'done'\nstep2 = 'done'";
    let replacement = "step1 = 'ready'\nstep2 = 'ready'";

    let res = apply_fuzzy_ast_patch("pipeline.py", source, target, replacement, false)
        .expect("Patch must handle mixed EOL cleanly");

    assert!(res.success);
    let patched = res.patched_content.expect("Patched content must exist");
    assert!(patched.contains("step1 = 'ready'"));
    assert!(patched.contains("step2 = 'ready'"));
}

#[test]
fn test_is_dominant_crlf_detector() {
    assert!(is_dominant_crlf("line1\r\nline2\r\nline3\r\n"));
    assert!(is_dominant_crlf("line1\r\nline2\nline3\r\n"));
    assert!(!is_dominant_crlf("line1\nline2\nline3\n"));
    assert!(!is_dominant_crlf("line1\nline2\r\nline3\n"));
    assert!(!is_dominant_crlf(""));
}

#[test]
fn test_ast_syntax_verification_detects_malformed_code() {
    let source = "def calculate(a, b):\n    return a + b\n";
    let target = "return a + b";
    
    // Malformed syntax: missing colon and closing paren
    let bad_replacement = "if a >: return";

    let res = apply_fuzzy_ast_patch("calc.py", source, target, bad_replacement, false)
        .expect("Patch application executes");

    assert!(res.success);
    assert_eq!(res.ast_verified, false, "Must detect syntax error in patched AST");
}

#[test]
fn test_real_crmnew_file_fuzzy_patch() {
    let source = r#""""
Main FastAPI application entry point.
"""
from __future__ import annotations

import logging
import os
from pathlib import Path

from dotenv import load_dotenv
from fastapi import FastAPI, Request, HTTPException, Depends

load_dotenv(Path(__file__).resolve().parents[1] / ".env", override=False)

from app.auth import JWTAuthMiddleware, get_current_user, current_actor
"#;

    // Target with spaces around parentheses and operators (common LLM hallucinated whitespace)
    let fuzzy_target = r#"load_dotenv( Path( __file__ ).resolve().parents[ 1 ] / ".env" , override=False )"#;
    let replacement = r#"# Ensure local environment variables are loaded
load_dotenv(Path(__file__).resolve().parents[1] / ".env", override=False)"#;

    let res = apply_fuzzy_ast_patch("crmnew/api-geofal-crm/app/main.py", source, fuzzy_target, replacement, false)
        .expect("Must patch crmnew code successfully");

    assert!(res.success);
    assert_eq!(res.matched_by, "fuzzy_whitespace");
    assert!(res.ast_verified);
    let patched = res.patched_content.unwrap();
    assert!(patched.contains("# Ensure local environment variables are loaded"));
}
