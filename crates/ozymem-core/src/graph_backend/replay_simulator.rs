use anyhow::{Context, Result};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use crate::graph_backend::exploration::ExplorationNode;
use crate::graph_backend::GraphBackend;

/// Vector hiperparamétrico de búsqueda MCTS contrafáctico (Google Dream-RSI en Rust)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExplorationPolicy {
    pub name: String,
    pub c_puct: f64,
    pub prune_cost_threshold: i64,
    pub max_depth: i64,
    pub amygdala_penalty_weight: f64,
    pub branching_factor: usize,
    pub early_stop_reward_threshold: f64,
}

impl Default for ExplorationPolicy {
    fn default() -> Self {
        Self {
            name: "balanced_mcts_default".to_string(),
            c_puct: 1.414,
            prune_cost_threshold: 4000,
            max_depth: 10,
            amygdala_penalty_weight: 0.5,
            branching_factor: 3,
            early_stop_reward_threshold: 0.85,
        }
    }
}

impl ExplorationPolicy {
    /// Política ultra-rápida de reflejo para hotfixes y tareas directas
    pub fn fast_reflex() -> Self {
        Self {
            name: "fast_reflex".to_string(),
            c_puct: 0.8,
            prune_cost_threshold: 2000,
            max_depth: 5,
            amygdala_penalty_weight: 0.8,
            branching_factor: 2,
            early_stop_reward_threshold: 0.70,
        }
    }

    /// Política deliberativa profunda para refactorizaciones arquitectónicas
    pub fn deep_deliberative() -> Self {
        Self {
            name: "deep_deliberative".to_string(),
            c_puct: 2.0,
            prune_cost_threshold: 8000,
            max_depth: 15,
            amygdala_penalty_weight: 0.3,
            branching_factor: 4,
            early_stop_reward_threshold: 0.90,
        }
    }

    /// Política conservadora estricta para contratos de validación y prevención de regresiones
    pub fn conservative_validation() -> Self {
        Self {
            name: "conservative_validation".to_string(),
            c_puct: 1.2,
            prune_cost_threshold: 3000,
            max_depth: 8,
            amygdala_penalty_weight: 0.9,
            branching_factor: 2,
            early_stop_reward_threshold: 0.95,
        }
    }

    /// Conjunto estándar de políticas candidatas para barrido contrafáctico (Dream Sweeper)
    pub fn standard_candidates() -> Vec<Self> {
        vec![
            Self::default(),
            Self::fast_reflex(),
            Self::deep_deliberative(),
            Self::conservative_validation(),
            Self {
                name: "aggressive_pruning".to_string(),
                c_puct: 1.0,
                prune_cost_threshold: 1500,
                max_depth: 6,
                amygdala_penalty_weight: 0.7,
                branching_factor: 2,
                early_stop_reward_threshold: 0.75,
            },
            Self {
                name: "wide_exploration".to_string(),
                c_puct: 2.4,
                prune_cost_threshold: 6000,
                max_depth: 12,
                amygdala_penalty_weight: 0.2,
                branching_factor: 5,
                early_stop_reward_threshold: 0.88,
            },
        ]
    }
}

/// Métricas cuantitativas de evaluación contrafáctica de una política
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PolicyMetrics {
    pub steps_to_solution: usize,
    pub total_tokens: i64,
    pub total_latency_ms: i64,
    pub solution_found: bool,
    pub pruned_branches_count: usize,
    pub nodes_explored: usize,
    pub counterfactual_reward: f64,
}

/// Resultado contrafáctico comparativo entre una política base y una política candidata
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CounterfactualReplayResult {
    pub trajectory_id: String,
    pub baseline_policy: ExplorationPolicy,
    pub candidate_policy: ExplorationPolicy,
    pub baseline_metrics: PolicyMetrics,
    pub candidate_metrics: PolicyMetrics,
    pub efficiency_gain_pct: f64,
    pub is_optimal: bool,
    pub recommendation: String,
}

/// Motor de simulación de replay contrafáctico sin costo de inferencia (Google Dream-RSI en Rust)
pub struct ReplaySimulator;

