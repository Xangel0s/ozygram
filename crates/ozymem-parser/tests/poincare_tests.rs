// Copyright (c) 2026 Ozygram Project. All rights reserved.
// Unit and property test suite for 16D Poincare Hyperbolic Manifold.
// Zero-Emoji Standard compliant.

use ozymem_parser::poincare::{
    embed_ast_node_poincare, embed_path_hierarchy, PoincarePoint, EPSILON_BOUNDARY, POINCARE_DIM,
};

#[test]
fn test_origin_properties() {
    let origin = PoincarePoint::origin();
    assert_eq!(origin.norm_squared(), 0.0);
    assert_eq!(origin.norm(), 0.0);

    let d_self = origin.poincare_distance(&origin);
    assert!(d_self.abs() < 1e-6, "Distance from origin to itself must be zero");
}

#[test]
fn test_metric_axioms() {
    let mut coords_a = [0.0f32; POINCARE_DIM];
    coords_a[0] = 0.2;
    coords_a[1] = -0.3;
    coords_a[2] = 0.1;

    let mut coords_b = [0.0f32; POINCARE_DIM];
    coords_b[0] = -0.1;
    coords_b[1] = 0.4;
    coords_b[3] = 0.25;

    let mut coords_c = [0.0f32; POINCARE_DIM];
    coords_c[2] = 0.5;
    coords_c[4] = -0.2;

    let pt_a = PoincarePoint::from_coords(coords_a);
    let pt_b = PoincarePoint::from_coords(coords_b);
    let pt_c = PoincarePoint::from_coords(coords_c);

    // 1. Non-negativity
    assert!(pt_a.poincare_distance(&pt_b) >= 0.0);

    // 2. Identity of indiscernibles
    assert!(pt_a.poincare_distance(&pt_a) < 1e-6);

    // 3. Symmetry
    let d_ab = pt_a.poincare_distance(&pt_b);
    let d_ba = pt_b.poincare_distance(&pt_a);
    assert!(
        (d_ab - d_ba).abs() < 1e-5,
        "Symmetry violated: d(a,b) = {}, d(b,a) = {}",
        d_ab,
        d_ba
    );

    // 4. Triangle inequality: d(a, c) <= d(a, b) + d(b, c) + epsilon
    let d_ac = pt_a.poincare_distance(&pt_c);
    let d_bc = pt_b.poincare_distance(&pt_c);
    assert!(
        d_ac <= d_ab + d_bc + 1e-4,
        "Triangle inequality violated: d(a,c) = {} > d(a,b) + d(b,c) = {}",
        d_ac,
        d_ab + d_bc
    );
}

#[test]
fn test_boundary_clamping_and_numerical_stability() {
    // Attempt to construct a point outside the unit ball (norm = 2.0)
    let mut runaway_coords = [0.5f32; POINCARE_DIM];
    runaway_coords[0] = 1.5;

    let clamped = PoincarePoint::from_coords(runaway_coords);
    assert!(
        clamped.norm() <= 1.0 - EPSILON_BOUNDARY,
        "Clamped norm must remain strictly inside unit ball: got {}",
        clamped.norm()
    );

    // Extreme near-boundary point
    let mut near_boundary_coords = [0.0f32; POINCARE_DIM];
    near_boundary_coords[0] = 0.999999;
    let near_boundary = PoincarePoint::from_coords(near_boundary_coords);

    let origin = PoincarePoint::origin();
    let d_extreme = origin.poincare_distance(&near_boundary);
    assert!(!d_extreme.is_nan() && !d_extreme.is_infinite());
    assert!(d_extreme > 5.0, "Near boundary distance must be large");
}

#[test]
fn test_mobius_addition_identity_and_containment() {
    let mut coords = [0.0f32; POINCARE_DIM];
    coords[0] = 0.4;
    coords[1] = -0.3;
    let u = PoincarePoint::from_coords(coords);
    let origin = PoincarePoint::origin();

    // u (+) 0 = u
    let res = u.mobius_add(&origin);
    assert!((res.poincare_distance(&u)).abs() < 1e-5);

    // Adding two interior points remains strictly inside ball
    let mut coords_v = [0.0f32; POINCARE_DIM];
    coords_v[0] = 0.5;
    coords_v[2] = 0.4;
    let v = PoincarePoint::from_coords(coords_v);

    let uv = u.mobius_add(&v);
    assert!(uv.norm() < 1.0);
}

#[test]
fn test_path_hierarchy_preserves_architectural_locality() {
    let path_utils_a = "frontend/src/utils/tableDataCache.ts";
    let path_utils_b = "frontend/src/utils/formatting.ts";
    let path_backend = "backend/app/routers/auth.py";

    let pt_a = embed_path_hierarchy(path_utils_a);
    let pt_b = embed_path_hierarchy(path_utils_b);
    let pt_backend = embed_path_hierarchy(path_backend);

    let d_same_submodule = pt_a.poincare_distance(&pt_b);
    let d_cross_stack = pt_a.poincare_distance(&pt_backend);

    assert!(
        d_same_submodule < d_cross_stack,
        "Submodule distance ({}) must be strictly smaller than cross-stack distance ({})",
        d_same_submodule,
        d_cross_stack
    );

    // High coupling score for co-located utilities
    let coupling_same = pt_a.hyperbolic_coupling_score(&pt_b, 1.0);
    let coupling_cross = pt_a.hyperbolic_coupling_score(&pt_backend, 1.0);
    assert!(
        coupling_same > coupling_cross,
        "Coupling score must be higher for co-located utilities: {} vs {}",
        coupling_same,
        coupling_cross
    );
}

#[test]
fn test_ast_node_hierarchy_embedding() {
    let file = "frontend/src/components/TableEditor.tsx";
    let node_hook = embed_ast_node_poincare(file, "useTableSelection", 2, 0, 3);
    let node_subcomp = embed_ast_node_poincare(file, "TableHeaderCell", 2, 1, 3);

    let file_point = embed_path_hierarchy(file);
    let d_hook_to_file = node_hook.poincare_distance(&file_point);
    let d_subcomp_to_file = node_subcomp.poincare_distance(&file_point);

    assert!(d_hook_to_file < 3.0, "AST node must remain close to its host file");
    assert!(d_subcomp_to_file < 3.0, "AST node must remain close to its host file");

    let d_siblings = node_hook.poincare_distance(&node_subcomp);
    assert!(d_siblings > 0.05, "Siblings must diverge angularly in hyperbolic space");
}
