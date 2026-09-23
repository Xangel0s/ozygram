use anyhow::{anyhow, Result};
use ozymem_core::graph_backend::{GraphBackend, RecordBatchParams, RecordNodeParams};
use ozymem_core::mcp_common::{ContentBlock, ToolCallParams, ToolCallResult};
use serde_json::{json, Value};

/// Manejador unificado para la herramienta MCP `ozy_exploration` (Dream-RSI & MCTS)
pub fn handle_exploration(
    backend: &GraphBackend,
    tool_call: &ToolCallParams,
) -> Result<ToolCallResult> {
    let args = &tool_call.arguments;

    let action = args
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or("record_step");

    match action {
        "start" => {
            let task_description = args
                .get("task_description")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("Falta el parámetro 'task_description' para iniciar la trayectoria"))?;
            let project_path = args
                .get("project_path")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| backend.project_path())
                .unwrap_or_else(|| ".".to_string());
            let policy_version = args.get("policy_version").and_then(Value::as_str);

            let traj_id = backend.start_trajectory(&project_path, task_description, policy_version)?;

            let res = json!({
                "status": "ok",
                "trajectory_id": traj_id,
                "project_path": project_path,
                "task_description": task_description,
                "policy_version": policy_version.unwrap_or("v1.0.0"),
                "message": "Trayectoria de exploración iniciada exitosamente"
            });

            Ok(ToolCallResult {
                content: vec![ContentBlock {
                    kind: "text",
                    text: serde_json::to_string_pretty(&res)?,
                }],
                is_error: None,
            })
        }

        "record_step" => {
            let trajectory_id = args
                .get("trajectory_id")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("Falta el parámetro 'trajectory_id' para registrar el paso"))?
                .to_string();

            let parent_id = args
                .get("parent_id")
                .and_then(Value::as_str)
                .map(str::to_string);

            let action_type = args
                .get("action_type")
                .and_then(Value::as_str)
                .unwrap_or("tool_call")
                .to_string();

            let action_payload = args
                .get("action_payload")
                .map(|v| if v.is_string() { v.as_str().unwrap().to_string() } else { v.to_string() })
                .unwrap_or_else(|| "{}".to_string());

            let observation = args
                .get("observation")
                .map(|v| if v.is_string() { v.as_str().unwrap().to_string() } else { v.to_string() })
                .unwrap_or_else(|| "{}".to_string());

            let cost_tokens = args.get("cost_tokens").and_then(Value::as_i64);
            let latency_ms = args.get("latency_ms").and_then(Value::as_i64);
            let is_solution = args.get("is_solution").and_then(Value::as_bool);
            let is_pruned = args.get("is_pruned").and_then(Value::as_bool);

            // Recompensa automática heurística si no se suministra explícitamente
            let reward_score = args
                .get("reward_score")
                .and_then(Value::as_f64)
                .or_else(|| {
                    if is_solution == Some(true) {
                        Some(10.0)
                    } else if is_pruned == Some(true) {
                        Some(-5.0)
                    } else {
                        None
                    }
                });

            let node = backend.record_exploration_node(RecordNodeParams {
                trajectory_id,
                parent_id,
                action_type,
                action_payload,
                observation,
                cost_tokens,
                latency_ms,
                reward_score,
                is_solution,
                is_pruned,
            })?;

            let res = json!({
                "status": "ok",
                "node": node,
                "message": "Paso de exploración registrado y propagado por MCTS"
            });

            Ok(ToolCallResult {
                content: vec![ContentBlock {
                    kind: "text",
                    text: serde_json::to_string_pretty(&res)?,
                }],
                is_error: None,
            })
        }

        "record_batch" => {
            let trajectory_id = args
                .get("trajectory_id")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("Falta el parámetro 'trajectory_id' para registrar el lote"))?
                .to_string();

            let steps_arr = args
                .get("steps")
                .and_then(Value::as_array)
                .ok_or_else(|| anyhow!("Falta el array 'steps' para record_batch"))?;

            let mut batch_steps = Vec::new();
            for s in steps_arr {
                let action_type = s.get("action_type").and_then(Value::as_str).unwrap_or("tool_call").to_string();
                let action_payload = s.get("action_payload")
                    .map(|v| if v.is_string() { v.as_str().unwrap().to_string() } else { v.to_string() })
                    .unwrap_or_else(|| "{}".to_string());
                let observation = s.get("observation")
                    .map(|v| if v.is_string() { v.as_str().unwrap().to_string() } else { v.to_string() })
                    .unwrap_or_else(|| "{}".to_string());
                let parent_id = s.get("parent_id").and_then(Value::as_str).map(str::to_string);
                let cost_tokens = s.get("cost_tokens").and_then(Value::as_i64);
                let latency_ms = s.get("latency_ms").and_then(Value::as_i64);
                let is_solution = s.get("is_solution").and_then(Value::as_bool);
                let is_pruned = s.get("is_pruned").and_then(Value::as_bool);
                let reward_score = s.get("reward_score").and_then(Value::as_f64).or_else(|| {
                    if is_solution == Some(true) {
                        Some(10.0)
                    } else if is_pruned == Some(true) {
                        Some(-5.0)
                    } else {
                        None
                    }
                });

                batch_steps.push(RecordNodeParams {
                    trajectory_id: trajectory_id.clone(),
                    parent_id,
                    action_type,
                    action_payload,
                    observation,
                    cost_tokens,
                    latency_ms,
                    reward_score,
                    is_solution,
                    is_pruned,
                });
            }

            let nodes = backend.record_exploration_batch(RecordBatchParams {
                trajectory_id,
                steps: batch_steps,
            })?;

            let res = json!({
                "status": "ok",
                "recorded_steps": nodes.len(),
                "nodes": nodes,
                "message": format!("Lote de {} pasos registrado y propagado exitosamente", nodes.len())
            });

            Ok(ToolCallResult {
                content: vec![ContentBlock {
                    kind: "text",
                    text: serde_json::to_string_pretty(&res)?,
                }],
                is_error: None,
            })
        }

        "complete" => {
            let trajectory_id = args
                .get("trajectory_id")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("Falta 'trajectory_id' para completar la trayectoria"))?;
            let status = args
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("completed");
            let final_reward = args.get("reward_score").and_then(Value::as_f64);

            backend.complete_trajectory(trajectory_id, status, final_reward)?;

            let res = json!({
                "status": "ok",
                "trajectory_id": trajectory_id,
                "final_status": status,
                "message": "Trayectoria finalizada correctamente"
            });

            Ok(ToolCallResult {
                content: vec![ContentBlock {
                    kind: "text",
                    text: serde_json::to_string_pretty(&res)?,
                }],
                is_error: None,
            })
        }

        "get_tree" => {
            let trajectory_id = args
                .get("trajectory_id")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("Falta 'trajectory_id' para obtener el árbol"))?;

            let tree = backend.get_trajectory_tree(trajectory_id)?;

            let res = json!({
                "status": "ok",
                "trajectory_id": trajectory_id,
                "tree": tree
            });

            Ok(ToolCallResult {
                content: vec![ContentBlock {
                    kind: "text",
                    text: serde_json::to_string_pretty(&res)?,
                }],
                is_error: None,
            })
        }

        "diagnose" => {
            let trajectory_id = match args.get("trajectory_id").and_then(Value::as_str) {
                Some(tid) => tid.to_string(),
                None => {
                    let recent = backend.list_trajectories(None, 1)?;
                    match recent.into_iter().next() {
                        Some(t) => t.id,
                        None => {
                            let res = json!({
                                "status": "ok",
                                "message": "No hay trayectorias de exploración registradas en este proyecto.",
                                "diagnosis": null
                            });
                            return Ok(ToolCallResult {
                                content: vec![ContentBlock {
                                    kind: "text",
                                    text: serde_json::to_string_pretty(&res)?,
                                }],
                                is_error: None,
                            });
                        }
                    }
                }
            };

            let diag = backend.diagnose_trajectory(&trajectory_id)?;

            let res = json!({
                "status": "ok",
                "diagnosis": diag,
                "message": format!(
                    "Diagnóstico para '{}': {} pasos, {} tokens, eficiencia: {:.1}%, cuellos de botella: {}",
                    trajectory_id,
                    diag.total_steps,
                    diag.total_tokens,
                    diag.token_efficiency_percent,
                    diag.bottlenecks.len()
                )
            });

            Ok(ToolCallResult {
                content: vec![ContentBlock {
                    kind: "text",
                    text: serde_json::to_string_pretty(&res)?,
                }],
                is_error: None,
            })
        }

        "resume" => {
            let trajectory_id = match args.get("trajectory_id").and_then(Value::as_str) {
                Some(tid) => tid.to_string(),
                None => {
                    let recent = backend.list_trajectories(None, 1)?;
                    match recent.into_iter().next() {
                        Some(t) => t.id,
                        None => {
                            let res = json!({
                                "status": "ok",
                                "message": "No hay trayectorias de exploración registradas para reanudar.",
                                "resume_node": null
                            });
                            return Ok(ToolCallResult {
                                content: vec![ContentBlock {
                                    kind: "text",
                                    text: serde_json::to_string_pretty(&res)?,
                                }],
                                is_error: None,
                            });
                        }
                    }
                }
            };

            let node = backend.get_recommended_resume_node(&trajectory_id)?;
            let res = json!({
                "status": "ok",
                "trajectory_id": trajectory_id,
                "resume_node": node,
                "message": if node.is_some() {
                    "Nodo de reanudación identificado exitosamente (mayor valor estimado no podado)"
                } else {
                    "No hay nodos activos pendientes en la trayectoria (todos podados o marcados como solución)"
                }
            });

            Ok(ToolCallResult {
                content: vec![ContentBlock {
                    kind: "text",
                    text: serde_json::to_string_pretty(&res)?,
                }],
                is_error: None,
            })
        }

        "list" => {
            let project_path = args
                .get("project_path")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| backend.project_path());
            let limit = args
                .get("limit")
                .and_then(Value::as_u64)
                .unwrap_or(20) as usize;

            let list = backend.list_trajectories(project_path.as_deref(), limit)?;

            let res = json!({
                "status": "ok",
                "trajectories": list
            });

            Ok(ToolCallResult {
                content: vec![ContentBlock {
                    kind: "text",
                    text: serde_json::to_string_pretty(&res)?,
                }],
                is_error: None,
            })
        }

        "delete" => {
            let trajectory_id = args
                .get("trajectory_id")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("Falta 'trajectory_id' para eliminar"))?;

            backend.delete_trajectory(trajectory_id)?;

            let res = json!({
                "status": "ok",
                "trajectory_id": trajectory_id,
                "message": "Trayectoria eliminada en cascada"
            });

            Ok(ToolCallResult {
                content: vec![ContentBlock {
                    kind: "text",
                    text: serde_json::to_string_pretty(&res)?,
                }],
                is_error: None,
            })
        }

        other => Err(anyhow!("Acción de exploración desconocida: '{other}'")),
    }
}
