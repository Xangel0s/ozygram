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

        // 2. Determinar profundidad basada en nodo padre
        let depth = if let Some(ref pid) = params.parent_id {
            let parent_depth: Option<i64> = inner.sqlite.query_row(
                "SELECT depth FROM exploration_nodes WHERE id = ?1",
                params![pid],
                |row| row.get(0),
            ).ok();
            parent_depth.map(|d| d + 1).unwrap_or(0)
        } else {
            0
        };

        let reward = params.reward_score.unwrap_or(0.0);
        let cost_tokens = params.cost_tokens.unwrap_or(0);
        let latency_ms = params.latency_ms.unwrap_or(0);
        let is_solution = params.is_solution.unwrap_or(false);
        let is_pruned = params.is_pruned.unwrap_or(false);

        // 3. Inserción del nodo
        inner.sqlite.execute(
            "INSERT INTO exploration_nodes (
                id, trajectory_id, parent_id, depth, action_type, action_payload,
                observation, cost_tokens, latency_ms, reward_score, visit_count,
                value_estimate, is_solution, is_pruned, created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 1, ?11, ?12, ?13, datetime('now'))",
            params![
                node_id,
                params.trajectory_id,
                params.parent_id,
                depth,
                params.action_type,
                params.action_payload,
                observation,
                cost_tokens,
                latency_ms,
                reward,
                reward, // Initial value estimate is its own reward
                is_solution,
                is_pruned,
            ],
        ).context("Error al registrar nodo de exploración")?;

        // 4. Actualizar total_steps y acumulado en la cabecera de la trayectoria
        inner.sqlite.execute(
            "UPDATE exploration_trajectories
             SET total_steps = total_steps + 1,
                 cumulative_reward = cumulative_reward + ?1
             WHERE id = ?2",
            params![reward, params.trajectory_id],
        ).ok();

        // 5. MCTS Backpropagation: Actualizar visit_count y value_estimate hacia los ancestros
        if let Some(mut curr_parent_id) = params.parent_id.clone() {
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
            parent_id: params.parent_id,
            depth,
            action_type: params.action_type,
            action_payload: params.action_payload,
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

    /// Elimina una trayectoria y todos sus nodos en cascada
    pub fn delete_trajectory(&self, trajectory_id: &str) -> Result<()> {
        let inner = self.inner.lock().unwrap();
        inner.sqlite.execute(
            "DELETE FROM exploration_trajectories WHERE id = ?1",
            params![trajectory_id],
        ).context("Error al eliminar trayectoria")?;
        Ok(())
    }
}
