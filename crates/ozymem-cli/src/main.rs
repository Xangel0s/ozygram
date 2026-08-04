use anyhow::Context;
use clap::{Parser, Subcommand};
use ozymem_core::{
    FileGraphContext, GraphSummary, LessonRecord,
    StoredFunction,
    graph_backend::{SqliteBackend, legacy_global_db_path, auto_manage_gitignore},
};
use ozymem_parser::{
    extract_dependency_hints, is_binary_file, is_internal_dependency_hint, parse_source,
    resolve_dependency_target, ParsedDependencyHint, SupportedLanguage, FileDefinitionMap,
};
use serde::{Serialize, Deserialize};
use std::collections::HashSet;
use std::convert::TryFrom;
use std::fs;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use walkdir::{DirEntry, WalkDir};

#[derive(Debug, Serialize, Deserialize)]
pub struct OzymemConfig {
    pub projects: std::collections::HashMap<String, String>,
}

impl Default for OzymemConfig {
    fn default() -> Self {
        Self {
            projects: std::collections::HashMap::new(),
        }
    }
}

fn load_config() -> anyhow::Result<(PathBuf, OzymemConfig)> {
    let home_dir = home::home_dir().context("No se pudo determinar el directorio home.")?;
    let config_path = home_dir.join(".ozymem.toml");
    if !config_path.exists() {
        let default_config = OzymemConfig::default();
        let toml_str = toml::to_string_pretty(&default_config)?;
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&config_path, toml_str)?;
        Ok((config_path, default_config))
    } else {
        let content = fs::read_to_string(&config_path)?;
        let config: OzymemConfig = toml::from_str(&content)?;
        Ok((config_path, config))
    }
}

fn save_config(path: &Path, config: &OzymemConfig) -> anyhow::Result<()> {
    let toml_str = toml::to_string_pretty(config)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, toml_str)?;
    Ok(())
}

#[derive(Parser)]
#[command(
    name = "ozymem-cli",
    version,
    about = "Interfaz local de Ozymem para terminal"
)]
struct Args {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Mostrar el estado logistico de Ozymem, incluyendo la tabla de watchers desde SQLite
    Status {
        #[arg(long)]
        json: bool,
    },
    /// Run Ozymem environment diagnostics (DB, directories, daemon)
    #[command(alias = "check")]
    Doctor {
        #[arg(long)]
        json: bool,
    },
    /// Escanear un directorio local de manera sincrona e indexarlo en el grafo
    Scan {
        path: String,

        #[arg(long)]
        reset: bool,

        #[arg(long)]
        force: bool,
    },
    /// Ver la base de lecciones aprendidas o errores corregidos
    Lessons {
        #[arg(short, long, default_value_t = 10)]
        limit: usize,

        #[arg(long)]
        file: Option<String>,
    },
    /// Mostrar el arbol de dependencias salientes de un archivo indexado
    Tree {
        file_path: String,

        #[arg(long, default_value_t = 2)]
        depth: u32,
    },
    /// Analizar el impacto de cambios (quien depende de este archivo en reversa)
    Trace {
        file_path: String,

        #[arg(long, default_value_t = 2)]
        depth: u32,
    },
    /// Actualizar e indexar cambios de archivos pendientes
    Update,
    /// Configurar u obtener patrones de ignore (.ozymemignore)
    Ignore,
    /// Auditar contratos de exportación Excel y cabeceras Content-Disposition
    Verify {
        #[arg(default_value = "export-contracts")]
        target: String,
    },
    /// Limpiar simbolos y dependencias de un archivo
    Clean {
        path: Option<PathBuf>,
    },
    /// Iniciar watcher local reactivo en primer plano sobre un directorio
    Watch {
        #[arg(default_value = ".")]
        path: String,

        #[arg(long)]
        force: bool,
    },
    /// Despertar el proyecto en el daemon central (modo Fluido)
    Start {
        path: Option<String>,

        #[arg(long)]
        force: bool,
    },
    /// Suspender o apagar un proyecto en el daemon central (modo Fluido)
    Stop {
        project: Option<String>,
    },
    /// Ver las bitacoras en vivo de un proyecto o del watcher activo
    Logs {
        project: Option<String>,
    },
    /// Registrar un nuevo proyecto y autorizar su ruta en el registro SQLite
    Register {
        name: Option<String>,
    },
    /// Eliminar un proyecto del registro SQLite
    #[command(alias = "unregister", alias = "remove")]
    Deregister {
        name: Option<String>,
    },
    /// Listar todos los proyectos registrados en registry.db
    #[command(alias = "projects")]
    List,
    /// Inicializar credenciales y entornos locales de Ozymem
    Init,
    Mcp {
        #[arg(default_value = "run")]
        action: String,
    },
    Parse {
        file_path: String,
    },
    Vector {
        #[command(subcommand)]
        subcommand: VectorSubcommand,
    },
    Dashboard,
}

#[derive(Debug, Subcommand)]
pub enum VectorSubcommand {
    Search {
        query: String,
        #[arg(short, long, default_value_t = 5)]
        limit: usize,
        #[arg(short, long)]
        category: Option<String>,
    },
    List {
        #[arg(short, long)]
        project: Option<String>,
        #[arg(short, long)]
        category: Option<String>,
    },
    Inspect {
        id: String,
    },
    Forget {
        id: String,
    },
    Prune {
        #[arg(long)]
        apply: bool,
    },
    Top {
        #[arg(short, long)]
        project: Option<String>,
    },
}



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

struct AppContext {
    connection: BackendClient,
    display_uri: String,
}

#[derive(Debug, Serialize)]
struct StatusJsonOutput {
    database: DatabaseJsonOutput,
    metrics: StatusMetricsJson,
}

#[derive(Debug, Serialize)]
struct DatabaseJsonOutput {
    status: &'static str,
    uri: String,
}

#[derive(Debug, Serialize)]
struct StatusMetricsJson {
    files_indexed: i64,
    functions_mapped: i64,
    engrams_formed: i64,
}


mod mcp;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    match &args.command {
        Commands::Doctor { json } => {
            return run_doctor(*json).await;
        }
        Commands::Start { path, force } => {
            return run_start(path.clone(), *force);
        }
        Commands::Stop { project } => {
            return run_stop(project.clone());
        }
        Commands::Logs { project } => {
            return run_logs_tail(project.clone()).await;
        }
        Commands::Register { name } => {
            return run_register(name.clone());
        }
        Commands::Deregister { name } => {
            return run_deregister(name.clone()).await;
        }
        Commands::List => {
            return run_list();
        }
        Commands::Init => {
            return run_init().await;
        }
        Commands::Mcp { .. } => {
            return mcp::run_mcp_server().await;
        }
        _ => {}
    }

    let connection = build_backend_client().await?;
    let is_sqlite = matches!(connection.mode, BackendMode::Sqlite(_));
    let display_uri = connection.display_uri();
    let context = AppContext {
        connection,
        display_uri,
    };

    // Legacy DB warning + .gitignore management (only once per session, on SQLite)
    if is_sqlite {
        let legacy = legacy_global_db_path();
        if legacy.exists() {
            eprintln!("[ozymem] legacy global DB detected at {}. Use `ozymem lessons --legacy` to view old data.", legacy.display());
        }
        if let Ok(cwd) = std::env::current_dir() {
            auto_manage_gitignore(&cwd).ok();
        }
    }

    match args.command {
        Commands::Status { json } => print_status(&context, json).await?,
        Commands::Scan { path, reset, force } => scan_directory(&context.connection, &path, reset, force).await?,
        Commands::Lessons { limit, file } => print_lessons(&context.connection, limit, file).await?,
        Commands::Tree { file_path, depth } => {
            print_tree(&context.connection, &file_path, depth).await?
        }
        Commands::Trace { file_path, depth } => {
            print_trace(&context.connection, &file_path, depth).await?
        }
        Commands::Update => run_update().await?,
        Commands::Ignore => run_ignore().await?,
        Commands::Verify { target: _ } => {
            let backend = ozymem_core::graph_backend::GraphBackend::open(None)?;
            if let Ok(cwd) = std::env::current_dir() {
                backend.full_scan(&cwd.to_string_lossy(), None)?;
            }
            let report = backend.verify_export_contracts()?;
            println!("Templates revisados: {}", report.templates_reviewed);
            println!("Endpoints de exportación: {}", report.endpoints_reviewed);
            println!("Versiones inconsistentes: {}", report.version_mismatches.len());
            println!("Templates faltantes: {}", report.missing_templates.len());

            for m in &report.version_mismatches {
                println!("  [ADVERTENCIA] {}", m.message);
            }
            for m in &report.missing_templates {
                println!("  [ERROR] {}", m.message);
            }
        }
        Commands::Watch { path, force } => run_watch(&context, &path, force).await?,
        Commands::Clean { path } => {
            if let Some(file_path) = path {
                let absolute_path = if file_path.is_absolute() {
                    file_path
                } else {
                    std::env::current_dir()?.join(&file_path)
                };
                let sanitized_path = clean_path(&absolute_path);
                match context.connection.delete_file_definition(&sanitized_path).await {
                    Ok(true) => {
                        println!("[Core] El archivo {} y sus funciones fueron eliminados del grafo.", sanitized_path);
                    }
                    Ok(false) => {
                        println!("[Core] El archivo {} no se encontró en el grafo. Nada que eliminar.", sanitized_path);
                    }
                    Err(e) => {
                        eprintln!("[Core] Error al eliminar el archivo {}: {:?}", sanitized_path, e);
                    }
                }
            } else {
                context.connection.clear_graph().await?;
                println!("[Core] Estructura física del grafo purgada. Conservando base de conocimientos a largo plazo.");
            }
        }
        Commands::Start { .. } => unreachable!(),
        Commands::Stop { .. } => unreachable!(),
        Commands::Logs { .. } => unreachable!(),
        Commands::Register { .. } => unreachable!(),
        Commands::Deregister { .. } => unreachable!(),
        Commands::List => unreachable!(),
        Commands::Init => unreachable!(),
        Commands::Mcp { .. } => unreachable!(),
        Commands::Doctor { .. } => unreachable!(),

        Commands::Parse { file_path } => {
            let path = Path::new(&file_path);
            let content = std::fs::read_to_string(path)?;
            let extension = path.extension()
                .and_then(|ext| ext.to_str())
                .unwrap_or("");
            let language = match extension.to_lowercase().as_str() {
                "py" => SupportedLanguage::Python,
                "go" => SupportedLanguage::Go,
                "rs" => SupportedLanguage::Rust,
                "js" => SupportedLanguage::JavaScript,
                "ts" | "tsx" => SupportedLanguage::TypeScriptReact,
                "sql" => SupportedLanguage::SQL,
                _ => SupportedLanguage::Unknown,
            };
            let map = parse_source(&file_path, language, &content)?;
            let dependency_hints = extract_dependency_hints(&file_path, language, &content)?;
            
            let output = serde_json::json!({
                "definition_map": map,
                "dependency_hints": dependency_hints,
            });
            println!("{}", serde_json::to_string(&output)?);
        }
        Commands::Vector { subcommand } => {
            run_vector_subcommand(&subcommand).await?;
        }
        Commands::Dashboard => {
            run_dashboard().await?;
        }
    }

    Ok(())
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

async fn print_status(context: &AppContext, json_output: bool) -> anyhow::Result<()> {
    context.connection.ping().await?;
    let summary = context.connection.get_graph_summary().await?;

    if json_output {
        let payload = StatusJsonOutput {
            database: DatabaseJsonOutput {
                status: "ACTIVE",
                uri: context.display_uri.clone(),
            },
            metrics: StatusMetricsJson {
                files_indexed: summary.file_count,
                functions_mapped: summary.function_count,
                engrams_formed: summary.engram_count,
            },
        };

        println!("{}", serde_json::to_string(&payload)?);
        return Ok(());
    }

    println!("OZYMEM CORE LOGISTICS");
    println!("---------------------");
    println!("Database Target: {}", context.display_uri);
    println!("Storage Status: ACTIVE");
    println!();
    println!("Graph Topology:");
    println!(
        "  Files: {} | Functions: {} | Engrams: {}",
        summary.file_count, summary.function_count, summary.engram_count
    );

    // Centralized Monitoring of Watchers by Project from SQLite Registry
    let home_dir = home::home_dir().unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    if let Ok(reg) = ozymem_core::registry::ProjectRegistry::open() {
        println!();
        println!("Project Environment Watchers (SQLite Registry):");
        println!("+-----------------+------------------------------------------+-----------------------+-------------------------------------------------------------+");
        println!("| {:<15} | {:<40} | {:<21} | {:<59} |", "Proyecto", "Ruta Asignada", "Estado", "Ultima Bitacora");
        println!("+-----------------+------------------------------------------+-----------------------+-------------------------------------------------------------+");
        
        if let Ok(projects) = reg.list_projects() {
            for p in projects {
                let log_file = home_dir.join(format!(".ozymem-{}.log", p.name));
                let shortened_path = shorten_path(&p.path, 40);
                
                let estado = match p.status {
                    ozymem_core::registry::ProjectStatus::Active => {
                        if let Some(pid) = p.watcher_pid {
                            format!("ACTIVO (PID: {})", pid)
                        } else {
                            "ACTIVO".to_string()
                        }
                    }
                    ozymem_core::registry::ProjectStatus::Sleeping => "SUSPENDIDO".to_string(),
                    ozymem_core::registry::ProjectStatus::Scanning => "ESCANEO".to_string(),
                };
                
                let mut ultima_bitacora = get_last_log_line(&log_file);
                if ultima_bitacora == "Watcher no inicializado." && p.status == ozymem_core::registry::ProjectStatus::Sleeping {
                    ultima_bitacora = "Suspendido en modo Fluido.".to_string();
                }
                
                println!("| {:<15} | {:<40} | {:<21} | {:<59} |", p.name, shortened_path, estado, ultima_bitacora);
            }
        }
        
        // General service row for the MCP Server (ozymem-mcp)
        let mcp_pid_file = home_dir.join(".ozymem-mcp.pid");
        let mcp_log_file = home_dir.join(".ozymem-mcp.log");
        let mut mcp_estado = "INACTIVO".to_string();
        let mut mcp_ultima_bitacora = "Servidor no inicializado.".to_string();
        
        if mcp_pid_file.exists() {
            if let Ok(pid_str) = std::fs::read_to_string(&mcp_pid_file) {
                if let Ok(pid) = pid_str.trim().parse::<u32>() {
                    if is_pid_alive(pid) {
                        mcp_estado = format!("ACTIVO (PID: {})", pid);
                        mcp_ultima_bitacora = get_last_log_line(&mcp_log_file);
                    } else {
                        mcp_estado = "TUMBADO".to_string();
                        let _ = std::fs::remove_file(&mcp_pid_file);
                        let last_line = get_last_log_line(&mcp_log_file);
                        mcp_ultima_bitacora = if last_line == "Watcher no inicializado." || last_line == "Bitacora vacia." || last_line == "Servidor no inicializado." {
                            "Proceso terminado inesperadamente.".to_string()
                        } else {
                            format!("Error: {}", last_line)
                        };
                    }
                }
            }
        }
        
        println!("| {:<15} | {:<40} | {:<21} | {:<59} |", "ozymem-mcp", "Servidor Global de Red / Stdio", mcp_estado, mcp_ultima_bitacora);
        println!("+-----------------+------------------------------------------+-----------------------+-------------------------------------------------------------+");
    }

    Ok(())
}

