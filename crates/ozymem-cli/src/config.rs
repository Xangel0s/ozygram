use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

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

pub fn load_config() -> Result<(PathBuf, OzymemConfig)> {
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

pub fn save_config(path: &Path, config: &OzymemConfig) -> Result<()> {
    let toml_str = toml::to_string_pretty(config)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, toml_str)?;
    Ok(())
}
