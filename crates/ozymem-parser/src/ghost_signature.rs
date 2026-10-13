use crate::SupportedLanguage;
use regex::Regex;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FunctionParam {
    pub name: String,
    pub type_annotation: Option<String>,
    pub has_default: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FunctionSignature {
    pub name: String,
    pub parameters: Vec<FunctionParam>,
    pub required_param_count: usize,
    pub total_param_count: usize,
    pub is_async: bool,
    pub return_type: Option<String>,
}

/// Parses candidate or existing function source to extract its complete signature and arity.
pub fn parse_function_signature(
    source: &str,
    target_symbol: Option<&str>,
    lang: SupportedLanguage,
) -> Option<FunctionSignature> {
    match lang {
        SupportedLanguage::Python => parse_python_signature(source, target_symbol),
        SupportedLanguage::TypeScript
        | SupportedLanguage::TypeScriptReact
        | SupportedLanguage::JavaScript => parse_ts_js_signature(source, target_symbol),
        SupportedLanguage::Rust => parse_rust_signature(source, target_symbol),
        _ => parse_heuristic_signature(source, target_symbol),
    }
}

/// Parses a Python function declaration: `(async )?def name(param1, param2=val) -> ret:`
fn parse_python_signature(source: &str, target_symbol: Option<&str>) -> Option<FunctionSignature> {
    let fn_re = Regex::new(
        r#"(?m)^\s*(async\s+)?def\s+([a-zA-Z_][a-zA-Z0-9_]*)\s*\(([\s\S]*?)\)\s*(?:->\s*([^:]+))?:"#
    ).ok()?;

    for caps in fn_re.captures_iter(source) {
        let is_async = caps.get(1).is_some();
        let name = caps.get(2)?.as_str().to_string();

        if let Some(target) = target_symbol {
            if name != target {
                continue;
            }
        }

        let raw_params = caps.get(3).map(|m| m.as_str()).unwrap_or("");
        let return_type = caps.get(4).map(|m| m.as_str().trim().to_string());

        let mut parameters = Vec::new();
        let mut required_count = 0;

        for part in split_top_level_commas(raw_params) {
            let trimmed = part.trim();
            if trimmed.is_empty() || trimmed == "self" || trimmed == "cls" {
                continue;
            }

            let has_default = trimmed.contains('=');
            let (param_name, type_ann) = parse_python_param_name_and_type(trimmed);

            if !has_default && !param_name.starts_with('*') {
                required_count += 1;
            }

            parameters.push(FunctionParam {
                name: param_name,
                type_annotation: type_ann,
                has_default,
            });
        }

        let total_count = parameters.len();
        return Some(FunctionSignature {
            name,
            parameters,
            required_param_count: required_count,
            total_param_count: total_count,
            is_async,
            return_type,
        });
    }

    None
}

fn parse_python_param_name_and_type(raw: &str) -> (String, Option<String>) {
    let without_default = raw.split('=').next().unwrap_or(raw).trim();
    if let Some((name, ty)) = without_default.split_once(':') {
        (name.trim().to_string(), Some(ty.trim().to_string()))
    } else {
        (without_default.to_string(), None)
    }
}

/// Parses TypeScript / JavaScript function declarations or arrow functions:
/// `(export )?(async )?function name(p1: type, p2 = val): ret`
fn parse_ts_js_signature(source: &str, target_symbol: Option<&str>) -> Option<FunctionSignature> {
    let fn_re = Regex::new(
        r#"(?m)^\s*(?:export\s+)?(?:default\s+)?(async\s+)?function\s+([a-zA-Z_][a-zA-Z0-9_]*)\s*\(([\s\S]*?)\)\s*(?::\s*([^{]+))?\{"#
    ).ok()?;

    for caps in fn_re.captures_iter(source) {
        let is_async = caps.get(1).is_some();
        let name = caps.get(2)?.as_str().to_string();

        if let Some(target) = target_symbol {
            if name != target {
                continue;
            }
        }

        let raw_params = caps.get(3).map(|m| m.as_str()).unwrap_or("");
        let return_type = caps.get(4).map(|m| m.as_str().trim().to_string());

        let mut parameters = Vec::new();
        let mut required_count = 0;

        for part in split_top_level_commas(raw_params) {
            let trimmed = part.trim();
            if trimmed.is_empty() {
                continue;
            }

            let has_default = trimmed.contains('=') || trimmed.contains('?');
            let (param_name, type_ann) = parse_ts_param_name_and_type(trimmed);

            if !has_default && !param_name.starts_with("...") {
                required_count += 1;
            }

            parameters.push(FunctionParam {
                name: param_name,
                type_annotation: type_ann,
                has_default,
            });
        }

        let total_count = parameters.len();
        return Some(FunctionSignature {
            name,
            parameters,
            required_param_count: required_count,
            total_param_count: total_count,
            is_async,
            return_type,
        });
    }

    // Arrow function fallback: `const name = (async )?(...) =>`
    let arrow_re = Regex::new(
        r#"(?m)^\s*(?:export\s+)?(?:const|let|var)\s+([a-zA-Z_][a-zA-Z0-9_]*)\s*=\s*(async\s+)?\(([\s\S]*?)\)\s*(?::\s*([^=]+))?=>"#
    ).ok()?;

    for caps in arrow_re.captures_iter(source) {
        let name = caps.get(1)?.as_str().to_string();
        let is_async = caps.get(2).is_some();

        if let Some(target) = target_symbol {
            if name != target {
                continue;
            }
        }

        let raw_params = caps.get(3).map(|m| m.as_str()).unwrap_or("");
        let return_type = caps.get(4).map(|m| m.as_str().trim().to_string());

        let mut parameters = Vec::new();
        let mut required_count = 0;

        for part in split_top_level_commas(raw_params) {
            let trimmed = part.trim();
            if trimmed.is_empty() {
                continue;
            }

            let has_default = trimmed.contains('=') || trimmed.contains('?');
            let (param_name, type_ann) = parse_ts_param_name_and_type(trimmed);

            if !has_default && !param_name.starts_with("...") {
                required_count += 1;
            }

            parameters.push(FunctionParam {
                name: param_name,
                type_annotation: type_ann,
                has_default,
            });
        }

        let total_count = parameters.len();
        return Some(FunctionSignature {
            name,
            parameters,
            required_param_count: required_count,
            total_param_count: total_count,
            is_async,
            return_type,
        });
    }

    None
}

fn parse_ts_param_name_and_type(raw: &str) -> (String, Option<String>) {
    let without_default = raw.split('=').next().unwrap_or(raw).trim();
    if let Some((name, ty)) = without_default.split_once(':') {
        let clean_name = name.trim().trim_end_matches('?').trim().to_string();
        (clean_name, Some(ty.trim().to_string()))
    } else {
        (without_default.trim_end_matches('?').trim().to_string(), None)
    }
}

/// Parses Rust function signatures: `(pub )?(async )?fn name(p1: type, p2: type) -> ret`
fn parse_rust_signature(source: &str, target_symbol: Option<&str>) -> Option<FunctionSignature> {
    let fn_re = Regex::new(
        r#"(?m)^\s*(?:pub(?:\([^)]+\))?\s+)?(async\s+)?fn\s+([a-zA-Z_][a-zA-Z0-9_]*)\s*(?:<[^>]+>)?\s*\(([\s\S]*?)\)\s*(?:->\s*([^{]+))?\{"#
    ).ok()?;

    for caps in fn_re.captures_iter(source) {
        let is_async = caps.get(1).is_some();
        let name = caps.get(2)?.as_str().to_string();

        if let Some(target) = target_symbol {
            if name != target {
                continue;
            }
        }

        let raw_params = caps.get(3).map(|m| m.as_str()).unwrap_or("");
        let return_type = caps.get(4).map(|m| m.as_str().trim().to_string());

        let mut parameters = Vec::new();
        let mut required_count = 0;

        for part in split_top_level_commas(raw_params) {
            let trimmed = part.trim();
            if trimmed.is_empty() || trimmed == "&self" || trimmed == "&mut self" || trimmed == "self" {
                continue;
            }

            let (param_name, type_ann) = if let Some((n, ty)) = trimmed.split_once(':') {
                (n.trim().to_string(), Some(ty.trim().to_string()))
            } else {
                (trimmed.to_string(), None)
            };

            required_count += 1;
            parameters.push(FunctionParam {
                name: param_name,
                type_annotation: type_ann,
                has_default: false,
            });
        }

        let total_count = parameters.len();
        return Some(FunctionSignature {
            name,
            parameters,
            required_param_count: required_count,
            total_param_count: total_count,
            is_async,
            return_type,
        });
    }

    None
}

/// Generic heuristic fallback for Go, C, etc.
fn parse_heuristic_signature(source: &str, target_symbol: Option<&str>) -> Option<FunctionSignature> {
    let fn_re = Regex::new(
        r#"(?m)^\s*(?:func|def|fn)\s+([a-zA-Z_][a-zA-Z0-9_]*)\s*\(([\s\S]*?)\)"#
    ).ok()?;

    for caps in fn_re.captures_iter(source) {
        let name = caps.get(1)?.as_str().to_string();

        if let Some(target) = target_symbol {
            if name != target {
                continue;
            }
        }

        let raw_params = caps.get(2).map(|m| m.as_str()).unwrap_or("");
        let mut parameters = Vec::new();

        for part in split_top_level_commas(raw_params) {
            let trimmed = part.trim();
            if !trimmed.is_empty() {
                parameters.push(FunctionParam {
                    name: trimmed.to_string(),
                    type_annotation: None,
                    has_default: trimmed.contains('='),
                });
            }
        }

        let total = parameters.len();
        return Some(FunctionSignature {
            name,
            parameters,
            required_param_count: total,
            total_param_count: total,
            is_async: false,
            return_type: None,
        });
    }

    None
}

/// Splits a parameter list by commas at top-level parentheses/brackets/braces.
fn split_top_level_commas(input: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut paren_depth: usize = 0;
    let mut bracket_depth: usize = 0;
    let mut brace_depth: usize = 0;

    for c in input.chars() {
        match c {
            '(' => { paren_depth += 1; current.push(c); }
            ')' => { paren_depth = paren_depth.saturating_sub(1); current.push(c); }
            '[' => { bracket_depth += 1; current.push(c); }
            ']' => { bracket_depth = bracket_depth.saturating_sub(1); current.push(c); }
            '{' => { brace_depth += 1; current.push(c); }
            '}' => { brace_depth = brace_depth.saturating_sub(1); current.push(c); }
            ',' if paren_depth == 0 && bracket_depth == 0 && brace_depth == 0 => {
                parts.push(current.trim().to_string());
                current.clear();
            }
            _ => current.push(c),
        }
    }

    if !current.trim().is_empty() {
        parts.push(current.trim().to_string());
    }

    parts
}

/// Estimates the number of arguments passed at a call site line:
/// e.g. `result = calculate_kpi(100, factor, memo=True)` -> 3 arguments.
pub fn parse_call_site_argument_count(line: &str, symbol_name: &str) -> Option<usize> {
    let pattern = format!(r#"\b{}\s*\("#, regex::escape(symbol_name));
    let re = Regex::new(&pattern).ok()?;
    let mat = re.find(line)?;
    let start_idx = mat.end();

    let remaining = &line[start_idx..];
    let mut depth = 1;
    let mut end_idx = None;

    for (i, c) in remaining.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    end_idx = Some(i);
                    break;
                }
            }
            _ => {}
        }
    }

    let end = end_idx.unwrap_or(remaining.len());
    let raw_args = remaining[..end].trim();

    if raw_args.is_empty() {
        return Some(0);
    }

    let parts = split_top_level_commas(raw_args);
    Some(parts.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_python_signature_extraction() {
        let py_code = r#"
def calculate_cost(base: float, multiplier: float = 1.0, debug: bool = False) -> float:
    return base * multiplier
"#;
        let sig = parse_function_signature(py_code, Some("calculate_cost"), SupportedLanguage::Python).unwrap();
        assert_eq!(sig.name, "calculate_cost");
        assert_eq!(sig.required_param_count, 1);
        assert_eq!(sig.total_param_count, 3);
        assert!(!sig.is_async);
        assert_eq!(sig.return_type.as_deref(), Some("float"));

        let async_py = r#"
async def fetch_user(user_id: int) -> dict:
    return {}
"#;
        let sig2 = parse_function_signature(async_py, Some("fetch_user"), SupportedLanguage::Python).unwrap();
        assert_eq!(sig2.name, "fetch_user");
        assert_eq!(sig2.required_param_count, 1);
        assert_eq!(sig2.total_param_count, 1);
        assert!(sig2.is_async);
    }

    #[test]
    fn test_typescript_signature_extraction() {
        let ts_code = r#"
export function renderReport(title: string, count: number, verbose?: boolean): string {
    return title;
}
"#;
        let sig = parse_function_signature(ts_code, Some("renderReport"), SupportedLanguage::TypeScript).unwrap();
        assert_eq!(sig.name, "renderReport");
        assert_eq!(sig.required_param_count, 2);
        assert_eq!(sig.total_param_count, 3);
        assert!(!sig.is_async);

        let arrow_code = r#"
const updateItem = async (id: string, payload: any = {}): Promise<void> => {
    // update
};
"#;
        let sig2 = parse_function_signature(arrow_code, Some("updateItem"), SupportedLanguage::TypeScriptReact).unwrap();
        assert_eq!(sig2.name, "updateItem");
        assert_eq!(sig2.required_param_count, 1);
        assert_eq!(sig2.total_param_count, 2);
        assert!(sig2.is_async);
    }

    #[test]
    fn test_call_site_argument_count() {
        assert_eq!(parse_call_site_argument_count("let x = calc();", "calc"), Some(0));
        assert_eq!(parse_call_site_argument_count("let x = calc(10);", "calc"), Some(1));
        assert_eq!(parse_call_site_argument_count("let x = calc(10, [1, 2, 3]);", "calc"), Some(2));
        assert_eq!(parse_call_site_argument_count("let x = calc(10, fn(a, b), 30);", "calc"), Some(3));
        assert_eq!(parse_call_site_argument_count("let unrelated = other(10);", "calc"), None);
    }
}
