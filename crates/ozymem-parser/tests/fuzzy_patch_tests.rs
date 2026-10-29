// Copyright (c) 2026 Ozygram Project. All rights reserved.
// Unit and integration tests for Fuzzy AST Block Patching Engine.
// Zero-Emoji Standard compliant.

use ozymem_parser::patching::{apply_fuzzy_ast_patch, find_target_block, normalize_fuzzy_line};

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