impl ReplaySimulator {
    /// Reconstruye el árbol de nodos y evalúa la trayectoria que habría seguido una política candidata
    pub fn simulate_policy(nodes: &[ExplorationNode], policy: &ExplorationPolicy) -> PolicyMetrics {
        if nodes.is_empty() {
            return PolicyMetrics {
                steps_to_solution: 0,
                total_tokens: 0,
                total_latency_ms: 0,
                solution_found: false,
                pruned_branches_count: 0,
                nodes_explored: 0,
                counterfactual_reward: 0.0,
            };
        }

        // Construir mapa de hijos por parent_id
        let mut children_map: HashMap<Option<String>, Vec<&ExplorationNode>> = HashMap::new();
        let mut node_by_id: HashMap<String, &ExplorationNode> = HashMap::new();

        for node in nodes {
            node_by_id.insert(node.id.clone(), node);
            children_map
                .entry(node.parent_id.clone())
                .or_default()
                .push(node);
        }

        // Identificar nodos raíz (parent_id es None o apunta a un nodo inexistente)
        let root_nodes: Vec<&ExplorationNode> = nodes
            .iter()
            .filter(|n| {
                n.parent_id.is_none()
                    || !node_by_id.contains_key(n.parent_id.as_ref().unwrap())
            })
            .collect();

        if root_nodes.is_empty() {
            return PolicyMetrics {
                steps_to_solution: 0,
                total_tokens: 0,
                total_latency_ms: 0,
                solution_found: false,
                pruned_branches_count: 0,
                nodes_explored: 0,
                counterfactual_reward: 0.0,
            };
        }

        let mut explored_ids: HashSet<String> = HashSet::new();
        let mut steps_to_solution = 0;
        let mut total_tokens: i64 = 0;
        let mut total_latency_ms: i64 = 0;
        let mut solution_found = false;
        let mut pruned_count = 0;
        let mut cumulative_reward = 0.0;

        // Cola de prioridad simulada (UCT score)
        // Estructura: (node_ref, parent_visits, depth)
        let mut frontier: Vec<(&ExplorationNode, i64, i64)> = root_nodes
            .into_iter()
            .map(|r| (r, 1, 0))
            .collect();

        while !frontier.is_empty() {
            // Ordenar frontera según función PUCT de la política candidata
            frontier.sort_by(|a, b| {
                let score_a = Self::compute_uct_score(a.0, a.1, a.2, policy);
                let score_b = Self::compute_uct_score(b.0, b.1, b.2, policy);
                score_b.partial_cmp(&score_a).unwrap_or(std::cmp::Ordering::Equal)
            });

            let (current, _parent_visits, current_depth) = frontier.remove(0);

            if explored_ids.contains(&current.id) {
                continue;
            }

            // Verificar poda por profundidad máxima
            if current_depth > policy.max_depth {
                pruned_count += 1;
                continue;
            }

            // Verificar poda por umbral de costo de tokens y recompensa negativa
            if current.cost_tokens > policy.prune_cost_threshold && current.reward_score < 0.0 {
                pruned_count += 1;
                continue;
            }

            explored_ids.insert(current.id.clone());
            steps_to_solution += 1;
            total_tokens += current.cost_tokens;
            total_latency_ms += current.latency_ms;
            cumulative_reward += current.reward_score;

            // Condición de solución encontrada
            if current.is_solution || current.reward_score >= policy.early_stop_reward_threshold {
                solution_found = true;
                break;
            }

            // Expandir hijos respetando el branching_factor de la política
            if let Some(children) = children_map.get(&Some(current.id.clone())) {
                let mut sorted_children = children.clone();
                let visits = current.visit_count.max(1);
                sorted_children.sort_by(|ca, cb| {
                    let sc_a = Self::compute_uct_score(ca, visits, current_depth + 1, policy);
                    let sc_b = Self::compute_uct_score(cb, visits, current_depth + 1, policy);
                    sc_b.partial_cmp(&sc_a).unwrap_or(std::cmp::Ordering::Equal)
                });

                let allowed_children: Vec<&ExplorationNode> = sorted_children
                    .into_iter()
                    .take(policy.branching_factor)
                    .collect();

                let pruned_in_branch = children.len().saturating_sub(policy.branching_factor);
                pruned_count += pruned_in_branch;

                for child in allowed_children {
                    frontier.push((child, visits, current_depth + 1));
                }
            }
        }

        PolicyMetrics {
            steps_to_solution,
            total_tokens,
            total_latency_ms,
            solution_found,
            pruned_branches_count: pruned_count,
            nodes_explored: explored_ids.len(),
            counterfactual_reward: cumulative_reward,
        }
    }

    /// Calcula la puntuación UCT / PUCT con penalización amigdalina
    fn compute_uct_score(
        node: &ExplorationNode,
        parent_visits: i64,
        depth: i64,
        policy: &ExplorationPolicy,
    ) -> f64 {
        let q = node.value_estimate.max(node.reward_score);
        let n = (node.visit_count as f64).max(1.0);
        let total_n = (parent_visits as f64).max(1.0);

        let exploration = policy.c_puct * (total_n.ln() / n).sqrt();

        // Penalización si el payload o la observación contiene señales de amenaza/error
        let mut penalty = 0.0;
        let lower_pay = node.action_payload.to_lowercase();
        let lower_obs = node.observation.to_lowercase();
        if lower_pay.contains("error") || lower_obs.contains("error") || lower_pay.contains("fail") || lower_obs.contains("fail") {
            penalty += policy.amygdala_penalty_weight;
        }

        // Penalización leve por profundidad excesiva
        let depth_penalty = (depth as f64) * 0.02;

        q + exploration - penalty - depth_penalty
    }

