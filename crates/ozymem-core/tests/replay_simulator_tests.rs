use ozymem_core::graph_backend::exploration::ExplorationNode;
use ozymem_core::graph_backend::replay_simulator::{
    ExplorationPolicy, ReplaySimulator,
};
use ozymem_core::graph_backend::GraphBackend;

fn make_dummy_node(
    id: &str,
    parent_id: Option<&str>,
    depth: i64,
    action: &str,
    cost: i64,
    reward: f64,
    is_solution: bool,
) -> ExplorationNode {
    ExplorationNode {
        id: id.to_string(),
        trajectory_id: "traj_test_1".to_string(),
        parent_id: parent_id.map(|s| s.to_string()),
        depth,
        action_type: "command".to_string(),
        action_payload: action.to_string(),
        observation: "dummy observation".to_string(),
        cost_tokens: cost,
        latency_ms: 100,
        reward_score: reward,
        visit_count: 1,
        value_estimate: reward,
        is_solution,
        is_pruned: false,
        rollback_snapshot: None,
        execution_receipt: None,
        created_at: "2026-11-12T00:00:00Z".to_string(),
    }
}

#[test]
fn test_replay_simulator_linear_tree() {
    let nodes = vec![
        make_dummy_node("n0", None, 0, "root init", 500, 0.1, false),
        make_dummy_node("n1", Some("n0"), 1, "failing test with error", 2500, -0.8, false),
        make_dummy_node("n2", Some("n0"), 1, "direct clean fix", 800, 0.95, true),
    ];

    let baseline = ExplorationPolicy::default();
    let candidate = ExplorationPolicy::fast_reflex();

    let result = ReplaySimulator::replay_counterfactual("traj_test_1", &nodes, &baseline, &candidate);

    assert_eq!(result.trajectory_id, "traj_test_1");
    assert!(result.candidate_metrics.solution_found);
    assert!(result.candidate_metrics.steps_to_solution >= 1);
}

#[test]
fn test_replay_simulator_sweep_candidates() {
    let nodes = vec![
        make_dummy_node("n0", None, 0, "root", 100, 0.0, false),
        make_dummy_node("n1", Some("n0"), 1, "expensive dead end error", 5000, -1.0, false),
        make_dummy_node("n2", Some("n0"), 1, "sub-branch", 300, 0.3, false),
        make_dummy_node("n3", Some("n2"), 2, "verified solution", 400, 1.0, true),
    ];

    let candidates = ExplorationPolicy::standard_candidates();
    let sweep = ReplaySimulator::sweep_candidate_policies("traj_test_1", &nodes, &candidates);

    assert_eq!(sweep.len(), candidates.len());
    for res in &sweep {
        assert_eq!(res.trajectory_id, "traj_test_1");
    }

    let optimal = ReplaySimulator::find_optimal_policy("traj_test_1", &nodes, &candidates);
    assert!(optimal.is_some());
    let best = optimal.unwrap();
    assert!(best.candidate_metrics.solution_found);
}

#[test]
fn test_sqlite_persistence_counterfactual() {
    let tmp = tempfile::tempdir().unwrap();
    let db_path = tmp.path().join("memory.db");
    let backend = GraphBackend::open(Some(&db_path.to_string_lossy())).unwrap();

    let traj_id = backend.start_trajectory(".", "test persist task", Some("v1.0.0")).unwrap();

    let nodes = vec![
        make_dummy_node("n0", None, 0, "root", 100, 0.0, false),
        make_dummy_node("n1", Some("n0"), 1, "fix applied", 200, 0.95, true),
    ];

    let baseline = ExplorationPolicy::default();
    let candidate = ExplorationPolicy::fast_reflex();
    let mut replay = ReplaySimulator::replay_counterfactual(&traj_id, &nodes, &baseline, &candidate);
    replay.is_optimal = true;

    backend.save_counterfactual_simulation(&replay).unwrap();

    let list = backend.get_counterfactual_simulations(&traj_id).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].trajectory_id, traj_id);
    assert_eq!(list[0].candidate_policy.name, "fast_reflex");
    assert!(list[0].is_optimal);

    let optimal = backend.get_latest_optimal_policy(None).unwrap();
    assert!(optimal.is_some());
    assert_eq!(optimal.unwrap().name, "fast_reflex");
}
