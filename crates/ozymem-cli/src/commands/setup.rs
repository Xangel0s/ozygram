use anyhow::Result;
use serde_json::Value;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

pub async fn run_setup(auto: bool) -> Result<()> {
    run_init(None, auto, None).await
}

pub async fn run_init(
    path: Option<PathBuf>,
    auto: bool,
    client_filter: Option<String>,
) -> Result<()> {
    println!("========================================================================");
    println!("   [OZYGRAM] Autonomous AI Project Initialization (v{})               ", env!("CARGO_PKG_VERSION"));
    println!("========================================================================");
    println!();

    let home_dir = home::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let ozy_dir = home_dir.join(".ozymem");
    let bin_dir = ozy_dir.join("bin");

    // 1. Prepare installation directories & binaries
    println!("[1/5] Verifying binary deployment in ~/.ozymem/bin...");
    fs::create_dir_all(&bin_dir)?;
    deploy_binaries(&bin_dir);

    // 2. Configure PATH
    println!("[2/5] Verifying environment PATH...");
    configure_path(&bin_dir);

    // 3. Initialize current workspace
    println!("[3/5] Initializing workspace cognitive substrate...");
    let workspace = path
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let project_name = workspace
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("workspace");

    let ws_ozymem = workspace.join(".ozymem");
    let _ = fs::create_dir_all(&ws_ozymem);

    // Create default .ozymemignore if not present
    let ignore_file = workspace.join(".ozymemignore");
    if !ignore_file.exists() {
        let default_ignore = "# Ozygram Cognitive Ignore Patterns\n\
                              node_modules/\n\
                              target/\n\
                              dist/\n\
                              build/\n\
                              .git/\n\
                              __pycache__/\n\
                              .pytest_cache/\n\
                              *.pyc\n\
                              venv/\n\
                              .venv/\n\
                              *.log\n\
                              *.sqlite*\n\
                              *.duckdb*\n";
        let _ = fs::write(&ignore_file, default_ignore);
        println!(" -> Created default .ozymemignore in {}", workspace.display());
    }

    // Auto-manage .gitignore
    let _ = ozymem_core::graph_backend::auto_manage_gitignore(&workspace);

    // 4. Verify reasoning engine (standalone vs python)
    println!("[4/5] Checking reasoning worker substrate...");
    let brain_bin_name = if cfg!(windows) { "ozy-brain.exe" } else { "ozy-brain" };
    let target_brain = bin_dir.join(brain_bin_name);
    if target_brain.exists() {
        println!(" [STATUS: READY] Standalone Python-free worker engine detected: {}", target_brain.display());
    } else {
        println!(" [STATUS: READY] Python runtime / fast-lane deterministic fallback active.");
    }

    // 5. Auto-detect & configure AI services
    println!("[5/5] Scanning and auto-configuring AI coding clients...");
    let server_bin_name = if cfg!(windows) { "ozymem-server.exe" } else { "ozymem-server" };
    let target_server = bin_dir.join(server_bin_name);
    let server_path_str = if target_server.exists() {
        target_server.to_string_lossy().to_string()
    } else {
        "ozymem-server".to_string()
    };

    let configured_clients = auto_configure_mcp_clients(&server_path_str, &home_dir, &workspace, client_filter.as_deref());

    if configured_clients.is_empty() {
        println!(" -> No supported client configuration files found to inject. Manual snippet below.");
    } else {
        for (name, cfg_path) in &configured_clients {
            println!(" [STATUS: AUTO-CONFIGURED] {} -> {}", name, cfg_path);
        }
    }

    println!();
    println!("------------------------------------------------------------------------");
    println!("  MCP Configuration (for any MCP-compatible AI agent):                  ");
    println!("------------------------------------------------------------------------");
    let snippet = serde_json::json!({
        "mcpServers": {
            "ozygram": {
                "command": server_path_str,
                "args": []
            }
        }
    });
    println!("{}", serde_json::to_string_pretty(&snippet)?);
    println!("------------------------------------------------------------------------");
    println!();
    println!("[STATUS: ACTIVE] Project '{}' initialized with Ozygram!", project_name);
    println!("Restart your AI editor (Cursor, Antigravity, Claude, VS Code) to start pair-programming.");
    println!();

    if !auto {
        print!("Press Enter to exit...");
        let _ = io::stdout().flush();
        let mut buffer = String::new();
        let _ = io::stdin().read_line(&mut buffer);
    }

    Ok(())
}

fn deploy_binaries(bin_dir: &Path) {
    let server_bin_name = if cfg!(windows) { "ozymem-server.exe" } else { "ozymem-server" };
    let cli_bin_name = if cfg!(windows) { "ozymem.exe" } else { "ozymem" };
    let ozygram_bin_name = if cfg!(windows) { "ozygram.exe" } else { "ozygram" };
    let brain_bin_name = if cfg!(windows) { "ozy-brain.exe" } else { "ozy-brain" };

    let target_server = bin_dir.join(server_bin_name);
    let target_cli = bin_dir.join(cli_bin_name);
    let target_ozygram = bin_dir.join(ozygram_bin_name);
    let target_brain = bin_dir.join(brain_bin_name);

    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(parent) = current_exe.parent() {
            // Copy server if alongside
            let src_server = parent.join(server_bin_name);
            if src_server.exists() && src_server != target_server {
                let _ = fs::copy(&src_server, &target_server);
                println!(" -> Deployed {} to {}", server_bin_name, bin_dir.display());
            }

            // Copy cli if alongside
            let src_cli = parent.join(cli_bin_name);
            if src_cli.exists() && src_cli != target_cli {
                let _ = fs::copy(&src_cli, &target_cli);
                println!(" -> Deployed {} to {}", cli_bin_name, bin_dir.display());
            }

            // Copy self as ozygram.exe
            if current_exe != target_ozygram {
                let _ = fs::copy(&current_exe, &target_ozygram);
                println!(" -> Deployed {} to {}", ozygram_bin_name, bin_dir.display());
            }

            // Copy standalone brain if alongside
            let src_brain = parent.join(brain_bin_name);
            if src_brain.exists() && src_brain != target_brain {
                let _ = fs::copy(&src_brain, &target_brain);
                println!(" -> Deployed {} to {}", brain_bin_name, bin_dir.display());
            }
        }
    }
}