    /// Compara contrafácticamente la política candidata contra la línea base
    pub fn replay_counterfactual(
        trajectory_id: &str,
        nodes: &[ExplorationNode],
        baseline: &ExplorationPolicy,
        candidate: &ExplorationPolicy,
    ) -> CounterfactualReplayResult {
        let baseline_metrics = Self::simulate_policy(nodes, baseline);
        let candidate_metrics = Self::simulate_policy(nodes, candidate);

        let baseline_cost = baseline_metrics.total_tokens as f64 + (baseline_metrics.steps_to_solution as f64 * 500.0);
        let candidate_cost = candidate_metrics.total_tokens as f64 + (candidate_metrics.steps_to_solution as f64 * 500.0);

        let efficiency_gain_pct = if baseline_cost > 0.0 {
            ((baseline_cost - candidate_cost) / baseline_cost) * 100.0
        } else {
            0.0
        };

        let is_optimal = candidate_metrics.solution_found
            && (!baseline_metrics.solution_found || efficiency_gain_pct >= 10.0);

        let recommendation = if candidate_metrics.solution_found && !baseline_metrics.solution_found {
            format!(
                "[RECOMMENDATION: ADOPT_POLICY] La política '{}' resolvió la tarea donde la base no pudo.",
                candidate.name
            )
        } else if efficiency_gain_pct >= 15.0 {
            format!(
                "[RECOMMENDATION: ADOPT_POLICY] La política '{}' ahorra {:.1}% de cómputo y tokens respecto a '{}'.",
                candidate.name, efficiency_gain_pct, baseline.name
            )
        } else if efficiency_gain_pct < -10.0 {
            format!(
                "[RECOMMENDATION: REJECT_POLICY] La política '{}' degrada la eficiencia en {:.1}%.",
                candidate.name, efficiency_gain_pct.abs()
            )
        } else {
            format!(
                "[RECOMMENDATION: NEUTRAL] Desempeño comparable entre '{}' y '{}' ({:+.1}%).",
                candidate.name, baseline.name, efficiency_gain_pct
            )
        };

        CounterfactualReplayResult {
            trajectory_id: trajectory_id.to_string(),
            baseline_policy: baseline.clone(),
            candidate_policy: candidate.clone(),
            baseline_metrics,
            candidate_metrics,
            efficiency_gain_pct,
            is_optimal,
            recommendation,
        }
    }

    /// Realiza un barrido completo de políticas candidatas sobre una trayectoria
    pub fn sweep_candidate_policies(
        trajectory_id: &str,
        nodes: &[ExplorationNode],
        candidates: &[ExplorationPolicy],
    ) -> Vec<CounterfactualReplayResult> {
        let baseline = ExplorationPolicy::default();
        candidates
            .iter()
            .map(|cand| Self::replay_counterfactual(trajectory_id, nodes, &baseline, cand))
            .collect()
    }

