use anyhow::Result;
use serde_json::Value;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

pub async fn run_setup(_auto: bool) -> Result<()> {
    println!("========================================================================");
    println!("   [OZYGRAM] One-Click Setup & Autonomous MCP Client Configurator       ");
    println!("========================================================================");
    println!();

    let home_dir = home::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let ozy_dir = home_dir.join(".ozymem");
    let bin_dir = ozy_dir.join("bin");

    // 1. Prepare directory structure
    println!("[1/4] Preparing installation directories...");
    fs::create_dir_all(&bin_dir)?;
    println!(" -> Installation directory: {}", bin_dir.display());

    // 2. Install executables if running from external directory
    println!("[2/4] Verifying binary deployment...");
    let server_bin_name = if cfg!(windows) { "ozymem-server.exe" } else { "ozymem-server" };
    let cli_bin_name = if cfg!(windows) { "ozymem.exe" } else { "ozymem" };
    let ozygram_bin_name = if cfg!(windows) { "ozygram.exe" } else { "ozygram" };

    let target_server = bin_dir.join(server_bin_name);
    let target_cli = bin_dir.join(cli_bin_name);
    let target_ozygram = bin_dir.join(ozygram_bin_name);

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
        }
    }

    // 3. Configure PATH
    println!("[3/4] Configuring environment PATH...");
    configure_path(&bin_dir);

    // 4. Auto-detect & configure AI services
    println!("[4/4] Scanning for installed AI coding clients...");
    let server_path_str = if target_server.exists() {
        target_server.to_string_lossy().to_string()
    } else {
        "ozymem-server".to_string()
    };

    let configured_clients = auto_configure_mcp_clients(&server_path_str, &home_dir);

    if configured_clients.is_empty() {
        println!(" -> No standard client configuration files detected. Manual config snippet provided below.");
    } else {
        for (name, path) in &configured_clients {
            println!(" [STATUS: AUTO-CONFIGURED] {} -> {}", name, path);
        }
    }

    println!();
    println!("------------------------------------------------------------------------");
    println!("  MCP Configuration (copy-paste for any MCP-compatible AI service):     ");
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
    println!("[STATUS: ACTIVE] Ozygram setup complete! Your AI agent is now ready to code.");
    println!();

    // Pause so Windows explorer double-click doesn't vanish
    if !_auto {
        print!("Press Enter to exit...");
        let _ = io::stdout().flush();
        let mut buffer = String::new();
        let _ = io::stdin().read_line(&mut buffer);
    }

    Ok(())
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

fn auto_configure_mcp_clients(server_command: &str, home: &Path) -> Vec<(String, String)> {
    let mut results = Vec::new();

    let candidates = get_client_candidates(home);

    for (client_name, path) in candidates {
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

fn get_client_candidates(home: &Path) -> Vec<(String, PathBuf)> {
    let mut list = Vec::new();

    // 1. Antigravity IDE
    list.push((
        "Antigravity IDE".to_string(),
        home.join(".gemini").join("antigravity-ide").join("mcp_config.json"),
    ));

    // 2. Cursor
    list.push((
        "Cursor".to_string(),
        home.join(".cursor").join("mcp.json"),
    ));

    // 3. Claude Desktop
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

    // 4. Windsurf
    list.push((
        "Windsurf".to_string(),
        home.join(".codeium").join("windsurf").join("mcp_config.json"),
    ));

    // 5. VS Code (Cline extension)
    #[cfg(windows)]
    if let Ok(appdata) = std::env::var("APPDATA") {
        list.push((
            "VS Code (Cline)".to_string(),
            PathBuf::from(appdata)
                .join("Code")
                .join("User")
                .join("globalStorage")
                .join("saoudrizwan.claude-dev")
                .join("settings")
                .join("cline_mcp_settings.json"),
        ));
    }

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
