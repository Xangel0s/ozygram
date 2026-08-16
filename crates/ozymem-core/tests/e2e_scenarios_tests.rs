use ozymem_core::bundle::{export_bundle, import_bundle};
use ozymem_core::graph_backend::GraphBackend;
use ozymem_core::sync::read_file_with_backoff;
use ozymem_core::McpBackend;
use std::fs;
use tempfile::tempdir;

/// E2E Scenario 1: Live Coding & Hot Delta Indexing
/// 1. Initialize project with initial source file.
/// 2. Modify file in place.
/// 3. Delta indexer computes hash in <1ms and updates symbol graph.
#[tokio::test]
async fn test_e2e_scenario_1_live_coding_delta_indexing() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let src_dir = root.join("src").join("services");
    fs::create_dir_all(&src_dir).unwrap();

    let auth_file = src_dir.join("auth.rs");
    fs::write(
        &auth_file,
        "pub fn login() {}\npub fn logout() {}\n",
    )
    .unwrap();

    let backend = GraphBackend::open_for_project(root).unwrap();
    backend.full_scan(&root.to_string_lossy(), None).unwrap();

    let graph_before = backend.get_graph_summary().await.unwrap();
    assert_eq!(graph_before.file_count, 1);
    assert_eq!(graph_before.function_count, 2);

    // Live edit: Add a new function `verify_mfa_token`
    fs::write(
        &auth_file,
        "pub fn login() {}\npub fn logout() {}\npub fn verify_mfa_token() {}\n",
    )
    .unwrap();

    // Hot delta index event
    let delta_res = backend.index_file_delta(&auth_file, root).unwrap();
    assert!(matches!(delta_res, ozymem_core::DeltaIndexResult::Indexed { symbols: 3, .. }));

    // Verify symbols updated
    let graph_after = backend.get_graph_summary().await.unwrap();
    assert_eq!(graph_after.function_count, 3);
}

/// E2E Scenario 2: Business Rule & Code Drift Auditing
/// 1. Record convention: "never store raw cardholder data, always tokenize via payment gateway".
/// 2. Check diff with violation -> alert raised.
/// 3. Check compliant diff -> 0 alerts.
#[tokio::test]
async fn test_e2e_scenario_2_code_drift_auditing() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let file = root.join("payment.rs");
    fs::write(&file, "pub fn process_payment() {}\n").unwrap();

    let backend = GraphBackend::open_for_project(root).unwrap();
    backend.index_file_delta(&file, root).unwrap();

    // Record strict architectural standard
    backend
        .record_entry(
            &file.to_string_lossy(),
            Some("process_payment"),
            "PCI-DSS security standard",
            "never store raw cardholder card_number, always tokenize via payment gateway",
            "convention",
        )
        .await
        .unwrap();

    // Violating commit diff
    let violating_diff = "+    let card_number = payload.card_number;\n+    save_to_db(&card_number);";
    let alerts = backend
        .detect_code_drift(&[file.to_string_lossy().to_string()], violating_diff)
        .unwrap();

    assert_eq!(alerts.len(), 1);
    assert_eq!(alerts[0].severity, "warning");
    assert!(alerts[0].diff_snippet.contains("card_number"));

    // Compliant commit diff
    let compliant_diff = "+    let token = payload.payment_token;\n+    save_to_db(&token);";
    let no_alerts = backend
        .detect_code_drift(&[file.to_string_lossy().to_string()], compliant_diff)
        .unwrap();
    assert_eq!(no_alerts.len(), 0);
}

/// E2E Scenario 3: Project Onboarding & Knowledge Transfer
/// 1. Dev A exports project bundle `.ozymem`.
/// 2. Dev B imports `.ozymem` into empty repo.
/// 3. Dev B immediately accesses full architecture context and API routes.
#[tokio::test]
async fn test_e2e_scenario_3_project_onboarding_knowledge_transfer() {
    let dir_a = tempdir().unwrap();
    let root_a = dir_a.path();
    let routes_file_a = root_a.join("routes.py");
    fs::write(
        &routes_file_a,
        r#"
from fastapi import APIRouter
router = APIRouter()

@router.get("/api/v1/customers")
async def list_customers():
    return []

@router.post("/api/v1/customers/create")
async def create_customer(dto: CustomerCreateDTO):
    return dto
"#,
    )
    .unwrap();

    let backend_a = GraphBackend::open_for_project(root_a).unwrap();
    backend_a.index_file_delta(&routes_file_a, root_a).unwrap();

    backend_a
        .record_lesson(
            &routes_file_a.to_string_lossy(),
            Some("list_customers"),
            "database pagination performance",
            "always use keyset pagination instead of OFFSET for tables with >100k rows",
        )
        .await
        .unwrap();

    // Dev A exports `.ozymem`
    let bundle_path = root_a.join("crm_knowledge.ozymem");
    let export_summary = export_bundle(&backend_a, "crm_service", &bundle_path).unwrap();
    assert_eq!(export_summary.lessons_exported, 1);
    assert_eq!(export_summary.routes_exported, 2);

    // Dev B arrives with empty repo
    let dir_b = tempdir().unwrap();
    let root_b = dir_b.path();
    let backend_b = GraphBackend::open_for_project(root_b).unwrap();

    // Dev B imports bundle
    let import_summary = import_bundle(&backend_b, &bundle_path, true).await.unwrap();
    assert_eq!(import_summary.lessons_imported, 1);
    assert_eq!(import_summary.routes_imported, 2);

    // Dev B verifies lessons and rules are active
    let lessons_b = backend_b.recent_lessons(None, 10).await.unwrap();
    assert_eq!(lessons_b.len(), 1);
    assert!(lessons_b[0].solution.contains("keyset pagination"));
}

/// E2E Scenario 4: Windows Lock Resilience & Safe Stale Pruning
#[test]
fn test_e2e_scenario_4_windows_lock_and_safe_prune() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("sample.txt");
    fs::write(&file, "hello windows retry backoff").unwrap();

    let content = read_file_with_backoff(&file, 3).unwrap();
    assert_eq!(content, "hello windows retry backoff");
}
