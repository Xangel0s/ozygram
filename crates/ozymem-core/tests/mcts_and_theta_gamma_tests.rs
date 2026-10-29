use std::collections::HashMap;
use std::time::Instant;
use ozymem_core::mcts::{MctsAction, MctsState, MctsStatePatch, MctsTree};
use ozymem_core::theta_gamma::ThetaGammaBuffer;

#[test]
fn test_mcts_expected_free_energy_calculation() {
    let tree = MctsTree::new(std::f64::consts::SQRT_2, 0.35, 6, 15);

    let clean_state = MctsState {
        file_path: "app/services/crm.rs".to_string(),
        symbol: "update_customer".to_string(),
        syntax_valid: true,
        backward_compatible: true,
        guard_applied: true,
        isolated: true,
        blast_ratio: 0.05,
        poincare_distance: 0.2,
    };

    let efe_clean = tree.compute_expected_free_energy(&clean_state, 0);
    assert!(efe_clean.pragmatic_value > 0.8, "Pragmatic value must be high for invariant-aligned state");
    assert!(efe_clean.epistemic_value > 0.5, "Epistemic value must be high for unvisited node");
    assert!(efe_clean.efe_score > 0.85, "Total EFE score must satisfy solution threshold");

    // Broken state
    let broken_state = MctsState {
        file_path: "app/services/crm.rs".to_string(),
        symbol: "update_customer".to_string(),
        syntax_valid: false,
        backward_compatible: false,
        guard_applied: false,
        isolated: false,
        blast_ratio: 0.9,
        poincare_distance: 2.5,
    };
    let efe_broken = tree.compute_expected_free_energy(&broken_state, 0);
    assert!(efe_broken.pragmatic_value < 0.0, "Pragmatic value must penalize invalid syntax and high blast");
    assert!(efe_broken.efe_score < efe_clean.efe_score);
}

#[test]
fn test_mcts_tree_search_submillisecond_latency() {
    let mut tree = MctsTree::new(std::f64::consts::SQRT_2, 0.35, 6, 25);

    let root_state = MctsState {
        file_path: "crmnew/api-geofal-crm/app/routers/leads.py".to_string(),
        symbol: "create_lead_endpoint".to_string(),
        syntax_valid: true,
        backward_compatible: true,
        guard_applied: false,
        isolated: false,
        blast_ratio: 0.15,
        poincare_distance: 0.4,
    };

    let start = Instant::now();
    let best_node = tree.run_search(root_state, 0.0);
    let elapsed = start.elapsed();

    assert!(best_node.is_some(), "Search must return a valid node");
    assert!(
        elapsed.as_millis() < 5,
        "25 MCTS simulations in volatile RAM must complete in under 5ms (was {:?})",
        elapsed
    );

    let best_action = tree.best_action();
    assert!(best_action.is_some(), "Best action must be extracted");
    assert!(tree.nodes.len() > 1, "Tree must be expanded with candidate mutations");
}

#[test]
fn test_mcts_prunes_direct_breaking_cutover() {
    let mut tree = MctsTree::new(std::f64::consts::SQRT_2, 0.35, 6, 10);

    let root_state = MctsState::default();
    let root_efe = tree.compute_expected_free_energy(&root_state, 0);
    tree.nodes.push(ozymem_core::mcts::MctsNode {
        node_id: "root".to_string(),
        state: root_state,
        parent: None,
        children: Vec::new(),
        action: None,
        visits: 0,
        value_sum: 0.0,
        depth: 0,
        pragmatic_value: root_efe.pragmatic_value,
        epistemic_value: root_efe.epistemic_value,
        efe_score: root_efe.efe_score,
        is_terminal: false,
        is_solution: false,
    });

    let breaking_action = MctsAction {
        action_type: "breaking_cutover".to_string(),
        description: "Hard cutover removing deprecated endpoint without adapter".to_string(),
        has_shim: false,
        patch_code: None,
        state_patch: Some(MctsStatePatch {
            param_added: None,
            backward_compatible: Some(false),
            guard_applied: None,
            syntax_valid: Some(true),
            isolated: None,
            blast_ratio: Some(0.9),
        }),
    };

    let children = tree.expand(0, Some(vec![breaking_action]));
    assert!(children.is_empty(), "Direct breaking cutover without shim must be pruned by World Model");
    assert_eq!(tree.pruned_by_world_model, 1);
}

#[test]
fn test_theta_phase_classification() {
    assert_eq!(ThetaGammaBuffer::classify_file_phase("app/models/user_schema.py"), ("leaf_contracts", 1));
    assert_eq!(ThetaGammaBuffer::classify_file_phase("frontend/src/types/table.d.ts"), ("leaf_contracts", 1));
    assert_eq!(ThetaGammaBuffer::classify_file_phase("app/services/billing.py"), ("domain_services", 2));
    assert_eq!(ThetaGammaBuffer::classify_file_phase("frontend/src/utils/tableDataCache.ts"), ("domain_services", 2));
    assert_eq!(ThetaGammaBuffer::classify_file_phase("app/routers/auth.py"), ("entrypoints", 3));
    assert_eq!(ThetaGammaBuffer::classify_file_phase("frontend/src/components/TableEditor.tsx"), ("entrypoints", 3));
    assert_eq!(ThetaGammaBuffer::classify_file_phase("tests/test_billing.py"), ("validation", 4));
}

#[test]
fn test_gamma_window_extraction_and_compression() {
    let buffer = ThetaGammaBuffer::new(4, 250);

    // Create 100-line simulated file
    let mut lines = Vec::new();
    for i in 1..=100 {
        if i == 50 {
            lines.push("pub fn process_order(order_id: u64, amount: f64) -> Result<(), Error> {");
        } else {
            lines.push("    // filler code line");
        }
    }
    let content = lines.join("\n");

    let slot = buffer.extract_gamma_window("app/services/orders.rs", &content, Some("process_order"));
    assert_eq!(slot.start_line, 46); // 50 - 4
    assert_eq!(slot.end_line, 54);   // 50 + 4 + 1
    assert!(slot.snippet.contains("process_order"));
    assert!(slot.tokens < 100);

    // Multi-file context packing
    let files = vec![
        "app/models/account.rs".to_string(),
        "app/services/account_service.rs".to_string(),
        "app/routers/account_router.rs".to_string(),
        "tests/account_test.rs".to_string(),
    ];
    let mut contents = HashMap::new();
    let mut hints = HashMap::new();
    for f in &files {
        contents.insert(f.clone(), content.clone());
        hints.insert(f.clone(), vec!["process_order".to_string()]);
    }

    let packed = buffer.pack_context(&files, &hints, &contents);
    assert_eq!(packed.phase_bins.len(), 4);
    assert_eq!(packed.phase_bins[0].phase_name, "leaf_contracts");
    assert_eq!(packed.phase_bins[1].phase_name, "domain_services");
    assert_eq!(packed.phase_bins[2].phase_name, "entrypoints");
    assert_eq!(packed.phase_bins[3].phase_name, "validation");

    assert!(packed.raw_tokens_total > 1500);
    assert!(packed.gamma_tokens_total < 400);
    assert!(packed.compression_ratio > 75.0, "Context compression must exceed 75% (was {}%)", packed.compression_ratio);
}

