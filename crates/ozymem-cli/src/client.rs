use crate::config::{load_config, OzymemConfig};
use ozymem_core::graph_backend::SqliteBackend;
use ozymem_core::{FileGraphContext, GraphSummary, LessonRecord};
use serde::Serialize;
use std::path::PathBuf;

#[derive(Clone)]
pub enum BackendMode {
    Sqlite(SqliteBackend),
}

#[derive(Clone)]
pub struct BackendClient {
    pub mode: BackendMode,
}

impl BackendClient {
    pub fn tenant_id(&self) -> String {
        "local".to_string()
    }

    pub fn display_uri(&self) -> String {
        match &self.mode {
            BackendMode::Sqlite(sqlite) => sqlite.display_name(),
        }
    }

    pub async fn ping(&self) -> anyhow::Result<i64> {
        match &self.mode {
            BackendMode::Sqlite(sqlite) => {
                sqlite.ping().map_err(Into::into)
            }
        }
    }

    pub async fn clear_graph(&self) -> anyhow::Result<()> {
        match &self.mode {
            BackendMode::Sqlite(sqlite) => {
                sqlite.clear_graph(&self.tenant_id()).map_err(Into::into)
            }
        }
    }

    pub async fn save_file_definition(&self, file_map: &ozymem_parser::FileDefinitionMap, workspace_root: &str) -> anyhow::Result<()> {
        match &self.mode {
            BackendMode::Sqlite(sqlite) => {
                sqlite.save_file_definition(&self.tenant_id(), file_map, workspace_root).map_err(Into::into)
            }
        }
    }

    pub async fn save_dependency_relation(&self, origin_path: &str, destination_path: &str, workspace_root: &str) -> anyhow::Result<()> {
        match &self.mode {
            BackendMode::Sqlite(sqlite) => {
                sqlite.save_dependency_relation(&self.tenant_id(), origin_path, destination_path, workspace_root).map_err(Into::into)
            }
        }
    }

    pub async fn record_lesson(&self, file_path: &str, symbol_name: Option<&str>, error_context: &str, solution: &str, workspace_root: &str) -> anyhow::Result<()> {
        match &self.mode {
            BackendMode::Sqlite(sqlite) => {
                sqlite.record_lesson(&self.tenant_id(), file_path, symbol_name, error_context, solution, workspace_root).map_err(Into::into)
            }
        }
    }

    pub async fn clear_file_symbols_and_dependencies(&self, file_path: &str) -> anyhow::Result<()> {
        match &self.mode {
            BackendMode::Sqlite(sqlite) => {
                sqlite.clear_file_symbols_and_dependencies(&self.tenant_id(), file_path).map_err(Into::into)
            }
        }
    }

    pub async fn delete_file_definition(&self, file_path: &str) -> anyhow::Result<bool> {
        match &self.mode {
            BackendMode::Sqlite(sqlite) => {
                sqlite.delete_file_definition(&self.tenant_id(), file_path).map_err(Into::into)
            }
        }
    }

    pub async fn delete_project_files(&self, project_path: &str) -> anyhow::Result<i64> {
        match &self.mode {
            BackendMode::Sqlite(sqlite) => {
                sqlite.delete_project_files(&self.tenant_id(), project_path).map_err(Into::into)
            }
        }
    }

    pub async fn get_all_file_paths(&self) -> anyhow::Result<Vec<String>> {
        match &self.mode {
            BackendMode::Sqlite(sqlite) => {
                sqlite.get_all_file_paths(&self.tenant_id()).map_err(Into::into)
            }
        }
    }

    pub async fn get_historical_engram_solutions(&self, file_path: &str) -> anyhow::Result<Vec<String>> {
        match &self.mode {
            BackendMode::Sqlite(sqlite) => {
                sqlite.get_historical_engram_solutions(&self.tenant_id(), file_path).map_err(Into::into)
            }
        }
    }

    pub async fn get_recent_lessons(&self, limit: i64, file_filter: Option<String>) -> anyhow::Result<Vec<LessonRecord>> {
        match &self.mode {
            BackendMode::Sqlite(sqlite) => {
                sqlite.get_recent_lessons(&self.tenant_id(), limit, file_filter).map_err(Into::into)
            }
        }
    }

    pub async fn get_outgoing_dependencies(&self, file_path: &str) -> anyhow::Result<Vec<String>> {
        match &self.mode {
            BackendMode::Sqlite(sqlite) => {
                sqlite.get_outgoing_dependencies(&self.tenant_id(), file_path).map_err(Into::into)
            }
        }
    }

    pub async fn get_incoming_dependencies(&self, file_path: &str) -> anyhow::Result<Vec<String>> {
        match &self.mode {
            BackendMode::Sqlite(sqlite) => {
                sqlite.get_incoming_dependencies(&self.tenant_id(), file_path).map_err(Into::into)
            }
        }
    }

    pub async fn get_file_context(&self, file_path: &str) -> anyhow::Result<Option<FileGraphContext>> {
        match &self.mode {
            BackendMode::Sqlite(sqlite) => {
                sqlite.get_file_context(&self.tenant_id(), file_path).map_err(Into::into)
            }
        }
    }

    pub async fn get_graph_summary(&self) -> anyhow::Result<GraphSummary> {
        match &self.mode {
            BackendMode::Sqlite(sqlite) => {
                sqlite.get_graph_summary(&self.tenant_id()).map_err(Into::into)
            }
        }
    }

    pub async fn find_symbol(&self, symbol_name: &str, project_path: &str) -> anyhow::Result<Vec<String>> {
        match &self.mode {
            BackendMode::Sqlite(sqlite) => {
                sqlite.find_symbol(&self.tenant_id(), symbol_name, project_path).map_err(Into::into)
            }
        }
    }
}

pub struct AppContext {
    pub connection: BackendClient,
    pub display_uri: String,
}

#[derive(Debug, Serialize)]
pub struct StatusJsonOutput {
    pub database: DatabaseJsonOutput,
    pub metrics: StatusMetricsJson,
}

#[derive(Debug, Serialize)]
pub struct DatabaseJsonOutput {
    pub status: &'static str,
    pub uri: String,
}

#[derive(Debug, Serialize)]
pub struct StatusMetricsJson {
    pub files_indexed: i64,
    pub functions_mapped: i64,
    pub engrams_formed: i64,
}





pub async fn build_backend_client() -> anyhow::Result<BackendClient> {
    build_backend_client_with_path(None).await
}

pub async fn build_backend_client_with_path(project_path: Option<PathBuf>) -> anyhow::Result<BackendClient> {
    // Priority:
    //   1. Remote mode if OZYMEM_SERVER_URL or OZYBASE_MCP_TOKEN is http(s)://
    //   1. SQLite mode by default (no Memgraph needed)
    let (_, _config) = load_config().unwrap_or_else(|_| (PathBuf::new(), OzymemConfig::default()));

    // Default: SQLite mode, project-scoped DB
    let db_path = match project_path {
        Some(p) => p,
        None => std::env::current_dir()?,
    };
    let sqlite = SqliteBackend::open_for_project(&db_path)?;
    Ok(BackendClient {
        mode: BackendMode::Sqlite(sqlite)
    })
}

