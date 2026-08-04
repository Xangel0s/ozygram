mod dependency_resolution;
pub mod contract_parser;
pub mod excel_parser;

pub use contract_parser::{parse_export_contracts, ExportContractHint};
pub use dependency_resolution::{is_internal_dependency_hint, resolve_dependency_target};
pub use excel_parser::{extract_version_tag, is_excel_template_candidate, parse_excel_template, ExcelTemplateMetadata};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tree_sitter::{Parser, Query, QueryCursor, StreamingIterator};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParsedSymbol {
    pub name: String,
    pub kind: SymbolKind,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum SymbolKind {
    Function,
    Class,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParsedRelation {
    pub caller: String,
    pub callee: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum DependencyHintKind {
    UseDeclaration,
    ModItem,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParsedDependencyHint {
    pub file_path: String,
    pub kind: DependencyHintKind,
    pub label: String,
    pub raw_text: String,
    pub start_line: usize,
    pub end_line: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExtractedFunction {
    pub name: String,
    pub kind: SymbolKind,
    pub start_line: usize,
    pub end_line: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileDefinitionMap {
    pub file_path: String,
    pub language: String,
    pub strategy: ParseStrategy,
    pub functions: Vec<ExtractedFunction>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupportedLanguage {
    Python,
    Go,
    Rust,
    JavaScript,
    TypeScriptReact,
    SQL,
    Unknown,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ParseStrategy {
    #[serde(rename = "NativeAST")]
    NativeAst,
    #[serde(rename = "ExtensionWASM")]
    ExtensionWasm,
    #[serde(rename = "TextHeuristic")]
    TextHeuristic,
}

impl SupportedLanguage {
    pub fn as_str(self) -> &'static str {
        match self {
            SupportedLanguage::Python => "Python",
            SupportedLanguage::Go => "Go",
            SupportedLanguage::Rust => "Rust",
            SupportedLanguage::JavaScript => "JavaScript",
            SupportedLanguage::TypeScriptReact => "TypeScriptReact",
            SupportedLanguage::SQL => "SQL",
            SupportedLanguage::Unknown => "Unknown",
        }
    }

    pub fn native_tree_sitter_language(self) -> Option<tree_sitter::Language> {
        match self {
            SupportedLanguage::Python => Some(tree_sitter_python::LANGUAGE.into()),
            SupportedLanguage::Go => Some(tree_sitter_go::LANGUAGE.into()),
            SupportedLanguage::Rust => Some(tree_sitter_rust::LANGUAGE.into()),
            SupportedLanguage::JavaScript => Some(tree_sitter_javascript::LANGUAGE.into()),
            SupportedLanguage::TypeScriptReact
            | SupportedLanguage::SQL
            | SupportedLanguage::Unknown => None,
        }
    }

    pub fn query_pattern(self) -> Option<&'static str> {
        match self {
            SupportedLanguage::Python => Some(
                r#"
                (function_definition
                    name: (identifier) @symbol.name
                ) @symbol.definition

                (class_definition
                    name: (identifier) @symbol.name
                ) @symbol.definition
                "#,
            ),
            SupportedLanguage::Go => Some(
                r#"
                (function_declaration
                    name: (identifier) @symbol.name
                ) @symbol.definition

                (type_spec
                    name: (type_identifier) @symbol.name
                ) @symbol.definition
                "#,
            ),
            SupportedLanguage::JavaScript => Some(
                r#"
                (function_declaration
                    name: (identifier) @symbol.name
                ) @symbol.definition

                (class_declaration
                    name: (identifier) @symbol.name
                ) @symbol.definition
                "#,
            ),
            SupportedLanguage::Rust => Some(
                r#"
                (function_item
                    name: (identifier) @symbol.name
                ) @symbol.definition

                (struct_item
                    name: (type_identifier) @symbol.name
                ) @symbol.definition

                (enum_item
                    name: (type_identifier) @symbol.name
                ) @symbol.definition

                (trait_item
                    name: (type_identifier) @symbol.name
                ) @symbol.definition
                "#,
            ),
            SupportedLanguage::TypeScriptReact
            | SupportedLanguage::SQL
            | SupportedLanguage::Unknown => None,
        }
    }

    pub fn dependency_query_pattern(self) -> Option<&'static str> {
        match self {
            SupportedLanguage::Rust => Some(
                r#"
                (use_declaration
                    argument: (_) @dependency.label
                ) @dependency.definition

                (mod_item
                    name: (identifier) @dependency.label
                ) @dependency.definition
                "#,
            ),
            _ => None,
        }
    }
}

impl ParseStrategy {
    pub fn as_str(self) -> &'static str {
        match self {
            ParseStrategy::NativeAst => "NativeAST",
            ParseStrategy::ExtensionWasm => "ExtensionWASM",
            ParseStrategy::TextHeuristic => "TextHeuristic",
        }
    }
}

#[derive(Debug, Error)]
pub enum ParserError {
    #[error("failed to configure parser")]
    ConfigureParser(#[from] tree_sitter::LanguageError),
    #[error("failed to build Tree-sitter query")]
    BuildQuery(#[from] tree_sitter::QueryError),
    #[error("could not parse source code into an AST")]
    ParseFailed,
}

pub fn parse_source(
    file_path: &str,
    language: SupportedLanguage,
    source_code: &str,
) -> Result<FileDefinitionMap, ParserError> {
    if let Some(ts_language) = language.native_tree_sitter_language() {
        if let Ok(map) = parse_with_tree_sitter(file_path, language, source_code, ts_language) {
            return Ok(map);
        }
    }

    Ok(parse_with_heuristics(file_path, language, source_code))
}

pub fn extract_dependency_hints(
    file_path: &str,
    language: SupportedLanguage,
    source_code: &str,
) -> Result<Vec<ParsedDependencyHint>, ParserError> {
    let Some(ts_language) = language.native_tree_sitter_language() else {
        return Ok(Vec::new());
    };

    let Some(query_pattern) = language.dependency_query_pattern() else {
        return Ok(Vec::new());
    };

    let mut parser = Parser::new();
    parser.set_language(&ts_language)?;

    let tree = parser
        .parse(source_code, None)
        .ok_or(ParserError::ParseFailed)?;

    let query = Query::new(&ts_language, query_pattern)?;
    let mut query_cursor = QueryCursor::new();
    let mut dependency_hints = Vec::new();

    let mut matches = query_cursor.matches(&query, tree.root_node(), source_code.as_bytes());
    while let Some(match_result) = matches.next() {
        let mut kind = None;
        let mut label = None;
        let mut raw_text = None;
        let mut start_line = None;
        let mut end_line = None;

        for capture in match_result.captures.iter() {
            let capture_name = query.capture_names()[capture.index as usize];

            match capture_name {
                "dependency.definition" => {
                    kind = Some(match capture.node.kind() {
                        "use_declaration" => DependencyHintKind::UseDeclaration,
                        "mod_item" => DependencyHintKind::ModItem,
                        _ => continue,
                    });
                    raw_text = capture
                        .node
                        .utf8_text(source_code.as_bytes())
                        .ok()
                        .map(|text| text.to_string());
                    start_line = Some(capture.node.start_position().row + 1);
                    end_line = Some(capture.node.end_position().row + 1);
                }
                "dependency.label" => {
                    label = capture
                        .node
                        .utf8_text(source_code.as_bytes())
                        .ok()
                        .map(|text| text.to_string());
                }
                _ => {}
            }
        }

        if let (Some(kind), Some(label), Some(raw_text), Some(start_line), Some(end_line)) =
            (kind, label, raw_text, start_line, end_line)
        {
            dependency_hints.push(ParsedDependencyHint {
                file_path: file_path.to_string(),
                kind,
                label,
                raw_text,
                start_line,
                end_line,
            });
        }
    }

    Ok(dependency_hints)
}

pub fn parse_python_source(
    file_path: &str,
    source_code: &str,
) -> Result<FileDefinitionMap, ParserError> {
    parse_source(file_path, SupportedLanguage::Python, source_code)
}

pub fn parse_with_tree_sitter(
    file_path: &str,
    language: SupportedLanguage,
    source_code: &str,
    ts_language: tree_sitter::Language,
) -> Result<FileDefinitionMap, ParserError> {
    let mut parser = Parser::new();
    parser.set_language(&ts_language)?;

    let Some(query_pattern) = language.query_pattern() else {
        return Err(ParserError::ParseFailed);
    };

    let tree = parser
        .parse(source_code, None)
        .ok_or(ParserError::ParseFailed)?;

    let query = Query::new(&ts_language, query_pattern)?;
    let mut query_cursor = QueryCursor::new();
    let mut functions = Vec::new();

    let mut matches = query_cursor.matches(&query, tree.root_node(), source_code.as_bytes());
    while let Some(match_result) = matches.next() {
        let mut name = None;
        let mut kind = None;
        let mut start_line = None;
        let mut end_line = None;

        for capture in match_result.captures.iter() {
            let capture_name = query.capture_names()[capture.index as usize];

            match capture_name {
                "symbol.name" => {
                    if let Ok(text) = capture.node.utf8_text(source_code.as_bytes()) {
                        name = Some(text.to_string());
                    }
                }
                "symbol.definition" => {
                    kind = Some(match capture.node.kind() {
                        "function_definition" | "function_declaration" | "function_item" => {
                            SymbolKind::Function
                        }
                        "class_definition" | "class_declaration" | "type_spec" | "struct_item"
                        | "enum_item" | "trait_item" | "type_item" | "union_item" => {
                            SymbolKind::Class
                        }
                        _ => continue,
                    });
                    start_line = Some(capture.node.start_position().row + 1);
                    end_line = Some(capture.node.end_position().row + 1);
                }
                _ => {}
            }
        }

        if let (Some(name), Some(kind), Some(start_line), Some(end_line)) =
            (name, kind, start_line, end_line)
        {
            functions.push(ExtractedFunction {
                name,
                kind,
                start_line,
                end_line,
            });
        }
    }

    Ok(FileDefinitionMap {
        file_path: file_path.to_string(),
        language: language.as_str().to_string(),
        strategy: ParseStrategy::NativeAst,
        functions,
    })
}

pub fn parse_with_heuristics(
    file_path: &str,
    language: SupportedLanguage,
    source_code: &str,
) -> FileDefinitionMap {
    let functions = extract_heuristic_blocks(source_code);
    let language_label = match language {
        SupportedLanguage::Unknown => determine_raw_label(file_path),
        _ => language.as_str().to_string(),
    };

    FileDefinitionMap {
        file_path: file_path.to_string(),
        language: language_label,
        strategy: ParseStrategy::TextHeuristic,
        functions,
    }
}

fn determine_raw_label(file_path: &str) -> String {
    std::path::Path::new(file_path)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .filter(|extension| !extension.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

fn extract_heuristic_blocks(source_code: &str) -> Vec<ExtractedFunction> {
    let lines: Vec<&str> = source_code.lines().collect();
    let mut functions = Vec::new();

    for (index, line) in lines.iter().enumerate() {
        if let Some((name, kind, style)) = parse_declaration(line.trim_start()) {
            let start_line = index + 1;
            let end_line = match style {
                HeuristicStyle::Brace => find_brace_block_end(&lines, index),
                HeuristicStyle::Indentation => find_indented_block_end(&lines, index),
                HeuristicStyle::SingleLine => start_line,
            };

            functions.push(ExtractedFunction {
                name,
                kind,
                start_line,
                end_line,
            });
        }
    }

    functions
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HeuristicStyle {
    Brace,
    Indentation,
    SingleLine,
}

fn parse_declaration(line: &str) -> Option<(String, SymbolKind, HeuristicStyle)> {
    let declaration_prefixes = [
        ("class ", SymbolKind::Class),
        ("struct ", SymbolKind::Class),
        ("interface ", SymbolKind::Class),
        ("type ", SymbolKind::Class),
        ("def ", SymbolKind::Function),
        ("func ", SymbolKind::Function),
        ("function ", SymbolKind::Function),
    ];

    for (prefix, kind) in declaration_prefixes {
        if let Some(rest) = line.strip_prefix(prefix) {
            let name = extract_identifier(rest);
            let style = if line.contains('{') {
                HeuristicStyle::Brace
            } else if line.ends_with(':') {
                HeuristicStyle::Indentation
            } else {
                HeuristicStyle::SingleLine
            };

            return Some((name, kind, style));
        }
    }

    if let Some(rest) = line.strip_prefix("async function ") {
        return Some((
            extract_identifier(rest),
            SymbolKind::Function,
            HeuristicStyle::Brace,
        ));
    }

    if let Some(rest) = line.strip_prefix("export function ") {
        return Some((
            extract_identifier(rest),
            SymbolKind::Function,
            HeuristicStyle::Brace,
        ));
    }

    None
}

fn extract_identifier(fragment: &str) -> String {
    fragment
        .chars()
        .take_while(|character| {
            character.is_ascii_alphanumeric() || *character == '_' || *character == '$'
        })
        .collect()
}

fn find_brace_block_end(lines: &[&str], start_index: usize) -> usize {
    let mut depth = 0usize;
    let mut seen_open = false;
    let mut in_string = false;
    let mut string_char = '"';
    let mut in_block_comment = false;

    for (index, line) in lines.iter().enumerate().skip(start_index) {
        let chars: Vec<char> = line.chars().collect();
        let mut i = 0;

        while i < chars.len() {
            let character = chars[i];

            if in_block_comment {
                if character == '*' && i + 1 < chars.len() && chars[i + 1] == '/' {
                    in_block_comment = false;
                    i += 2;
                } else {
                    i += 1;
                }
                continue;
            }

            if in_string {
                if character == string_char {
                    let mut backslash_count = 0;
                    let mut temp = i as i64 - 1;
                    while temp >= 0 && chars[temp as usize] == '\\' {
                        backslash_count += 1;
                        temp -= 1;
                    }
                    if backslash_count % 2 == 0 {
                        in_string = false;
                    }
                }
                i += 1;
                continue;
            }

            if character == '/' && i + 1 < chars.len() && chars[i + 1] == '/' {
                break;
            }
            if character == '#' {
                break;
            }
            if character == '/' && i + 1 < chars.len() && chars[i + 1] == '*' {
                in_block_comment = true;
                i += 2;
                continue;
            }

            if character == '"' || character == '\'' {
                in_string = true;
                string_char = character;
                i += 1;
                continue;
            }

            if character == '{' {
                depth += 1;
                seen_open = true;
            } else if character == '}' {
                if depth > 0 {
                    depth -= 1;
                }
            }
            i += 1;
        }

        if seen_open && depth == 0 {
            return index + 1;
        }
    }

    start_index + 1
}

fn find_indented_block_end(lines: &[&str], start_index: usize) -> usize {
    let start_indent = indent_width(lines[start_index]);
    let mut end_index = start_index;

    for (index, line) in lines.iter().enumerate().skip(start_index + 1) {
        if line.trim().is_empty() {
            continue;
        }

        if indent_width(line) <= start_indent {
            break;
        }

        end_index = index;
    }

    end_index + 1
}

fn indent_width(line: &str) -> usize {
    line.chars()
        .take_while(|character| character.is_whitespace())
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn parses_python_functions_and_classes() {
        let source = r#"class Greeter:
    def greet(self):
        return "hello"


def top_level():
    return Greeter()
"#;

        let result = parse_source("sample.py", SupportedLanguage::Python, source)
            .expect("parser should succeed");

        assert_eq!(result.file_path, "sample.py");
        assert_eq!(result.language, "Python");
        assert_eq!(result.strategy, ParseStrategy::NativeAst);
        assert_eq!(result.functions.len(), 3);
        assert_eq!(result.functions[0].name, "Greeter");
        assert_eq!(result.functions[0].kind, SymbolKind::Class);
        assert_eq!(result.functions[0].start_line, 1);
        assert_eq!(result.functions[0].end_line, 3);

        assert_eq!(result.functions[1].name, "greet");
        assert_eq!(result.functions[1].kind, SymbolKind::Function);
        assert_eq!(result.functions[1].start_line, 2);
        assert_eq!(result.functions[1].end_line, 3);

        assert_eq!(result.functions[2].name, "top_level");
        assert_eq!(result.functions[2].kind, SymbolKind::Function);
        assert_eq!(result.functions[2].start_line, 6);
        assert_eq!(result.functions[2].end_line, 7);
    }

    #[test]
    fn falls_back_to_heuristics_for_unknown_language() {
        let source = r#"func calcular_interes() {
    return 1;
}
"#;

        let result = parse_source("sample.rb", SupportedLanguage::Unknown, source)
            .expect("heuristic parser should succeed");

        assert_eq!(result.language, "rb");
        assert_eq!(result.strategy, ParseStrategy::TextHeuristic);
        assert_eq!(result.functions.len(), 1);
        assert_eq!(result.functions[0].name, "calcular_interes");
        assert_eq!(result.functions[0].kind, SymbolKind::Function);
        assert_eq!(result.functions[0].start_line, 1);
        assert_eq!(result.functions[0].end_line, 3);
    }

    #[test]
    fn parses_rust_symbols_with_native_tree_sitter() {
        let source = r#"pub struct Client {
    id: usize,
}

pub enum Mode {
    A,
}

pub fn connect() {}
"#;

        let result = parse_source("src/lib.rs", SupportedLanguage::Rust, source)
            .expect("rust parser should succeed");

        assert_eq!(result.language, "Rust");
        assert_eq!(result.strategy, ParseStrategy::NativeAst);
        assert!(result.functions.len() >= 2);
        assert!(result
            .functions
            .iter()
            .any(|function| function.name == "Mode"));
        assert!(result
            .functions
            .iter()
            .any(|function| function.name == "connect"));
    }

    #[test]
    fn extracts_rust_dependency_hints_for_use_and_mod() {
        let source = r#"pub mod router;
mod internal;

use crate::domain::User;
use ozymem_core::GraphBackend;
"#;

        let hints = extract_dependency_hints("src/lib.rs", SupportedLanguage::Rust, source)
            .expect("dependency hint extraction should succeed");

        assert_eq!(hints.len(), 4);

        let kinds: HashSet<_> = hints.iter().map(|hint| hint.kind).collect();
        assert!(kinds.contains(&DependencyHintKind::UseDeclaration));
        assert!(kinds.contains(&DependencyHintKind::ModItem));

        let labels: HashSet<_> = hints.iter().map(|hint| hint.label.as_str()).collect();
        assert!(labels.contains("router"));
        assert!(labels.contains("internal"));
        assert!(labels.contains("crate::domain::User"));
        assert!(labels.contains("ozymem_core::GraphBackend"));
    }

    #[test]
    fn identifies_binary_files() {
        assert!(is_binary_file(std::path::Path::new("document.pdf")));
        assert!(is_binary_file(std::path::Path::new("archive.tar.gz")));
        assert!(is_binary_file(std::path::Path::new("image.png")));
        assert!(!is_binary_file(std::path::Path::new("source.rs")));
        assert!(!is_binary_file(std::path::Path::new("script.py")));
    }

    #[test]
    fn parses_braces_ignoring_strings_and_comments() {
        let source = r#"func test_escapes() {
            let s = "brace } inside string"; // a comment with { open brace
            /* block comment with } close brace */
            let s2 = "escaped \" quotes }";
        }
"#;
        let lines: Vec<&str> = source.lines().collect();
        let end = find_brace_block_end(&lines, 0);
        assert_eq!(end, 5);
    }
}

pub fn is_binary_file(path: &std::path::Path) -> bool {
    let path_str = path.to_string_lossy().to_lowercase();
    if path_str.ends_with(".tar.gz") {
        return true;
    }
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        let ext = ext.to_lowercase();
        matches!(
            ext.as_str(),
            "pdf" | "rar" | "zip" | "jpeg" | "jpg" | "png" | "exe" | "gif" | "ico" | "bin" | "tar" | "gz" | "7z"
        )
    } else {
        false
    }
}

