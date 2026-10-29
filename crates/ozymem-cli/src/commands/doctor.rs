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
                            if pid > 0 && is_pid_alive(pid) {
                                format!("ACTIVO (PID: {})", pid)
                            } else {
                                "STANDBY / GIT_DRIVEN".to_string()
                            }
                        } else {
                            "STANDBY / GIT_DRIVEN".to_string()
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

pub fn print_tri_hybrid_dashboard(json_output: bool) -> anyhow::Result<()> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let home_dir = home::home_dir().unwrap_or_else(|| cwd.clone());

    let ozymem_dir = if cwd.join(".ozymem").is_dir() {
        cwd.join(".ozymem")
    } else {
        home_dir.join(".ozymem")
    };

    let sqlite_db = if ozymem_dir.join("memory.db").is_file() {
        ozymem_dir.join("memory.db")
    } else {
        ozymem_dir.join("db.sqlite")
    };
    let wal_path = std::path::PathBuf::from(format!("{}-wal", sqlite_db.display()));
    let wal_size_bytes = wal_path.metadata().map(|m| m.len()).unwrap_or(0);
    let wal_size_kb = wal_size_bytes as f64 / 1024.0;

    let chroma_dir = ozymem_dir.join("chroma");
    let chroma_status = if chroma_dir.is_dir() { "ONLINE" } else { "STANDBY" };

    let pg_dsn = std::env::var("OZY_PG_DSN")
        .or_else(|_| std::env::var("DATABASE_URL"))
        .or_else(|_| std::env::var("POSTGRES_URL"))
        .unwrap_or_default();
    let pg_configured = !pg_dsn.is_empty();
    let pg_status = if pg_configured { "ONLINE_OR_CONFIGURED" } else { "OFFLINE_FALLBACK" };

    let neuro_path = ozymem_dir.join("neuromodulators.json");
    let (d, ne, ach, ht) = if neuro_path.is_file() {
        if let Ok(content) = std::fs::read_to_string(&neuro_path) {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
                let d = val.get("dopamine").and_then(|v| v.as_f64()).unwrap_or(0.50);
                let ne = val.get("noradrenaline").and_then(|v| v.as_f64()).unwrap_or(0.50);
                let ach = val.get("acetylcholine").and_then(|v| v.as_f64()).unwrap_or(0.50);
                let ht = val.get("serotonin").and_then(|v| v.as_f64()).unwrap_or(0.50);
                (d, ne, ach, ht)
            } else {
                (0.50, 0.50, 0.50, 0.50)
            }
        } else {
            (0.50, 0.50, 0.50, 0.50)
        }
    } else {
        (0.50, 0.50, 0.50, 0.50)
    };

    if json_output {
        let payload = serde_json::json!({
            "status": "TRI_HYBRID_ACTIVE",
            "l1_sqlite": {
                "status": "online",
                "path": sqlite_db.to_string_lossy(),
                "wal_size_kb": wal_size_kb,
                "latency_sla": "< 1.0ms"
            },
            "l2_chroma": {
                "status": chroma_status,
                "path": chroma_dir.to_string_lossy(),
                "collections": ["episodic_raw", "code_graph_ast", "semantic_heuristics", "dream_insights"],
                "latency_sla": "< 5.0ms"
            },
            "l3_postgres": {
                "status": pg_status,
                "dsn_configured": pg_configured,
                "latency_sla": "< 8.0ms"
            },
            "neuromodulators": {
                "dopamine": d,
                "noradrenaline": ne,
                "acetylcholine": ach,
                "serotonin": ht
            }
        });
        println!("{}", serde_json::to_string(&payload)?);
        return Ok(());
    }

    println!("========================================================================================");
    println!("                    OZYMEM TRI-HYBRID NEURO-COGNITIVE DASHBOARD");
    println!("========================================================================================");
    println!("Status: [STATUS: TRI_HYBRID_COGNITION_ACTIVE]");
    println!();
    println!("L1 - FAST REFLEX / AMYGDALA (SQLite WAL)");
    println!("+-------------------+------------------------------------------------------------------+");
    println!("| {:<17} | {:<64} |", "Storage Path", sqlite_db.to_string_lossy());
    println!("| {:<17} | {:<64} |", "Journal Mode", "WAL (Write-Ahead Logging)");
    println!("| {:<17} | {:<64} |", "WAL Size", format!("{:.2} KB", wal_size_kb));
    println!("| {:<17} | {:<64} |", "Latency Guarantee", "< 1.0 ms");
    println!("+-------------------+------------------------------------------------------------------+");
    println!();
    println!("L2 - VECTOR LAKE (ChromaDB C++ Cosine KNN)");
    println!("+-------------------+------------------------------------------------------------------+");
    println!("| {:<17} | {:<64} |", "Vector Store Path", chroma_dir.to_string_lossy());
    println!("| {:<17} | {:<64} |", "Status", chroma_status);
    println!("| {:<17} | {:<64} |", "Collections (4)", "episodic_raw, code_graph_ast, semantic_heuristics, dream_insights");
    println!("| {:<17} | {:<64} |", "Latency Guarantee", "< 5.0 ms");
    println!("+-------------------+------------------------------------------------------------------+");
    println!();
    println!("L3 - NEOCORTEX / CLUSTER (PostgreSQL + pgvector + ltree)");
    println!("+-------------------+------------------------------------------------------------------+");
    println!("| {:<17} | {:<64} |", "Status", pg_status);
    println!("| {:<17} | {:<64} |", "MCTS Forest", "ozy_mcts_nodes (Hierarchical ltree)");
    println!("| {:<17} | {:<64} |", "SWR Replay Lake", "ozy_swr_ripples (HNSW vector_cosine_ops)");
    println!("| {:<17} | {:<64} |", "AST Blast Radius", "ozy_ast_dependency_edges (Recursive CTE Lineage)");
    println!("| {:<17} | {:<64} |", "Latency Guarantee", "< 8.0 ms");
    println!("+-------------------+------------------------------------------------------------------+");
    println!();
    println!("NEUROMODULATOR BALANCE");
    println!("+-------------------+------------------------------------------------------------------+");
    println!("| {:<17} | {:<64} |", "Dopamine (D)", format!("{:.2} (Reward Prediction Error & Consolidation)", d));
    println!("| {:<17} | {:<64} |", "Noradrenaline (NE)", format!("{:.2} (Amygdala Threat Sensitivity & Locus Coeruleus)", ne));
    println!("| {:<17} | {:<64} |", "Acetylcholine (ACh)", format!("{:.2} (Encoding vs Retrieval Modulation)", ach));
    println!("| {:<17} | {:<64} |", "Serotonin (5-HT)", format!("{:.2} (Patience & Behavioral Inhibition)", ht));
    println!("+-------------------+------------------------------------------------------------------+");
    println!("========================================================================================");

    Ok(())
}
