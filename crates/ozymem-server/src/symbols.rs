use anyhow::Result;
use ozymem_core::graph_backend::GraphBackend;
use ozymem_core::mcp_common::{ContentBlock, ToolCallParams, ToolCallResult};
use serde_json::{json, Value};

pub fn handle_get_symbol(
    backend: &GraphBackend,
    tool_call: &ToolCallParams,
) -> Result<ToolCallResult> {
    let file_path = tool_call
        .arguments
        .get("file_path")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("missing 'file_path'"))?;
    let symbol_name = tool_call
        .arguments
        .get("symbol_name")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("missing 'symbol_name'"))?;

    match backend.get_symbol(file_path, symbol_name)? {
        Some(sym) => {
            let json_res = json!({
                "status": "success",
                "symbol": sym
            });
            Ok(ToolCallResult {
                content: vec![ContentBlock {
                    kind: "text",
                    text: serde_json::to_string_pretty(&json_res)?,
                }],
                is_error: None,
            })
        }
        None => {
            Ok(ToolCallResult {
                content: vec![ContentBlock {
                    kind: "text",
                    text: serde_json::to_string_pretty(&json!({
                        "status": "not_found",
                        "message": format!("[ALERT: SYMBOL_NOT_FOUND] Symbol '{}' not found in '{}'", symbol_name, file_path)
                    }))?,
                }],
                is_error: Some(true),
            })
        }
    }
}

pub fn handle_file_tree(
    backend: &GraphBackend,
    tool_call: &ToolCallParams,
) -> Result<ToolCallResult> {
    let file_path = tool_call
        .arguments
        .get("file_path")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("missing 'file_path'"))?;
    let depth = tool_call
        .arguments
        .get("depth")
        .and_then(Value::as_u64)
        .unwrap_or(2) as u32;
    let format = tool_call
        .arguments
        .get("format")
        .and_then(Value::as_str)
        .unwrap_or("text");

    let tree_output = backend.get_file_tree(file_path, depth, format)?;
    Ok(ToolCallResult {
        content: vec![ContentBlock {
            kind: "text",
            text: tree_output,
        }],
        is_error: None,
    })
}

pub fn handle_parse(
    backend: &GraphBackend,
    tool_call: &ToolCallParams,
) -> Result<ToolCallResult> {
    let file_path = tool_call
        .arguments
        .get("file_path")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("missing 'file_path'"))?;

    let resolved = backend.resolve_target_path(file_path).unwrap_or_else(|| file_path.to_string());
    let path_obj = std::path::Path::new(&resolved);
    let effective = if path_obj.exists() {
        path_obj.to_path_buf()
    } else if let Some(root) = backend.project_path() {
        std::path::Path::new(&root).join(&resolved)
    } else {
        std::path::PathBuf::from(&resolved)
    };

    if !effective.exists() {
        return Ok(ToolCallResult {
            content: vec![ContentBlock {
                kind: "text",
                text: serde_json::to_string_pretty(&json!({
                    "status": "error",
                    "error": format!("[ALERT: FILE_NOT_FOUND] File '{}' not found", file_path)
                }))?,
            }],
            is_error: Some(true),
        });
    }

    let source = std::fs::read_to_string(&effective)?;
    let lang = ozymem_parser::SupportedLanguage::from_path(&effective);
    let map = ozymem_parser::parse_source(&resolved, lang, &source)?;
    let hints = ozymem_parser::extract_dependency_hints(&resolved, lang, &source).unwrap_or_default();

    let output = json!({
        "status": "success",
        "file_path": resolved,
        "language": map.language,
        "strategy": map.strategy.as_str(),
        "functions": map.functions,
        "dependency_hints": hints,
    });

    Ok(ToolCallResult {
        content: vec![ContentBlock {
            kind: "text",
            text: serde_json::to_string_pretty(&output)?,
        }],
        is_error: None,
    })
}

pub fn handle_replace_symbol(
    backend: &GraphBackend,
    tool_call: &ToolCallParams,
) -> Result<ToolCallResult> {
    let file_path = tool_call
        .arguments
        .get("file_path")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("missing 'file_path'"))?;
    let symbol_name = tool_call
        .arguments
        .get("symbol_name")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("missing 'symbol_name'"))?;
    let new_code = tool_call
        .arguments
        .get("new_code")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("missing 'new_code'"))?;
    let dry_run = tool_call
        .arguments
        .get("dry_run")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let res = backend.replace_symbol(file_path, symbol_name, new_code, dry_run)?;
    let is_err = if res.success { None } else { Some(true) };

    Ok(ToolCallResult {
        content: vec![ContentBlock {
            kind: "text",
            text: serde_json::to_string_pretty(&res)?,
        }],
        is_error: is_err,
    })
}

