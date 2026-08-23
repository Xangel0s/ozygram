use anyhow::{Context, Result};
use memmap2::{Mmap, MmapOptions};
use ozymem_parser::{ArchivedEngramTable, EngramContract, EngramTable};
use std::fs::File;
use std::path::Path;

/// Lector de tablas Engram basado en mapeo de memoria con tiempo de acceso O(1)
pub struct FastEngramReader {
    _mmap: Mmap,
    archived_table: &'static ArchivedEngramTable,
}

// Safety: El puntero `archived_table` apunta directamente a la memoria protegida del `_mmap`
// y vive mientras el `_mmap` esté activo.
unsafe impl Send for FastEngramReader {}
unsafe impl Sync for FastEngramReader {}

impl FastEngramReader {
    /// Abre un archivo binario `.rkyv` y crea un mapeo de memoria zero-copy
    pub fn open(path: &Path) -> Result<Self> {
        let file = File::open(path).with_context(|| format!("No se pudo abrir el archivo de engrams: {:?}", path))?;
        let mmap = unsafe { MmapOptions::new().map(&file)? };

        // Acceso zero-copy con validación segura de bytes
        let archived = rkyv::check_archived_root::<EngramTable>(&mmap)
            .map_err(|e| anyhow::anyhow!("Error en validación de bytes de EngramTable: {:?}", e))?;

        let archived_static: &'static ArchivedEngramTable = unsafe {
            std::mem::transmute(archived)
        };

        Ok(Self {
            _mmap: mmap,
            archived_table: archived_static,
        })
    }

    /// Consulta un contrato por su ruta completa en tiempo O(1)
    pub fn lookup(&self, symbol_path: &str) -> Option<EngramContract> {
        let key = EngramTable::hash_symbol(symbol_path);
        self.archived_table.entries.get(&key).map(|archived| {
            EngramContract {
                symbol_path: archived.symbol_path.as_str().to_string(),
                file_path: archived.file_path.as_str().to_string(),
                name: archived.name.as_str().to_string(),
                signature: archived.signature.as_str().to_string(),
                line_number: archived.line_number,
                end_line: archived.end_line,
                kind: archived.kind.as_str().to_string(),
                language: archived.language.as_str().to_string(),
                dependencies: archived.dependencies.iter().map(|s| s.as_str().to_string()).collect(),
                doc_summary: archived.doc_summary.as_str().to_string(),
            }
        })
    }

    /// Itera sobre todos los contratos archivados
    pub fn iter_all(&self) -> Vec<EngramContract> {
        self.archived_table
            .entries
            .values()
            .map(|archived| EngramContract {
                symbol_path: archived.symbol_path.as_str().to_string(),
                file_path: archived.file_path.as_str().to_string(),
                name: archived.name.as_str().to_string(),
                signature: archived.signature.as_str().to_string(),
                line_number: archived.line_number,
                end_line: archived.end_line,
                kind: archived.kind.as_str().to_string(),
                language: archived.language.as_str().to_string(),
                dependencies: archived.dependencies.iter().map(|s| s.as_str().to_string()).collect(),
                doc_summary: archived.doc_summary.as_str().to_string(),
            })
            .collect()
    }

    /// Cantidad de entradas en la tabla archivada
    pub fn len(&self) -> usize {
        self.archived_table.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.archived_table.entries.is_empty()
    }
}
