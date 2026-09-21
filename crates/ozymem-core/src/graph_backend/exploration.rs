use anyhow::{Context, Result};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::graph_backend::GraphBackend;

/// Límite de seguridad para evitar inflar SQLite con trazas gigantescas (32 KB)
pub const MAX_OBSERVATION_BYTES: usize = 32 * 1024;

/// Generador thread-safe de IDs monotónicos únicos
fn generate_id(prefix: &str) -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{}_{:x}_{:x}", prefix, now, count)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrajectoryHeader {
    pub id: String,
    pub project_path: String,
    pub task_description: String,
    pub policy_version: String,
    pub status: String,
    pub total_steps: i64,
    pub cumulative_reward: f64,
    pub created_at: String,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExplorationNode {
    pub id: String,
    pub trajectory_id: String,
    pub parent_id: Option<String>,
    pub depth: i64,
    pub action_type: String,
    pub action_payload: String,
    pub observation: String,
    pub cost_tokens: i64,
    pub latency_ms: i64,
    pub reward_score: f64,
    pub visit_count: i64,
    pub value_estimate: f64,
    pub is_solution: bool,
    pub is_pruned: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExplorationTreeNode {
    pub node: ExplorationNode,
    pub uct_score: f64,
    pub children: Vec<ExplorationTreeNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordNodeParams {
    pub trajectory_id: String,
    pub parent_id: Option<String>,
    pub action_type: String,
    pub action_payload: String,
    pub observation: String,
    pub cost_tokens: Option<i64>,
    pub latency_ms: Option<i64>,
    pub reward_score: Option<f64>,
    pub is_solution: Option<bool>,
    pub is_pruned: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordBatchParams {
    pub trajectory_id: String,
    pub steps: Vec<RecordNodeParams>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrajectoryDiagnosis {
    pub trajectory_id: String,
    pub total_steps: usize,
    pub total_tokens: i64,
    pub total_latency_ms: i64,
    pub bottlenecks: Vec<String>,
    pub pruning_opportunities: Vec<String>,
    pub suggested_ucb1_c: f64,
    pub token_efficiency_percent: f64,
    pub has_solution: bool,
    pub max_depth: i64,
}

/// Detecta señales objetivas de fallo en la observación o payload para calibrar la recompensa
fn detect_failure_signals(observation: &str, payload: &str) -> bool {
    let lower_obs = observation.to_lowercase();
    let lower_pay = payload.to_lowercase();

    let failure_patterns = [
        "exit code: 1",
        "exit code 1",
        "exit code: 2",
        "exit code 2",
        "command failed",
        "build failed",
        "test failed",
        "tests failed",
        "failures: ",
        "syntaxerror",
        "typeerror",
        "nameerror",
        "operationalerror",
        "panic at",
        "fatal error",
        "timed out",
        "timeout error",
        "access is denied",
        "no such file or directory",
    ];

    for pattern in &failure_patterns {
        if lower_obs.contains(pattern) || lower_pay.contains(pattern) {
            if *pattern == "failures: " && lower_obs.contains("failures: 0") {
                continue;
            }
            if *pattern == "test failed" && (lower_obs.contains("0 failed") || lower_obs.contains("failed: 0")) {
                continue;
            }
            return true;
        }
    }
    false
}

impl GraphBackend {
    /// Inicia una nueva trayectoria de exploración para una tarea
    pub fn start_trajectory(
        &self,
        project_path: &str,
        task_description: &str,
        policy_version: Option<&str>,
    ) -> Result<String> {
        let id = generate_id("traj");
        let policy_ver = policy_version.unwrap_or("v1.0.0");
        let inner = self.inner.lock().unwrap();

        inner.sqlite.execute(
            "INSERT INTO exploration_trajectories (id, project_path, task_description, policy_version, status, total_steps, cumulative_reward)
             VALUES (?1, ?2, ?3, ?4, 'in_progress', 0, 0.0)",
            params![id, project_path, task_description, policy_ver],
        ).context("Error al iniciar trayectoria de exploración")?;

        Ok(id)
    }

    /// Registra un nodo en el árbol de exploración con truncado de seguridad a 32 KB y MCTS backpropagation
    pub fn record_exploration_node(&self, params: RecordNodeParams) -> Result<ExplorationNode> {
        let node_id = generate_id("node");
        let inner = self.inner.lock().unwrap();

        // 1. Truncado de seguridad a 32 KB para mantener SQLite rápido y ágil
        let observation = if params.observation.len() > MAX_OBSERVATION_BYTES {
            let mut truncated = params.observation[..MAX_OBSERVATION_BYTES].to_string();
            truncated.push_str("\n...[truncated to 32KB by Ozymem Dream-RSI guard]");
            truncated
        } else {
            params.observation
        };

        // 2. Auto-Parenting Inteligente: Si no se provee parent_id, enlazar a la última hoja activa
        let (effective_parent_id, depth) = if let Some(ref pid) = params.parent_id {
            let parent_depth: Option<i64> = inner.sqlite.query_row(
                "SELECT depth FROM exploration_nodes WHERE id = ?1",
                params![pid],
                |row| row.get(0),
            ).ok();
            (Some(pid.clone()), parent_depth.map(|d| d + 1).unwrap_or(0))
        } else {
            let last_node: Option<(String, i64)> = inner.sqlite.query_row(
                "SELECT id, depth FROM exploration_nodes 
                 WHERE trajectory_id = ?1 AND is_pruned = 0
                 ORDER BY depth DESC, created_at DESC LIMIT 1",
                params![params.trajectory_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            ).ok();
            if let Some((last_id, last_depth)) = last_node {
                (Some(last_id), last_depth + 1)
            } else {
                (None, 0)
            }
        };

        // 3. Calibración Objetiva de Recompensa: Detección de fallos para neutralizar sesgos optimistas
        let raw_reward = params.reward_score.unwrap_or(0.0);
        let (reward, action_payload) = if raw_reward > 0.0 && detect_failure_signals(&observation, &params.action_payload) {
            let mut payload = params.action_payload.clone();
            if payload.starts_with('{') && payload.ends_with('}') {
                if let Ok(mut v) = serde_json::from_str::<serde_json::Value>(&payload) {
                    if let Some(obj) = v.as_object_mut() {
                        obj.insert("objective_reward_override".to_string(), serde_json::json!(true));
                        obj.insert("original_reward".to_string(), serde_json::json!(raw_reward));
                        payload = v.to_string();
                    }
                }
            }
            (-1.0, payload)
        } else {
            (raw_reward, params.action_payload)
        };

        let cost_tokens = params.cost_tokens.unwrap_or(0);
        let latency_ms = params.latency_ms.unwrap_or(0);
        let is_solution = params.is_solution.unwrap_or(false);
        let is_pruned = params.is_pruned.unwrap_or(false);

        // 4. Inserción del nodo
        inner.sqlite.execute(
            "INSERT INTO exploration_nodes (
                id, trajectory_id, parent_id, depth, action_type, action_payload,
                observation, cost_tokens, latency_ms, reward_score, visit_count,
                value_estimate, is_solution, is_pruned, created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 1, ?11, ?12, ?13, datetime('now'))",
            params![
                node_id,
                params.trajectory_id,
                effective_parent_id,
                depth,
                params.action_type,
                action_payload,
                observation,
                cost_tokens,
                latency_ms,
                reward,
                reward, // Initial value estimate is its own reward
                is_solution,
                is_pruned,
            ],
        ).context("Error al registrar nodo de exploración")?;

        // 5. Actualizar total_steps y acumulado en la cabecera de la trayectoria
        inner.sqlite.execute(
            "UPDATE exploration_trajectories
             SET total_steps = total_steps + 1,
                 cumulative_reward = cumulative_reward + ?1
             WHERE id = ?2",
            params![reward, params.trajectory_id],
        ).ok();

        // 6. MCTS Backpropagation: Actualizar visit_count y value_estimate hacia los ancestros
        if let Some(mut curr_parent_id) = effective_parent_id.clone() {
            let mut visited = std::collections::HashSet::new();
            while !curr_parent_id.is_empty() && visited.insert(curr_parent_id.clone()) {
                // Actualizar running average Q-value: Q = Q + (R - Q) / N
                let res = inner.sqlite.execute(
                    "UPDATE exploration_nodes
                     SET visit_count = visit_count + 1,
                         value_estimate = value_estimate + (?1 - value_estimate) / (visit_count + 1)
                     WHERE id = ?2",
                    params![reward, curr_parent_id],
                );
                if res.is_err() {
                    break;
                }

                // Obtener siguiente ancestro
                let next_parent: Option<String> = inner.sqlite.query_row(
                    "SELECT parent_id FROM exploration_nodes WHERE id = ?1",
                    params![curr_parent_id],
                    |row| row.get(0),
                ).ok().flatten();

                match next_parent {
                    Some(np) => curr_parent_id = np,
                    None => break,
                }
            }
        }

        // Consultar el nodo insertado
        let created_at: String = inner.sqlite.query_row(
            "SELECT created_at FROM exploration_nodes WHERE id = ?1",
            params![node_id],
            |row| row.get(0),
        ).unwrap_or_else(|_| "".to_string());

        Ok(ExplorationNode {
            id: node_id,
            trajectory_id: params.trajectory_id,
            parent_id: effective_parent_id,
            depth,
            action_type: params.action_type,
            action_payload,
            observation,
            cost_tokens,
            latency_ms,
            reward_score: reward,
            visit_count: 1,
            value_estimate: reward,
            is_solution,
            is_pruned,
            created_at,
        })
    }

    /// Registra un lote de pasos secuenciales atómicamente con auto-parenting en cadena
    pub fn record_exploration_batch(&self, params: RecordBatchParams) -> Result<Vec<ExplorationNode>> {
        let mut results = Vec::new();
        let mut last_parent_id: Option<String> = None;

        for mut step in params.steps {
            step.trajectory_id = params.trajectory_id.clone();
            // Si el paso del lote no define parent_id y ya insertamos un paso previo, encadenarlo
            if step.parent_id.is_none() && last_parent_id.is_some() {
                step.parent_id = last_parent_id.clone();
            }
            let node = self.record_exploration_node(step)?;
            last_parent_id = Some(node.id.clone());
            results.push(node);
        }

        Ok(results)
    }

    /// Finaliza una trayectoria de exploración marcando su estado y tiempo de cierre
    pub fn complete_trajectory(
        &self,
        trajectory_id: &str,
        status: &str,
        final_reward: Option<f64>,
    ) -> Result<()> {
        let inner = self.inner.lock().unwrap();
        inner.sqlite.execute(
            "UPDATE exploration_trajectories
             SET status = ?1,
                 completed_at = datetime('now'),
                 cumulative_reward = cumulative_reward + ?2
             WHERE id = ?3",
            params![status, final_reward.unwrap_or(0.0), trajectory_id],
        ).context("Error al completar trayectoria de exploración")?;

        Ok(())
    }

    /// Obtiene el árbol completo de exploración estructurado jerárquicamente con scores UCT / UCB1 para MCTS
    pub fn get_trajectory_tree(&self, trajectory_id: &str) -> Result<Vec<ExplorationTreeNode>> {
        let inner = self.inner.lock().unwrap();

        let mut stmt = inner.sqlite.prepare(
            "SELECT id, trajectory_id, parent_id, depth, action_type, action_payload,
                    observation, cost_tokens, latency_ms, reward_score, visit_count,
                    value_estimate, is_solution, is_pruned, created_at
             FROM exploration_nodes
             WHERE trajectory_id = ?1
             ORDER BY depth ASC, created_at ASC",
        )?;

        let rows = stmt.query_map(params![trajectory_id], |row| {
            Ok(ExplorationNode {
                id: row.get(0)?,
                trajectory_id: row.get(1)?,
                parent_id: row.get(2)?,
                depth: row.get(3)?,
                action_type: row.get(4)?,
                action_payload: row.get(5)?,
                observation: row.get(6)?,
                cost_tokens: row.get(7)?,
                latency_ms: row.get(8)?,
                reward_score: row.get(9)?,
                visit_count: row.get(10)?,
                value_estimate: row.get(11)?,
                is_solution: row.get(12)?,
                is_pruned: row.get(13)?,
                created_at: row.get(14)?,
            })
        })?;

        let mut all_nodes = Vec::new();
        for r in rows {
            all_nodes.push(r?);
        }

        // Agrupar por parent_id
        let mut children_by_parent: HashMap<Option<String>, Vec<ExplorationNode>> = HashMap::new();
        let mut node_visits: HashMap<String, i64> = HashMap::new();

        for n in &all_nodes {
            node_visits.insert(n.id.clone(), n.visit_count);
            children_by_parent
                .entry(n.parent_id.clone())
                .or_default()
                .push(n.clone());
        }

        // Construir recursivamente el árbol con UCB1
        fn build_tree(
            parent_id: Option<String>,
            parent_visits: i64,
            children_by_parent: &HashMap<Option<String>, Vec<ExplorationNode>>,
        ) -> Vec<ExplorationTreeNode> {
            let mut result = Vec::new();
            if let Some(children) = children_by_parent.get(&parent_id) {
                for child in children {
                    // Cálculo UCB1: Q + c * sqrt(ln(N_parent) / N_child)
                    let c = std::f64::consts::SQRT_2;
                    let uct_score = if parent_visits > 0 && child.visit_count > 0 {
                        let exploitation = child.value_estimate;
                        let exploration = c * ((parent_visits as f64).ln() / (child.visit_count as f64)).sqrt();
                        exploitation + exploration
                    } else {
                        child.value_estimate
                    };

                    let sub_children = build_tree(
                        Some(child.id.clone()),
                        child.visit_count,
                        children_by_parent,
                    );

                    result.push(ExplorationTreeNode {
                        node: child.clone(),
                        uct_score,
                        children: sub_children,
                    });
                }
            }
            result
        }

        let total_root_visits: i64 = all_nodes
            .iter()
            .filter(|n| n.parent_id.is_none())
            .map(|n| n.visit_count)
            .sum();

        let tree = build_tree(None, total_root_visits.max(1), &children_by_parent);
        Ok(tree)
    }

    /// Lista trayectorias registradas ordenadas por fecha más reciente
    pub fn list_trajectories(
        &self,
        project_path: Option<&str>,
        limit: usize,
    ) -> Result<Vec<TrajectoryHeader>> {
        let inner = self.inner.lock().unwrap();

        let (query, p_val) = match project_path {
            Some(p) => (
                "SELECT id, project_path, task_description, policy_version, status, total_steps, cumulative_reward, created_at, completed_at
                 FROM exploration_trajectories
                 WHERE project_path = ?1
                 ORDER BY created_at DESC LIMIT ?2",
                Some(p),
            ),
            None => (
                "SELECT id, project_path, task_description, policy_version, status, total_steps, cumulative_reward, created_at, completed_at
                 FROM exploration_trajectories
                 ORDER BY created_at DESC LIMIT ?1",
                None,
            ),
        };

        let mut list = Vec::new();
        if let Some(p) = p_val {
            let mut stmt = inner.sqlite.prepare(query)?;
            let rows = stmt.query_map(params![p, limit as i64], |row| {
                Ok(TrajectoryHeader {
                    id: row.get(0)?,
                    project_path: row.get(1)?,
                    task_description: row.get(2)?,
                    policy_version: row.get(3)?,
                    status: row.get(4)?,
                    total_steps: row.get(5)?,
                    cumulative_reward: row.get(6)?,
                    created_at: row.get(7)?,
                    completed_at: row.get(8)?,
                })
            })?;
            for r in rows {
                list.push(r?);
            }
        } else {
            let mut stmt = inner.sqlite.prepare(query)?;
            let rows = stmt.query_map(params![limit as i64], |row| {
                Ok(TrajectoryHeader {
                    id: row.get(0)?,
                    project_path: row.get(1)?,
                    task_description: row.get(2)?,
                    policy_version: row.get(3)?,
                    status: row.get(4)?,
                    total_steps: row.get(5)?,
                    cumulative_reward: row.get(6)?,
                    created_at: row.get(7)?,
                    completed_at: row.get(8)?,
                })
            })?;
            for r in rows {
                list.push(r?);
            }
        }

        Ok(list)
    }

    /// Diagnostica cuellos de botella, eficiencia de tokens y oportunidades de poda en una trayectoria
    pub fn diagnose_trajectory(&self, trajectory_id: &str) -> Result<TrajectoryDiagnosis> {
        let inner = self.inner.lock().unwrap();

        let mut stmt = inner.sqlite.prepare(
            "SELECT id, depth, action_type, cost_tokens, latency_ms, reward_score, is_solution, is_pruned, parent_id
             FROM exploration_nodes
             WHERE trajectory_id = ?1
             ORDER BY depth ASC, created_at ASC",
        )?;

        struct NodeDiag {
            id: String,
            depth: i64,
            action_type: String,
            cost_tokens: i64,
            latency_ms: i64,
            reward_score: f64,
            is_solution: bool,
            is_pruned: bool,
            parent_id: Option<String>,
        }

        let rows = stmt.query_map(params![trajectory_id], |row| {
            Ok(NodeDiag {
                id: row.get(0)?,
                depth: row.get(1)?,
                action_type: row.get(2)?,
                cost_tokens: row.get(3)?,
                latency_ms: row.get(4)?,
                reward_score: row.get(5)?,
                is_solution: row.get(6)?,
                is_pruned: row.get(7)?,
                parent_id: row.get(8)?,
            })
        })?;

        let mut nodes = Vec::new();
        for r in rows {
            nodes.push(r?);
        }

        let total_steps = nodes.len();
        let total_tokens: i64 = nodes.iter().map(|n| n.cost_tokens).sum();
        let total_latency_ms: i64 = nodes.iter().map(|n| n.latency_ms).sum();
        let max_depth = nodes.iter().map(|n| n.depth).max().unwrap_or(0);
        let has_solution = nodes.iter().any(|n| n.is_solution);

        let mut bottlenecks = Vec::new();
        let mut pruning_opportunities = Vec::new();
        let mut solution_node_id: Option<String> = None;

        for n in &nodes {
            if n.latency_ms > 3000 {
                bottlenecks.push(format!("{}: {} ms ({})", n.id, n.latency_ms, n.action_type));
            }
            if n.cost_tokens > 2000 {
                bottlenecks.push(format!("{}: {} tokens ({})", n.id, n.cost_tokens, n.action_type));
            }
            if n.reward_score < 0.0 && !n.is_pruned {
                pruning_opportunities.push(format!("{}: reward {:.2} no podado ({})", n.id, n.reward_score, n.action_type));
            }
            if n.is_solution && solution_node_id.is_none() {
                solution_node_id = Some(n.id.clone());
            }
        }

        // Calcular eficiencia de tokens (tokens en la rama de la solución vs tokens totales)
        let token_efficiency_percent = if let Some(sol_id) = solution_node_id {
            let mut sol_path_ids = std::collections::HashSet::new();
            let mut current = Some(sol_id);
            let node_map: HashMap<String, &NodeDiag> = nodes.iter().map(|n| (n.id.clone(), n)).collect();

            while let Some(cid) = current {
                sol_path_ids.insert(cid.clone());
                current = node_map.get(&cid).and_then(|n| n.parent_id.clone());
            }

            let sol_tokens: i64 = nodes.iter().filter(|n| sol_path_ids.contains(&n.id)).map(|n| n.cost_tokens).sum();
            if total_tokens > 0 {
                ((sol_tokens as f64) / (total_tokens as f64)) * 100.0
            } else {
                100.0
            }
        } else {
            0.0
        };

        // Sugerir constante UCB1 basada en varianza de recompensas
        let suggested_ucb1_c = if total_steps > 1 {
            let mean = nodes.iter().map(|n| n.reward_score).sum::<f64>() / (total_steps as f64);
            let variance = nodes.iter().map(|n| (n.reward_score - mean).powi(2)).sum::<f64>() / (total_steps as f64);
            if variance > 10.0 {
                1.8 // Alta dispersión -> mayor exploración
            } else if variance < 1.0 {
                1.1 // Recompensas homogéneas -> mayor explotación
            } else {
                1.4142 // Balance estándar sqrt(2)
            }
        } else {
            1.4142
        };

        Ok(TrajectoryDiagnosis {
            trajectory_id: trajectory_id.to_string(),
            total_steps,
            total_tokens,
            total_latency_ms,
            bottlenecks,
            pruning_opportunities,
            suggested_ucb1_c,
            token_efficiency_percent,
            has_solution,
            max_depth,
        })
    }

    /// Elimina una trayectoria y todos sus nodos en cascada
    pub fn delete_trajectory(&self, trajectory_id: &str) -> Result<()> {
        let inner = self.inner.lock().unwrap();
        inner.sqlite.execute(
            "DELETE FROM exploration_nodes WHERE trajectory_id = ?1",
            params![trajectory_id],
        ).ok();
        inner.sqlite.execute(
            "DELETE FROM exploration_trajectories WHERE id = ?1",
            params![trajectory_id],
        ).context("Error al eliminar trayectoria")?;
        Ok(())
    }
}
