use calamine::{open_workbook_auto, Reader};
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExcelTemplateMetadata {
    pub file_name: String,
    pub rel_path: String,
    pub canonical_hash: String,
    pub version_tag: Option<String>,
    pub sheets: Vec<String>,
    pub is_template_candidate: bool,
}

/// Checks if a file path is a candidate for Excel template indexing based on directory or filename heuristics.
pub fn is_excel_template_candidate(rel_path: &str) -> bool {
    let lower = rel_path.to_lowercase();
    if !lower.ends_with(".xlsx") && !lower.ends_with(".xls") && !lower.ends_with(".xlsm") {
        return false;
    }

    // Heurística por directorio
    let in_template_dir = lower.contains("template")
        || lower.contains("plantilla")
        || lower.contains("asset")
        || lower.contains("report")
        || lower.contains("excel")
        || lower.contains("formato");

    // Heurística por nombre de archivo
    let file_name = Path::new(rel_path)
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    let matches_name_pattern = file_name.starts_with("1-inf")
        || file_name.starts_with("inf")
        || file_name.contains("plantilla")
        || file_name.contains("template")
        || file_name.contains("cbr")
        || file_name.contains("export")
        || file_name.contains("formato");

    in_template_dir || matches_name_pattern
}

/// Extracts a version tag (e.g. V03, V04, v1, V_02) from a filename or path.
pub fn extract_version_tag(path_str: &str) -> Option<String> {
    let re = Regex::new(r"(?i)(?:^|[_\W])[vV]_?(\d{1,3})(?:[_\W]|$)").ok()?;
    if let Some(caps) = re.captures(path_str) {
        if let Some(num) = caps.get(1) {
            return Some(format!("V{:02}", num.as_str().parse::<u32>().unwrap_or(0)));
        }
    }
    None
}

/// Parses an Excel template file and extracts metadata, version tag, sheet names, and SHA256 hash.
pub fn parse_excel_template(
    file_path: &Path,
    rel_path: &str,
) -> anyhow::Result<Option<ExcelTemplateMetadata>> {
    if !is_excel_template_candidate(rel_path) {
        return Ok(None);
    }

    let file_name = file_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| rel_path.to_string());

    let content = std::fs::read(file_path)?;
    let hash = format!("{:x}", Sha256::digest(&content));

    let sheets = match open_workbook_auto(file_path) {
        Ok(workbook) => workbook.sheet_names().to_vec(),
        Err(_) => Vec::new(),
    };

    let version_tag = extract_version_tag(rel_path).or_else(|| extract_version_tag(&file_name));

    Ok(Some(ExcelTemplateMetadata {
        file_name,
        rel_path: rel_path.to_string(),
        canonical_hash: hash,
        version_tag,
        sheets,
        is_template_candidate: true,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_candidate_detection() {
        assert!(is_excel_template_candidate("templates/report.xlsx"));
        assert!(is_excel_template_candidate("assets/cbr_export.xlsx"));
        assert!(is_excel_template_candidate("src/reports/1-INF.-N-v3.xlsx"));
        assert!(!is_excel_template_candidate("data/tmp_output.csv"));
        assert!(!is_excel_template_candidate("random_dump.xlsx"));
    }

    #[test]
    fn test_version_extraction() {
        assert_eq!(extract_version_tag("1-INF.-N-V03.xlsx"), Some("V03".to_string()));
        assert_eq!(extract_version_tag("report_v4.xlsx"), Some("V04".to_string()));
        assert_eq!(extract_version_tag("template_V_02.xlsx"), Some("V02".to_string()));
        assert_eq!(extract_version_tag("report_no_ver.xlsx"), None);
    }
}
