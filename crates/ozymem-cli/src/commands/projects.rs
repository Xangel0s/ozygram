use crate::commands::scan::{canonicalize_target, clean_path, is_critical_root};
use anyhow::Context;
use std::path::{Path, PathBuf};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
use crate::client::build_backend_client;
use crate::config::{load_config, save_config};

pub fn run_start(path_arg: Option<String>, force: bool) -> anyhow::Result<()> {
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

pub fn check_directory_authorized(target_path: &str) -> anyhow::Result<()> {
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

pub fn run_register(name_arg: Option<String>) -> anyhow::Result<()> {
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

pub async fn run_deregister(name_arg: Option<String>) -> anyhow::Result<()> {
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

pub fn run_list() -> anyhow::Result<()> {
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

pub fn run_stop(project_arg: Option<String>) -> anyhow::Result<()> {
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

pub async fn run_logs_tail(project_arg: Option<String>) -> anyhow::Result<()> {
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


pub fn get_project_identifier(target_path: &str) -> anyhow::Result<(String, String)> {
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

pub fn is_pid_alive(pid: u32) -> bool {
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

pub fn get_last_log_line(log_path: &Path) -> String {
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

pub fn shorten_path(path_str: &str, max_len: usize) -> String {
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



pub async fn run_mcp_start() -> anyhow::Result<()> {
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

pub async fn run_init() -> anyhow::Result<()> {
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


pub async fn send_daemon_command_cli(cmd: &serde_json::Value) -> anyhow::Result<serde_json::Value> {
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




