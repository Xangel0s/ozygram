// Copyright (c) 2026 Ozygram Project. All rights reserved.
// CLI Command handler for 'ozymem config' (Track 1).
// Zero-Emoji Standard compliant.

use anyhow::{anyhow, Result};
use clap::Subcommand;
use ozymem_core::config::OzyConfig;
use std::path::PathBuf;

#[derive(Subcommand, Debug, Clone)]
pub enum ConfigSubcommand {
    /// List all resolved configuration keys, values, and their origins
    #[command(alias = "show")]
    List {
        #[arg(long)]
        json: bool,
    },
    /// Get the resolved value of a specific configuration key
    Get {
        key: String,
    },
    /// Set a configuration key in the workspace .ozy/config.toml file
    Set {
        key: String,
        value: String,
    },
    /// Initialize a new .ozy/config.toml file with an optional operational preset
    Init {
        #[arg(long, value_parser = ["laptop", "power", "ci", "default"])]
        preset: Option<String>,

        #[arg(long)]
        force: bool,
    },
}

pub fn handle_config_command(subcommand: Option<ConfigSubcommand>) -> Result<()> {
    let current_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    match subcommand.unwrap_or(ConfigSubcommand::List { json: false }) {
        ConfigSubcommand::List { json } => {
            let (config, origins) = OzyConfig::load_resolved(Some(&current_dir))?;

            if json {
                let json_val = serde_json::to_value(&config)?;
                println!("{}", serde_json::to_string_pretty(&json_val)?);
                return Ok(());
            }

            println!("================================================================================");
            println!("               OZYGRAM CONFIGURATION & RESOLVED PRECEDENCE                      ");
            println!("================================================================================");
            if let Some(ref path) = OzyConfig::discover_config_file(Some(&current_dir)) {
                println!("[STATUS: CONFIG_FILE_LOADED] {}", path.display());
            } else {
                println!("[STATUS: RUNNING_WITH_DEFAULTS] No .ozy/config.toml file found (using defaults/env)");
            }
            if let Some(ref p) = config.core.preset {
                println!("[STATUS: ACTIVE_PRESET] {}", p);
            }
            println!("--------------------------------------------------------------------------------");
            println!("{:<32} {:<22} {}", "CONFIGURATION KEY", "VALUE", "ORIGIN");
            println!("--------------------------------------------------------------------------------");

            for key in OzyConfig::all_keys() {
                let val = config.get_field(key).unwrap_or_else(|| "-".to_string());
                let origin = origins
                    .get(*key)
                    .map(|o| o.to_string())
                    .unwrap_or_else(|| "[DEFAULT]".to_string());
                println!("{:<32} {:<22} {}", key, val, origin);
            }
            println!("================================================================================");
        }

        ConfigSubcommand::Get { key } => {
            let (config, origins) = OzyConfig::load_resolved(Some(&current_dir))?;
            match config.get_field(&key) {
                Some(val) => {
                    let origin = origins.get(&key).map(|o| o.to_string()).unwrap_or_default();
                    println!("{} {}", val, origin);
                }
                None => {
                    return Err(anyhow!("[ALERT: ERROR] Clave de configuración '{}' no encontrada.\nUsa 'ozymem config list' para ver todas las claves disponibles.", key));
                }
            }
        }

        ConfigSubcommand::Set { key, value } => {
            let config_path = current_dir.join(".ozy").join("config.toml");
            let mut config = if config_path.exists() {
                let content = std::fs::read_to_string(&config_path)?;
                toml::from_str::<OzyConfig>(&content)
                    .unwrap_or_default()
            } else {
                OzyConfig::default()
            };

            config.set_field(&key, &value)?;
            config.save_to_file(&config_path)?;

            println!("[STATUS: CONFIG_UPDATED] {} = '{}' guardado en {}", key, value, config_path.display());
        }

        ConfigSubcommand::Init { preset, force } => {
            let ozy_dir = current_dir.join(".ozy");
            let config_path = ozy_dir.join("config.toml");

            if config_path.exists() && !force {
                println!("[ALERT: CONFIG_EXISTS] El archivo {} ya existe.", config_path.display());
                println!("Usa '--force' si deseas sobrescribirlo con una nueva plantilla.");
                return Ok(());
            }

            let template = OzyConfig::generate_template_toml(preset.as_deref());
            std::fs::create_dir_all(&ozy_dir)?;
            std::fs::write(&config_path, template)?;

            let preset_name = preset.unwrap_or_else(|| "default".to_string());
            println!("[STATUS: CONFIG_INITIALIZED] Plantilla creada en {} con preset '{}'", config_path.display(), preset_name);
        }
    }

    Ok(())
}
