// Unit and integration tests for ozymem-cli config commands (Track 1).
// Zero-Emoji Standard compliant.

use ozymem_cli::commands::config::{handle_config_command, ConfigSubcommand};
use ozymem_core::config::OzyConfig;
use std::fs;

#[test]
fn test_cli_config_init_and_get() {
    let tmp = std::env::temp_dir().join(format!("ozymem_cli_cfg_test_{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).unwrap();

    let original_cwd = std::env::current_dir().unwrap();
    std::env::set_current_dir(&tmp).unwrap();

    // 1. Init with preset laptop
    handle_config_command(Some(ConfigSubcommand::Init {
        preset: Some("laptop".to_string()),
        force: false,
    }))
    .expect("Init must succeed");

    let cfg_path = tmp.join(".ozy").join("config.toml");
    assert!(cfg_path.exists(), ".ozy/config.toml must be generated");

    // 2. Set custom value
    handle_config_command(Some(ConfigSubcommand::Set {
        key: "compute.mcts_simulations".to_string(),
        value: "35".to_string(),
    }))
    .expect("Set must succeed");

    // 3. Verify value persisted in file
    let (loaded, _) = OzyConfig::load_resolved(Some(&tmp)).unwrap();
    assert_eq!(loaded.compute.mcts_simulations, 35);
    assert_eq!(loaded.core.preset, Some("laptop".to_string()));

    std::env::set_current_dir(original_cwd).unwrap();
    let _ = fs::remove_dir_all(&tmp);
}