fn configure_path(bin_dir: &Path) {
    #[cfg(windows)]
    {
        let bin_str = bin_dir.to_string_lossy().to_string();
        let script = format!(
            "$p = [Environment]::GetEnvironmentVariable('Path', 'User'); \
             if ($p -notlike '*{0}*') {{ \
                 [Environment]::SetEnvironmentVariable('Path', \"$p;{0}\", 'User'); \
                 Write-Host ' -> Added to User PATH successfully.' \
             }} else {{ \
                 Write-Host ' -> User PATH already configured.' \
             }}",
            bin_str.replace("'", "''")
        );

        let output = std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", &script])
            .output();

        if let Ok(out) = output {
            let msg = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !msg.is_empty() {
                println!("{}", msg);
            }
        }
    }

    #[cfg(not(windows))]
    {
        println!(" -> Ensure '{}' is in your ~/.bashrc or ~/.zshrc PATH.", bin_dir.display());
    }
}

fn auto_configure_mcp_clients(
    server_command: &str,
    home: &Path,
    workspace: &Path,
    client_filter: Option<&str>,
) -> Vec<(String, String)> {
    let mut results = Vec::new();
    let candidates = get_client_candidates(home, workspace);

    for (client_name, path) in candidates {
        if let Some(filter) = client_filter {
            if !client_name.to_lowercase().contains(&filter.to_lowercase()) {
                continue;
            }
        }

        if let Some(parent) = path.parent() {
            if parent.exists() {
                if try_inject_mcp_server(&path, server_command).is_ok() {
                    results.push((client_name, path.to_string_lossy().to_string()));
                }
            }
        }
    }

    results
}

fn get_client_candidates(home: &Path, workspace: &Path) -> Vec<(String, PathBuf)> {
    let mut list = Vec::new();

    // 1. Antigravity IDE
    list.push((
        "Antigravity IDE".to_string(),
        home.join(".gemini").join("antigravity-ide").join("mcp_config.json"),
    ));

    // 2. Cursor (User global)
    list.push((
        "Cursor (Global)".to_string(),
        home.join(".cursor").join("mcp.json"),
    ));

    // 3. Cursor (Workspace local)
    list.push((
        "Cursor (Workspace)".to_string(),
        workspace.join(".cursor").join("mcp.json"),
    ));

    // 4. Claude Desktop
    #[cfg(windows)]
    if let Ok(appdata) = std::env::var("APPDATA") {
        list.push((
            "Claude Desktop".to_string(),
            PathBuf::from(appdata).join("Claude").join("claude_desktop_config.json"),
        ));
    }
    #[cfg(target_os = "macos")]
    list.push((
        "Claude Desktop".to_string(),
        home.join("Library").join("Application Support").join("Claude").join("claude_desktop_config.json"),
    ));
    #[cfg(target_os = "linux")]
    list.push((
        "Claude Desktop".to_string(),
        home.join(".config").join("Claude").join("claude_desktop_config.json"),
    ));

    // 5. Windsurf
    list.push((
        "Windsurf".to_string(),
        home.join(".codeium").join("windsurf").join("mcp_config.json"),
    ));

    // 6. VS Code (Cline extension)
    #[cfg(windows)]
    if let Ok(appdata) = std::env::var("APPDATA") {
        list.push((
            "VS Code (Cline)".to_string(),
            PathBuf::from(&appdata)
                .join("Code")
                .join("User")
                .join("globalStorage")
                .join("saoudrizwan.claude-dev")
                .join("settings")
                .join("cline_mcp_settings.json"),
        ));
        list.push((
            "VS Code (Roo Code)".to_string(),
            PathBuf::from(&appdata)
                .join("Code")
                .join("User")
                .join("globalStorage")
                .join("rooveterinaryinc.roo-cline")
                .join("settings")
                .join("cline_mcp_settings.json"),
        ));
    }

    // 7. Zed
    #[cfg(windows)]
    if let Ok(appdata) = std::env::var("APPDATA") {
        list.push((
            "Zed".to_string(),
            PathBuf::from(appdata).join("Zed").join("settings.json"),
        ));
    }
    #[cfg(not(windows))]
    list.push((
        "Zed".to_string(),
        home.join(".config").join("zed").join("settings.json"),
    ));

    list
}

fn try_inject_mcp_server(config_path: &Path, server_command: &str) -> Result<()> {
    let mut root_val: Value = if config_path.exists() {
        let content = fs::read_to_string(config_path)?;
        serde_json::from_str(&content).unwrap_or_else(|_| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    if !root_val.is_object() {
        root_val = serde_json::json!({});
    }

    let root_obj = root_val.as_object_mut().unwrap();

    let mcp_servers_entry = root_obj
        .entry("mcpServers")
        .or_insert_with(|| serde_json::json!({}));

    if !mcp_servers_entry.is_object() {
        *mcp_servers_entry = serde_json::json!({});
    }

    let mcp_obj = mcp_servers_entry.as_object_mut().unwrap();

    mcp_obj.insert(
        "ozygram".to_string(),
        serde_json::json!({
            "command": server_command,
            "args": []
        }),
    );

    let formatted = serde_json::to_string_pretty(&root_val)?;
    fs::write(config_path, formatted)?;

    Ok(())
}