async fn scan_directory(
    connection: &BackendClient,
    target_path: &str,
    reset: bool,
    force: bool,
) -> anyhow::Result<()> {
    let canonical_target = canonicalize_target(target_path)?;

    // Validación del entorno: Debe estar registrado en ozymem.toml
    if !force {
        let mut path_is_registered = false;
        if let Ok((_, config)) = load_config() {
            let clean_target_lower = clean_path(&canonical_target).to_lowercase();
            for (_, registered_path_str) in &config.projects {
                if let Ok(reg_path_buf) = PathBuf::from(registered_path_str).canonicalize() {
                    let clean_reg_path_lower = clean_path(&reg_path_buf).to_lowercase();
                    if clean_target_lower == clean_reg_path_lower 
                        || clean_target_lower.starts_with(&format!("{}\\", clean_reg_path_lower)) 
                        || clean_target_lower.starts_with(&format!("{}/", clean_reg_path_lower)) 
                    {
                        path_is_registered = true;
                        break;
                    }
                }
            }
        }

        if !path_is_registered {
            eprintln!("[ERROR] Ruta no autorizada o no registrada en ozymem.toml: {}", canonical_target.display());
            return Err(anyhow::anyhow!("El directorio de ejecución no pertenece a ningún proyecto registrado. Regístralo primero o usa --force."));
        }
    }

    if !force && is_critical_root(&canonical_target) {
        return Err(anyhow::anyhow!(
            "Error: No se permite indexar desde la raíz del perfil de usuario por seguridad. Muévete a la carpeta de tu proyecto."
        ));
    }
    if reset {
        connection.clear_graph().await?;
        println!("[Core] Estructura física del grafo purgada. Conservando base de conocimientos a largo plazo.");
    }

    println!("Scanning directory: {}", canonical_target.display());

    let mut rust_dependency_batches: Vec<RustDependencyBatch> = Vec::new();
    let project_root = resolve_project_root(&canonical_target);
    let ignore_patterns = load_ignore_patterns_for_project(&project_root);
    
    // Lista negra estricta de carpetas para evitar entrar en ellas de raíz
    const CARPETAS_EXCLUIDAS: &[&str] = &[
        "vendor",
        "node_modules",
        "target",
        ".git",
        "storage",
    ];

    let should_descend_fn = |entry: &DirEntry| {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            return true;
        };

        let name_lower = name.to_lowercase();
        let path_str_lower = path.to_string_lossy().to_lowercase();

        // Carpetas excluidas estrictas (evita lectura/recorrido)
        if CARPETAS_EXCLUIDAS.iter().any(|&excl| name_lower == excl) {
            return false;
        }

        // Carpetas de Sistema
        if name_lower == "appdata"
            || name_lower == "program files"
            || name_lower == "programdata"
            || name_lower == "system32"
            || name_lower == "windows"
            || name_lower == ".svn"
        {
            return false;
        }

        // Entornos de Desarrollo
        if name_lower == "node_modules"
            || name_lower == "__pycache__"
            || name_lower == ".venv"
            || name_lower == "env"
            || name_lower == "target"
            || name_lower == "dist"
            || name_lower == "build"
        {
            return false;
        }

        // Navegadores y WebViews
        if name_lower == "ebwebview"
            || name_lower == "bravesoftware"
            || path_str_lower.contains("google/chrome")
            || path_str_lower.contains("google\\chrome")
            || path_str_lower.contains("microsoft/edge")
            || path_str_lower.contains("microsoft\\edge")
            || name_lower == "cache"
            || name_lower == "local storage"
        {
            return false;
        }

        // Herramientas de IA y Editores
        if name_lower == ".cursor"
            || name_lower == ".vscode"
            || name_lower == ".idea"
            || name_lower == ".config"
            || name_lower == ".anthropic"
            || name_lower == ".ollama"
        {
            return false;
        }

        if name.starts_with('.') && name != "." {
            return false;
        }

        if is_ignored_by_patterns(path, &ignore_patterns, &project_root) {
            return false;
        }

        true
    };

    for entry in WalkDir::new(&canonical_target)
        .into_iter()
        .filter_entry(should_descend_fn)
        .filter_map(Result::ok)
    {
        let path = entry.path();

        if !path.is_file() {
            continue;
        }

        if is_ignored_by_patterns(path, &ignore_patterns, &project_root) {
            continue;
        }

        if is_garbage_file(path) {
            continue;
        }

        if is_binary_file(path) {
            println!("Skipped binary file: {}", path.to_string_lossy());
            continue;
        }

        let language = get_language_from_path(path);
        let absolute_path = match fs::canonicalize(path) {
            Ok(canonical) => canonical,
            Err(error) => {
                eprintln!("Failed to canonicalize {}: {error}", path.display());
                continue;
            }
        };
        let absolute_file_path = clean_path(&absolute_path);

        let source_code = match fs::read_to_string(path) {
            Ok(contents) => contents,
            Err(error) => {
                if error.kind() == std::io::ErrorKind::InvalidData {
                    println!("Skipped binary/non-UTF8 file: {}", path.display());
                } else {
                    eprintln!("Failed to read {}: {error}", path.display());
                }
                continue;
            }
        };

        match parse_source(&absolute_file_path, language, &source_code) {
            Ok(map) => {
                println!(
                    "Indexed {} [{} / {}] ({} symbols)",
                    map.file_path,
                    map.language,
                    map.strategy.as_str(),
                    map.functions.len()
                );

                let ws_root = canonical_target.to_string_lossy().to_string();
                if let Err(error) = connection.save_file_definition(&map, &ws_root).await {
                    eprintln!("Failed to persist {}: {error}", map.file_path);
                }

                if matches!(language, SupportedLanguage::Rust) {
                    match extract_dependency_hints(&absolute_file_path, language, &source_code) {
                        Ok(hints) => {
                            let internal_hints: Vec<_> = hints
                                .into_iter()
                                .filter(is_internal_dependency_hint)
                                .collect();

                            if !internal_hints.is_empty() {
                                rust_dependency_batches.push(RustDependencyBatch {
                                    origin_path: absolute_file_path.clone(),
                                    hints: internal_hints,
                                });
                            }
                        }
                        Err(error) => eprintln!(
                            "Failed to extract Rust dependency hints for {}: {error}",
                            absolute_file_path
                        ),
                    }
                }
            }
            Err(error) => {
                eprintln!("Error parsing {}: {error}", absolute_file_path);
            }
        }
    }

    for batch in &rust_dependency_batches {
        for hint in &batch.hints {
            let Some(destination_path) = resolve_dependency_target(hint, &batch.origin_path) else {
                continue;
            };

            let dest_path_cleaned = clean_path(&destination_path);
            let ws_root_str = project_root.to_string_lossy().to_string();
            if let Err(error) = connection
                .save_dependency_relation(&batch.origin_path, &dest_path_cleaned, &ws_root_str)
                .await
            {
                eprintln!(
                    "Failed to persist dependency {} -> {}: {error}",
                    batch.origin_path,
                    destination_path.display()
                );
            }
        }
    }

    Ok(())
}

async fn print_lessons(
    connection: &BackendClient,
    limit: usize,
    file_filter: Option<String>,
) -> anyhow::Result<()> {
    let limit = i64::try_from(limit).context("limit is too large")?;
    let lessons = connection.get_recent_lessons(limit, file_filter).await?;

    println!("HISTORICAL KNOWLEDGE BASE");
    println!("-------------------------");

    if lessons.is_empty() {
        println!("No historical lessons found.");
        return Ok(());
    }

    for lesson in lessons {
        print_lesson_record(&lesson);
    }

    Ok(())
}

fn print_lesson_record(lesson: &LessonRecord) {
    println!("[Error: {}] -> {}", lesson.error_type, lesson.file_path);
    println!("Solution: {}", lesson.solution);
    println!();
}

async fn print_tree(
    connection: &BackendClient,
    file_path: &str,
    depth: u32,
) -> anyhow::Result<()> {
    let absolute_path = canonicalize_file(file_path)?;
    let absolute_path_text = clean_path(&absolute_path);
    let mut visited = HashSet::new();

    let tree = load_tree_node(connection, &absolute_path_text, depth, &mut visited).await?;
    if tree.context.is_none() {
        println!("No indexed file found for {}", absolute_path_text);
        return Ok(());
    }

    render_tree_node(&tree, "", true, true);
    Ok(())
}

#[derive(Debug)]
struct TreeNode {
    path: String,
    context: Option<FileGraphContext>,
    functions: Vec<StoredFunction>,
    dependencies: Vec<TreeNode>,
    truncated: bool,
    cyclic: bool,
}

fn load_tree_node<'a>(
    connection: &'a BackendClient,
    file_path: &'a str,
    remaining_depth: u32,
    visited: &'a mut HashSet<String>,
) -> Pin<Box<dyn Future<Output = anyhow::Result<TreeNode>> + 'a>> {
    Box::pin(async move {
        let context = connection.get_file_context(file_path).await?;
        let functions = context
            .as_ref()
            .map(|context| context.functions.clone())
            .unwrap_or_default();
        let dependencies = connection.get_outgoing_dependencies(file_path).await?;

        let cyclic = !visited.insert(file_path.to_string());
        let truncated = remaining_depth == 0 && !dependencies.is_empty();

        let mut rendered_dependencies = Vec::new();
        if !cyclic && remaining_depth > 0 {
            for dependency in dependencies {
                let child_context = connection.get_file_context(&dependency).await?;
                let child_cyclic = visited.contains(&dependency);

                if child_cyclic {
                    rendered_dependencies.push(TreeNode {
                        path: dependency,
                        context: child_context,
                        functions: Vec::new(),
                        dependencies: Vec::new(),
                        truncated: false,
                        cyclic: true,
                    });
                    continue;
                }

                rendered_dependencies.push(
                    load_tree_node(connection, &dependency, remaining_depth - 1, visited).await?,
                );
            }
        }

        Ok(TreeNode {
            path: file_path.to_string(),
            context,
            functions,
            dependencies: rendered_dependencies,
            truncated,
            cyclic,
        })
    })
}

fn render_tree_node(node: &TreeNode, prefix: &str, is_last: bool, is_root: bool) {
    if !is_root && node.cyclic {
        let branch = if is_last { "└──" } else { "├──" };
        println!("{}{} [DEPENDS_ON] File: {} (already listed)", prefix, branch, node.path);
        return;
    }

    if is_root {
        println!("File: {}", node.path);
    } else {
        let branch = if is_last { "└──" } else { "├──" };
        println!("{}{} [DEPENDS_ON] File: {}", prefix, branch, node.path);
    }

    let next_prefix = if is_root {
        String::new()
    } else if is_last {
        format!("{prefix}    ")
    } else {
        format!("{prefix}│   ")
    };

    let has_dependencies = !node.dependencies.is_empty() || node.truncated;
    let functions_branch = if has_dependencies {
        "├──"
    } else {
        "└──"
    };
    println!("{}{} Functions", next_prefix, functions_branch);

    if node.functions.is_empty() {
        let leaf_prefix = if has_dependencies {
            format!("{next_prefix}│   ")
        } else {
            format!("{next_prefix}    ")
        };
        println!("{}└── (none)", leaf_prefix);
    } else {
        let function_prefix = if has_dependencies {
            format!("{next_prefix}│   ")
        } else {
            format!("{next_prefix}    ")
        };

        for (index, function) in node.functions.iter().enumerate() {
            let branch = if index + 1 == node.functions.len() {
                "└──"
            } else {
                "├──"
            };
            println!(
                "{}{} [MEMBER: {}] {} (lines {}-{}) via {}",
                function_prefix,
                branch,
                function.kind.to_uppercase(),
                function.name,
                function.start_line,
                function.end_line,
                function.strategy
            );
        }
    }

    println!("{}└── Dependencies", next_prefix);

    let dependency_prefix = format!("{next_prefix}    ");
    if node.cyclic {
        println!("{}└── (cycle)", dependency_prefix);
        return;
    }

    if node.truncated {
        println!("{}└── (depth limit reached)", dependency_prefix);
        return;
    }

    if node.dependencies.is_empty() {
        println!("{}└── (none)", dependency_prefix);
        return;
    }

    for (index, dependency) in node.dependencies.iter().enumerate() {
        render_tree_node(
            dependency,
            &dependency_prefix,
            index + 1 == node.dependencies.len(),
            false,
        );
    }
}

