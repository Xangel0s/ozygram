//! Unified configuration engine for Ozygram (Track 1).
//! Handles multi-tier configuration precedence:
//! CLI Arguments > Environment Variables > Workspace .ozy/config.toml > User ~/.ozy/config.toml > Hardcoded Defaults.
//! Supports operational presets ("laptop", "power", "ci").

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CoreConfig {
    #[serde(default)]
    pub preset: Option<String>,
    #[serde(default = "default_data_dir")]
    pub data_dir: String,
    #[serde(default = "default_idle_timeout_secs")]
    pub idle_timeout_secs: u64,
    #[serde(default)]
    pub disable_watcher: bool,
}

fn default_data_dir() -> String {
    ".ozymem".to_string()
}

fn default_idle_timeout_secs() -> u64 {
    1800
}

impl Default for CoreConfig {
    fn default() -> Self {
        Self {
            preset: None,
            data_dir: default_data_dir(),
            idle_timeout_secs: default_idle_timeout_secs(),
            disable_watcher: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct McpConfig {
    #[serde(default)]
    pub show_legacy_tools: bool,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: usize,
    #[serde(default = "default_lod")]
    pub default_lod: String,
}

fn default_max_tokens() -> usize {
    4000
}

fn default_lod() -> String {
    "contracts".to_string()
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            show_legacy_tools: false,
            max_tokens: default_max_tokens(),
            default_lod: default_lod(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BrainConfig {
    #[serde(default = "default_brain_mode")]
    pub mode: String,
    #[serde(default = "default_brain_port")]
    pub port: u16,
    #[serde(default)]
    pub bin_path: Option<String>,
    #[serde(default)]
    pub python_path: Option<String>,
    #[serde(default = "default_brain_model")]
    pub model: String,
}

fn default_brain_mode() -> String {
    "auto".to_string()
}

fn default_brain_port() -> u16 {
    50051
}

fn default_brain_model() -> String {
    "gemini-2.5-flash".to_string()
}

impl Default for BrainConfig {
    fn default() -> Self {
        Self {
            mode: default_brain_mode(),
            port: default_brain_port(),
            bin_path: None,
            python_path: None,
            model: default_brain_model(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ComputeConfig {
    #[serde(default)]
    pub lightweight_mode: bool,
    #[serde(default)]
    pub max_compute: bool,
    #[serde(default = "default_mcts_simulations")]
    pub mcts_simulations: usize,
    #[serde(default = "default_mcts_max_depth")]
    pub mcts_max_depth: usize,
    #[serde(default = "default_c_puct")]
    pub c_puct: f64,
    #[serde(default = "default_beta_threat")]
    pub beta_threat: f64,
    #[serde(default = "default_lambda_epist")]
    pub lambda_epist: f64,
}

fn default_mcts_simulations() -> usize {
    25
}

fn default_mcts_max_depth() -> usize {
    6
}

fn default_c_puct() -> f64 {
    std::f64::consts::SQRT_2
}

fn default_beta_threat() -> f64 {
    0.35
}

fn default_lambda_epist() -> f64 {
    0.35
}

impl Default for ComputeConfig {
    fn default() -> Self {
        Self {
            lightweight_mode: false,
            max_compute: false,
            mcts_simulations: default_mcts_simulations(),
            mcts_max_depth: default_mcts_max_depth(),
            c_puct: default_c_puct(),
            beta_threat: default_beta_threat(),
            lambda_epist: default_lambda_epist(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StorageConfig {
    #[serde(default)]
    pub postgres_dsn: Option<String>,
    #[serde(default)]
    pub neural_reranker: bool,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            postgres_dsn: None,
            neural_reranker: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct OzyConfig {
    #[serde(default)]
    pub core: CoreConfig,
    #[serde(default)]
    pub mcp: McpConfig,
    #[serde(default)]
    pub brain: BrainConfig,
    #[serde(default)]
    pub compute: ComputeConfig,
    #[serde(default)]
    pub storage: StorageConfig,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConfigOrigin {
    Default,
    File(PathBuf),
    Env(String),
    Preset(String),
}

impl std::fmt::Display for ConfigOrigin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigOrigin::Default => write!(f, "[DEFAULT]"),
            ConfigOrigin::File(path) => write!(f, "[FILE: {}]", path.display()),
            ConfigOrigin::Env(var_name) => write!(f, "[ENV: {}]", var_name),
            ConfigOrigin::Preset(name) => write!(f, "[PRESET: {}]", name),
        }
    }
}

impl OzyConfig {
    /// Loads configuration with full hierarchical precedence:
    /// Env Vars > File (.ozy/config.toml or ~/.ozy/config.toml) > Preset > Default
    pub fn load_resolved(workspace_root: Option<&Path>) -> Result<(Self, HashMap<String, ConfigOrigin>)> {
        let mut config = Self::default();
        let mut origins: HashMap<String, ConfigOrigin> = HashMap::new();

        // 1. Initial Defaults
        for key in Self::all_keys() {
            origins.insert(key.to_string(), ConfigOrigin::Default);
        }

        // 2. Discover configuration file
        let config_file = Self::discover_config_file(workspace_root);
        if let Some(ref path) = config_file {
            if path.exists() {
                let content = fs::read_to_string(path)
                    .with_context(|| format!("Error al leer archivo de configuración en {}", path.display()))?;
                let file_cfg: OzyConfig = toml::from_str(&content)
                    .with_context(|| format!("Error de sintaxis TOML en {}", path.display()))?;

                // Preset from file applied first as baseline
                if let Some(ref preset) = file_cfg.core.preset {
                    config.apply_preset(preset, &mut origins);
                }

                // Specific fields from file take precedence over preset defaults
                config.merge_from(&file_cfg, &mut origins, path);
            }
        }

        // 3. Environment preset override if explicitly provided via OZY_PRESET
        if let Ok(env_preset) = std::env::var("OZY_PRESET") {
            config.apply_preset(&env_preset, &mut origins);
        }

        // 4. Override with Environment Variables
        config.apply_env_overrides(&mut origins);

        Ok((config, origins))
    }

    pub fn discover_config_file(workspace_root: Option<&Path>) -> Option<PathBuf> {
        // Priority A: Workspace .ozy/config.toml
        if let Some(ws) = workspace_root {
            let candidate = ws.join(".ozy").join("config.toml");
            if candidate.exists() {
                return Some(candidate);
            }
            let candidate_alt = ws.join(".ozymem").join("config.toml");
            if candidate_alt.exists() {
                return Some(candidate_alt);
            }
        }

        // Priority B: Current directory .ozy/config.toml
        let local_candidate = Path::new(".ozy").join("config.toml");
        if local_candidate.exists() {
            return Some(local_candidate);
        }

        // Priority C: User Home ~/.ozy/config.toml or ~/.ozymem.toml
        if let Some(home) = home::home_dir() {
            let user_ozy = home.join(".ozy").join("config.toml");
            if user_ozy.exists() {
                return Some(user_ozy);
            }
            let user_legacy = home.join(".ozymem.toml");
            if user_legacy.exists() {
                return Some(user_legacy);
            }
        }

        None
    }

    pub fn apply_preset(&mut self, preset: &str, origins: &mut HashMap<String, ConfigOrigin>) {
        let p = preset.to_lowercase();
        match p.as_str() {
            "laptop" | "battery" | "lightweight" => {
                self.core.preset = Some("laptop".to_string());
                self.compute.lightweight_mode = true;
                self.compute.max_compute = false;
                self.compute.mcts_simulations = 10;
                self.core.disable_watcher = true;
                origins.insert("core.preset".to_string(), ConfigOrigin::Preset("laptop".to_string()));
                origins.insert("compute.lightweight_mode".to_string(), ConfigOrigin::Preset("laptop".to_string()));
                origins.insert("compute.mcts_simulations".to_string(), ConfigOrigin::Preset("laptop".to_string()));
                origins.insert("core.disable_watcher".to_string(), ConfigOrigin::Preset("laptop".to_string()));
            }
            "power" | "workstation" | "server" => {
                self.core.preset = Some("power".to_string());
                self.compute.max_compute = true;
                self.compute.lightweight_mode = false;
                self.compute.mcts_simulations = 50;
                self.storage.neural_reranker = true;
                origins.insert("core.preset".to_string(), ConfigOrigin::Preset("power".to_string()));
                origins.insert("compute.max_compute".to_string(), ConfigOrigin::Preset("power".to_string()));
                origins.insert("compute.mcts_simulations".to_string(), ConfigOrigin::Preset("power".to_string()));
                origins.insert("storage.neural_reranker".to_string(), ConfigOrigin::Preset("power".to_string()));
            }
            "ci" | "headless" => {
                self.core.preset = Some("ci".to_string());
                self.core.disable_watcher = true;
                self.core.idle_timeout_secs = 0;
                self.compute.mcts_simulations = 15;
                origins.insert("core.preset".to_string(), ConfigOrigin::Preset("ci".to_string()));
                origins.insert("core.disable_watcher".to_string(), ConfigOrigin::Preset("ci".to_string()));
                origins.insert("core.idle_timeout_secs".to_string(), ConfigOrigin::Preset("ci".to_string()));
                origins.insert("compute.mcts_simulations".to_string(), ConfigOrigin::Preset("ci".to_string()));
            }
            _ => {}
        }
    }

    fn merge_from(&mut self, other: &OzyConfig, origins: &mut HashMap<String, ConfigOrigin>, file_path: &Path) {
        let f_origin = ConfigOrigin::File(file_path.to_path_buf());

        if other.core.preset.is_some() {
            self.core.preset = other.core.preset.clone();
            origins.insert("core.preset".to_string(), f_origin.clone());
        }
        if other.core.data_dir != default_data_dir() {
            self.core.data_dir = other.core.data_dir.clone();
            origins.insert("core.data_dir".to_string(), f_origin.clone());
        }
        if other.core.idle_timeout_secs != default_idle_timeout_secs() {
            self.core.idle_timeout_secs = other.core.idle_timeout_secs;
            origins.insert("core.idle_timeout_secs".to_string(), f_origin.clone());
        }
        if other.core.disable_watcher {
            self.core.disable_watcher = true;
            origins.insert("core.disable_watcher".to_string(), f_origin.clone());
        }

        if other.mcp.show_legacy_tools {
            self.mcp.show_legacy_tools = true;
            origins.insert("mcp.show_legacy_tools".to_string(), f_origin.clone());
        }
        if other.mcp.max_tokens != default_max_tokens() {
            self.mcp.max_tokens = other.mcp.max_tokens;
            origins.insert("mcp.max_tokens".to_string(), f_origin.clone());
        }
        if other.mcp.default_lod != default_lod() {
            self.mcp.default_lod = other.mcp.default_lod.clone();
            origins.insert("mcp.default_lod".to_string(), f_origin.clone());
        }

        if other.brain.mode != default_brain_mode() {
            self.brain.mode = other.brain.mode.clone();
            origins.insert("brain.mode".to_string(), f_origin.clone());
        }
        if other.brain.port != default_brain_port() {
            self.brain.port = other.brain.port;
            origins.insert("brain.port".to_string(), f_origin.clone());
        }
        if other.brain.bin_path.is_some() {
            self.brain.bin_path = other.brain.bin_path.clone();
            origins.insert("brain.bin_path".to_string(), f_origin.clone());
        }
        if other.brain.python_path.is_some() {
            self.brain.python_path = other.brain.python_path.clone();
            origins.insert("brain.python_path".to_string(), f_origin.clone());
        }

        if other.compute.lightweight_mode {
            self.compute.lightweight_mode = true;
            origins.insert("compute.lightweight_mode".to_string(), f_origin.clone());
        }
        if other.compute.max_compute {
            self.compute.max_compute = true;
            origins.insert("compute.max_compute".to_string(), f_origin.clone());
        }
        if other.compute.mcts_simulations != default_mcts_simulations() {
            self.compute.mcts_simulations = other.compute.mcts_simulations;
            origins.insert("compute.mcts_simulations".to_string(), f_origin.clone());
        }
        if other.compute.mcts_max_depth != default_mcts_max_depth() {
            self.compute.mcts_max_depth = other.compute.mcts_max_depth;
            origins.insert("compute.mcts_max_depth".to_string(), f_origin.clone());
        }

        if other.storage.postgres_dsn.is_some() {
            self.storage.postgres_dsn = other.storage.postgres_dsn.clone();
            origins.insert("storage.postgres_dsn".to_string(), f_origin.clone());
        }
        if other.storage.neural_reranker {
            self.storage.neural_reranker = true;
            origins.insert("storage.neural_reranker".to_string(), f_origin.clone());
        }
    }

    fn apply_env_overrides(&mut self, origins: &mut HashMap<String, ConfigOrigin>) {
        if let Ok(v) = std::env::var("OZY_BRAIN_MODE") {
            self.brain.mode = v;
            origins.insert("brain.mode".to_string(), ConfigOrigin::Env("OZY_BRAIN_MODE".to_string()));
        }
        if let Ok(v) = std::env::var("OZY_BRAIN_PORT") {
            if let Ok(p) = v.parse::<u16>() {
                self.brain.port = p;
                origins.insert("brain.port".to_string(), ConfigOrigin::Env("OZY_BRAIN_PORT".to_string()));
            }
        }
        if let Ok(v) = std::env::var("OZY_BRAIN_BIN") {
            self.brain.bin_path = Some(v);
            origins.insert("brain.bin_path".to_string(), ConfigOrigin::Env("OZY_BRAIN_BIN".to_string()));
        }
        if let Ok(v) = std::env::var("OZY_BRAIN_PYTHON") {
            self.brain.python_path = Some(v);
            origins.insert("brain.python_path".to_string(), ConfigOrigin::Env("OZY_BRAIN_PYTHON".to_string()));
        }

        if let Ok(v) = std::env::var("OZY_LIGHTWEIGHT_MODE") {
            self.compute.lightweight_mode = matches!(v.as_str(), "1" | "true" | "TRUE");
            origins.insert("compute.lightweight_mode".to_string(), ConfigOrigin::Env("OZY_LIGHTWEIGHT_MODE".to_string()));
        }
        if let Ok(v) = std::env::var("OZY_MAX_COMPUTE") {
            self.compute.max_compute = matches!(v.as_str(), "1" | "true" | "TRUE");
            origins.insert("compute.max_compute".to_string(), ConfigOrigin::Env("OZY_MAX_COMPUTE".to_string()));
        }

        if let Ok(v) = std::env::var("OZYMEM_DISABLE_WATCHER") {
            self.core.disable_watcher = matches!(v.as_str(), "1" | "true" | "TRUE");
            origins.insert("core.disable_watcher".to_string(), ConfigOrigin::Env("OZYMEM_DISABLE_WATCHER".to_string()));
        }
        if let Ok(v) = std::env::var("OZYMEM_IDLE_TIMEOUT_SECS") {
            if let Ok(t) = v.parse::<u64>() {
                self.core.idle_timeout_secs = t;
                origins.insert("core.idle_timeout_secs".to_string(), ConfigOrigin::Env("OZYMEM_IDLE_TIMEOUT_SECS".to_string()));
            }
        }
        if let Ok(v) = std::env::var("OZYMEM_SHOW_LEGACY_TOOLS") {
            self.mcp.show_legacy_tools = matches!(v.as_str(), "1" | "true" | "TRUE");
            origins.insert("mcp.show_legacy_tools".to_string(), ConfigOrigin::Env("OZYMEM_SHOW_LEGACY_TOOLS".to_string()));
        }
        if let Ok(v) = std::env::var("OZYMEM_NEURAL_RERANKER") {
            self.storage.neural_reranker = matches!(v.as_str(), "1" | "true" | "TRUE");
            origins.insert("storage.neural_reranker".to_string(), ConfigOrigin::Env("OZYMEM_NEURAL_RERANKER".to_string()));
        }

        if let Ok(v) = std::env::var("OZY_PG_DSN").or_else(|_| std::env::var("POSTGRES_URL")) {
            self.storage.postgres_dsn = Some(v);
            origins.insert("storage.postgres_dsn".to_string(), ConfigOrigin::Env("OZY_PG_DSN".to_string()));
        }
    }

    pub fn get_field(&self, key: &str) -> Option<String> {
        match key {
            "core.preset" => self.core.preset.clone(),
            "core.data_dir" => Some(self.core.data_dir.clone()),
            "core.idle_timeout_secs" => Some(self.core.idle_timeout_secs.to_string()),
            "core.disable_watcher" => Some(self.core.disable_watcher.to_string()),
            "mcp.show_legacy_tools" => Some(self.mcp.show_legacy_tools.to_string()),
            "mcp.max_tokens" => Some(self.mcp.max_tokens.to_string()),
            "mcp.default_lod" => Some(self.mcp.default_lod.clone()),
            "brain.mode" => Some(self.brain.mode.clone()),
            "brain.port" => Some(self.brain.port.to_string()),
            "brain.bin_path" => self.brain.bin_path.clone(),
            "brain.python_path" => self.brain.python_path.clone(),
            "brain.model" => Some(self.brain.model.clone()),
            "compute.lightweight_mode" => Some(self.compute.lightweight_mode.to_string()),
            "compute.max_compute" => Some(self.compute.max_compute.to_string()),
            "compute.mcts_simulations" => Some(self.compute.mcts_simulations.to_string()),
            "compute.mcts_max_depth" => Some(self.compute.mcts_max_depth.to_string()),
            "compute.c_puct" => Some(self.compute.c_puct.to_string()),
            "compute.beta_threat" => Some(self.compute.beta_threat.to_string()),
            "compute.lambda_epist" => Some(self.compute.lambda_epist.to_string()),
            "storage.postgres_dsn" => self.storage.postgres_dsn.clone(),
            "storage.neural_reranker" => Some(self.storage.neural_reranker.to_string()),
            _ => None,
        }
    }

    pub fn set_field(&mut self, key: &str, value: &str) -> Result<()> {
        match key {
            "core.preset" => self.core.preset = Some(value.to_string()),
            "core.data_dir" => self.core.data_dir = value.to_string(),
            "core.idle_timeout_secs" => self.core.idle_timeout_secs = value.parse()?,
            "core.disable_watcher" => self.core.disable_watcher = value.parse()?,
            "mcp.show_legacy_tools" => self.mcp.show_legacy_tools = value.parse()?,
            "mcp.max_tokens" => self.mcp.max_tokens = value.parse()?,
            "mcp.default_lod" => self.mcp.default_lod = value.to_string(),
            "brain.mode" => self.brain.mode = value.to_string(),
            "brain.port" => self.brain.port = value.parse()?,
            "brain.bin_path" => self.brain.bin_path = Some(value.to_string()),
            "brain.python_path" => self.brain.python_path = Some(value.to_string()),
            "brain.model" => self.brain.model = value.to_string(),
            "compute.lightweight_mode" => self.compute.lightweight_mode = value.parse()?,
            "compute.max_compute" => self.compute.max_compute = value.parse()?,
            "compute.mcts_simulations" => self.compute.mcts_simulations = value.parse()?,
            "compute.mcts_max_depth" => self.compute.mcts_max_depth = value.parse()?,
            "compute.c_puct" => self.compute.c_puct = value.parse()?,
            "compute.beta_threat" => self.compute.beta_threat = value.parse()?,
            "compute.lambda_epist" => self.compute.lambda_epist = value.parse()?,
            "storage.postgres_dsn" => self.storage.postgres_dsn = Some(value.to_string()),
            "storage.neural_reranker" => self.storage.neural_reranker = value.parse()?,
            _ => return Err(anyhow!("Clave de configuración desconocida: '{}'", key)),
        }
        Ok(())
    }

    pub fn save_to_file(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let toml_str = toml::to_string_pretty(self)
            .context("Error al serializar configuración a TOML")?;
        fs::write(path, toml_str)
            .with_context(|| format!("Error al escribir archivo de configuración en {}", path.display()))?;
        Ok(())
    }

    pub fn generate_template_toml(preset: Option<&str>) -> String {
        let p_comment = preset.unwrap_or("default");
        format!(
r#"# Ozygram Workspace Configuration (.ozy/config.toml)
# Zero-Emoji Standard compliant.
# Precedence: CLI flags > Environment Variables > This File > Defaults

[core]
# Preset operativo: "laptop" (cooperativo <5% CPU), "power" (50 MCTS, Rayon), "ci" (fail-fast, no watcher)
preset = "{preset_val}"
data_dir = ".ozymem"
idle_timeout_secs = 1800
disable_watcher = false

[mcp]
# Exponer solo las 5 meta-herramientas maestras (false) o incluir los 80+ alias legacy (true)
show_legacy_tools = false
max_tokens = 4000
default_lod = "contracts"

[brain]
mode = "auto"
port = 50051
model = "gemini-2.5-flash"

[compute]
# Afinación de MCTS Active Inference en memoria volátil
lightweight_mode = false
max_compute = false
mcts_simulations = {sims}
mcts_max_depth = 6
c_puct = 1.414
beta_threat = 0.35
lambda_epist = 0.35

[storage]
# Conexión Neocortex L3 PostgreSQL + pgvector + ltree (opcional)
# postgres_dsn = "postgres://user:pass@localhost:5432/ozygram"
neural_reranker = false
"#,
            preset_val = preset.unwrap_or("default"),
            sims = if p_comment == "laptop" { 10 } else if p_comment == "power" { 50 } else { 25 }
        )
    }

    pub fn all_keys() -> &'static [&'static str] {
        &[
            "core.preset",
            "core.data_dir",
            "core.idle_timeout_secs",
            "core.disable_watcher",
            "mcp.show_legacy_tools",
            "mcp.max_tokens",
            "mcp.default_lod",
            "brain.mode",
            "brain.port",
            "brain.bin_path",
            "brain.python_path",
            "brain.model",
            "compute.lightweight_mode",
            "compute.max_compute",
            "compute.mcts_simulations",
            "compute.mcts_max_depth",
            "compute.c_puct",
            "compute.beta_threat",
            "compute.lambda_epist",
            "storage.postgres_dsn",
            "storage.neural_reranker",
        ]
    }
}
