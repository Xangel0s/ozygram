use ozymem_core::graph_backend::helpers::is_noise_path_with_root;
use ozymem_core::graph_backend::GraphBackend;
use ozymem_core::sync::{
    check_noise_or_huge_file_with_root, DeltaIndexResult, NoiseFilterDecision,
    DEFAULT_MAX_DELTA_FILE_BYTES,
};
use std::fs;
use tempfile::tempdir;

fn normalize(p: &str) -> String {
    p.replace('\\', "/")
}

#[tokio::test]
async fn test_reverse_impact_direct_and_transitive_callers() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let root_str = root.to_str().unwrap();

    // Hierarchy:
    // core.rs (leaf callee)
    // service.rs imports core.rs (direct caller)
    // controller.rs imports service.rs (transitive caller)
    let core_path = root.join("core.rs");
    let service_path = root.join("service.rs");
    let controller_path = root.join("controller.rs");

    fs::write(
        &core_path,
        "pub struct CoreConfig;\npub fn init_core() -> CoreConfig { CoreConfig }\n",
    )
    .unwrap();

    fs::write(
        &service_path,
        "mod core;\nuse core::{CoreConfig, init_core};\npub fn run_service() { let _ = init_core(); }\n",
    )
    .unwrap();

    fs::write(
        &controller_path,
        "mod service;\nuse service::run_service;\npub fn handle_request() { run_service(); }\n",
    )
    .unwrap();

    let backend = GraphBackend::open_for_project(root).unwrap();
    backend.full_scan(root_str, None).unwrap();

    // Calling analyze_impact on core_path must trace INCOMING edges (callers)
    let impacts = backend.analyze_impact(core_path.to_str().unwrap(), 3);

    let service_impact = impacts
        .iter()
        .find(|e| normalize(&e.file_path).ends_with("/service.rs"));
    let controller_impact = impacts
        .iter()
        .find(|e| normalize(&e.file_path).ends_with("/controller.rs"));

    assert!(
        service_impact.is_some(),
        "Direct caller service.rs must be in core.rs impact blast radius"
    );
    assert_eq!(
        service_impact.unwrap().depth,
        1,
        "service.rs must be at depth 1"
    );

    assert!(
        controller_impact.is_some(),
        "Transitive caller controller.rs must be in core.rs impact blast radius"
    );
    assert_eq!(
        controller_impact.unwrap().depth,
        2,
        "controller.rs must be at depth 2"
    );
}

#[tokio::test]
async fn test_reverse_impact_strictly_incoming_no_false_positives() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let root_str = root.to_str().unwrap();

    let core_path = root.join("core.rs");
    let service_path = root.join("service.rs");
    let controller_path = root.join("controller.rs");

    fs::write(&core_path, "pub fn core_fn() {}\n").unwrap();
    fs::write(
        &service_path,
        "mod core;\nuse core::core_fn;\npub fn service_fn() { core_fn(); }\n",
    )
    .unwrap();
    fs::write(
        &controller_path,
        "mod service;\nuse service::service_fn;\npub fn controller_fn() { service_fn(); }\n",
    )
    .unwrap();

    let backend = GraphBackend::open_for_project(root).unwrap();
    backend.full_scan(root_str, None).unwrap();

    // controller_path has no callers (incoming edges = 0)
    let impacts = backend.analyze_impact(controller_path.to_str().unwrap(), 3);
    assert!(
        impacts.is_empty(),
        "Top-level caller with no dependents must have empty impact, found: {:?}",
        impacts
    );
}

#[tokio::test]
async fn test_reverse_impact_cycle_handling() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let root_str = root.to_str().unwrap();

    // Circular dependency:
    // a.rs imports b.rs, b.rs imports a.rs
    let a_path = root.join("a.rs");
    let b_path = root.join("b.rs");

    fs::write(&a_path, "mod b;\nuse b::fn_b;\npub fn fn_a() { fn_b(); }\n").unwrap();
    fs::write(&b_path, "mod a;\nuse a::fn_a;\npub fn fn_b() { fn_a(); }\n").unwrap();

    let backend = GraphBackend::open_for_project(root).unwrap();
    backend.full_scan(root_str, None).unwrap();

    // analyze_impact must terminate without cycle explosion or infinite loop
    let impacts = backend.analyze_impact(a_path.to_str().unwrap(), 5);
    let b_impact = impacts
        .iter()
        .find(|e| normalize(&e.file_path).ends_with("/b.rs"));
    assert!(
        b_impact.is_some(),
        "b.rs must be reported as dependent caller of a.rs"
    );

    // Verify no duplicates
    let b_count = impacts
        .iter()
        .filter(|e| normalize(&e.file_path).ends_with("/b.rs"))
        .count();
    assert_eq!(b_count, 1, "Visited node must not be duplicated in results");
}