async fn print_trace(
    connection: &BackendClient,
    file_path: &str,
    depth: u32,
) -> anyhow::Result<()> {
    let absolute_path = canonicalize_file(file_path)?;
    let absolute_path_text = clean_path(&absolute_path);
    let mut visited = HashSet::new();

    let trace = load_trace_node(connection, &absolute_path_text, depth, &mut visited).await?;
    if trace.context.is_none() {
        println!("No indexed file found for {}", absolute_path_text);
        return Ok(());
    }

    render_trace_node(&trace, "", true, true);
    Ok(())
}

fn load_trace_node<'a>(
    connection: &'a BackendClient,
    file_path: &'a str,
    remaining_depth: u32,
    visited: &'a mut HashSet<String>,
) -> Pin<Box<dyn Future<Output = anyhow::Result<TreeNode>> + 'a>> {
    Box::pin(async move {
        let context = connection.get_file_context(file_path).await?;
        let functions = context
            .as_ref()
            .map(|context| context.functions.clone())
            .unwrap_or_default();
        let incoming = connection.get_incoming_dependencies(file_path).await?;

        let cyclic = !visited.insert(file_path.to_string());
        let truncated = remaining_depth == 0 && !incoming.is_empty();

        let mut rendered_incoming = Vec::new();
        if !cyclic && remaining_depth > 0 {
            for dependent in incoming {
                let child_context = connection.get_file_context(&dependent).await?;
                let child_cyclic = visited.contains(&dependent);

                if child_cyclic {
                    rendered_incoming.push(TreeNode {
                        path: dependent,
                        context: child_context,
                        functions: Vec::new(),
                        dependencies: Vec::new(),
                        truncated: false,
                        cyclic: true,
                      });
                      continue;
                }

                rendered_incoming.push(
                    load_trace_node(connection, &dependent, remaining_depth - 1, visited).await?,
                );
            }
        }

        Ok(TreeNode {
            path: file_path.to_string(),
            context,
            functions,
            dependencies: rendered_incoming,
            truncated,
            cyclic,
        })
    })
}

fn render_trace_node(node: &TreeNode, prefix: &str, is_last: bool, is_root: bool) {
    if !is_root && node.cyclic {
        let branch = if is_last { "└──" } else { "├──" };
        println!("{}{} [IMPACTED_BY] File: {} (already listed)", prefix, branch, node.path);
        return;
    }

    if is_root {
        println!("File: {} (Target)", node.path);
    } else {
        let branch = if is_last { "└──" } else { "├──" };
        println!("{}{} [IMPACTED_BY] File: {}", prefix, branch, node.path);
    }

    let next_prefix = if is_root {
        String::new()
    } else if is_last {
        format!("{prefix}    ")
    } else {
        format!("{prefix}│   ")
    };

    let has_incoming = !node.dependencies.is_empty() || node.truncated;
    let functions_branch = if has_incoming {
        "├──"
    } else {
        "└──"
    };
    println!("{}{} Functions", next_prefix, functions_branch);

    if node.functions.is_empty() {
        let leaf_prefix = if has_incoming {
            format!("{next_prefix}│   ")
        } else {
            format!("{next_prefix}    ")
        };
        println!("{}└── (none)", leaf_prefix);
    } else {
        let function_prefix = if has_incoming {
            format!("{next_prefix}│   ")
        } else {
            format!("{next_prefix}    ")
        };

        for (index, function) in node.functions.iter().enumerate() {
            let branch = if index + 1 == node.functions.len() {
                "└──"
            } else {
                "├──"
            };
            println!(
                "{}{} [MEMBER: {}] {} (lines {}-{}) via {}",
                function_prefix,
                branch,
                function.kind.to_uppercase(),
                function.name,
                function.start_line,
                function.end_line,
                function.strategy
            );
        }
    }

    println!("{}└── Incoming Dependencies", next_prefix);

    let incoming_prefix = format!("{next_prefix}    ");
    if node.cyclic {
        println!("{}└── (cycle)", incoming_prefix);
        return;
    }

    if node.truncated {
        println!("{}└── (depth limit reached)", incoming_prefix);
        return;
    }

    if node.dependencies.is_empty() {
        println!("{}└── (none)", incoming_prefix);
        return;
    }

    for (index, dependent) in node.dependencies.iter().enumerate() {
        render_trace_node(
            dependent,
            &incoming_prefix,
            index + 1 == node.dependencies.len(),
            false,
        );
    }
}

fn print_update_error() {
    println!("Error: El subcomando 'update' no puede ejecutarse en este directorio.");
    println!("---------------------------------------------------------------------");
    println!("Razón: Esta carpeta no es un repositorio Git válido o no cuenta con");
    println!("       el origen remoto del ecosistema Ozymem.");
    println!();
    println!("Solución: Para buscar y aplicar actualizaciones del sistema, primero");
    println!("          debes navegar a la carpeta raíz de tu monorepo local.");
}

fn canonicalize_target(target_path: &str) -> anyhow::Result<PathBuf> {
    let path = Path::new(target_path);
    if !path.exists() {
        // Intenta ver si coincide con el nombre de un proyecto registrado en la configuración
        if let Ok((_, config)) = load_config() {
            if let Some(registered_path) = config.projects.get(target_path) {
                let reg_path = Path::new(registered_path);
                if reg_path.exists() {
                    return fs::canonicalize(reg_path)
                        .with_context(|| format!("failed to resolve registered path for project: {target_path}"));
                }
            }
        }
    }
    fs::canonicalize(path).with_context(|| format!("failed to resolve path: {target_path}"))
}

async fn run_update() -> anyhow::Result<()> {
    // 1. Silently execute git fetch origin
    let fetch_status = std::process::Command::new("git")
        .args(&["fetch", "origin"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    let fetch_success = match fetch_status {
        Ok(status) => status.success(),
        Err(_) => false,
    };

    if !fetch_success {
        print_update_error();
        return Ok(());
    }

    // 2. Get current branch name
    let branch_output = match std::process::Command::new("git")
        .args(&["rev-parse", "--abbrev-ref", "HEAD"])
        .output() {
            Ok(output) => output,
            Err(_) => {
                print_update_error();
                return Ok(());
            }
        };
    if !branch_output.status.success() {
        print_update_error();
        return Ok(());
    }
    let branch = String::from_utf8_lossy(&branch_output.stdout).trim().to_string();

    // 3. Compare local and remote hashes
    let local_output = match std::process::Command::new("git")
        .args(&["rev-parse", "HEAD"])
        .output() {
            Ok(output) => output,
            Err(_) => {
                print_update_error();
                return Ok(());
            }
        };
    if !local_output.status.success() {
        print_update_error();
        return Ok(());
    }
    let local_hash = String::from_utf8_lossy(&local_output.stdout).trim().to_string();

    let remote_ref = format!("origin/{}", branch);
    let remote_output = match std::process::Command::new("git")
        .args(&["rev-parse", &remote_ref])
        .output() {
            Ok(output) => output,
            Err(_) => {
                print_update_error();
                return Ok(());
            }
        };

    if !remote_output.status.success() {
        print_update_error();
        return Ok(());
    }
    let remote_hash = String::from_utf8_lossy(&remote_output.stdout).trim().to_string();

    // Check if HEAD is ancestor of remote (local is behind)
    let is_behind = if local_hash != remote_hash {
        let ancestor_status = std::process::Command::new("git")
            .args(&["merge-base", "--is-ancestor", "HEAD", &remote_ref])
            .status();
        match ancestor_status {
            Ok(status) => status.success(),
            Err(_) => false,
        }
    } else {
        false
    };

    if is_behind {
        println!("A new version of Ozymem is available. Updating...");
        
        let pull_status = std::process::Command::new("git")
            .arg("pull")
            .status()?;
        if !pull_status.success() {
            anyhow::bail!("Failed to execute 'git pull'.");
        }

        println!("Reinstalling ozymem-cli globally...");
        let install_status = std::process::Command::new("cargo")
            .args(&["install", "--path", "crates/ozymem-cli", "--force"])
            .status()?;
        if !install_status.success() {
            anyhow::bail!("Failed to execute 'cargo install'.");
        }

        println!("Ozymem updated successfully!");
    } else {
        println!("Ozymem is already on the latest version.");
    }

    Ok(())
}

async fn run_watch(context: &AppContext, target_path: &str, force: bool) -> anyhow::Result<()> {
    check_directory_authorized(target_path)?;

    let canonical_target = canonicalize_target(target_path)?;
    let project_root = resolve_project_root(&canonical_target);
    let mut ignore_patterns = load_ignore_patterns_for_project(&project_root);

    if !force && is_critical_root(&canonical_target) {
        return Err(anyhow::anyhow!(
            "Error: No se permite indexar desde la raíz del perfil de usuario por seguridad. Muévete a la carpeta de tu proyecto."
        ));
    }
    // 1. Quick health check connecting to the backend
    if let Err(e) = context.connection.ping().await {
        eprintln!("Error: Could not connect to backend database. {e}");
        return Ok(());
    }

    // 2. Escaneo inicial de consistencia
    eprintln!("[WATCHER] Iniciando escaneo rápido de consistencia...");
    if let Err(e) = scan_directory(&context.connection, target_path, false, force).await {
        eprintln!("Advertencia en escaneo inicial: {e}");
    }

    // 3. Inicializar notify
    let (tx, rx) = std::sync::mpsc::channel();
    let mut watcher = notify::recommended_watcher(move |res| {
        if let Err(e) = tx.send(res) {
            eprintln!("Watcher channel send error: {:?}", e);
        }
    })?;

    use notify::Watcher;
    watcher.watch(&canonical_target, notify::RecursiveMode::Recursive)?;
    eprintln!("[WATCHER] Vigilando cambios reactivamente en: {}...", canonical_target.display());

    // 4. Bucle reactivo de eventos
    for res in rx {
        match res {
            Ok(event) => {
                let mut ignore_changed = false;
                for path in &event.paths {
                    if let Some(filename) = path.file_name().and_then(|f| f.to_str()) {
                        if filename == ".ozymemignore" || filename == ".gitignore" {
                            ignore_changed = true;
                            break;
                        }
                    }
                }

                if ignore_changed {
                    eprintln!("[WATCHER] Detectado cambio en archivos de ignore (.ozymemignore / .gitignore). Sincronizando y purgando archivos ignorados del grafo...");
                    ignore_patterns = load_ignore_patterns_for_project(&project_root);
                    match context.connection.get_all_file_paths().await {
                        Ok(all_paths) => {
                            for file_path_str in all_paths {
                                let path_obj = Path::new(&file_path_str);
                                if is_ignored_by_patterns(path_obj, &ignore_patterns, &project_root) {
                                    let _ = context.connection.delete_file_definition(&file_path_str).await;
                                }
                            }
                        }
                        Err(_) => {}
                    }
                }

                if event.kind.is_modify() || event.kind.is_create() {
                    for path in event.paths {
                        if let Some(filename) = path.file_name().and_then(|f| f.to_str()) {
                            if filename == ".ozymemignore" || filename == ".gitignore" {
                                continue;
                            }
                        }
                        if should_watch_path(&path, &ignore_patterns, &project_root) {
                            eprintln!("[WATCHER] Re-indexando incrementalmente: {}", path.display());
                            if let Err(e) = index_single_file(&context.connection, &path).await {
                                eprintln!("Error al indexar archivo {}: {:?}", path.display(), e);
                            }
                        }
                    }
                } else if event.kind.is_remove() {
                    for path in event.paths {
                        if let Some(filename) = path.file_name().and_then(|f| f.to_str()) {
                            if filename == ".ozymemignore" || filename == ".gitignore" {
                                continue;
                            }
                        }
                        if should_process_delete(&path, &ignore_patterns, &project_root) {
                            let resolved = canonicalize_deleted_path(&path).unwrap_or_else(|| path.clone());
                            let absolute_file_path = clean_path(&resolved);
                            eprintln!("[WATCHER] Detectada eliminación de: {}. Limpiando grafo...", absolute_file_path);
                            if let Err(e) = context.connection.delete_file_definition(&absolute_file_path).await {
                                eprintln!("Error al limpiar archivo {}: {:?}", absolute_file_path, e);
                            }
                        }
                    }
                }
            }
            Err(e) => eprintln!("Watcher error: {:?}", e),
        }
    }

    Ok(())
}

fn clean_path(path: &Path) -> String {
    let s = path.to_string_lossy().to_string();
    if s.starts_with(r"\\?\") {
        s[4..].to_string()
    } else {
        s
    }
}

fn canonicalize_deleted_path(path: &Path) -> Option<PathBuf> {
    let parent = path.parent()?;
    let canonical_parent = fs::canonicalize(parent).ok()?;
    let file_name = path.file_name()?;
    Some(canonical_parent.join(file_name))
}

fn should_process_delete(path: &Path, ignore_patterns: &[String], project_root: &Path) -> bool {
    if is_ignored_by_patterns(path, ignore_patterns, project_root) {
        return false;
    }
    if is_binary_file(path) {
        return false;
    }
    if is_garbage_file(path) {
        return false;
    }
    let path_str_lower = path.to_string_lossy().to_lowercase();
    for component in path.components() {
        if let Some(name) = component.as_os_str().to_str() {
            let name_lower = name.to_lowercase();

            // Carpetas de Sistema
            if name_lower == "appdata"
                || name_lower == "program files"
                || name_lower == "programdata"
                || name_lower == "system32"
                || name_lower == "windows"
                || name_lower == ".git"
                || name_lower == ".svn"
            {
                return false;
            }

            // Entornos de Desarrollo
            if name_lower == "node_modules"
                || name_lower == "__pycache__"
                || name_lower == ".venv"
                || name_lower == "env"
                || name_lower == "target"
                || name_lower == "dist"
                || name_lower == "build"
            {
                return false;
            }

            // Navegadores y WebViews
            if name_lower == "ebwebview"
                || name_lower == "bravesoftware"
                || path_str_lower.contains("google/chrome")
                || path_str_lower.contains("google\\chrome")
                || path_str_lower.contains("microsoft/edge")
                || path_str_lower.contains("microsoft\\edge")
                || name_lower == "cache"
                || name_lower == "local storage"
            {
                return false;
            }

            // Herramientas de IA y Editores
            if name_lower == ".cursor"
                || name_lower == ".vscode"
                || name_lower == ".idea"
                || name_lower == ".config"
                || name_lower == ".anthropic"
                || name_lower == ".ollama"
            {
                return false;
            }

            if name.starts_with('.') && name != "." {
                return false;
            }
        }
    }
    true
}

fn should_watch_path(path: &Path, ignore_patterns: &[String], project_root: &Path) -> bool {
    if is_ignored_by_patterns(path, ignore_patterns, project_root) {
        return false;
    }
    if !path.is_file() {
        return false;
    }
    if is_binary_file(path) {
        return false;
    }
    if is_garbage_file(path) {
        return false;
    }
    let path_str_lower = path.to_string_lossy().to_lowercase();
    for component in path.components() {
        if let Some(name) = component.as_os_str().to_str() {
            let name_lower = name.to_lowercase();

            // Carpetas de Sistema
            if name_lower == "appdata"
                || name_lower == "program files"
                || name_lower == "programdata"
                || name_lower == "system32"
                || name_lower == "windows"
                || name_lower == ".git"
                || name_lower == ".svn"
            {
                return false;
            }

            // Entornos de Desarrollo
            if name_lower == "node_modules"
                || name_lower == "__pycache__"
                || name_lower == ".venv"
                || name_lower == "env"
                || name_lower == "target"
                || name_lower == "dist"
                || name_lower == "build"
            {
                return false;
            }

            // Navegadores y WebViews
            if name_lower == "ebwebview"
                || name_lower == "bravesoftware"
                || path_str_lower.contains("google/chrome")
                || path_str_lower.contains("google\\chrome")
                || path_str_lower.contains("microsoft/edge")
                || path_str_lower.contains("microsoft\\edge")
                || name_lower == "cache"
                || name_lower == "local storage"
            {
                return false;
            }

            // Herramientas de IA y Editores
            if name_lower == ".cursor"
                || name_lower == ".vscode"
                || name_lower == ".idea"
                || name_lower == ".config"
                || name_lower == ".anthropic"
                || name_lower == ".ollama"
            {
                return false;
            }

            if name.starts_with('.') && name != "." {
                return false;
            }
        }
    }
    true
}

async fn index_single_file(connection: &BackendClient, path: &Path) -> anyhow::Result<()> {
    let language = get_language_from_path(path);
    let absolute_path = fs::canonicalize(path)?;
    let absolute_file_path = clean_path(&absolute_path);

    let source_code = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) => {
            if error.kind() == std::io::ErrorKind::InvalidData {
                println!("Skipped binary/non-UTF8 file: {}", path.display());
            } else {
                eprintln!("Failed to read {}: {error}", path.display());
            }
            return Ok(());
        }
    };

    let map = parse_source(&absolute_file_path, language, &source_code)?;
    let _ = connection.clear_file_symbols_and_dependencies(&absolute_file_path).await;
    let project_root = resolve_project_root(path).to_string_lossy().to_string();
    connection.save_file_definition(&map, &project_root).await?;

    if matches!(language, SupportedLanguage::Rust) {
        if let Ok(hints) = extract_dependency_hints(&absolute_file_path, language, &source_code) {
            let internal_hints: Vec<_> = hints.into_iter().filter(is_internal_dependency_hint).collect();
            for hint in internal_hints {
                if let Some(destination_path) = resolve_dependency_target(&hint, &absolute_file_path) {
                    let dest_path_cleaned = clean_path(&destination_path);
                    let _ = connection.save_dependency_relation(&absolute_file_path, &dest_path_cleaned, &project_root).await;
                }
            }
        }
    }

    Ok(())
}

