use ozymem_core::config::{ConfigOrigin, OzyConfig};
use std::fs;
use tempfile::tempdir;

#[test]
fn test_default_config_values() {
    let config = OzyConfig::default();
    assert_eq!(config.core.data_dir, ".ozymem");
    assert_eq!(config.core.idle_timeout_secs, 1800);
    assert_eq!(config.core.disable_watcher, false);
    assert_eq!(config.mcp.show_legacy_tools, false);
    assert_eq!(config.mcp.max_tokens, 4000);
    assert_eq!(config.brain.mode, "auto");
    assert_eq!(config.brain.port, 50051);
    assert_eq!(config.compute.mcts_simulations, 25);
    assert_eq!(config.compute.lightweight_mode, false);
    assert_eq!(config.compute.max_compute, false);
    assert_eq!(config.compute.acceleration, "auto");
}

#[test]
fn test_preset_laptop_cooperative() {
    let mut config = OzyConfig::default();
    let mut origins = std::collections::HashMap::new();

    config.apply_preset("laptop", &mut origins);

    assert_eq!(config.core.preset, Some("laptop".to_string()));
    assert_eq!(config.compute.lightweight_mode, true);
    assert_eq!(config.compute.mcts_simulations, 10);
    assert_eq!(config.core.disable_watcher, true);
    assert_eq!(config.compute.acceleration, "cpu");
    assert_eq!(origins.get("compute.lightweight_mode"), Some(&ConfigOrigin::Preset("laptop".to_string())));
}

#[test]
fn test_preset_power_workstation() {
    let mut config = OzyConfig::default();
    let mut origins = std::collections::HashMap::new();

    config.apply_preset("power", &mut origins);

    assert_eq!(config.core.preset, Some("power".to_string()));
    assert_eq!(config.compute.max_compute, true);
    assert_eq!(config.compute.mcts_simulations, 50);
    assert_eq!(config.storage.neural_reranker, true);
    assert_eq!(config.compute.acceleration, "cuda");
}

#[test]
fn test_preset_ci_headless() {
    let mut config = OzyConfig::default();
    let mut origins = std::collections::HashMap::new();

    config.apply_preset("ci", &mut origins);

    assert_eq!(config.core.preset, Some("ci".to_string()));
    assert_eq!(config.core.disable_watcher, true);
    assert_eq!(config.core.idle_timeout_secs, 0);
    assert_eq!(config.compute.mcts_simulations, 15);
}

#[test]
fn test_dynamic_get_and_set_field() {
    let mut config = OzyConfig::default();

    assert_eq!(config.get_field("compute.mcts_simulations"), Some("25".to_string()));
    config.set_field("compute.mcts_simulations", "40").unwrap();
    assert_eq!(config.compute.mcts_simulations, 40);
    assert_eq!(config.get_field("compute.mcts_simulations"), Some("40".to_string()));

    config.set_field("compute.acceleration", "cuda").unwrap();
    assert_eq!(config.compute.acceleration, "cuda");
    config.set_field("core.idle_timeout_secs", "900").unwrap();
    assert_eq!(config.core.idle_timeout_secs, 900);

    config.set_field("brain.port", "50055").unwrap();
    assert_eq!(config.brain.port, 50055);

    // Invalid key returns error
    assert!(config.set_field("non_existent.key", "val").is_err());
}

#[test]
fn test_file_persistence_roundtrip() {
    let tmp = tempdir().unwrap();
    let cfg_path = tmp.path().join(".ozy").join("config.toml");

    let mut config = OzyConfig::default();
    config.core.preset = Some("laptop".to_string());
    config.compute.mcts_simulations = 12;
    config.save_to_file(&cfg_path).unwrap();

    assert!(cfg_path.exists());

    let (loaded, origins) = OzyConfig::load_resolved(Some(tmp.path())).unwrap();
    assert_eq!(loaded.core.preset, Some("laptop".to_string()));
    assert_eq!(loaded.compute.mcts_simulations, 12);
    assert_eq!(origins.get("core.preset"), Some(&ConfigOrigin::File(cfg_path)));
}

#[test]
fn test_env_override_precedence() {
    let tmp = tempdir().unwrap();
    let cfg_path = tmp.path().join(".ozy").join("config.toml");

    let template = OzyConfig::generate_template_toml(Some("laptop"));
    fs::create_dir_all(cfg_path.parent().unwrap()).unwrap();
    fs::write(&cfg_path, template).unwrap();

    // Set environment variable
    std::env::set_var("OZY_BRAIN_MODE", "persistent");
    std::env::set_var("OZY_MAX_COMPUTE", "1");

    let (loaded, origins) = OzyConfig::load_resolved(Some(tmp.path())).unwrap();

    assert_eq!(loaded.brain.mode, "persistent");
    assert_eq!(origins.get("brain.mode"), Some(&ConfigOrigin::Env("OZY_BRAIN_MODE".to_string())));
    assert_eq!(loaded.compute.max_compute, true);
    assert_eq!(origins.get("compute.max_compute"), Some(&ConfigOrigin::Env("OZY_MAX_COMPUTE".to_string())));

    std::env::remove_var("OZY_BRAIN_MODE");
    std::env::remove_var("OZY_MAX_COMPUTE");
}

