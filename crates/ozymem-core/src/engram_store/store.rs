use anyhow::Result;
use ozymem_parser::{EngramContract, EngramTable};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use crate::engram_store::reader::FastEngramReader;

pub const ENGRAM_FILE: &str = "engrams.rkyv";

/// Entrada en la capa de deltas en RAM (MemTable)
#[derive(Clone, Debug, PartialEq)]
pub enum MemoryEntry {
    Active(EngramContract),
    Deleted, // Tombstone para símbolos eliminados
}

/// Motor híbrido incremental de Engrams (Two-Tier LSM-Tree: RAM + Base mmap)
#[derive(Clone)]
pub struct IncrementalEngramStore {
    base_reader: Arc<RwLock<Option<FastEngramReader>>>,
    mem_table: Arc<RwLock<HashMap<u64, MemoryEntry>>>,
    file_to_symbols: Arc<RwLock<HashMap<String, Vec<u64>>>>,
    storage_path: PathBuf,
}

impl IncrementalEngramStore {
    /// Abre o inicializa la tienda de Engrams en la ruta especificada
    pub fn open(storage_path: PathBuf) -> Result<Self> {
        let base_reader = if storage_path.exists() && fs::metadata(&storage_path)?.len() > 0 {
            match FastEngramReader::open(&storage_path) {
                Ok(reader) => Some(reader),
                Err(e) => {
                    eprintln!("[WARN] No se pudo leer base rkyv existente, iniciando fresca: {:?}", e);
                    None
                }
            }
        } else {
            None
        };

        Ok(Self {
            base_reader: Arc::new(RwLock::new(base_reader)),
            mem_table: Arc::new(RwLock::new(HashMap::new())),
            file_to_symbols: Arc::new(RwLock::new(HashMap::new())),
            storage_path,
        })
    }

    /// Abre la tienda de Engrams para un proyecto en su carpeta `.ozymem`
    pub fn open_for_project(project_dir: &Path) -> Result<Self> {
        let ozymem_dir = project_dir.join(".ozymem");
        fs::create_dir_all(&ozymem_dir).ok();
        let engram_path = ozymem_dir.join(ENGRAM_FILE);
        Self::open(engram_path)
    }

    /// Consulta un contrato por su ruta completa de símbolo (ej. "src/auth.rs::verify") en tiempo O(1)
    pub fn lookup(&self, symbol_path: &str) -> Option<EngramContract> {
        let key = EngramTable::hash_symbol(symbol_path);

        // 1. Nivel 1: MemTable en RAM (máxima frescura)
        if let Ok(mem) = self.mem_table.read() {
            if let Some(entry) = mem.get(&key) {
                return match entry {
                    MemoryEntry::Active(contract) => Some(contract.clone()),
                    MemoryEntry::Deleted => None,
                };
            }
        }

        // 2. Nivel 2: Snapshot base mapeado en memoria (mmap)
        if let Ok(reader_guard) = self.base_reader.read() {
            if let Some(ref reader) = *reader_guard {
                return reader.lookup(symbol_path);
            }
        }

        None
    }

    /// Inserta o actualiza un contrato directamente en la MemTable en RAM
    pub fn insert(&self, contract: EngramContract) {
        let key = EngramTable::hash_symbol(&contract.symbol_path);
        let mut mem = self.mem_table.write().unwrap();
        mem.insert(key, MemoryEntry::Active(contract));
    }

    /// Busca contratos por nombre de símbolo simple (ej. "open_db")
    pub fn lookup_by_name(&self, symbol_name: &str) -> Vec<EngramContract> {
        let mut results = Vec::new();
        let mut seen_keys = std::collections::HashSet::new();

        // 1. MemTable
        if let Ok(mem) = self.mem_table.read() {
            for (key, entry) in mem.iter() {
                seen_keys.insert(*key);
                if let MemoryEntry::Active(contract) = entry {
                    if contract.name == symbol_name || contract.symbol_path.ends_with(&format!("::{}", symbol_name)) {
                        results.push(contract.clone());
                    }
                }
            }
        }

        // 2. Base Reader
        if let Ok(reader_guard) = self.base_reader.read() {
            if let Some(ref reader) = *reader_guard {
                for contract in reader.iter_all() {
                    let key = EngramTable::hash_symbol(&contract.symbol_path);
                    if !seen_keys.contains(&key) {
                        if contract.name == symbol_name || contract.symbol_path.ends_with(&format!("::{}", symbol_name)) {
                            results.push(contract);
                        }
                    }
                }
            }
        }

        results
    }

    /// Actualiza incrementalmente los contratos extraídos de un archivo modificado
    pub fn update_file_incremental(&self, file_path: &str, new_contracts: Vec<EngramContract>) {
        let mut mem = self.mem_table.write().unwrap();
        let mut file_map = self.file_to_symbols.write().unwrap();

        // 1. Marcar como eliminados los símbolos previos asociados a este archivo
        if let Some(old_hashes) = file_map.get(file_path) {
            for old_hash in old_hashes {
                mem.insert(*old_hash, MemoryEntry::Deleted);
            }
        }

        // 2. Insertar los nuevos símbolos extraídos del AST
        let mut current_hashes = Vec::with_capacity(new_contracts.len());
        for contract in new_contracts {
            let key = EngramTable::hash_symbol(&contract.symbol_path);
            mem.insert(key, MemoryEntry::Active(contract));
            current_hashes.push(key);
        }

        // 3. Actualizar el índice inverso por archivo
        file_map.insert(file_path.to_string(), current_hashes);
    }

