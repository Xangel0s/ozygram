pub mod config;
pub mod client;
pub mod mcp;
pub mod commands;

pub use config::*;
pub use client::*;
pub use commands::*;

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use std::io::Write;
    use std::path::Path;
    use ozymem_parser::{parse_source, SupportedLanguage};

    #[test]
    fn maps_extensions_to_languages() {
        assert_eq!(
            get_language_from_path(Path::new("file.py")),
            SupportedLanguage::Python
        );
        assert_eq!(
            get_language_from_path(Path::new("file.go")),
            SupportedLanguage::Go
        );
        assert_eq!(
            get_language_from_path(Path::new("file.rs")),
            SupportedLanguage::Rust
        );
        assert_eq!(
            get_language_from_path(Path::new("file.js")),
            SupportedLanguage::JavaScript
        );
        assert_eq!(
            get_language_from_path(Path::new("file.ts")),
            SupportedLanguage::TypeScriptReact
        );
        assert_eq!(
            get_language_from_path(Path::new("file.tsx")),
            SupportedLanguage::TypeScriptReact
        );
        assert_eq!(
            get_language_from_path(Path::new("file.jsx")),
            SupportedLanguage::TypeScriptReact
        );
        assert_eq!(
            get_language_from_path(Path::new("file.sql")),
            SupportedLanguage::SQL
        );
        assert_eq!(
            get_language_from_path(Path::new("file.txt")),
            SupportedLanguage::Unknown
        );
        assert_eq!(
            get_language_from_path(Path::new("file")),
            SupportedLanguage::Unknown
        );
    }

    #[test]
    fn scans_python_file_in_temporary_directory() {
        let temp_root =
            std::env::temp_dir().join(format!("ozymem-cli-test-{}", std::process::id()));

        let _ = fs::remove_dir_all(&temp_root);
        fs::create_dir_all(&temp_root).expect("create temp root");

        let file_path = temp_root.join("sample.py");
        let mut file = File::create(&file_path).expect("create file");
        writeln!(file, "class Sample:").expect("write class");
        writeln!(file, "    def hello(self):").expect("write method");
        writeln!(file, "        return 1").expect("write body");

        let parsed = parse_source(
            &file_path.to_string_lossy(),
            SupportedLanguage::Python,
            &fs::read_to_string(&file_path).expect("read sample file"),
        )
        .expect("parser should succeed");

        assert_eq!(parsed.functions.len(), 2);

        let _ = fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn dynamic_ignore_patterns_load_and_check() {
        let temp_root =
            std::env::temp_dir().join(format!("ozymem-cli-ignore-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp_root);
        fs::create_dir_all(&temp_root).expect("create temp root");

        let ozymemignore_path = temp_root.join(".ozymemignore");
        let mut ozymemignore_file = File::create(&ozymemignore_path).expect("create ozymemignore");
        writeln!(ozymemignore_file, "pattern1").expect("write pattern1");
        writeln!(ozymemignore_file, "# comment").expect("write comment");
        writeln!(ozymemignore_file, "pattern2").expect("write pattern2");

        let gitignore_path = temp_root.join(".gitignore");
        let mut gitignore_file = File::create(&gitignore_path).expect("create gitignore");
        writeln!(gitignore_file, "pattern3").expect("write pattern3");

        let patterns = load_ignore_patterns_for_project(&temp_root);
        assert_eq!(patterns.len(), 3);
        assert!(patterns.contains(&"pattern1".to_string()));
        assert!(patterns.contains(&"pattern2".to_string()));
        assert!(patterns.contains(&"pattern3".to_string()));

        let file1 = temp_root.join("pattern1");
        let file2 = temp_root.join("other_file");
        let file3 = temp_root.join("pattern3");

        assert!(is_ignored_by_patterns(&file1, &patterns, &temp_root));
        assert!(!is_ignored_by_patterns(&file2, &patterns, &temp_root));
        assert!(is_ignored_by_patterns(&file3, &patterns, &temp_root));

        let _ = fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn render_trace_node_handles_cycles() {
        let node = TreeNode {
            path: "target.rs".to_string(),
            context: None,
            functions: Vec::new(),
            dependencies: vec![TreeNode {
                path: "dependent.rs".to_string(),
                context: None,
                functions: Vec::new(),
                dependencies: Vec::new(),
                truncated: false,
                cyclic: true,
            }],
            truncated: false,
            cyclic: false,
        };
        render_trace_node(&node, "", true, true);
        assert!(true);
    }
}


#[cfg(test)]
mod query_translator_tests {
    use super::*;

    #[test]
    fn split_query_tokens_preserves_quoted_text() {
        let tokens = split_query_tokens("grep \"record lesson\" 220-260");
        assert_eq!(tokens, vec!["grep", "record lesson", "220-260"]);
    }

    #[test]
    fn extract_line_range_removes_range_argument() {
        let args = vec!["auth".to_string(), "223-238".to_string()];
        let (rest, range) = extract_line_range(&args);
        assert_eq!(rest, vec!["auth"]);
        assert_eq!(range, Some((223, 238)));
    }

    #[test]
    fn unknown_query_is_safe_reject_without_shell() {
        let output = safe_unknown_query("rm -rf", "No seguro");
        assert_eq!(output.mode, "safe");
        assert_eq!(output.intent, "unknown");
        assert!(output.results[0].detail.contains("No se ejecutó shell externo"));
    }

    #[test]
    fn skills_query_uses_official_metadata() {
        let output = query_skills(&["react".to_string()], 5);
        assert_eq!(output.intent, "skills_official_metadata");
        assert!(output.results.iter().any(|r| r.detail.contains("skills.sh/official") || r.detail.contains("facebook/react")));
    }
}
