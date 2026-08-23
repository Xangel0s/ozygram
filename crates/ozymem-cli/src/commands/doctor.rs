use crate::commands::projects::{get_last_log_line, is_pid_alive, shorten_path};
use anyhow::Context;
use crate::client::{build_backend_client, AppContext, DatabaseJsonOutput, StatusJsonOutput, StatusMetricsJson};
use crate::config::load_config;

pub async fn print_status(context: &AppContext, json_output: bool) -> anyhow::Result<()> {
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


pub async fn run_doctor(json_output: bool) -> anyhow::Result<()> {
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