#[tokio::test]
async fn test_reverse_impact_depth_boundary() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let root_str = root.to_str().unwrap();

    // Chain: a <- b <- c <- d
    let a_path = root.join("a.rs");
    let b_path = root.join("b.rs");
    let c_path = root.join("c.rs");
    let d_path = root.join("d.rs");

    fs::write(&a_path, "pub fn a() {}\n").unwrap();
    fs::write(&b_path, "mod a;\nuse a::a;\npub fn b() { a(); }\n").unwrap();
    fs::write(&c_path, "mod b;\nuse b::b;\npub fn c() { b(); }\n").unwrap();
    fs::write(&d_path, "mod c;\nuse c::c;\npub fn d() { c(); }\n").unwrap();

    let backend = GraphBackend::open_for_project(root).unwrap();
    backend.full_scan(root_str, None).unwrap();

    // Depth 1: only direct caller b.rs
    let impacts_d1 = backend.analyze_impact(a_path.to_str().unwrap(), 1);
    assert_eq!(impacts_d1.len(), 1);
    assert!(normalize(&impacts_d1[0].file_path).ends_with("/b.rs"));
    assert_eq!(impacts_d1[0].depth, 1);

    // Depth 2: b.rs (depth 1) and c.rs (depth 2), but not d.rs
    let impacts_d2 = backend.analyze_impact(a_path.to_str().unwrap(), 2);
    assert_eq!(impacts_d2.len(), 2);
    assert!(impacts_d2
        .iter()
        .any(|e| normalize(&e.file_path).ends_with("/b.rs") && e.depth == 1));
    assert!(impacts_d2
        .iter()
        .any(|e| normalize(&e.file_path).ends_with("/c.rs") && e.depth == 2));
    assert!(!impacts_d2
        .iter()
        .any(|e| normalize(&e.file_path).ends_with("/d.rs")));

    // Depth 3: b.rs, c.rs, d.rs
    let impacts_d3 = backend.analyze_impact(a_path.to_str().unwrap(), 3);
    assert_eq!(impacts_d3.len(), 3);
    assert!(impacts_d3
        .iter()
        .any(|e| normalize(&e.file_path).ends_with("/d.rs") && e.depth == 3));
}

