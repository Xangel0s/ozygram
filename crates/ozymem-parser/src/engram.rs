use rkyv::{Archive, Deserialize, Serialize};
use serde::{Deserialize as SerdeDeserialize, Serialize as SerdeSerialize};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use twox_hash::XxHash64;
use crate::{extract_dependency_hints, parse_source, SupportedLanguage};

/// Contrato determinista de un símbolo de código indexado para lookup O(1)
#[derive(Archive, Deserialize, Serialize, SerdeSerialize, SerdeDeserialize, Debug, PartialEq, Eq, Clone)]
#[archive(check_bytes)]
pub struct EngramContract {
    pub symbol_path: String,
    pub file_path: String,
    pub name: String,
    pub signature: String,
    pub line_number: u32,
    pub end_line: u32,
    pub kind: String,
    pub language: String,
    pub dependencies: Vec<String>,
    pub doc_summary: String,
}

/// Tabla global de Engrams archivada con soporte Zero-Copy
#[derive(Archive, Deserialize, Serialize, SerdeSerialize, SerdeDeserialize, Debug, Default, Clone, PartialEq)]
#[archive(check_bytes)]
pub struct EngramTable {
    pub entries: HashMap<u64, EngramContract>,
}

impl EngramTable {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    /// Calcula el hash rápido 64-bit del identificador del símbolo
    pub fn hash_symbol(symbol_path: &str) -> u64 {
        let mut hasher = XxHash64::default();
        symbol_path.hash(&mut hasher);
        hasher.finish()
    }

    /// Inserta un contrato indexado por su hash
    pub fn insert(&mut self, contract: EngramContract) {
        let key = Self::hash_symbol(&contract.symbol_path);
        self.entries.insert(key, contract);
    }

    /// Consulta un contrato en tiempo O(1)
    pub fn get(&self, symbol_path: &str) -> Option<&EngramContract> {
        let key = Self::hash_symbol(symbol_path);
        self.entries.get(&key)
    }

    /// Cantidad de contratos registrados
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Extrae contratos Engram a partir del contenido de un archivo y su AST
pub fn extract_engram_contracts(
    file_path: &str,
    language: SupportedLanguage,
    content: &str,
) -> Vec<EngramContract> {
    let mut contracts = Vec::new();
    let lines: Vec<&str> = content.lines().collect();

    // 1. Extraer dependencias léxicas/AST del archivo
    let dep_hints = extract_dependency_hints(file_path, language, content).unwrap_or_default();
    let file_deps: Vec<String> = dep_hints.into_iter().map(|d| d.label).collect();

    // 2. Parsear símbolos con Tree-sitter / heurística
    if let Ok(file_map) = parse_source(file_path, language, content) {
        for func in file_map.functions {
            let start_idx = if func.start_line > 0 && func.start_line <= lines.len() {
                func.start_line - 1
            } else {
                0
            };

            // Extraer la firma (primera línea o líneas de declaración)
            let signature = lines
                .get(start_idx)
                .map(|s| s.trim().to_string())
                .unwrap_or_else(|| func.name.clone());

            // Extraer docstrings/comentarios precedentes si existen
            let mut doc_lines = Vec::new();
            let mut curr = start_idx as isize - 1;
            while curr >= 0 {
                let line_str = lines[curr as usize].trim();
                if line_str.starts_with("///") || line_str.starts_with("//") || line_str.starts_with("#") || line_str.starts_with("*") {
                    doc_lines.push(line_str.trim_start_matches(|c| c == '/' || c == '#' || c == '*' || c == ' ').trim().to_string());
                    curr -= 1;
                } else {
                    break;
                }
            }
            doc_lines.reverse();
            let doc_summary = doc_lines.join(" ");

            let symbol_path = format!("{}::{}", file_path, func.name);

            contracts.push(EngramContract {
                symbol_path,
                file_path: file_path.to_string(),
                name: func.name,
                signature,
                line_number: func.start_line as u32,
                end_line: func.end_line as u32,
                kind: format!("{:?}", func.kind),
                language: language.as_str().to_string(),
                dependencies: file_deps.clone(),
                doc_summary,
            });
        }
    }

    contracts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_engram_rust_function() {
        let code = r#"
/// Abre una conexión segura con la base de datos
pub fn open_db(path: &str) -> Result<Connection, Error> {
    Connection::open(path)
}
"#;
        let contracts = extract_engram_contracts("src/db.rs", SupportedLanguage::Rust, code);
        assert!(!contracts.is_empty(), "Debe extraer al menos un contrato Engram");
        let c = &contracts[0];
        assert_eq!(c.name, "open_db");
        assert_eq!(c.file_path, "src/db.rs");
        assert_eq!(c.symbol_path, "src/db.rs::open_db");
        assert!(c.signature.contains("pub fn open_db"));
        assert!(c.doc_summary.contains("Abre una conexión segura"));
    }

    #[test]
    fn test_engram_table_rkyv_roundtrip() {
        let mut table = EngramTable::new();
        table.insert(EngramContract {
            symbol_path: "src/auth.rs::verify".to_string(),
            file_path: "src/auth.rs".to_string(),
            name: "verify".to_string(),
            signature: "pub fn verify(t: &str) -> bool".to_string(),
            line_number: 10,
            end_line: 25,
            kind: "Function".to_string(),
            language: "Rust".to_string(),
            dependencies: vec!["Claims".to_string()],
            doc_summary: "Verifica el token JWT".to_string(),
        });

        // Serializar con rkyv
        let bytes = rkyv::to_bytes::<_, 1024>(&table).expect("Error serializando EngramTable con rkyv");
        assert!(!bytes.is_empty());

        // Zero-copy access y verificación de bytes
        let archived = rkyv::check_archived_root::<EngramTable>(&bytes).expect("Error verificando bytes archivados");
        let hash = EngramTable::hash_symbol("src/auth.rs::verify");
        let entry = archived.entries.get(&hash).expect("Debe encontrar el símbolo en el buffer archivado");

        assert_eq!(entry.name.as_str(), "verify");
        assert_eq!(entry.signature.as_str(), "pub fn verify(t: &str) -> bool");
        assert_eq!(entry.line_number, 10);
    }
}