    /// Elimina todos los contratos asociados a un archivo eliminado
    pub fn remove_file_incremental(&self, file_path: &str) {
        let mut mem = self.mem_table.write().unwrap();
        let mut file_map = self.file_to_symbols.write().unwrap();

        if let Some(old_hashes) = file_map.remove(file_path) {
            for old_hash in old_hashes {
                mem.insert(old_hash, MemoryEntry::Deleted);
            }
        }
    }

    /// Compacta los deltas de la MemTable con la tabla base y persiste atómicamente a disco
    pub fn compact_and_persist(&self) -> Result<usize> {
        let mut full_table = EngramTable::new();

        // 1. Cargar estado base archivado
        {
            let reader_guard = self.base_reader.read().unwrap();
            if let Some(ref reader) = *reader_guard {
                for contract in reader.iter_all() {
                    full_table.insert(contract);
                }
            }
        }

        // 2. Aplicar deltas acumulados en MemTable
        {
            let mut mem = self.mem_table.write().unwrap();
            for (key, entry) in mem.drain() {
                match entry {
                    MemoryEntry::Active(contract) => {
                        full_table.entries.insert(key, contract);
                    }
                    MemoryEntry::Deleted => {
                        full_table.entries.remove(&key);
                    }
                }
            }
        }

        let total_saved = full_table.len();

        // 3. Serializar con rkyv a un archivo temporal
        if let Some(parent) = self.storage_path.parent() {
            fs::create_dir_all(parent).ok();
        }

        let temp_path = self.storage_path.with_extension("tmp");
        let bytes = rkyv::to_bytes::<_, 2048>(&full_table)
            .map_err(|e| anyhow::anyhow!("Error serializando tabla compactada de engrams: {:?}", e))?;

        let mut file = File::create(&temp_path)?;
        file.write_all(&bytes)?;
        file.flush()?;
        drop(file);

        // 4. Renombrar atómicamente en disco (reemplazo seguro)
        fs::rename(&temp_path, &self.storage_path)?;

        // 5. Reabrir el mapeo de memoria
        let new_reader = FastEngramReader::open(&self.storage_path)?;
        let mut reader_guard = self.base_reader.write().unwrap();
        *reader_guard = Some(new_reader);

        Ok(total_saved)
    }

    /// Retorna el total de contratos activos registrados
    pub fn total_contracts(&self) -> usize {
        let mut active_keys = std::collections::HashSet::new();
        if let Ok(reader_guard) = self.base_reader.read() {
            if let Some(ref reader) = *reader_guard {
                for c in reader.iter_all() {
                    active_keys.insert(EngramTable::hash_symbol(&c.symbol_path));
                }
            }
        }

        if let Ok(mem) = self.mem_table.read() {
            for (key, entry) in mem.iter() {
                match entry {
                    MemoryEntry::Active(_) => {
                        active_keys.insert(*key);
                    }
                    MemoryEntry::Deleted => {
                        active_keys.remove(key);
                    }
                }
            }
        }

        active_keys.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_incremental_engram_store_lifecycle() {
        let dir = tempdir().unwrap();
        let store_path = dir.path().join("test_engrams.rkyv");
        let store = IncrementalEngramStore::open(store_path.clone()).unwrap();

        let c1 = EngramContract {
            symbol_path: "src/auth.rs::login".to_string(),
            file_path: "src/auth.rs".to_string(),
            name: "login".to_string(),
            signature: "pub fn login(user: &str) -> bool".to_string(),
            line_number: 5,
            end_line: 20,
            kind: "Function".to_string(),
            language: "Rust".to_string(),
            dependencies: vec!["User".to_string()],
            doc_summary: "Inicia sesión".to_string(),
        };

        // 1. Insertar en MemTable
        store.update_file_incremental("src/auth.rs", vec![c1.clone()]);
        assert_eq!(store.lookup("src/auth.rs::login"), Some(c1.clone()));
        assert_eq!(store.total_contracts(), 1);

        // 2. Compactar y persistir a disco
        let saved = store.compact_and_persist().unwrap();
        assert_eq!(saved, 1);
        assert!(store_path.exists());

        // 3. Consultar desde base mmap tras compactación
        let lookup_res = store.lookup("src/auth.rs::login");
        assert!(lookup_res.is_some());
        assert_eq!(lookup_res.unwrap().name, "login");

        // 4. Actualización incremental (modificación)
        let mut c1_v2 = c1.clone();
        c1_v2.signature = "pub fn login(user: &str, pass: &str) -> bool".to_string();
        store.update_file_incremental("src/auth.rs", vec![c1_v2.clone()]);

        let updated_res = store.lookup("src/auth.rs::login").unwrap();
        assert_eq!(updated_res.signature, "pub fn login(user: &str, pass: &str) -> bool");

        // 5. Eliminación incremental (tombstone)
        store.remove_file_incremental("src/auth.rs");
        assert_eq!(store.lookup("src/auth.rs::login"), None);
        assert_eq!(store.total_contracts(), 0);

        // 6. Compactar tras eliminación
        store.compact_and_persist().unwrap();
        assert_eq!(store.total_contracts(), 0);
    }
}
