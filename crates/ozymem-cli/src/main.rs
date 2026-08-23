use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
use ozymem_cli::commands::vector::VectorSubcommand;
use ozymem_cli::commands::*;
use ozymem_cli::client::*;
use ozymem_cli::mcp;
use ozymem_core::graph_backend::{auto_manage_gitignore, legacy_global_db_path};
use ozymem_parser::{extract_dependency_hints, parse_source, SupportedLanguage};

#[derive(Parser)]
#[command(name = "ozymem", author, version, about = "CLI para gestión de grafos y lecciones", long_about = None)]
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
    /// Exportar el conocimiento del proyecto a un paquete portable (.ozymem)
    Export {
        #[arg(short, long)]
        output: Option<PathBuf>,
        #[arg(long)]
        project: Option<String>,
    },
    /// Importar un paquete de conocimiento (.ozymem) al proyecto actual
    Import {
        path: PathBuf,
        #[arg(long, default_value_t = true)]
        merge: bool,
    },
    /// Vincular o consultar proyectos relacionados (Cross-Repo)
    Link {
        #[arg(long)]
        target: Option<String>,
        #[arg(long, default_value = "depends_on")]
        relation: String,
        #[arg(long)]
        list: bool,
    },
    /// Traductor seguro y compacto para consultas de agentes (grep/find/ctx/trace/tree/arch/doctor/code/skills)
    #[command(alias = "q", alias = "ask", alias = "x")]
    Query {
        /// Consulta libre o comando corto, por ejemplo: grep auth, find GraphBackend, trace src/main.rs
        input: Vec<String>,
        /// Salida JSON compacta para scripts/agentes
        #[arg(long)]
        json: bool,
        /// Máximo de resultados a mostrar
        #[arg(short, long, default_value_t = 8)]
        limit: usize,
        /// Presupuesto aproximado de tokens para la salida textual
        #[arg(long, default_value_t = 1200)]
        tokens: usize,
    },
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
        Commands::Export { output, project } => {
            let cwd = std::env::current_dir()?;
            let backend = ozymem_core::graph_backend::GraphBackend::open_for_project(&cwd)?;
            let proj_name = project.clone().unwrap_or_else(|| {
                cwd.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("project")
                    .to_string()
            });
            let out_file = output.clone().unwrap_or_else(|| PathBuf::from(format!("{}.ozymem", proj_name)));
            let summary = ozymem_core::export_bundle(&backend, &proj_name, &out_file)?;
            println!("[OzyMem] Paquete de conocimiento exportado exitosamente:");
            println!("  Archivo:   {}", summary.file_path);
            println!("  Proyecto:  {}", summary.project_name);
            println!("  Lecciones: {}", summary.lessons_exported);
            println!("  Rutas API: {}", summary.routes_exported);
            println!("  Checksum:  {}", summary.sha256);
            println!("  Tamano:    {} bytes", summary.file_size_bytes);
            return Ok(());
        }
        Commands::Import { path, merge } => {
            let cwd = std::env::current_dir()?;
            let backend = ozymem_core::graph_backend::GraphBackend::open_for_project(&cwd)?;
            let summary = ozymem_core::import_bundle(&backend, path, *merge).await?;
            println!("[OzyMem] Paquete de conocimiento importado exitosamente:");
            println!("  Archivo:     {}", summary.file_path);
            println!("  Proyecto:    {}", summary.project_name);
            println!("  Importadas:  {}", summary.lessons_imported);
            println!("  Duplicadas:  {} (omitidas)", summary.lessons_skipped_duplicate);
            println!("  Rutas API:   {}", summary.routes_imported);
            println!("  Modo:        {}", summary.mode);
            return Ok(());
        }
        Commands::Link { target, relation, list } => {
            let reg = ozymem_core::registry::ProjectRegistry::open()?;
            if *list || target.is_none() {
                let links = reg.list_all_links()?;
                println!("[OzyMem] Enlaces entre proyectos registrados ({})", links.len());
                for l in links {
                    println!("  #{} {} --[{}]--> {}", l.id, l.source_project_name, l.relation_type, l.target_project_name);
                }
                return Ok(());
            }

            let cwd = std::env::current_dir()?;
            let current_proj = cwd.file_name().and_then(|n| n.to_str()).unwrap_or("current");
            let target_name = target.as_ref().unwrap();
            reg.link_projects(current_proj, target_name, relation)?;
            println!("[OzyMem] Vinculado exitosamente: {} --[{}]--> {}", current_proj, relation, target_name);
            return Ok(());
        }
        Commands::Mcp { .. } => {
            return mcp::run_mcp_server().await;
        }
        Commands::Query { input, json, limit, tokens } => {
            let connection = build_backend_client().await?;
            return run_query_translator(&connection, input.clone(), *json, *limit, *tokens).await;
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
        Commands::Export { .. } => unreachable!(),
        Commands::Import { .. } => unreachable!(),
        Commands::Link { .. } => unreachable!(),
        Commands::Mcp { .. } => unreachable!(),
        Commands::Doctor { .. } => unreachable!(),
        Commands::Query { .. } => unreachable!(),

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