    /// Encuentra la política óptima con mayor ganancia de eficiencia que conserve la solución
    pub fn find_optimal_policy(
        trajectory_id: &str,
        nodes: &[ExplorationNode],
        candidates: &[ExplorationPolicy],
    ) -> Option<CounterfactualReplayResult> {
        let results = Self::sweep_candidate_policies(trajectory_id, nodes, candidates);
        results
            .into_iter()
            .filter(|r| r.candidate_metrics.solution_found)
            .max_by(|a, b| {
                a.efficiency_gain_pct
                    .partial_cmp(&b.efficiency_gain_pct)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    }
}

impl GraphBackend {
    /// Persiste el resultado de una simulación contrafáctica en SQLite WAL
    pub fn save_counterfactual_simulation(&self, result: &CounterfactualReplayResult) -> Result<()> {
        let id = format!("sim_{}_{}", result.trajectory_id, result.candidate_policy.name);
        let inner = self.inner.lock().unwrap();

        let baseline_json = serde_json::to_string(&result.baseline_policy)?;
        let candidate_json = serde_json::to_string(&result.candidate_policy)?;
        let base_metrics_json = serde_json::to_string(&result.baseline_metrics)?;
        let cand_metrics_json = serde_json::to_string(&result.candidate_metrics)?;

        inner.sqlite.execute(
            "INSERT OR REPLACE INTO dream_counterfactual_simulations (
                id, trajectory_id, baseline_policy, candidate_policy,
                baseline_metrics, candidate_metrics, efficiency_gain_pct,
                is_optimal, recommendation, simulated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, datetime('now'))",
            params![
                id,
                result.trajectory_id,
                baseline_json,
                candidate_json,
                base_metrics_json,
                cand_metrics_json,
                result.efficiency_gain_pct,
                if result.is_optimal { 1 } else { 0 },
                result.recommendation,
            ],
        ).context("Error al guardar simulación contrafáctica en SQLite")?;

        Ok(())
    }

    /// Obtiene las simulaciones contrafácticas persistidas para una trayectoria
    pub fn get_counterfactual_simulations(
        &self,
        trajectory_id: &str,
    ) -> Result<Vec<CounterfactualReplayResult>> {
        let inner = self.inner.lock().unwrap();
        let mut stmt = inner.sqlite.prepare(
            "SELECT trajectory_id, baseline_policy, candidate_policy,
                    baseline_metrics, candidate_metrics, efficiency_gain_pct,
                    is_optimal, recommendation
             FROM dream_counterfactual_simulations
             WHERE trajectory_id = ?1
             ORDER BY efficiency_gain_pct DESC",
        )?;

        let rows = stmt.query_map(params![trajectory_id], |row| {
            let traj_id: String = row.get(0)?;
            let base_pol_str: String = row.get(1)?;
            let cand_pol_str: String = row.get(2)?;
            let base_met_str: String = row.get(3)?;
            let cand_met_str: String = row.get(4)?;
            let eff_gain: f64 = row.get(5)?;
            let is_opt: i64 = row.get(6)?;
            let rec: String = row.get(7)?;

            let baseline_policy: ExplorationPolicy =
                serde_json::from_str(&base_pol_str).unwrap_or_default();
            let candidate_policy: ExplorationPolicy =
                serde_json::from_str(&cand_pol_str).unwrap_or_default();
            let baseline_metrics: PolicyMetrics = serde_json::from_str(&base_met_str).unwrap_or(
                PolicyMetrics {
                    steps_to_solution: 0,
                    total_tokens: 0,
                    total_latency_ms: 0,
                    solution_found: false,
                    pruned_branches_count: 0,
                    nodes_explored: 0,
                    counterfactual_reward: 0.0,
                },
            );
            let candidate_metrics: PolicyMetrics = serde_json::from_str(&cand_met_str).unwrap_or(
                PolicyMetrics {
                    steps_to_solution: 0,
                    total_tokens: 0,
                    total_latency_ms: 0,
                    solution_found: false,
                    pruned_branches_count: 0,
                    nodes_explored: 0,
                    counterfactual_reward: 0.0,
                },
            );

            Ok(CounterfactualReplayResult {
                trajectory_id: traj_id,
                baseline_policy,
                candidate_policy,
                baseline_metrics,
                candidate_metrics,
                efficiency_gain_pct: eff_gain,
                is_optimal: is_opt == 1,
                recommendation: rec,
            })
        })?;

        let mut results = Vec::new();
        for r in rows {
            if let Ok(res) = r {
                results.push(res);
            }
        }
        Ok(results)
    }

    /// Obtiene la última política óptima descubierta para el proyecto
    pub fn get_latest_optimal_policy(
        &self,
        _project_path: Option<&str>,
    ) -> Result<Option<ExplorationPolicy>> {
        let inner = self.inner.lock().unwrap();
        let mut stmt = inner.sqlite.prepare(
            "SELECT candidate_policy
             FROM dream_counterfactual_simulations
             WHERE is_optimal = 1
             ORDER BY simulated_at DESC
             LIMIT 1",
        )?;

        let mut rows = stmt.query([])?;
        if let Some(row) = rows.next()? {
            let cand_pol_str: String = row.get(0)?;
            if let Ok(policy) = serde_json::from_str::<ExplorationPolicy>(&cand_pol_str) {
                return Ok(Some(policy));
            }
        }
        Ok(None)
    }

    /// Ejecuta el barrido contrafáctico offline sobre las trayectorias de las últimas 24 horas
    pub fn run_offline_counterfactual_dream_sweep(
        &self,
        project_path: Option<&str>,
    ) -> Result<Vec<CounterfactualReplayResult>> {
        let candidates = ExplorationPolicy::standard_candidates();
        let trajectories = self.list_trajectories(project_path, 10)?;
        let mut all_results = Vec::new();

        for traj in trajectories {
            let nodes = self.get_exploration_nodes(&traj.id)?;
            if nodes.len() < 2 {
                continue;
            }

            let sweep = ReplaySimulator::sweep_candidate_policies(&traj.id, &nodes, &candidates);
            for res in &sweep {
                let _ = self.save_counterfactual_simulation(res);
            }
            all_results.extend(sweep);
        }

        Ok(all_results)
    }
}