fn canonicalize_file(file_path: &str) -> anyhow::Result<PathBuf> {
    canonicalize_target(file_path)
}

fn resolve_project_root(target_path: &Path) -> PathBuf {
    if let Ok((_, config)) = load_config() {
        let clean_target_lower = clean_path(target_path).to_lowercase();
        for (_, registered_path_str) in &config.projects {
            if let Ok(reg_path_buf) = PathBuf::from(registered_path_str).canonicalize() {
                let clean_reg_path_lower = clean_path(&reg_path_buf).to_lowercase();
                if clean_target_lower == clean_reg_path_lower 
                    || clean_target_lower.starts_with(&format!("{}\\", clean_reg_path_lower)) 
                    || clean_target_lower.starts_with(&format!("{}/", clean_reg_path_lower)) 
                {
                    return reg_path_buf;
                }
            }
        }
    }
    target_path.to_path_buf()
}

fn load_ignore_patterns_for_project(project_root: &Path) -> Vec<String> {
    let mut patterns = Vec::new();

    // 1. Cargar .ozymemignore
    let ozymemignore_path = project_root.join(".ozymemignore");
    if let Ok(content) = fs::read_to_string(&ozymemignore_path) {
        for line in content.lines() {
            let trimmed = line.trim();
            if !trimmed.is_empty() && !trimmed.starts_with('#') {
                patterns.push(trimmed.to_string());
            }
        }
    }

    // 2. Cargar .gitignore (Manejo Dinámico)
    let gitignore_path = project_root.join(".gitignore");
    if let Ok(content) = fs::read_to_string(&gitignore_path) {
        for line in content.lines() {
            let trimmed = line.trim();
            if !trimmed.is_empty() && !trimmed.starts_with('#') {
                patterns.push(trimmed.to_string());
            }
        }
    }

    patterns
}

fn is_ignored_by_patterns(path: &Path, patterns: &[String], project_root: &Path) -> bool {
    if patterns.is_empty() {
        return false;
    }
    let cleaned_path_str = clean_path(path);
    let cleaned_path = Path::new(&cleaned_path_str);

    let relative_path = if let Ok(rel) = cleaned_path.strip_prefix(project_root) {
        rel.to_path_buf()
    } else {
        cleaned_path.to_path_buf()
    };

    let rel_str = relative_path.to_string_lossy().replace('\\', "/");
    let rel_str_lower = rel_str.to_lowercase();

    for pattern in patterns {
        let pattern_lower = pattern.to_lowercase().replace('\\', "/");
        if rel_str_lower == pattern_lower {
            return true;
        }
        let prefix_dir = format!("{}/", pattern_lower);
        if rel_str_lower.starts_with(&prefix_dir) {
            return true;
        }
        for component in relative_path.components() {
            if let Some(comp_str) = component.as_os_str().to_str() {
                if comp_str.to_lowercase() == pattern_lower {
                    return true;
                }
            }
        }
    }
    false
}

async fn run_ignore() -> anyhow::Result<()> {
    let current_dir = std::env::current_dir()?;
    let mut entries = Vec::new();
    for entry in fs::read_dir(&current_dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name == ".git" {
            continue;
        }
        entries.push(name);
    }
    entries.sort();

    if entries.is_empty() {
        println!("No files or directories found in the current directory.");
        return Ok(());
    }

    use dialoguer::{theme::ColorfulTheme, MultiSelect};
    let selections = MultiSelect::with_theme(&ColorfulTheme::default())
        .with_prompt("Selecciona los archivos/directorios a ignorar (flechas para mover, espacio para marcar, enter para confirmar)")
        .items(&entries)
        .interact()?;

    let mut ignore_file = fs::File::create(".ozymemignore")?;
    use std::io::Write;
    for index in selections {
        writeln!(ignore_file, "{}", entries[index])?;
    }

    println!("[Config] Archivo .ozymemignore guardado correctamente.");
    Ok(())
}

fn get_language_from_path(path: &Path) -> SupportedLanguage {
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    match extension.as_str() {
        "py" => SupportedLanguage::Python,
        "go" => SupportedLanguage::Go,
        "rs" => SupportedLanguage::Rust,
        "js" => SupportedLanguage::JavaScript,
        "ts" | "tsx" | "jsx" => SupportedLanguage::TypeScriptReact,
        "sql" => SupportedLanguage::SQL,
        _ => SupportedLanguage::Unknown,
    }
}

struct RustDependencyBatch {
    origin_path: String,
    hints: Vec<ParsedDependencyHint>,
}

fn is_critical_root(path: &Path) -> bool {
    let mut components = path.components();
    let _first = components.next();
    let second = components.next();
    if second.is_none() || (second.is_some() && components.next().is_none() && matches!(second.unwrap(), std::path::Component::RootDir)) {
        return true;
    }
    let path_str = path.to_string_lossy().to_lowercase();
    let path_cleaned = path_str.trim_end_matches('\\').trim_end_matches('/');
    if path_cleaned == "c:\\users" || path_cleaned == "c:/users" {
        return true;
    }
    if let Ok(user_profile) = std::env::var("USERPROFILE") {
        if path_cleaned == user_profile.to_lowercase().trim_end_matches('\\').trim_end_matches('/') {
            return true;
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        if path_cleaned == home.to_lowercase().trim_end_matches('\\').trim_end_matches('/') {
            return true;
        }
    }
    false
}

fn is_garbage_file(path: &Path) -> bool {
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        let ext_lower = ext.to_lowercase();
        match ext_lower.as_str() {
            "log" | "history" | "bin" | "dat" | "cache" | "exe" | "dll" | "so" | "dylib" | "db" | "sqlite" | "sqlite3" | "pstat" | "lock" | "pid" => true,
            _ => false,
        }
    } else {
        false
    }
}

fn run_start(path_arg: Option<String>, force: bool) -> anyhow::Result<()> {
    let target_path = path_arg.unwrap_or_else(|| ".".to_string());
    
    // Authorization Check
    check_directory_authorized(&target_path)?;

    let canonical = canonicalize_target(&target_path)?;
    if !force && is_critical_root(&canonical) {
        let err_msg = "Error: No se permite indexar desde la raíz del perfil de usuario por seguridad. Muévete a la carpeta de tu proyecto.";
        println!("{}", err_msg);
        return Err(anyhow::anyhow!(err_msg));
    }

    // Get project identifier
    let (project_name, _) = get_project_identifier(&target_path)?;

    // Send wake command to daemon socket
    let clean_path_str = clean_path(&canonical);
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async {
        match send_daemon_command_cli(&serde_json::json!({
            "action": "wake",
            "project_name": project_name,
            "project_path": clean_path_str
        })).await {
            Ok(res) => {
                println!("[SUCCESS] Daemon despertó el proyecto con éxito: {}", serde_json::to_string_pretty(&res).unwrap_or_default());
            }
            Err(e) => {
                println!("[WARNING] No se pudo conectar al daemon central (¿está encendido?). Detalle: {e}");
                println!("[INFO] Arrancando watcher en modo fallback heredado local...");
                
                // Fallback watcher code...
                let home_dir = home::home_dir().unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
                let pid_file = home_dir.join(format!(".ozymem-{}.pid", project_name));
                if pid_file.exists() {
                    if let Ok(pid_str) = std::fs::read_to_string(&pid_file) {
                        if let Ok(pid) = pid_str.trim().parse::<u32>() {
                            if is_pid_alive(pid) {
                                println!("[INFO] El watcher para '{}' ya se encuentra activo (PID: {}).", project_name, pid);
                                return;
                            }
                        }
                    }
                }

                let exe_path = std::env::current_exe().unwrap();
                let mut cmd = std::process::Command::new(exe_path);
                cmd.arg("watch").arg(&target_path);
                if force {
                    cmd.arg("--force");
                }

                #[cfg(windows)]
                {
                    use std::os::windows::process::CommandExt;
                    cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
                }

                let log_path = home_dir.join(format!(".ozymem-{}.log", project_name));
                let log_file = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&log_path).unwrap();
                let stdout_file = log_file.try_clone().unwrap();
                let stderr_file = log_file.try_clone().unwrap();
                cmd.stdout(stdout_file);
                cmd.stderr(stderr_file);

                let child = cmd.spawn().unwrap();
                let pid = child.id();
                let _ = std::fs::write(&pid_file, pid.to_string());
                println!("[SUCCESS] Watcher para '{}' iniciado en segundo plano (PID: {}).", project_name, pid);
            }
        }
    });

    Ok(())
}

fn check_directory_authorized(target_path: &str) -> anyhow::Result<()> {
    let canonical_target = canonicalize_target(target_path)?;
    let clean_target = clean_path(&canonical_target);
    let clean_target_lower = clean_target.to_lowercase();
    
    let reg = ozymem_core::registry::ProjectRegistry::open()?;
    let project = reg.get_project_by_path(&clean_target_lower)?;
    
    if project.is_none() {
        return Err(anyhow::anyhow!(
            "Error: Este directorio no está registrado en ozymem. Ejecuta 'ozymem register' primero para autorizarlo."
        ));
    }
    
    Ok(())
}

