// Copyright (c) 2026 Ozygram Project. All rights reserved.
// Mathematical substrate for 16-dimensional Poincare Ball embeddings
// Mapping hierarchical AST and directory graphs into non-Euclidean hyperbolic space.
// Zero-Emoji Standard compliant.

use serde::{Deserialize, Serialize};
use std::hash::Hasher;

pub const POINCARE_DIM: usize = 16;
pub const EPSILON_BOUNDARY: f32 = 1e-6;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PoincarePoint {
    pub coords: [f32; POINCARE_DIM],
}

impl Default for PoincarePoint {
    fn default() -> Self {
        Self::origin()
    }
}

impl PoincarePoint {
    /// Constructs a point at the origin of the Poincare ball (0, 0, ..., 0).
    pub fn origin() -> Self {
        Self {
            coords: [0.0; POINCARE_DIM],
        }
    }

    /// Constructs a point from a 16-dimensional coordinate array.
    /// Clamps the Euclidean norm strictly within the open unit ball: ||x|| <= 1.0 - EPSILON_BOUNDARY.
    pub fn from_coords(mut raw_coords: [f32; POINCARE_DIM]) -> Self {
        let norm_sq = Self::compute_norm_sq(&raw_coords);
        let max_norm = 1.0 - EPSILON_BOUNDARY;
        let max_norm_sq = max_norm * max_norm;

        if norm_sq >= max_norm_sq {
            let norm = norm_sq.sqrt().max(1e-12);
            let scale = max_norm / norm;
            for val in &mut raw_coords {
                *val *= scale;
            }
        }

        Self { coords: raw_coords }
    }

    /// Computes squared Euclidean norm ||u||^2.
    #[inline]
    pub fn norm_squared(&self) -> f32 {
        Self::compute_norm_sq(&self.coords)
    }

    /// Computes Euclidean norm ||u||.
    #[inline]
    pub fn norm(&self) -> f32 {
        self.norm_squared().sqrt()
    }

    /// Computes the exact hyperbolic distance in the Poincare Ball model B^16:
    /// d_B(u, v) = arcosh(1 + 2 * ||u - v||^2 / ((1 - ||u||^2) * (1 - ||v||^2)))
    pub fn poincare_distance(&self, other: &PoincarePoint) -> f32 {
        let mut diff_sq = 0.0f32;
        let mut norm_u_sq = 0.0f32;
        let mut norm_v_sq = 0.0f32;

        for i in 0..POINCARE_DIM {
            let u = self.coords[i];
            let v = other.coords[i];
            let diff = u - v;
            diff_sq += diff * diff;
            norm_u_sq += u * u;
            norm_v_sq += v * v;
        }

        // Clamp norms to prevent division by zero or boundary overflow
        let max_norm_sq = (1.0 - EPSILON_BOUNDARY) * (1.0 - EPSILON_BOUNDARY);
        let denom_u = (1.0 - norm_u_sq.min(max_norm_sq)).max(EPSILON_BOUNDARY);
        let denom_v = (1.0 - norm_v_sq.min(max_norm_sq)).max(EPSILON_BOUNDARY);

        let arg = 1.0 + 2.0 * (diff_sq / (denom_u * denom_v));
        // Numerically stable arcosh(x) = ln(x + sqrt(x^2 - 1.0)) for x >= 1.0
        let clamped_arg = arg.max(1.0);
        let delta_sq = (clamped_arg * clamped_arg - 1.0).max(0.0);
        (clamped_arg + delta_sq.sqrt()).ln().max(0.0)
    }

    /// Computes hyperbolic coupling score: w(d) = exp(-d / tau)
    /// Higher values (close to 1.0) indicate tight architectural coupling.
    pub fn hyperbolic_coupling_score(&self, other: &PoincarePoint, tau: f32) -> f32 {
        let d = self.poincare_distance(other);
        let temperature = if tau <= 0.0 { 1.0 } else { tau };
        (-d / temperature).exp()
    }