pub fn handle_diagnostics_quick(
    backend: &GraphBackend,
    tool_call: &ToolCallParams,
) -> Result<ToolCallResult> {
    let file_path = tool_call
        .arguments
        .get("file_path")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("missing 'file_path'"))?;
    let source_code_opt = tool_call
        .arguments
        .get("source_code")
        .and_then(Value::as_str);
    let lang_override = tool_call
        .arguments
        .get("language")
        .and_then(Value::as_str);

    let resolved = backend.resolve_target_path(file_path).unwrap_or_else(|| file_path.to_string());
    let path_obj = std::path::Path::new(&resolved);
    let effective = if path_obj.exists() {
        path_obj.to_path_buf()
    } else if let Some(root) = backend.project_path() {
        std::path::Path::new(&root).join(&resolved)
    } else {
        std::path::PathBuf::from(&resolved)
    };

    let source = if let Some(sc) = source_code_opt {
        sc.to_string()
    } else {
        if !effective.exists() {
            return Ok(ToolCallResult {
                content: vec![ContentBlock {
                    kind: "text",
                    text: serde_json::to_string_pretty(&json!({
                        "status": "error",
                        "error": format!("[ALERT: FILE_NOT_FOUND] File '{}' not found", file_path)
                    }))?,
                }],
                is_error: Some(true),
            });
        }
        std::fs::read_to_string(&effective)?
    };

    let lang = if let Some(lo) = lang_override {
        match lo.to_lowercase().as_str() {
            "python" | "py" => ozymem_parser::SupportedLanguage::Python,
            "typescript" | "ts" => ozymem_parser::SupportedLanguage::TypeScript,
            "tsx" | "typescriptreact" => ozymem_parser::SupportedLanguage::TypeScriptReact,
            "javascript" | "js" | "jsx" => ozymem_parser::SupportedLanguage::JavaScript,
            "rust" | "rs" => ozymem_parser::SupportedLanguage::Rust,
            "go" => ozymem_parser::SupportedLanguage::Go,
            _ => ozymem_parser::SupportedLanguage::from_path(&effective),
        }
    } else {
        ozymem_parser::SupportedLanguage::from_path(&effective)
    };

    let diagnostics = ozymem_parser::extract_ast_diagnostics(&resolved, lang, &source);
    let output = json!({
        "status": if diagnostics.is_empty() { "clean" } else { "has_errors" },
        "file_path": resolved,
        "language": format!("{:?}", lang),
        "is_valid": diagnostics.is_empty(),
        "error_count": diagnostics.len(),
        "diagnostics": diagnostics,
    });

    Ok(ToolCallResult {
        content: vec![ContentBlock {
            kind: "text",
            text: serde_json::to_string_pretty(&output)?,
        }],
        is_error: None,
    })
}

pub fn handle_search_literal(
    backend: &GraphBackend,
    tool_call: &ToolCallParams,
) -> Result<ToolCallResult> {
    let query = tool_call
        .arguments
        .get("query")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("missing 'query'"))?;
    let path_prefix = tool_call
        .arguments
        .get("path_prefix")
        .and_then(Value::as_str);
    let is_regex = tool_call
        .arguments
        .get("is_regex")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let case_sensitive = tool_call
        .arguments
        .get("case_sensitive")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let token_budget = tool_call
        .arguments
        .get("token_budget")
        .and_then(Value::as_u64)
        .unwrap_or(800) as usize;
    let max_matches = tool_call
        .arguments
        .get("max_matches")
        .and_then(Value::as_u64)
        .unwrap_or(25) as usize;
    let context_lines = tool_call
        .arguments
        .get("context_lines")
        .and_then(Value::as_u64)
        .unwrap_or(0) as usize;

    let res = backend.search_literal(
        query,
        path_prefix,
        is_regex,
        case_sensitive,
        token_budget,
        max_matches,
        context_lines,
    )?;

    Ok(ToolCallResult {
        content: vec![ContentBlock {
            kind: "text",
            text: serde_json::to_string_pretty(&res)?,
        }],
        is_error: None,
    })
}

pub fn handle_find_references(
    backend: &GraphBackend,
    tool_call: &ToolCallParams,
) -> Result<ToolCallResult> {
    let symbol_name = tool_call
        .arguments
        .get("symbol_name")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("missing 'symbol_name'"))?;
    let file_path = tool_call
        .arguments
        .get("file_path")
        .and_then(Value::as_str);
    let max_references = tool_call
        .arguments
        .get("max_references")
        .and_then(Value::as_u64)
        .unwrap_or(50) as usize;
    let token_budget = tool_call
        .arguments
        .get("token_budget")
        .and_then(Value::as_u64)
        .unwrap_or(1000) as usize;

    let res = backend.find_symbol_references(
        symbol_name,
        file_path,
        max_references,
        token_budget,
    )?;

    Ok(ToolCallResult {
        content: vec![ContentBlock {
            kind: "text",
            text: serde_json::to_string_pretty(&res)?,
        }],
        is_error: None,
    })
}
