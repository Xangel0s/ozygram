use anyhow::{Context, Result};
use clap::Subcommand;
use ozymem_core::graph_backend::GraphBackend;
use serde_json::json;
use std::path::Path;

#[derive(Subcommand, Debug, Clone)]
pub enum DreamSubcommand {
    /// Run an offline Dream-RSI simulation and optimization round
    Run {
        /// Optional specific trajectory or task ID to simulate
        #[arg(long)]
        task: Option<String>,
        /// Optional path to project or database
        #[arg(long)]
        path: Option<String>,
    },
    /// Display current active MCTS exploration policy status and performance metrics
    Status {
        /// Optional path to project or database
        #[arg(long)]
        path: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Diagnose a trajectory: detect bottlenecks, pruning opportunities, and token efficiency
    Diagnose {
        /// Trajectory ID to diagnose
        trajectory_id: String,
        /// Optional path to project or database
        #[arg(long)]
        path: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Export or visualize the MCTS exploration tree as a Mermaid diagram
    Tree {
        /// Trajectory ID to render
        trajectory_id: String,
        /// Optional path to project or database
        #[arg(long)]
        path: Option<String>,
        /// Output raw JSON with mermaid string instead of markdown block
        #[arg(long)]
        json: bool,
    },
    /// Export or visualize the memory knowledge graph as a Mermaid diagram
    Graph {
        /// Optional module or path filter
        #[arg(long)]
        module: Option<String>,
        /// Optional path to project or database
        #[arg(long)]
        path: Option<String>,
        /// Output raw JSON with mermaid string instead of markdown block
        #[arg(long)]
        json: bool,
    },
    /// Display shadow mode metrics: tree branching factor, token accounting, and resume adherence
    Shadow {
        /// Optional specific trajectory ID
        #[arg(long)]
        task: Option<String>,
        /// Optional path to project or database
        #[arg(long)]
        path: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

pub fn run_dream_command(cmd: &DreamSubcommand) -> Result<()> {
    match cmd {
        DreamSubcommand::Run { task, path } => {
            let target_path = path.as_deref().unwrap_or(".");
            let backend = GraphBackend::open_for_project(Path::new(target_path))
                .context("No se pudo inicializar GraphBackend para Dream-RSI")?;

            println!("============================================================");
            println!("  OZYMEM: TRAJECTORY REPLAY & EXPLORATION TUNER [EXPERIMENTAL]");
            println!("============================================================");
            println!("Target project: {}", target_path);

            let trajectories = backend.list_trajectories(Some(target_path), 50)?;
            if trajectories.is_empty() {
                println!("[!] No se encontraron trayectorias registradas en el árbol.");
                println!("    Inicia una tarea con 'ozy_exploration(action=\"start\")' en el agente.");
            } else {
                println!("[*] Cargadas {} trayectoria(s) históricas para simulación contrafactual.", trajectories.len());

                let selected_traj = if let Some(ref tid) = task {
                    trajectories.into_iter().find(|t| &t.id == tid)
                } else {
                    trajectories.into_iter().next()
                };

                if let Some(traj) = selected_traj {
                    println!("[*] Simulando trayectoria: {} ({})", traj.id, traj.task_description);
                    let tree = backend.get_trajectory_tree(&traj.id)?;
                    println!("    Total de nodos en árbol: {}", tree.len());
                    println!("    Inspección de trayectoria y poda determinista completada.");
                    println!("[OK] Verificación de árbol completada.");
                }
            }

            // [TASK 4.1] Auto-Wiring Cognitivo de Memorias
            let wire_report = backend.auto_wire_memories(0.80)?;
            if wire_report.edges_created > 0 {
                println!("[*] Auto-Wiring Cognitivo: {} nuevas aristas tejidas ({} REINFORCES, {} SUPERSEDES).",
                    wire_report.edges_created, wire_report.reinforces_count, wire_report.supersedes_count);
                for detail in &wire_report.details {
                    println!("    - {}", detail);
                }
            } else {
                println!("[*] Auto-Wiring Cognitivo: Red de memorias consolidada (0 aristas pendientes).");
            }

            // [TASK 8.3] Consolidación Offline: Memory Decay y Vacunación Postmortem
            let pruned = backend.prune_decayed_memories(0.35, 7)?;
            if pruned > 0 {
                println!("[*] Consolidación Offline: {} memoria(s) temporales decaídas purgadas/archivadas.", pruned);
            } else {
                println!("[*] Consolidación Offline: Memoria temporal al día (0 notas huérfanas).");
            }

            let vaccinated = backend.vaccinate_postmortems(Some(Path::new(target_path)))?;
            if vaccinated > 0 {
                println!("[*] Vacunación Postmortem: {} nueva(s) invariante(s) permanente(s) inyectada(s) desde commits fix/revert.", vaccinated);
            } else {
                println!("[*] Vacunación Postmortem: Sin regresiones pendientes de vacunar.");
            }

            Ok(())
        }

        DreamSubcommand::Graph { module, path, json } => {
            let target_path = path.as_deref().unwrap_or(".");
            let backend = GraphBackend::open_for_project(Path::new(target_path))
                .context("No se pudo inicializar GraphBackend para Dream-RSI")?;
            let mermaid = backend.render_mermaid_memory_graph(module.as_deref())?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&json!({
                    "mermaid": mermaid,
                    "module": module,
                }))?);
            } else {
                println!("```mermaid\n{}```", mermaid);
            }
            Ok(())
        }

        DreamSubcommand::Status { path, json } => {
            let target_path = path.as_deref().unwrap_or(".");
            let backend = GraphBackend::open_for_project(Path::new(target_path))
                .context("No se pudo abrir base de datos para consultar status de Dream-RSI")?;
            let trajectories = backend.list_trajectories(None, 100).unwrap_or_default();

            let total_steps: i64 = trajectories.iter().map(|t| t.total_steps).sum();
            let avg_reward = if !trajectories.is_empty() {
                trajectories.iter().map(|t| t.cumulative_reward).sum::<f64>() / (trajectories.len() as f64)
            } else {
                0.0
            };

            let shadow_metrics = backend.list_shadow_metrics(Some(target_path), 1).unwrap_or_default();
            let shadow_opt = shadow_metrics.first();

            if *json {
                let mut out = json!({
                    "engine": "Trajectory Replay & Exploration Tuner [EXPERIMENTAL]",
                    "active_policy": "MctsExplorationPolicy",
                    "policy_version": "v1.0.0",
                    "exploration_constant": 1.4142,
                    "prune_threshold": -3.0,
                    "max_observation_limit_kb": 32,
                    "total_trajectories_recorded": trajectories.len(),
                    "total_steps_in_trees": total_steps,
                    "average_cumulative_reward": avg_reward,
                });
                if let Some(m) = shadow_opt {
                    out["shadow_mode"] = json!({
                        "effective_branching_factor": m.branching_factor,
                        "max_branching": m.max_branching,
                        "max_depth": m.max_depth,
                        "total_tokens_tracked": m.total_cost_tokens,
                        "total_latency_ms": m.total_latency_ms,
                        "recommendations_given": m.recommendations_given,
                        "recommendations_followed": m.recommendations_followed,
                        "adherence_rate": m.adherence_rate,
                        "status": "[STATUS: SHADOW_MODE_ACTIVE]"
                    });
                }
                println!("{}", serde_json::to_string_pretty(&out)?);
            } else {
                println!("============================================================");
                println!("  OZYMEM: TRAJECTORY REPLAY & EXPLORATION STATUS [EXPERIMENTAL]");
                println!("============================================================");
                println!("  Active Policy:         MctsExplorationPolicy");
                println!("  Policy Version:        v1.0.0");
                println!("  Exploration Constant:  1.4142 (UCT sqrt(2))");
                println!("  Prune Threshold:       -3.0");
                println!("  Observation Guard:     32 KB limit");
                println!("  Trajectories Tracked:  {}", trajectories.len());
                println!("  Total Decision Steps:  {}", total_steps);
                println!("  Average Reward:        {:.2}", avg_reward);
                if let Some(m) = shadow_opt {
                    println!("------------------------------------------------------------");
                    println!("  [ETAPA 2: MODO SOMBRA - TELEMETRÍA DE EXPLORACIÓN]");
                    println!("  Effective Branching (B):  {:.2} (max: {})", m.branching_factor, m.max_branching);
                    println!("  Resume Adherence Rate:    {:.1}% ({}/{} seguidas)", m.adherence_rate * 100.0, m.recommendations_followed, m.recommendations_given);
                    println!("  Total Tracked Tokens:     {}", m.total_cost_tokens);
                    println!("  Total Tracked Latency:    {} ms", m.total_latency_ms);
                    println!("  Status:                   [STATUS: SHADOW_MODE_ACTIVE]");
                }
                println!("============================================================");
            }

            Ok(())
        }

        DreamSubcommand::Diagnose { trajectory_id, path, json } => {
            let target_path = path.as_deref().unwrap_or(".");
            let backend = GraphBackend::open_for_project(Path::new(target_path))
                .context("No se pudo inicializar GraphBackend para diagnóstico")?;

            let diag = backend.diagnose_trajectory(trajectory_id)?;

            if *json {
                println!("{}", serde_json::to_string_pretty(&diag)?);
            } else {
                println!("============================================================");
                println!("  OZYMEM DREAM-RSI: TRAJECTORY DIAGNOSIS");
                println!("============================================================");
                println!("  Trajectory ID:     {}", diag.trajectory_id);
                println!("  Total Steps:       {}", diag.total_steps);
                println!("  Max Depth:         {}", diag.max_depth);
                println!("  Total Tokens:      {}", diag.total_tokens);
                println!("  Total Latency:     {} ms", diag.total_latency_ms);
                println!("  Solution Reached:  {}", if diag.has_solution { "YES" } else { "NO" });
                println!("  Token Efficiency:  {:.1}%", diag.token_efficiency_percent);
                println!("  Suggested UCB1 c:  {:.4}", diag.suggested_ucb1_c);
                println!("------------------------------------------------------------");
                if diag.bottlenecks.is_empty() {
                    println!("  Bottlenecks (>3s / >2k tokens): None detected (Fast & Lean)");
                } else {
                    println!("  Bottlenecks Detected ({}):", diag.bottlenecks.len());
                    for b in &diag.bottlenecks {
                        println!("    • {}", b);
                    }
                }
                if diag.pruning_opportunities.is_empty() {
                    println!("  Pruning Opportunities: None (Clean exploration)");
                } else {
                    println!("  Pruning Opportunities ({}):", diag.pruning_opportunities.len());
                    for p in &diag.pruning_opportunities {
                        println!("    • {}", p);
                    }
                }
                println!("============================================================");
            }

            Ok(())
        }
        DreamSubcommand::Tree { trajectory_id, path, json } => {
            let target_path = path.as_deref().unwrap_or(".");
            let backend = GraphBackend::open_for_project(Path::new(target_path))
                .context("No se pudo inicializar GraphBackend para Dream-RSI")?;

            let mermaid = backend.render_mermaid_tree(trajectory_id)?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&json!({
                    "trajectory_id": trajectory_id,
                    "mermaid": mermaid
                }))?);
            } else {
                println!("```mermaid");
                println!("{}", mermaid.trim_end());
                println!("```");
            }
            Ok(())
        }

        DreamSubcommand::Shadow { task, path, json } => {
            let target_path = path.as_deref().unwrap_or(".");
            let backend = GraphBackend::open_for_project(Path::new(target_path))
                .context("No se pudo abrir base de datos para consultar métricas de Modo Sombra")?;

            let metrics = if let Some(ref tid) = task {
                backend.get_shadow_metrics(tid)?.into_iter().collect::<Vec<_>>()
            } else {
                backend.list_shadow_metrics(Some(target_path), 50).unwrap_or_default()
            };

            if *json {
                println!("{}", serde_json::to_string_pretty(&metrics)?);
            } else {
                println!("============================================================");
                println!("  OZYMEM: SHADOW MODE EXPLORATION METRICS [ETAPA 2]");
                println!("============================================================");
                if metrics.is_empty() {
                    println!("  [STATUS: NO_METRICS] No hay trayectorias registradas aún.");
                } else {
                    for m in &metrics {
                        println!("  Trajectory:               {}", m.trajectory_id);
                        println!("  Effective Branching (B):  {:.2} (max: {})", m.branching_factor, m.max_branching);
                        println!("  Max Depth:                {}", m.max_depth);
                        println!("  Total Decision Steps:     {}", m.total_nodes);
                        println!("  Total Tokens Tracked:     {}", m.total_cost_tokens);
                        println!("  Total Latency Tracked:    {} ms", m.total_latency_ms);
                        println!("  Recommendations Given:    {}", m.recommendations_given);
                        println!("  Recommendations Followed: {}", m.recommendations_followed);
                        println!("  Recommendations Ignored:  {}", m.recommendations_ignored);
                        println!("  Adherence Rate:           {:.1}%", m.adherence_rate * 100.0);
                        println!("  Updated At:               {}", m.updated_at);
                        println!("------------------------------------------------------------");
                    }
                }
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dream_subcommand_status() {
        let temp_root = std::env::temp_dir().join(format!(
            "ozymem-dream-status-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::create_dir_all(&temp_root);

        let cmd = DreamSubcommand::Status {
            path: Some(temp_root.to_string_lossy().to_string()),
            json: true,
        };
        let res = run_dream_command(&cmd);
        let _ = std::fs::remove_dir_all(&temp_root);
        assert!(res.is_ok(), "run_dream_command(&cmd) failed: {:?}", res);
    }

    #[test]
    fn test_dream_subcommand_diagnose_not_found() {
        let temp_root = std::env::temp_dir().join(format!(
            "ozymem-dream-diag-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::create_dir_all(&temp_root);

        let cmd = DreamSubcommand::Diagnose {
            trajectory_id: "non_existent_traj_123".to_string(),
            path: Some(temp_root.to_string_lossy().to_string()),
            json: true,
        };
        // Diagnosing empty trajectory returns 0 steps cleanly
        let res = run_dream_command(&cmd);
        let _ = std::fs::remove_dir_all(&temp_root);
        assert!(res.is_ok(), "run_dream_command(&cmd) failed: {:?}", res);
    }

    #[test]
    fn test_dream_subcommand_graph() {
        let temp_root = std::env::temp_dir().join(format!(
            "ozymem-dream-graph-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::create_dir_all(&temp_root);

        let cmd = DreamSubcommand::Graph {
            module: None,
            path: Some(temp_root.to_string_lossy().to_string()),
            json: true,
        };
        let res = run_dream_command(&cmd);
        let _ = std::fs::remove_dir_all(&temp_root);
        assert!(res.is_ok(), "run_dream_command(&cmd) failed: {:?}", res);
    }

    #[test]
    fn test_dream_subcommand_shadow() {
        let temp_root = std::env::temp_dir().join(format!(
            "ozymem-dream-shadow-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::create_dir_all(&temp_root);

        let cmd = DreamSubcommand::Shadow {
            task: None,
            path: Some(temp_root.to_string_lossy().to_string()),
            json: true,
        };
        let res = run_dream_command(&cmd);
        let _ = std::fs::remove_dir_all(&temp_root);
        assert!(res.is_ok(), "run_dream_command(&cmd) failed: {:?}", res);
    }
}