    /// Mobius addition in the Poincare ball: u (+) v
    /// Represents the hyperbolic translation of v by u.
    pub fn mobius_add(&self, other: &PoincarePoint) -> PoincarePoint {
        let norm_u_sq = self.norm_squared();
        let norm_v_sq = other.norm_squared();

        let mut inner_uv = 0.0f32;
        for i in 0..POINCARE_DIM {
            inner_uv += self.coords[i] * other.coords[i];
        }

        let denom = 1.0 + 2.0 * inner_uv + norm_u_sq * norm_v_sq;
        let denom = if denom.abs() < 1e-8 { 1e-8 } else { denom };

        let coeff_u = 1.0 + 2.0 * inner_uv + norm_v_sq;
        let coeff_v = 1.0 - norm_u_sq;

        let mut res = [0.0f32; POINCARE_DIM];
        for i in 0..POINCARE_DIM {
            res[i] = (coeff_u * self.coords[i] + coeff_v * other.coords[i]) / denom;
        }

        PoincarePoint::from_coords(res)
    }

    #[inline]
    fn compute_norm_sq(arr: &[f32; POINCARE_DIM]) -> f32 {
        let mut sum = 0.0f32;
        for &val in arr {
            sum += val * val;
        }
        sum
    }
}

/// Embeds a hierarchical file path into the 16D Poincare Ball.
/// Root directories reside near the origin; deeper nested modules project
/// radially towards the boundary, capturing exponential subtree expansion.
pub fn embed_path_hierarchy(file_path: &str) -> PoincarePoint {
    let clean_path = file_path.replace('\\', "/");
    let segments: Vec<&str> = clean_path
        .split('/')
        .filter(|s| !s.is_empty() && *s != ".")
        .collect();

    if segments.is_empty() {
        return PoincarePoint::origin();
    }

    let mut current_point = PoincarePoint::origin();

    for (depth, segment) in segments.iter().enumerate() {
        // Deterministic pseudo-orthogonal subspace hashing for segment
        let mut hasher = twox_hash::XxHash64::with_seed(0);
hasher.write(segment.as_bytes());
let hash = hasher.finish();
        let dim_primary = (hash as usize) % POINCARE_DIM;
        let dim_secondary = ((hash >> 16) as usize) % POINCARE_DIM;

        // Radial scale grows with depth: tanh(depth * alpha)
        let depth_factor = ((depth + 1) as f32 * 0.35).tanh() * 0.45;
        let sign_primary = if (hash & 1) == 0 { 1.0 } else { -1.0 };
        let sign_secondary = if (hash & 2) == 0 { 1.0 } else { -1.0 };

        let mut step_coords = [0.0f32; POINCARE_DIM];
        step_coords[dim_primary] += sign_primary * depth_factor * 0.7071;
        step_coords[dim_secondary] += sign_secondary * depth_factor * 0.7071;

        let step_point = PoincarePoint::from_coords(step_coords);
        current_point = current_point.mobius_add(&step_point);
    }

    current_point
}

/// Embeds an AST node hierarchy into the 16D Poincare Ball.
/// Preserves parent-child containment and sibling divergence.
pub fn embed_ast_node_poincare(
    file_path: &str,
    symbol_name: &str,
    depth_in_ast: usize,
    sibling_index: usize,
    sibling_count: usize,
) -> PoincarePoint {
    let path_point = embed_path_hierarchy(file_path);

    let mut hasher = twox_hash::XxHash64::with_seed(0);
hasher.write(symbol_name.as_bytes());
let symbol_hash = hasher.finish();
    let dim_target = (symbol_hash as usize) % POINCARE_DIM;
    let dim_orthogonal = (dim_target + 1) % POINCARE_DIM;

    // Sibling angle distribution
    let angle = if sibling_count > 1 {
        (sibling_index as f32 / sibling_count as f32) * std::f32::consts::TAU
    } else {
        0.0
    };

    let local_r = ((depth_in_ast + 1) as f32 * 0.25).tanh() * 0.35;
    let mut ast_step = [0.0f32; POINCARE_DIM];
    ast_step[dim_target] = local_r * angle.cos();
    ast_step[dim_orthogonal] = local_r * angle.sin();

    let ast_point = PoincarePoint::from_coords(ast_step);
    path_point.mobius_add(&ast_point)
}
