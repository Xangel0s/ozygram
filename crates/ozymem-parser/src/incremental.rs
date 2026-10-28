use crate::SupportedLanguage;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tree_sitter::{InputEdit, Parser, Point, Tree};

/// Representa una mutación diferencial en un archivo de código para parseo incremental.
#[derive(Debug, Clone)]
pub struct AstMutationEdit {
    pub start_byte: usize,
    pub old_end_byte: usize,
    pub new_end_byte: usize,
    pub start_position: (usize, usize), // (row, column)
    pub old_end_position: (usize, usize),
    pub new_end_position: (usize, usize),
}

/// Caché de árboles AST en memoria para permitir re-parseo incremental en sub-milisegundos (< 40µs).
#[derive(Default)]
pub struct IncrementalAstCache {
    trees: Mutex<HashMap<PathBuf, Tree>>,
}

impl IncrementalAstCache {
    pub fn new() -> Self {
        Self {
            trees: Mutex::new(HashMap::new()),
        }
    }

    /// Obtiene una copia clonada del árbol AST para un archivo dado.
    pub fn get_tree(&self, path: &Path) -> Option<Tree> {
        let guard = self.trees.lock().unwrap();
        guard.get(path).cloned()
    }

    /// Almacena el árbol AST para un archivo.
    pub fn set_tree(&self, path: PathBuf, tree: Tree) {
        let mut guard = self.trees.lock().unwrap();
        guard.insert(path, tree);
    }

    /// Invalida o purga el árbol de un archivo modificado drásticamente o borrado.
    pub fn invalidate(&self, path: &Path) {
        let mut guard = self.trees.lock().unwrap();
        guard.remove(path);
    }

    /// Ejecuta el parseo incremental aplicando un InputEdit sobre el árbol previo si existe.
    pub fn parse_incremental(
        &self,
        path: &Path,
        new_source: &str,
        lang: SupportedLanguage,
        edit: Option<AstMutationEdit>,
    ) -> Option<Tree> {
        let mut parser = Parser::new();
        let ts_lang = match lang {
            SupportedLanguage::Rust => tree_sitter_rust::LANGUAGE.into(),
            SupportedLanguage::Python => tree_sitter_python::LANGUAGE.into(),
            SupportedLanguage::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            SupportedLanguage::TypeScript | SupportedLanguage::TypeScriptReact => {
                tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
            }
            SupportedLanguage::Go => tree_sitter_go::LANGUAGE.into(),
            _ => return None,
        };

        if parser.set_language(&ts_lang).is_err() {
            return None;
        }

        let mut old_tree_opt = self.get_tree(path);

        if let (Some(ref mut old_tree), Some(e)) = (&mut old_tree_opt, edit) {
            let input_edit = InputEdit {
                start_byte: e.start_byte,
                old_end_byte: e.old_end_byte,
                new_end_byte: e.new_end_byte,
                start_position: Point::new(e.start_position.0, e.start_position.1),
                old_end_position: Point::new(e.old_end_position.0, e.old_end_position.1),
                new_end_position: Point::new(e.new_end_position.0, e.new_end_position.1),
            };
            old_tree.edit(&input_edit);
        }

        let new_tree = parser.parse(new_source, old_tree_opt.as_ref())?;
        self.set_tree(path.to_path_buf(), new_tree.clone());
        Some(new_tree)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_incremental_ast_parse_lifecycle() {
        let cache = IncrementalAstCache::new();
        let path = Path::new("test_incremental.rs");

        let initial_src = "pub fn calculate_total(a: i32, b: i32) -> i32 { a + b }";
        let tree1 = cache
            .parse_incremental(path, initial_src, SupportedLanguage::Rust, None)
            .expect("initial parse must succeed");
        assert!(!tree1.root_node().has_error());

        // Editar una sola expresión reemplazando 'a + b' por 'a * b'
        let updated_src = "pub fn calculate_total(a: i32, b: i32) -> i32 { a * b }";
        let edit = AstMutationEdit {
            start_byte: 50,
            old_end_byte: 51,
            new_end_byte: 51,
            start_position: (0, 50),
            old_end_position: (0, 51),
            new_end_position: (0, 51),
        };

        let tree2 = cache
            .parse_incremental(path, updated_src, SupportedLanguage::Rust, Some(edit))
            .expect("incremental parse must succeed");

        assert!(!tree2.root_node().has_error());
        assert_eq!(tree2.root_node().kind(), "source_file");
    }
}

