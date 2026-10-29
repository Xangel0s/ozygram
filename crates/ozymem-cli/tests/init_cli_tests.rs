use ozymem_cli::commands::setup::{rollback_mcp_clients, try_inject_mcp_server, run_init};
use serde_json::Value;
use std::fs;

#[test]
fn test_try_inject_dry_run_preserves_file_and_no_backup() {
    let temp_dir = std::env::temp_dir().join(format!("ozymem-init-dryrun-{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).unwrap();

    let cfg_path = temp_dir.join("mcp_config.json");
    let original_json = r#"{"mcpServers":{"custom":{"command":"test"}}}"#;
    fs::write(&cfg_path, original_json).unwrap();

    // Run in dry_run mode
    let res = try_inject_mcp_server(&cfg_path, "ozymem-server", true);
    assert!(res.is_ok());

    // File content must be identical
    let after_content = fs::read_to_string(&cfg_path).unwrap();
    assert_eq!(after_content, original_json);

    // No .ozybak file should be generated in dry_run
    let entries: Vec<_> = fs::read_dir(&temp_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .collect();
    assert_eq!(entries.len(), 1);

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_try_inject_creates_ozybak_and_modifies_file() {
    let temp_dir = std::env::temp_dir().join(format!("ozymem-init-backup-{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).unwrap();

    let cfg_path = temp_dir.join("mcp_config.json");
    let original_json = r#"{"mcpServers":{"custom":{"command":"test"}}}"#;
    fs::write(&cfg_path, original_json).unwrap();

    // Run with real injection (dry_run = false)
    let res = try_inject_mcp_server(&cfg_path, "ozymem-server", false);
    assert!(res.is_ok());

    // File must have ozygram injected
    let modified: Value = serde_json::from_str(&fs::read_to_string(&cfg_path).unwrap()).unwrap();
    assert_eq!(
        modified["mcpServers"]["ozygram"]["command"].as_str(),
        Some("ozymem-server")
    );
    assert_eq!(
        modified["mcpServers"]["custom"]["command"].as_str(),
        Some("test")
    );

    // .ozybak.<timestamp> must exist and contain original content
    let entries: Vec<_> = fs::read_dir(&temp_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .collect();
    assert_eq!(entries.len(), 2);

    let backup_entry = entries
        .iter()
        .find(|e| e.file_name().to_string_lossy().contains(".ozybak."))
        .expect("must create a backup file with .ozybak.");

    let backup_content = fs::read_to_string(backup_entry.path()).unwrap();
    assert_eq!(backup_content, original_json);

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_rollback_mcp_clients_restores_and_cleans_up_backup() {
    let temp_dir = std::env::temp_dir().join(format!("ozymem-init-rollback-{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).unwrap();

    let cursor_dir = temp_dir.join(".cursor");
    fs::create_dir_all(&cursor_dir).unwrap();

    let target_cfg = cursor_dir.join("mcp.json");
    let corrupted_content = r#"{"mcpServers":{"ozygram":{"command":"corrupted"}}}"#;
    fs::write(&target_cfg, corrupted_content).unwrap();

    // Create a mock .ozybak backup
    let original_content = r#"{"mcpServers":{"original":{"command":"clean"}}}"#;
    let backup_file = cursor_dir.join("mcp.ozybak.1700000000");
    fs::write(&backup_file, original_content).unwrap();

    let dummy_home = temp_dir.join("home");
    fs::create_dir_all(&dummy_home).unwrap();

    let restored = rollback_mcp_clients(&dummy_home, &temp_dir, Some("Cursor (Workspace)"));
    assert_eq!(restored.len(), 1);
    assert_eq!(restored[0].0, "Cursor (Workspace)");

    // Target file must be restored to original content
    let final_content = fs::read_to_string(&target_cfg).unwrap();
    assert_eq!(final_content, original_content);

    // Backup file must have been cleaned up
    assert!(!backup_file.exists());

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_run_init_dry_run_isolated_workspace() {
    let temp_dir = std::env::temp_dir().join(format!("ozymem-init-ws-{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).unwrap();

    // Call run_init with dry_run = true
    let res = run_init(Some(temp_dir.clone()), true, None, true, false).await;
    assert!(res.is_ok());

    // Workspace .ozymemignore should have been created
    assert!(temp_dir.join(".ozymemignore").exists());

    let _ = fs::remove_dir_all(&temp_dir);
}