fn run_register(name_arg: Option<String>) -> anyhow::Result<()> {
    let current_dir = std::env::current_dir()?;
    let canonical_path = current_dir.canonicalize()
        .context("Failed to canonicalize current directory path")?;
    let cleaned_path = clean_path(&canonical_path);

    let name = match name_arg {
        Some(n) => n,
        None => {
            use dialoguer::Input;
            Input::<String>::new()
                .with_prompt("Nombre del proyecto")
                .interact_text()?
        }
    };

    let reg = ozymem_core::registry::ProjectRegistry::open()?;
    let project = reg.register(&name, &cleaned_path)?;

    println!("[SUCCESS] Proyecto '{}' registrado en registry.db en {}", project.name, project.path);
    Ok(())
}

async fn run_deregister(name_arg: Option<String>) -> anyhow::Result<()> {
    let (config_path, mut config) = load_config()?;
    if config.projects.is_empty() {
        println!("[INFO] No hay proyectos registrados todavía.");
        return Ok(());
    }

    let project_name = match name_arg {
        Some(p) => p,
        None => {
            let current_dir = std::env::current_dir()?;
            let cleaned_curr = clean_path(&current_dir.canonicalize()?);
            let mut found_name = None;
            for (name, registered_path_str) in &config.projects {
                if let Ok(reg_path_buf) = PathBuf::from(registered_path_str).canonicalize() {
                    if clean_path(&reg_path_buf) == cleaned_curr {
                        found_name = Some(name.clone());
                        break;
                    }
                }
            }
            
            match found_name {
                Some(name) => {
                    use dialoguer::Confirm;
                    if Confirm::with_theme(&dialoguer::theme::ColorfulTheme::default())
                        .with_prompt(format!("¿Desea desregistrar el proyecto '{}' del directorio actual?", name))
                        .default(true)
                        .interact()?
                    {
                        name
                    } else {
                        println!("Operación cancelada.");
                        return Ok(());
                    }
                }
                None => {
                    let mut project_names: Vec<String> = config.projects.keys().cloned().collect();
                    project_names.sort();
                    use dialoguer::{theme::ColorfulTheme, Select};
                    let selection = Select::with_theme(&ColorfulTheme::default())
                        .with_prompt("Seleccione el proyecto que desea desregistrar")
                        .items(&project_names)
                        .default(0)
                        .interact_opt()?;
                    match selection {
                        Some(idx) => project_names[idx].clone(),
                        None => {
                            println!("Operación cancelada.");
                            return Ok(());
                        }
                    }
                }
            }
        }
    };

    if !config.projects.contains_key(&project_name) {
        return Err(anyhow::anyhow!("El proyecto '{}' no está registrado.", project_name));
    }

    let project_path = config.projects.get(&project_name).cloned();

    let home_dir = home::home_dir().unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    let pid_file = home_dir.join(format!(".ozymem-{}.pid", project_name));
    if pid_file.exists() {
        use dialoguer::Confirm;
        if Confirm::with_theme(&dialoguer::theme::ColorfulTheme::default())
            .with_prompt(format!("El watcher para '{}' está activo. ¿Desea detenerlo automáticamente antes de desregistrar?", project_name))
            .default(true)
            .interact()?
        {
            let _ = run_stop(Some(project_name.clone()));
        } else {
            println!("Operación abortada por seguridad (el watcher sigue activo).");
            return Ok(());
        }
    }

    use dialoguer::Confirm;
    if Confirm::with_theme(&dialoguer::theme::ColorfulTheme::default())
        .with_prompt(format!("¿Está seguro de que desea eliminar el registro de '{}'?", project_name))
        .default(false)
        .interact()?
    {
        config.projects.remove(&project_name);
        save_config(&config_path, &config)?;
        
        let log_file = home_dir.join(format!(".ozymem-{}.log", project_name));
        if log_file.exists() {
            let _ = std::fs::remove_file(log_file);
        }
        
        println!("[SUCCESS] Registro del proyecto '{}' eliminado de ozymem.toml.", project_name);

        if let Some(ref path_str) = project_path {
            if let Ok(conn) = build_backend_client().await {
                if conn.ping().await.is_ok() {
                    use dialoguer::Confirm;
                    if Confirm::with_theme(&dialoguer::theme::ColorfulTheme::default())
                        .with_prompt("Delete all indexed files for this project from the graph as well?")
                        .default(true)
                        .interact()?
                    {
                        println!("[Core] Eliminando archivos del proyecto del grafo...");
                        match conn.delete_project_files(path_str).await {
                            Ok(deleted) => {
                                println!("[SUCCESS] Se eliminaron {} archivos y sus funciones asociadas del grafo.", deleted);
                            }
                            Err(e) => {
                                eprintln!("[ERROR] No se pudieron eliminar los archivos del grafo: {:?}", e);
                            }
                        }
                    }
                }
            }
        }
    } else {
        println!("Operación cancelada.");
    }

    Ok(())
}

fn run_list() -> anyhow::Result<()> {
    let reg = ozymem_core::registry::ProjectRegistry::open()?;
    let projects = reg.list_projects()?;
    if projects.is_empty() {
        println!("[INFO] No hay proyectos registrados todavía. Usa 'ozymem register' para registrar uno.");
        return Ok(());
    }

    println!("+---------------------------+------------------------------------------------------------+");
    println!("| Nombre del Proyecto       | Ruta Registrada                                            |");
    println!("+---------------------------+------------------------------------------------------------+");
    for p in &projects {
        println!("| {:<25} | {:<58} |", p.name, p.path);
    }
    println!("+---------------------------+------------------------------------------------------------+");
    Ok(())
}

fn run_stop(project_arg: Option<String>) -> anyhow::Result<()> {
    let home_dir = home::home_dir().unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    
    let project_name = match project_arg {
        Some(p) => p,
        None => {
            let current_dir = std::env::current_dir()?;
            let cleaned_curr = clean_path(&current_dir.canonicalize()?);
            let reg = ozymem_core::registry::ProjectRegistry::open()?;
            let found = reg.get_project_by_path(&cleaned_curr)?;
            match found {
                Some(p) => p.name,
                None => {
                    let global_pid = home_dir.join(".ozymem.pid");
                    if global_pid.exists() {
                        let pid_str = std::fs::read_to_string(&global_pid)?.trim().to_string();
                        let _ = std::process::Command::new("taskkill")
                            .args(&["/PID", &pid_str, "/F"])
                            .status()?;
                        let _ = std::fs::remove_file(&global_pid);
                        println!("[SUCCESS] Proceso del watcher global (PID: {}) detenido y limpiado.", pid_str);
                        return Ok(());
                    }
                    return Err(anyhow::anyhow!("No se pudo determinar el proyecto del directorio actual. Especifica el nombre del proyecto."));
                }
            }
        }
    };

    let reg = ozymem_core::registry::ProjectRegistry::open()?;
    let project = reg.get_project_by_name(&project_name)?;
    
    match project {
        Some(p) => {
            let runtime = tokio::runtime::Runtime::new()?;
            runtime.block_on(async {
                match send_daemon_command_cli(&serde_json::json!({
                    "action": "sleep",
                    "project_name": p.name
                })).await {
                    Ok(res) => {
                        println!("[SUCCESS] Daemon suspendió el proyecto '{}': {}", p.name, serde_json::to_string_pretty(&res).unwrap_or_default());
                    }
                    Err(e) => {
                        println!("[WARNING] No se pudo enviar comando al daemon (¿está encendido?). Detalle: {e}");
                        println!("[INFO] Intentando detener watcher local como fallback...");
                        let pid_file = home_dir.join(format!(".ozymem-{}.pid", p.name));
                        if pid_file.exists() {
                            if let Ok(pid_str) = std::fs::read_to_string(&pid_file) {
                                let pid_str = pid_str.trim();
                                let _ = std::process::Command::new("taskkill")
                                    .args(&["/PID", pid_str, "/F"])
                                    .status();
                                let _ = std::fs::remove_file(&pid_file);
                                println!("[SUCCESS] Watcher local detenido (PID: {}).", pid_str);
                            }
                        }
                    }
                }
            });
        }
        None => println!("[ERROR] Proyecto '{}' no encontrado en el registro.", project_name)
    }

    Ok(())
}

