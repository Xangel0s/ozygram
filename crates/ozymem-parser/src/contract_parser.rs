use regex::Regex;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExportContractHint {
    pub file_path: String,
    pub endpoint_path: Option<String>,
    pub template_ref: Option<String>,
    pub content_disposition_filename: Option<String>,
    pub start_line: usize,
}

/// Parses source code to extract export contract hints (template references, Content-Disposition headers, endpoints).
pub fn parse_export_contracts(file_path: &str, source: &str) -> Vec<ExportContractHint> {
    let mut results = Vec::new();

    let template_re = Regex::new(r#"(?i)(?:TEMPLATE_FILENAME|TEMPLATE_PATH|EXCEL_TEMPLATE)\s*=\s*["']([^"']+)["']"#).unwrap();
    let cd_re = Regex::new(r#"(?i)filename\s*=\s*["']?([^"';\r\n]+)["']?"#).unwrap();
    let endpoint_re = Regex::new(r#"(?i)@(?:app|router)\.(?:get|post)\s*\(\s*["']([^"']+)["']"#).unwrap();

    let lines: Vec<&str> = source.lines().collect();
    let mut current_endpoint: Option<String> = None;

    for (idx, line) in lines.iter().enumerate() {
        if let Some(caps) = endpoint_re.captures(line) {
            if let Some(m) = caps.get(1) {
                current_endpoint = Some(m.as_str().to_string());
            }
        }

        let template_ref = template_re.captures(line).and_then(|c| c.get(1).map(|m| m.as_str().to_string()));
        let cd_filename = cd_re.captures(line).and_then(|c| c.get(1).map(|m| m.as_str().trim().to_string()));

        if template_ref.is_some() || cd_filename.is_some() {
            results.push(ExportContractHint {
                file_path: file_path.to_string(),
                endpoint_path: current_endpoint.clone(),
                template_ref,
                content_disposition_filename: cd_filename,
                start_line: idx + 1,
            });
        }
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_export_contracts() {
        let code = r#"
        @router.get("/api/v1/cbr/export")
        async fn export_cbr():
            TEMPLATE_FILENAME = "1-INF.-N-V04.xlsx"
            return FileResponse(path, headers={"Content-Disposition": 'attachment; filename="1-INF.-N-V03.xlsx"'})
        "#;

        let hints = parse_export_contracts("src/cbr/router.py", code);
        assert_eq!(hints.len(), 2);
        assert_eq!(hints[0].endpoint_path, Some("/api/v1/cbr/export".to_string()));
        assert_eq!(hints[0].template_ref, Some("1-INF.-N-V04.xlsx".to_string()));
        assert_eq!(hints[1].content_disposition_filename, Some("1-INF.-N-V03.xlsx".to_string()));
    }
}