#[tokio::test]
async fn test_project_inside_gemini_and_scratch_parent_path_indexed() {
    let base_temp = tempdir().unwrap();
    // Path containing .gemini and scratch in parent directory structure
    let project_root = base_temp
        .path()
        .join(".gemini")
        .join("scratch")
        .join("my_service");
    fs::create_dir_all(&project_root).unwrap();

    let src_dir = project_root.join("src");
    fs::create_dir_all(&src_dir).unwrap();

    let main_rs = src_dir.join("main.rs");
    let utils_rs = src_dir.join("utils.rs");

    fs::write(&utils_rs, "pub fn helper() -> i32 { 42 }\n").unwrap();
    fs::write(
        &main_rs,
        "mod utils;\nuse utils::helper;\nfn main() { helper(); }\n",
    )
    .unwrap();

    // 1. Verify path helper with project root
    assert!(
        !is_noise_path_with_root(&main_rs, Some(&project_root)),
        "main.rs inside .gemini/scratch/my_service must NOT be filtered as noise when project_root is provided"
    );
    assert_eq!(
        check_noise_or_huge_file_with_root(
            &main_rs,
            DEFAULT_MAX_DELTA_FILE_BYTES,
            Some(&project_root)
        ),
        NoiseFilterDecision::Process,
        "main.rs must evaluate to Process decision"
    );

    // 2. Full scan verification
    let backend = GraphBackend::open_for_project(&project_root).unwrap();
    backend
        .full_scan(project_root.to_str().unwrap(), None)
        .unwrap();

    assert!(
        !backend.is_empty(),
        "Backend graph must not be empty for project under .gemini/scratch"
    );

    let files = backend.list_all_files().unwrap();
    assert!(
        files.iter().any(|f| normalize(f).ends_with("/src/main.rs")),
        "main.rs must be present in indexed files: {:?}",
        files
    );
    assert!(
        files
            .iter()
            .any(|f| normalize(f).ends_with("/src/utils.rs")),
        "utils.rs must be present in indexed files: {:?}",
        files
    );

    // 3. Live Delta indexing verification
    let delta_res = backend.index_file_delta(&main_rs, &project_root).unwrap();
    match delta_res {
        DeltaIndexResult::Indexed { .. } | DeltaIndexResult::Unchanged => {}
        other => panic!("Expected Indexed or Unchanged, got {:?}", other),
    }

    // 4. Impact query verification inside this project
    let impacts = backend.analyze_impact(utils_rs.to_str().unwrap(), 2);
    assert!(
        impacts
            .iter()
            .any(|e| normalize(&e.file_path).ends_with("/src/main.rs")),
        "main.rs must be reported as impacted caller of utils.rs"
    );
}

#[tokio::test]
async fn test_subdirectories_inside_workspace_still_filtered_as_noise() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let root_str = root.to_str().unwrap();

    let node_modules_dir = root.join("node_modules").join("lib");
    let target_dir = root.join("target").join("debug");
    let git_dir = root.join(".git").join("objects");
    let src_dir = root.join("src");

    fs::create_dir_all(&node_modules_dir).unwrap();
    fs::create_dir_all(&target_dir).unwrap();
    fs::create_dir_all(&git_dir).unwrap();
    fs::create_dir_all(&src_dir).unwrap();

    let node_file = node_modules_dir.join("index.js");
    let target_file = target_dir.join("build.rs");
    let git_file = git_dir.join("pack.idx");
    let lock_file = root.join("Cargo.lock");
    let app_file = src_dir.join("app.rs");

    fs::write(&node_file, "console.log('vendor');").unwrap();
    fs::write(&target_file, "fn main() {}").unwrap();
    fs::write(&git_file, "binary").unwrap();
    fs::write(&lock_file, "# lockfile\n[[package]]").unwrap();
    fs::write(&app_file, "pub fn run_app() {}\n").unwrap();

    // Check decisions with project root provided
    assert_eq!(
        check_noise_or_huge_file_with_root(&node_file, DEFAULT_MAX_DELTA_FILE_BYTES, Some(root)),
        NoiseFilterDecision::NoiseDir
    );
    assert_eq!(
        check_noise_or_huge_file_with_root(&target_file, DEFAULT_MAX_DELTA_FILE_BYTES, Some(root)),
        NoiseFilterDecision::NoiseDir
    );
    assert_eq!(
        check_noise_or_huge_file_with_root(&lock_file, DEFAULT_MAX_DELTA_FILE_BYTES, Some(root)),
        NoiseFilterDecision::LockFile
    );
    assert_eq!(
        check_noise_or_huge_file_with_root(&app_file, DEFAULT_MAX_DELTA_FILE_BYTES, Some(root)),
        NoiseFilterDecision::Process
    );

    // Full scan must index app.rs and exclude noise files
    let backend = GraphBackend::open_for_project(root).unwrap();
    backend.full_scan(root_str, None).unwrap();

    let files = backend.list_all_files().unwrap();
    assert!(
        files.iter().any(|f| normalize(f).ends_with("/src/app.rs")),
        "app.rs must be indexed"
    );
    assert!(
        !files.iter().any(|f| normalize(f).contains("node_modules")),
        "node_modules must NOT be indexed"
    );
    assert!(
        !files.iter().any(|f| normalize(f).contains("target")),
        "target directory must NOT be indexed"
    );
    assert!(
        !files.iter().any(|f| normalize(f).ends_with("Cargo.lock")),
        "Cargo.lock must NOT be indexed"
    );
}