async fn run_logs_tail(project_arg: Option<String>) -> anyhow::Result<()> {
    let home_dir = home::home_dir().unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    
    let project_name = match project_arg {
        Some(p) => p,
        None => {
            let current_dir = std::env::current_dir()?;
            let cleaned_curr = clean_path(&current_dir.canonicalize()?);
            let mut found_name = "global".to_string();
            if let Ok((_, config)) = load_config() {
                for (name, registered_path_str) in &config.projects {
                    if let Ok(reg_path_buf) = PathBuf::from(registered_path_str).canonicalize() {
                        if clean_path(&reg_path_buf) == cleaned_curr {
                            found_name = name.clone();
                            break;
                        }
                    }
                }
            }
            found_name
        }
    };
    
    let path = if project_name == "global" {
        home_dir.join(".ozymem.log")
    } else {
        home_dir.join(format!(".ozymem-{}.log", project_name))
    };

    if !path.exists() {
        println!("[INFO] No hay registros de logs disponibles todavía para '{}'.", project_name);
        return Ok(());
    }

    println!("[INFO] Mostrando registros en tiempo real para '{}' (Ruta: {}). Presiona Ctrl+C para salir.", project_name, path.display());

    let mut file = std::fs::File::open(&path)?;
    use std::io::{Read, Seek, SeekFrom};
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;
    if !buffer.is_empty() {
        print!("{}", String::from_utf8_lossy(&buffer));
    }

    let mut pos = file.metadata()?.len();
    loop {
        tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
        if let Ok(metadata) = std::fs::metadata(&path) {
            let new_len = metadata.len();
            if new_len > pos {
                if let Ok(mut f) = std::fs::File::open(&path) {
                    if f.seek(SeekFrom::Start(pos)).is_ok() {
                        let mut new_bytes = Vec::new();
                        if f.read_to_end(&mut new_bytes).is_ok() {
                            print!("{}", String::from_utf8_lossy(&new_bytes));
                            use std::io::Write;
                            let _ = std::io::stdout().flush();
                        }
                    }
                }
                pos = new_len;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use std::io::Write;

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

fn get_project_identifier(target_path: &str) -> anyhow::Result<(String, String)> {
    let canonical = canonicalize_target(target_path)?;
    let clean_target = clean_path(&canonical);
    let clean_target_lower = clean_target.to_lowercase();
    
    let (_, config) = load_config()?;
    for (name, registered_path_str) in &config.projects {
        if let Ok(reg_path_buf) = PathBuf::from(registered_path_str).canonicalize() {
            let clean_reg_path_lower = clean_path(&reg_path_buf).to_lowercase();
            if clean_target_lower == clean_reg_path_lower {
                return Ok((name.clone(), clean_target));
            }
        }
    }
    
    let folder_name = canonical.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    Ok((format!("unregistered-{}", folder_name), clean_target))
}

fn is_pid_alive(pid: u32) -> bool {
    #[cfg(target_os = "windows")]
    {
        let output = std::process::Command::new("tasklist")
            .args(&["/FI", &format!("PID eq {}", pid), "/NH"])
            .output();
        if let Ok(out) = output {
            let stdout = String::from_utf8_lossy(&out.stdout);
            stdout.contains(&pid.to_string())
        } else {
            false
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let status = std::process::Command::new("kill")
            .args(&["-0", &pid.to_string()])
            .status();
        match status {
            Ok(s) => s.success(),
            Err(_) => false,
        }
    }
}

fn get_last_log_line(log_path: &Path) -> String {
    if !log_path.exists() {
        return "Watcher no inicializado.".to_string();
    }
    if let Ok(content) = std::fs::read_to_string(log_path) {
        let lines: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();
        if let Some(last) = lines.last() {
            let last_str = last.trim();
            if last_str.len() > 60 {
                format!("{}...", &last_str[..57])
            } else {
                last_str.to_string()
            }
        } else {
            "Bitacora vacia.".to_string()
        }
    } else {
        "Error al leer bitacora.".to_string()
    }
}

fn shorten_path(path_str: &str, max_len: usize) -> String {
    if path_str.len() <= max_len {
        return path_str.to_string();
    }
    let separator = if path_str.contains('\\') { '\\' } else { '/' };
    let components: Vec<&str> = path_str.split(separator).collect();
    
    let mut result = String::new();
    let mut current_len = 3;
    for comp in components.iter().rev() {
        if current_len + comp.len() + 1 > max_len {
            break;
        }
        if result.is_empty() {
            result = comp.to_string();
        } else {
            result = format!("{}{}{}", comp, separator, result);
        }
        current_len += comp.len() + 1;
    }
    
    if result.is_empty() {
        format!("...{}", &path_str[path_str.len() - max_len + 3..])
    } else {
        format!("...{}{}", separator, result)
    }
}

/// Helper for ratatui popup centering
fn centered_rect(percent_x: u16, percent_y: u16, r: ratatui::prelude::Rect) -> ratatui::prelude::Rect {
    let popup_layout = ratatui::layout::Layout::vertical([
        ratatui::layout::Constraint::Length((r.height * (100 - percent_y)) / 200),
        ratatui::layout::Constraint::Length((r.height * percent_y) / 100),
        ratatui::layout::Constraint::Length((r.height * (100 - percent_y)) / 200),
    ])
    .split(r);

    ratatui::layout::Layout::horizontal([
        ratatui::layout::Constraint::Length((r.width * (100 - percent_x)) / 200),
        ratatui::layout::Constraint::Length((r.width * percent_x) / 100),
        ratatui::layout::Constraint::Length((r.width * (100 - percent_x)) / 200),
    ])
    .split(popup_layout[1])[1]
}

async fn run_mcp_start() -> anyhow::Result<()> {
    let home_dir = home::home_dir().unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    let pid_file = home_dir.join(".ozymem-mcp.pid");
    
    if pid_file.exists() {
        if let Ok(pid_str) = std::fs::read_to_string(&pid_file) {
            if let Ok(pid) = pid_str.trim().parse::<u32>() {
                if is_pid_alive(pid) {
                    println!("[INFO] El servidor MCP ya se encuentra activo bajo el PID {}.", pid);
                    return Ok(());
                } else {
                    let _ = std::fs::remove_file(&pid_file);
                }
            }
        }
    }

    let exe_path = std::env::current_exe()?;
    let mut cmd = std::process::Command::new(exe_path);
    cmd.arg("mcp").arg("run");
    cmd.env("OZYMEM_DAEMON", "1");

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }

    let log_path = home_dir.join(".ozymem-mcp.log");
    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;
    let stdout_file = log_file.try_clone()?;
    let stderr_file = log_file.try_clone()?;
    cmd.stdin(std::process::Stdio::null());
    cmd.stdout(stdout_file);
    cmd.stderr(stderr_file);

    let child = cmd.spawn()?;
    let pid = child.id();
    std::fs::write(&pid_file, pid.to_string())?;
    println!("[SUCCESS] Servidor MCP iniciado en segundo plano (PID: {})", pid);
    Ok(())
}

async fn run_init() -> anyhow::Result<()> {
    let (_, config) = load_config()?;
    if config.projects.is_empty() {
        println!("[INFO] No hay proyectos registrados todavía. Usa 'ozymem register' para registrar uno.");
        return Ok(());
    }

    let mut project_names: Vec<String> = config.projects.keys().cloned().collect();
    project_names.sort();

    use dialoguer::{theme::ColorfulTheme, Select};
    let selection = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("Seleccione el proyecto que desea iniciar")
        .items(&project_names)
        .default(0)
        .interact_opt()?;

    let Some(idx) = selection else {
        println!("Operación cancelada.");
        return Ok(());
    };

    let selected_project_name = &project_names[idx];
    let selected_project_path = &config.projects[selected_project_name];

    println!("[INFO] Inicializando entorno para el proyecto '{}'...", selected_project_name);

    // Paso 1: Inicializar backend SQLite
    let conn = build_backend_client().await?;
    let db_uri = conn.display_uri();
    let db_status_str = format!("CONECTADO ({})", db_uri);

    // Paso 2: Iniciar Servidor MCP en segundo plano (si la DB está activa o de forma resiliente)
    let mcp_res = run_mcp_start().await;
    let mcp_status_str = match mcp_res {
        Ok(_) => {
            let home_dir = home::home_dir().unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
            let pid_file = home_dir.join(".ozymem-mcp.pid");
            if pid_file.exists() {
                let pid_str = std::fs::read_to_string(&pid_file).unwrap_or_default().trim().to_string();
                format!("ACTIVO (PID: {})", pid_str)
            } else {
                "ACTIVO".to_string()
            }
        }
        Err(e) => format!("ERROR ({:?})", e),
    };

    // Paso 3: Levantar el Watcher del proyecto seleccionado
    let watcher_res = run_start(Some(selected_project_path.clone()), false);
    let watcher_status_str = match watcher_res {
        Ok(_) => {
            let home_dir = home::home_dir().unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
            let pid_file = home_dir.join(format!(".ozymem-{}.pid", selected_project_name));
            if pid_file.exists() {
                let pid_str = std::fs::read_to_string(&pid_file).unwrap_or_default().trim().to_string();
                format!("ACTIVO (PID: {})", pid_str)
            } else {
                "ACTIVO".to_string()
            }
        }
        Err(e) => format!("ERROR ({:?})", e),
    };

    // Limpiar pantalla e imprimir resumen espectacular
    print!("\x1B[2J\x1B[1;1H");
    use std::io::Write;
    let _ = std::io::stdout().flush();

    println!("[SUCCESS] ¡Entorno Ozymem inicializado con éxito!");
    println!();
    println!("Resumen de Servicios:");
    println!("  Backend DB: {}", db_status_str);
    println!("  ✔ Servidor MCP:      {}", mcp_status_str);
    println!("  ✔ Watcher Proyecto:  {} -> {}", watcher_status_str, selected_project_path);
    println!();
    println!("Para auditar los registros en tiempo real, utiliza:");
    println!("  - Logs del Watcher:  ozymem logs {}", selected_project_name);
    println!("  - Logs del MCP:      ozymem logs mcp");

    Ok(())
}

async fn run_doctor(json_output: bool) -> anyhow::Result<()> {
    let home_dir = home::home_dir().context("No se pudo determinar el directorio home.")?;
    let config_path = home_dir.join(".ozymem.toml");
    let config_exists = config_path.exists();
    let config_valid = if config_exists {
        load_config().is_ok()
    } else {
        false
    };

    let connection_res = build_backend_client().await;
    let (db_connected, db_ping_ok) = match &connection_res {
        Ok(conn) => {
            let ping_res = conn.ping().await;
            (true, ping_res.is_ok())
        }
        Err(_) => (false, false),
    };

    if json_output {
        let payload = serde_json::json!({
            "config_exists": config_exists,
            "config_valid": config_valid,
            "db_connected": db_connected,
            "db_ping_ok": db_ping_ok,
        });
        println!("{}", serde_json::to_string(&payload)?);
        return Ok(());
    }

    println!("=========================================");
    println!("     OZYMEM SYSTEM ENVIRONMENT DOCTOR    ");
    println!("=========================================");
    println!();

    if config_exists && config_valid {
        println!("  [✔] Configuración Local: Encontrada y válida (.ozymem.toml)");
    } else if config_exists {
        println!("  [✘] Configuración Local: Encontrada pero INVÁLIDA (.ozymem.toml)");
    } else {
        println!("  [✘] Configuración Local: No encontrada (.ozymem.toml no existe)");
    }

    if db_ping_ok {
        println!("  [✔] Conexión a la base de datos: EXITOSA");
    } else if db_connected {
        println!("  [✘] Conexión a la base de datos: Establecida pero falló el PING");
    } else {
        println!("  [✘] Conexión a la base de datos: CONEXIÓN FALLIDA");
    }

    println!("=========================================");

    Ok(())
}

async fn run_vector_subcommand(subcommand: &VectorSubcommand) -> anyhow::Result<()> {
    // Determine target project path
    let project_path = if let Ok(cwd) = std::env::current_dir() {
        cwd.to_string_lossy().to_string()
    } else {
        ".".to_string()
    };
    
    let db_path = std::path::Path::new(&project_path)
        .join(".ozymem")
        .join("vectors")
        .join("vectors.json");

    match subcommand {
        VectorSubcommand::Search { query, limit, category } => {
            if !db_path.exists() {
                println!("La base de datos vectorial no existe en: {:?}", db_path);
                return Ok(());
            }
            let data = std::fs::read_to_string(&db_path)?;
            let mut records: Vec<crate::mcp::VectorRecord> = serde_json::from_str(&data)?;

            // Pre-filtering by category and strictly filtering by schema_version == 1
            if let Some(cat) = category {
                records.retain(|r| r.category.eq_ignore_ascii_case(cat));
            }
            records.retain(|r| r.schema_version == 1);

            let query_emb = crate::mcp::get_embedding(query).await?;
            
            type SearchMatch = (String, String, f32, String); // (id, source, score, category)
            let mut matches: Vec<SearchMatch> = Vec::new();
            for r in records {
                let score = crate::mcp::cosine_similarity(&query_emb, &r.embedding);
                matches.push((r.id, r.source_path, score, r.category));
            }

            matches.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
            matches.truncate(*limit);

            use comfy_table::Table;
            let mut table = Table::new();
            table.set_header(vec!["ID", "Procedencia (Source)", "Categoría", "Similitud"]);
            for (id, source, score, cat) in matches {
                table.add_row(vec![id, source, cat, format!("{:.4}", score)]);
            }
            println!("{}", table);
        }
        VectorSubcommand::List { project, category } => {
            if !db_path.exists() {
                println!("La base de datos vectorial no existe en: {:?}", db_path);
                return Ok(());
            }
            let data = std::fs::read_to_string(&db_path)?;
            let mut records: Vec<crate::mcp::VectorRecord> = serde_json::from_str(&data)?;

            if let Some(proj) = project {
                records.retain(|r| r.project.eq_ignore_ascii_case(proj));
            }
            if let Some(cat) = category {
                records.retain(|r| r.category.eq_ignore_ascii_case(cat));
            }

            use comfy_table::Table;
            let mut table = Table::new();
            table.set_header(vec!["ID", "Procedencia (Source)", "Categoría", "Impactos (Hits)", "Fecha"]);
            for r in records {
                let date_str = format!("{}", r.timestamp);
                table.add_row(vec![r.id, r.source_path, r.category, r.hit_count.to_string(), date_str]);
            }
            println!("{}", table);
        }
        VectorSubcommand::Inspect { id } => {
            if !db_path.exists() {
                println!("La base de datos vectorial no existe.");
                return Ok(());
            }
            let data = std::fs::read_to_string(&db_path)?;
            let records: Vec<crate::mcp::VectorRecord> = serde_json::from_str(&data)?;
            if let Some(r) = records.iter().find(|rec| &rec.id == id) {
                println!("=========================================");
                println!("INSPECCIÓN DE RECUERDO VECTORIAL");
                println!("=========================================");
                println!("ID:           {}", r.id);
                println!("Proyecto:     {}", r.project);
                println!("Categoría:    {}", r.category);
                println!("Procedencia:  {}", r.source_path);
                println!("Versión Esq:  {}", r.schema_version);
                println!("Padre ID:     {}", r.parent_id.as_deref().unwrap_or("None"));
                println!("Impactos:     {}", r.hit_count);
                println!("Fecha (Unix): {}", r.timestamp);
                println!("-----------------------------------------");
                println!("Texto:\n{}", r.text);
                println!("=========================================");
            } else {
                println!("No se encontró ningún recuerdo con ID: {}", id);
            }
        }
        VectorSubcommand::Forget { id } => {
            if !db_path.exists() {
                println!("La base de datos vectorial no existe.");
                return Ok(());
            }
            let data = std::fs::read_to_string(&db_path)?;
            let mut records: Vec<crate::mcp::VectorRecord> = serde_json::from_str(&data)?;
            let initial_len = records.len();
            records.retain(|r| &r.id != id);
            if records.len() < initial_len {
                let serialized = serde_json::to_string_pretty(&records)?;
                std::fs::write(&db_path, serialized)?;
                println!("[SUCCESS] Recuerdo '{}' eliminado de la base de datos vectorial.", id);
            } else {
                println!("No se encontró ningún recuerdo con ID: {}", id);
            }
        }
        VectorSubcommand::Prune { apply } => {
            if !db_path.exists() {
                println!("La base de datos vectorial no existe.");
                return Ok(());
            }
            let data = std::fs::read_to_string(&db_path)?;
            let mut records: Vec<crate::mcp::VectorRecord> = serde_json::from_str(&data)?;
            let initial_len = records.len();
            
            let mut orphans = Vec::new();
            for r in &records {
                if !std::path::Path::new(&r.source_path).exists() {
                    orphans.push(r.id.clone());
                }
            }

            if orphans.is_empty() {
                println!("No se encontraron recuerdos huérfanos o inactivos para depurar.");
                return Ok(());
            }

            println!("Vectores huérfanos detectados (cuyo archivo de origen ya no existe):");
            for id in &orphans {
                println!("  - {}", id);
            }

            if *apply {
                records.retain(|r| !orphans.contains(&r.id));
                let serialized = serde_json::to_string_pretty(&records)?;
                std::fs::write(&db_path, serialized)?;
                println!("[SUCCESS] Depuración ejecutada. {} recuerdos eliminados.", initial_len - records.len());
            } else {
                println!("\n[DRY RUN] Ejecuta con '--apply' para eliminar estos recuerdos.");
            }
        }
        VectorSubcommand::Top { project } => {
            if !db_path.exists() {
                println!("La base de datos vectorial no existe.");
                return Ok(());
            }
            let data = std::fs::read_to_string(&db_path)?;
            let mut records: Vec<crate::mcp::VectorRecord> = serde_json::from_str(&data)?;

            if let Some(proj) = project {
                records.retain(|r| r.project.eq_ignore_ascii_case(proj));
            }

            records.sort_by(|a, b| b.hit_count.cmp(&a.hit_count));
            records.truncate(10);

            use comfy_table::Table;
            let mut table = Table::new();
            table.set_header(vec!["ID", "Procedencia", "Categoría", "Impactos (Hits)"]);
            for r in records {
                table.add_row(vec![r.id, r.source_path, r.category, r.hit_count.to_string()]);
            }
            println!("{}", table);
        }
    }
    Ok(())
}

async fn run_dashboard() -> anyhow::Result<()> {
    use ratatui::{
        backend::CrosstermBackend,
        layout::{Constraint, Direction, Layout},
        style::{Color, Modifier, Style},
        text::{Line, Span},
        widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
        Terminal,
    };

    #[derive(Copy, Clone, Debug, PartialEq)]
    enum ActiveTab {
        Memories,
        SystemStatus,
        GraphPRs,
    }

    // 1. Setup terminal raw mode
    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    crossterm::execute!(
        stdout,
        crossterm::terminal::EnterAlternateScreen,
        crossterm::event::EnableMouseCapture
    )?;
    
    struct CleanupTerminal;
    impl Drop for CleanupTerminal {
        fn drop(&mut self) {
            let _ = crossterm::terminal::disable_raw_mode();
            let _ = crossterm::execute!(
                std::io::stdout(),
                crossterm::terminal::LeaveAlternateScreen,
                crossterm::event::DisableMouseCapture
            );
        }
    }
    let _cleanup = CleanupTerminal;
    
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    
    // 2. Setup backend connection
    let connection = build_backend_client().await?;
    let display_uri = connection.display_uri();
    
    // 3. Application State variables
    let mut active_tab = ActiveTab::Memories;
    
    // Tab 1: Memories state
    let project_path = if let Ok(cwd) = std::env::current_dir() {
        cwd.to_string_lossy().to_string()
    } else {
        ".".to_string()
    };
    let db_path = std::path::Path::new(&project_path)
        .join(".ozymem")
        .join("vectors")
        .join("vectors.json");
        
    let mut records: Vec<crate::mcp::VectorRecord> = if db_path.exists() {
        let data = std::fs::read_to_string(&db_path)?;
        serde_json::from_str(&data).unwrap_or_default()
    } else {
        Vec::new()
    };
    let mut last_db_mtime = db_path.metadata().ok().and_then(|m| m.modified().ok());
    
    let mut selected_index = 0;
    let mut scroll_offset = 0;
    let mut search_query = String::new();
    let mut input_mode = false;
    let mut prune_confirm = false;
    let mut prune_list: Vec<String> = Vec::new();
    
    // Tab 2: System Status & Watchers state
    let mut status_ping_ok = Some(connection.ping().await.is_ok());
    let mut sorted_projects: Vec<(String, String)> = Vec::new();
    let mut selected_project_idx = 0;
    let mut log_lines: Vec<String> = Vec::new();
    
    // Tab 3: Graph PRs state
    let _gpr_list: Vec<()> = Vec::new();
    let _selected_gpr_idx = 0;
    let _active_gpr_details: Option<(String, String, Vec<FileDefinitionMap>, Vec<LessonRecord>)> = None;
    let _gpr_scroll_offset = 0;
    
    let mut status_message = "Bienvenido a OzyMem Dashboard! Pulse 1, 2 o 3 para navegar por pestañas.".to_string();
    
    // Initial fetches
    if let Ok(reg) = ozymem_core::registry::ProjectRegistry::open() {
        if let Ok(projects) = reg.list_projects() {
            let mut projs: Vec<(String, String)> = projects.into_iter().map(|p| (p.name, p.path)).collect();
            projs.sort_by(|a, b| a.0.cmp(&b.0));
            sorted_projects = projs;
        }
    }
    
    // Helper function to load selected project logs
    let load_current_project_logs = |sorted_projects: &[(String, String)], idx: usize| -> Vec<String> {
        if let Some((name, _)) = sorted_projects.get(idx) {
            let home_dir = home::home_dir().unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
            let log_file = home_dir.join(format!(".ozymem-{}.log", name));
            if log_file.exists() {
                if let Ok(content) = std::fs::read_to_string(&log_file) {
                    let mut lines = content.lines()
                        .rev()
                        .take(50)
                        .map(|s| s.to_string())
                        .collect::<Vec<_>>();
                    lines.reverse();
                    return lines;
                }
            }
        }
        vec!["No se encontraron bitacoras para este proyecto.".to_string()]
    };
    
    if !sorted_projects.is_empty() {
        log_lines = load_current_project_logs(&sorted_projects, selected_project_idx);
    }
    
    // Helper function to fetch GPR details (no-op in SQLite mode)
    let _fetch_gpr_details_sync = |_connection: &BackendClient, _gpr_id: i64| -> Option<(String, String, Vec<FileDefinitionMap>, Vec<LessonRecord>)> {
        None
    };
    
    loop {
        // Auto-reload de vectors.json si hay cambios en disco
        if let Ok(metadata) = std::fs::metadata(&db_path) {
            if let Ok(mtime) = metadata.modified() {
                if Some(mtime) != last_db_mtime {
                    last_db_mtime = Some(mtime);
                    if let Ok(data) = std::fs::read_to_string(&db_path) {
                        if let Ok(parsed) = serde_json::from_str::<Vec<crate::mcp::VectorRecord>>(&data) {
                            records = parsed;
                            status_message = "Recuerdos vectoriales actualizados automáticamente.".to_string();
                        }
                    }
                }
            }
        }

        // Filter records by search query and schema_version == 1
        let filtered_records: Vec<&crate::mcp::VectorRecord> = records.iter()
            .filter(|r| r.schema_version == 1)
            .filter(|r| {
                if search_query.is_empty() {
                    true
                } else {
                    r.text.to_lowercase().contains(&search_query.to_lowercase())
                        || r.source_path.to_lowercase().contains(&search_query.to_lowercase())
                        || r.category.to_lowercase().contains(&search_query.to_lowercase())
                        || r.id.to_lowercase().contains(&search_query.to_lowercase())
                }
            })
            .collect();
            
        if selected_index >= filtered_records.len() && !filtered_records.is_empty() {
            selected_index = filtered_records.len() - 1;
        }
        
        terminal.draw(|f| {
            let size = f.size();
            
            // Layout: Title Block (3 lines), Main Area (flexible), Bottom Area (4 lines)
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),
                    Constraint::Min(10),
                    Constraint::Length(4),
                ])
                .split(size);
                
            // 1. Title Block with Tabs (No Emojis)
            let tabs_items = vec![
                Line::from(" [1] Recuerdos "),
                Line::from(" [2] Monitoreo y Watchers "),
                Line::from(" (Graph PRs no disponible) "),
            ];
            
            let selected_tab_idx = match active_tab {
                ActiveTab::Memories => 0,
                ActiveTab::SystemStatus => 1,
                ActiveTab::GraphPRs => 2,
            };
            
            let header_layout = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(45),
                    Constraint::Percentage(55),
                ])
                .split(chunks[0]);
                
            let title_para = Paragraph::new(Line::from(vec![
                Span::styled(" OZYMEM ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled("INTELLIGENT HYBRID VECTOR STORE ", Style::default().fg(Color::White)),
                Span::styled(format!("v{}", env!("CARGO_PKG_VERSION")), Style::default().fg(Color::DarkGray)),
            ]))
            .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::Cyan)));
            f.render_widget(title_para, header_layout[0]);
            
            let tabs_widget = ratatui::widgets::Tabs::new(tabs_items)
                .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::Cyan)))
                .select(selected_tab_idx)
                .highlight_style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD));
            f.render_widget(tabs_widget, header_layout[1]);
            
            // 2. Main Area (Depends on Active Tab)
            match active_tab {
                ActiveTab::Memories => {
                    let main_chunks = Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints([
                            Constraint::Percentage(40),
                            Constraint::Percentage(60),
                        ])
                        .split(chunks[1]);
                        
                    // 2a. Left: List of memories
                    let list_title = if search_query.is_empty() {
                        format!(" Recuerdos ({}) ", filtered_records.len())
                    } else {
                        format!(" Busqueda: '{}' ({}) ", search_query, filtered_records.len())
                    };
                    
                    let list_block = Block::default()
                        .title(list_title)
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(if input_mode { Color::Yellow } else { Color::White }));
                        
                    let items: Vec<ListItem> = filtered_records.iter().enumerate().map(|(idx, r)| {
                        let symbol_name = r.id.split("::").last().unwrap_or(&r.id);
                        let base_name = std::path::Path::new(&r.source_path)
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or(&r.source_path);
                            
                        let bg_color = if idx == selected_index { Color::Rgb(30, 60, 90) } else { Color::Reset };
                        let is_selected_style = if idx == selected_index {
                            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(Color::White)
                        };
                        
                        let cat_color = match r.category.to_lowercase().as_str() {
                            "lesson" => Color::LightRed,
                            "fact" => Color::LightGreen,
                            _ => Color::LightBlue,
                        };
                        
                        ListItem::new(Line::from(vec![
                            Span::styled(format!(" {:02} ", idx + 1), Style::default().fg(Color::DarkGray)),
                            Span::styled(format!("{:<15} ", base_name), Style::default().fg(Color::Cyan)),
                            Span::styled(format!(" [{}] ", r.category.to_uppercase()), Style::default().fg(cat_color)),
                            Span::styled(symbol_name.to_string(), is_selected_style),
                        ]))
                        .style(Style::default().bg(bg_color))
                    }).collect();
                    
                    if items.is_empty() {
                        let empty_para = Paragraph::new("No se encontraron recuerdos en la base de datos.")
                            .alignment(ratatui::layout::Alignment::Center)
                            .block(list_block);
                        f.render_widget(empty_para, main_chunks[0]);
                    } else {
                        let mut list_state = ListState::default();
                        list_state.select(Some(selected_index));
                        let list_widget = List::new(items)
                            .block(list_block)
                            .highlight_symbol(">> ")
                            .highlight_style(Style::default().fg(Color::Green).bg(Color::Rgb(30, 60, 90)));
                        f.render_stateful_widget(list_widget, main_chunks[0], &mut list_state);
                    }
                    
                    // 2b. Right: Details pane
                    let details_block = Block::default()
                        .title(" Detalles del Recuerdo ")
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::White));
                        
                    if let Some(r) = filtered_records.get(selected_index) {
                        let mut details_text = Vec::new();
                        details_text.push(Line::from(vec![
                            Span::styled("ID:          ", Style::default().fg(Color::DarkGray)),
                            Span::styled(r.id.clone(), Style::default().fg(Color::Yellow)),
                        ]));
                        details_text.push(Line::from(vec![
                            Span::styled("Procedencia: ", Style::default().fg(Color::DarkGray)),
                            Span::styled(r.source_path.clone(), Style::default().fg(Color::Cyan)),
                        ]));
                        
                        let cat_color = match r.category.to_lowercase().as_str() {
                            "lesson" => Color::LightRed,
                            "fact" => Color::LightGreen,
                            _ => Color::LightBlue,
                        };
                        
                        details_text.push(Line::from(vec![
                            Span::styled("Categoria:   ", Style::default().fg(Color::DarkGray)),
                            Span::styled(r.category.to_uppercase(), Style::default().fg(cat_color).add_modifier(Modifier::BOLD)),
                            Span::styled("  |  Hits: ", Style::default().fg(Color::DarkGray)),
                            Span::styled(r.hit_count.to_string(), Style::default().fg(Color::LightYellow)),
                            Span::styled("  |  Esquema: ", Style::default().fg(Color::DarkGray)),
                            Span::styled(r.schema_version.to_string(), Style::default().fg(Color::White)),
                        ]));
                        details_text.push(Line::from(vec![
                            Span::styled("Fecha (Unix):", Style::default().fg(Color::DarkGray)),
                            Span::styled(r.timestamp.to_string(), Style::default().fg(Color::White)),
                        ]));
                        if let Some(p_id) = &r.parent_id {
                            details_text.push(Line::from(vec![
                                Span::styled("Padre ID:    ", Style::default().fg(Color::DarkGray)),
                                Span::styled(p_id.clone(), Style::default().fg(Color::Magenta)),
                            ]));
                        }
                        details_text.push(Line::from(""));
                        details_text.push(Line::from(Span::styled("Contenido / Codigo:", Style::default().add_modifier(Modifier::UNDERLINED))));
                        details_text.push(Line::from(""));
                        
                        for line_str in r.text.lines() {
                            details_text.push(Line::from(line_str));
                        }
                        
                        let para = Paragraph::new(details_text)
                            .block(details_block)
                            .scroll((scroll_offset, 0))
                            .wrap(Wrap { trim: false });
                            
                        f.render_widget(para, main_chunks[1]);
                    } else {
                        let para = Paragraph::new("Selecciona un recuerdo para ver sus detalles.")
                            .alignment(ratatui::layout::Alignment::Center)
                            .block(details_block);
                        f.render_widget(para, main_chunks[1]);
                    }
                }
                ActiveTab::SystemStatus => {
                    let main_chunks = Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints([
                            Constraint::Percentage(40),
                            Constraint::Percentage(60),
                        ])
                        .split(chunks[1]);
                        
                    // 2a. Left: List of projects
                    let proj_block = Block::default()
                        .title(" Proyectos Configurados ")
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::White));
                        
                    let items: Vec<ListItem> = sorted_projects.iter().enumerate().map(|(idx, (name, path))| {
                        let bg_color = if idx == selected_project_idx { Color::Rgb(30, 60, 90) } else { Color::Reset };
                        let is_selected_style = if idx == selected_project_idx {
                            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(Color::White)
                        };
                        
                        ListItem::new(Line::from(vec![
                            Span::styled(format!(" {:02} ", idx + 1), Style::default().fg(Color::DarkGray)),
                            Span::styled(format!("{:<15} ", name), is_selected_style),
                            Span::styled(path.clone(), Style::default().fg(Color::DarkGray)),
                        ]))
                        .style(Style::default().bg(bg_color))
                    }).collect();
                    
                    if items.is_empty() {
                        let empty_para = Paragraph::new("No hay proyectos registrados en ozymem.toml")
                            .alignment(ratatui::layout::Alignment::Center)
                            .block(proj_block);
                        f.render_widget(empty_para, main_chunks[0]);
                    } else {
                        let mut list_state = ListState::default();
                        list_state.select(Some(selected_project_idx));
                        let list_widget = List::new(items)
                            .block(proj_block)
                            .highlight_symbol(">> ")
                            .highlight_style(Style::default().fg(Color::Green).bg(Color::Rgb(30, 60, 90)));
                        f.render_stateful_widget(list_widget, main_chunks[0], &mut list_state);
                    }
                    
                    // 2b. Right: Connection details and log tail
                    let logs_block = Block::default()
                        .title(" Monitoreo y Logs en Vivo ")
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::White));
                        
                    let mut status_lines = Vec::new();
                    status_lines.push(Line::from(vec![
                        Span::styled("Backend DB URL:  ", Style::default().fg(Color::DarkGray)),
                        Span::styled(display_uri.clone(), Style::default().fg(Color::Cyan)),
                    ]));
                    
                    let ping_style = match status_ping_ok {
                        Some(true) => Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD),
                        Some(false) => Style::default().fg(Color::LightRed).add_modifier(Modifier::BOLD),
                        None => Style::default().fg(Color::DarkGray),
                    };
                    let ping_text = match status_ping_ok {
                        Some(true) => "ACTIVO / RESPONDIENDO PING",
                        Some(false) => "INACTIVO / SIN CONEXION",
                        None => "CARGANDO...",
                    };
                    status_lines.push(Line::from(vec![
                        Span::styled("Estado Conexion:  ", Style::default().fg(Color::DarkGray)),
                        Span::styled(ping_text, ping_style),
                    ]));
                    status_lines.push(Line::from(""));
                    status_lines.push(Line::from(Span::styled("Ultimas 50 lineas de Bitacora:", Style::default().add_modifier(Modifier::UNDERLINED))));
                    status_lines.push(Line::from(""));
                    
                    for line_str in &log_lines {
                        status_lines.push(Line::from(line_str.as_str()));
                    }
                    
                    let para = Paragraph::new(status_lines)
                        .block(logs_block)
                        .wrap(Wrap { trim: false });
                    f.render_widget(para, main_chunks[1]);
                }
                ActiveTab::GraphPRs => {
                    let gpr_block = Block::default()
                        .title(" Graph PRs ")
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::White));

                    let para = Paragraph::new(
                        "GPR functionality is not available in local mode."
                    )
                    .alignment(ratatui::layout::Alignment::Center)
                    .block(gpr_block);
                    f.render_widget(para, chunks[1]);
                }
            }
            
            // 3. Stats / Controls Block
            let stats_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(60),
                    Constraint::Percentage(40),
                ])
                .split(chunks[2]);
                
            let stats_block = Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::DarkGray));
            
            let input_info = if input_mode {
                format!("BUSCAR (Escribe y pulsa Enter): {}", search_query)
            } else {
                match active_tab {
                    ActiveTab::Memories => "[q] Salir  [Tab] Ciclador  [s] Buscar  [f] Olvidar  [p] Depurar  [Esc] Limpiar  [,] Subir  [.] Bajar".to_string(),
                    ActiveTab::SystemStatus => "[q] Salir  [Tab] Ciclador  [r] Recargar Logs  [↑/↓] Navegar proyectos".to_string(),
                    ActiveTab::GraphPRs => "[q] Salir  [Tab] Ciclador".to_string(),
                }
            };
            
            let input_style = if input_mode {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            
            let cmd_para = Paragraph::new(vec![
                Line::from(Span::styled(input_info, input_style)),
                Line::from(Span::styled(status_message.clone(), Style::default().fg(Color::Gray))),
            ])
            .block(stats_block.clone().title(" Atajos y Estatus "));
            f.render_widget(cmd_para, stats_chunks[0]);
            
            let total_hits: i64 = records.iter().map(|r| r.hit_count).sum();
            let num_lessons = records.iter().filter(|r| r.category.eq_ignore_ascii_case("lesson")).count();
            let num_facts = records.iter().filter(|r| r.category.eq_ignore_ascii_case("fact")).count();
            let num_contexts = records.iter().filter(|r| r.category.eq_ignore_ascii_case("context")).count();
            
            let stats_lines = vec![
                Line::from(vec![
                    Span::styled("Total: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(records.len().to_string(), Style::default().fg(Color::White)),
                    Span::styled("  Facts: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(num_facts.to_string(), Style::default().fg(Color::LightGreen)),
                    Span::styled("  Lessons: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(num_lessons.to_string(), Style::default().fg(Color::LightRed)),
                ]),
                Line::from(vec![
                    Span::styled("Contexts: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(num_contexts.to_string(), Style::default().fg(Color::LightBlue)),
                    Span::styled("  Hits Acumulados: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(total_hits.to_string(), Style::default().fg(Color::LightYellow)),
                ]),
            ];
            
            let stats_para = Paragraph::new(stats_lines)
                .block(stats_block.title(" Metricas de Memoria "));
            f.render_widget(stats_para, stats_chunks[1]);
            
            if prune_confirm {
                let block = Block::default()
                    .title(" Depuracion de Huerfanos ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::LightRed));
                    
                let text = vec![
                    Line::from(format!("Se han detectado {} recuerdos huerfanos.", prune_list.len())),
                    Line::from("¿Deseas eliminarlos de forma definitiva?"),
                    Line::from(""),
                    Line::from(vec![
                        Span::styled(" [y] Si, aplicar depuracion ", Style::default().fg(Color::LightRed).add_modifier(Modifier::BOLD)),
                        Span::styled("  |  ", Style::default().fg(Color::DarkGray)),
                        Span::styled(" [n] Cancelar ", Style::default().fg(Color::White)),
                    ]),
                ];
                
                let area = centered_rect(60, 25, size);
                f.render_widget(Clear, area);
                let popup_para = Paragraph::new(text)
                    .block(block)
                    .alignment(ratatui::layout::Alignment::Center);
                f.render_widget(popup_para, area);
            }
        })?;
        
        // 4. Event loop poll
        if crossterm::event::poll(std::time::Duration::from_millis(100))? {
            if let crossterm::event::Event::Key(key) = crossterm::event::read()? {
                if key.kind != crossterm::event::KeyEventKind::Press {
                    continue;
                }
                if prune_confirm {
                    match key.code {
                        crossterm::event::KeyCode::Char('y') | crossterm::event::KeyCode::Char('Y') => {
                            records.retain(|r| !prune_list.contains(&r.id));
                            if let Ok(serialized) = serde_json::to_string_pretty(&records) {
                                let _ = std::fs::write(&db_path, serialized);
                                status_message = format!("Depuracion ejecutada. {} recuerdos eliminados.", prune_list.len());
                            } else {
                                status_message = "Error al guardar los cambios en vectors.json".to_string();
                            }
                            prune_confirm = false;
                        }
                        _ => {
                            status_message = "Depuracion cancelada.".to_string();
                            prune_confirm = false;
                        }
                    }
                    continue;
                }
                
                if input_mode {
                    match key.code {
                        crossterm::event::KeyCode::Enter => {
                            input_mode = false;
                            selected_index = 0;
                            status_message = format!("Busqueda aplicada: '{}'", search_query);
                        }
                        crossterm::event::KeyCode::Esc => {
                            input_mode = false;
                            search_query.clear();
                            status_message = "Busqueda cancelada.".to_string();
                        }
                        crossterm::event::KeyCode::Backspace => {
                            search_query.pop();
                        }
                        crossterm::event::KeyCode::Char(c) => {
                            search_query.push(c);
                        }
                        _ => {}
                    }
                } else {
                    match key.code {
                        crossterm::event::KeyCode::Char('q') | crossterm::event::KeyCode::Esc => {
                            break;
                        }
                        crossterm::event::KeyCode::Tab => {
                            active_tab = match active_tab {
                                ActiveTab::Memories => ActiveTab::SystemStatus,
                                ActiveTab::SystemStatus => ActiveTab::Memories,
                                ActiveTab::GraphPRs => ActiveTab::Memories,
                            };
                            status_message = format!("Pestana activa: {:?}", active_tab);
                            
                            // Load corresponding data on tab switch
                            if active_tab == ActiveTab::SystemStatus {
                                status_ping_ok = Some(connection.ping().await.is_ok());
                                if !sorted_projects.is_empty() {
                                    log_lines = load_current_project_logs(&sorted_projects, selected_project_idx);
                                }
                            }
                        }
                        crossterm::event::KeyCode::Char('1') => {
                            active_tab = ActiveTab::Memories;
                            status_message = "Pestana activa: Recuerdos".to_string();
                        }
                        crossterm::event::KeyCode::Char('2') => {
                            active_tab = ActiveTab::SystemStatus;
                            status_message = "Pestana activa: Monitoreo y Watchers".to_string();
                            status_ping_ok = Some(connection.ping().await.is_ok());
                            if !sorted_projects.is_empty() {
                                log_lines = load_current_project_logs(&sorted_projects, selected_project_idx);
                            }
                        }
                        crossterm::event::KeyCode::Char('3') => {
                            status_message = "Graph PRs no disponible en modo local.".to_string();
                        }
                        crossterm::event::KeyCode::Char('r') | crossterm::event::KeyCode::Char('R') => {
                            match active_tab {
                                ActiveTab::Memories => {
                                    if db_path.exists() {
                                        if let Ok(data) = std::fs::read_to_string(&db_path) {
                                            records = serde_json::from_str(&data).unwrap_or_default();
                                            status_message = "Recuerdos vectoriales recargados.".to_string();
                                        }
                                    }
                                }
                                ActiveTab::SystemStatus => {
                                    status_ping_ok = Some(connection.ping().await.is_ok());
                                    if !sorted_projects.is_empty() {
                                        log_lines = load_current_project_logs(&sorted_projects, selected_project_idx);
                                    }
                                    status_message = "Monitoreo y logs actualizados.".to_string();
                                }
                                ActiveTab::GraphPRs => {
                                    status_message = "Graph PRs no disponible en modo local.".to_string();
                                }
                            }
                        }
                        crossterm::event::KeyCode::Up => {
                            match active_tab {
                                ActiveTab::Memories => {
                                    if selected_index > 0 {
                                        selected_index -= 1;
                                        scroll_offset = 0;
                                    }
                                }
                                ActiveTab::SystemStatus => {
                                    if selected_project_idx > 0 {
                                        selected_project_idx -= 1;
                                        log_lines = load_current_project_logs(&sorted_projects, selected_project_idx);
                                    }
                                }
                                ActiveTab::GraphPRs => {}
                            }
                        }
                        crossterm::event::KeyCode::Down => {
                            match active_tab {
                                ActiveTab::Memories => {
                                    if selected_index + 1 < filtered_records.len() {
                                        selected_index += 1;
                                        scroll_offset = 0;
                                    }
                                }
                                ActiveTab::SystemStatus => {
                                    if selected_project_idx + 1 < sorted_projects.len() {
                                        selected_project_idx += 1;
                                        log_lines = load_current_project_logs(&sorted_projects, selected_project_idx);
                                    }
                                }
                                ActiveTab::GraphPRs => {}
                            }
                        }
                        crossterm::event::KeyCode::Char(',') => {
                            match active_tab {
                                ActiveTab::Memories => {
                                    if scroll_offset > 0 {
                                        scroll_offset -= 1;
                                    }
                                }
                                _ => {}
                            }
                        }
                        crossterm::event::KeyCode::Char('.') => {
                            match active_tab {
                                ActiveTab::Memories => {
                                    scroll_offset += 1;
                                }
                                _ => {}
                            }
                        }
                        crossterm::event::KeyCode::Char('s') | crossterm::event::KeyCode::Char('S') => {
                            if active_tab == ActiveTab::Memories {
                                input_mode = true;
                                search_query.clear();
                                status_message = "Escribe para buscar...".to_string();
                            }
                        }
                        crossterm::event::KeyCode::Char('f') | crossterm::event::KeyCode::Char('F') => {
                            if active_tab == ActiveTab::Memories {
                                if let Some(r) = filtered_records.get(selected_index) {
                                    let id_to_delete = r.id.clone();
                                    records.retain(|rec| rec.id != id_to_delete);
                                    if let Ok(serialized) = serde_json::to_string_pretty(&records) {
                                        let _ = std::fs::write(&db_path, serialized);
                                        status_message = format!("Olvidado: {}", id_to_delete);
                                    } else {
                                        status_message = "Error al guardar cambios en vectors.json".to_string();
                                    }
                                }
                            }
                        }
                        crossterm::event::KeyCode::Char('p') | crossterm::event::KeyCode::Char('P') => {
                            if active_tab == ActiveTab::Memories {
                                prune_list.clear();
                                for r in &records {
                                    if !std::path::Path::new(&r.source_path).exists() {
                                        prune_list.push(r.id.clone());
                                    }
                                }
                                if prune_list.is_empty() {
                                    status_message = "No se detectaron recuerdos huerfanos.".to_string();
                                } else {
                                    prune_confirm = true;
                                }
                            }
                        }
                        crossterm::event::KeyCode::Char('m') | crossterm::event::KeyCode::Char('M') => {
                            if active_tab == ActiveTab::GraphPRs {
                                status_message = "GPR merge no disponible en modo local.".to_string();
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    
    Ok(())
}

/// Helper function to send commands to the central Go daemon over the TCP socket.
async fn send_daemon_command_cli(cmd: &serde_json::Value) -> anyhow::Result<serde_json::Value> {
    use tokio::net::TcpStream;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    
    let mut stream = TcpStream::connect("127.0.0.1:17399").await?;
    let payload = format!("{}\n", serde_json::to_string(cmd)?);
    stream.write_all(payload.as_bytes()).await?;
    stream.flush().await?;
    
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).await?;
    let response: serde_json::Value = serde_json::from_str(&line)?;
    Ok(response)
}



