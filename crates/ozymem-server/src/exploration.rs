use anyhow::{anyhow, Result};
use ozymem_core::graph_backend::{
    ExplorationTreeNode, GraphBackend, RecordBatchParams, RecordNodeParams,
};
use ozymem_core::mcp_common::{ContentBlock, ToolCallParams, ToolCallResult};
use serde_json::{json, Value};

/// Resuelve el trajectory_id explícito o auto-detecta la trayectoria activa 'in_progress'
fn resolve_trajectory_id(
    backend: &GraphBackend,
    args: &serde_json::Map<String, Value>,
) -> Result<String> {
    if let Some(id) = args.get("trajectory_id").and_then(Value::as_str) {
        let trimmed = id.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }
    let project_path_owned = args
        .get("project_path")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| backend.project_path());
    if let Ok(Some(active)) = backend.get_active_trajectory(project_path_owned.as_deref()) {
        return Ok(active.id);
    }
    Err(anyhow!(
        "[ERROR: MISSING_TRAJECTORY_ID] No se proporcionó 'trajectory_id' y no hay ninguna trayectoria activa ('in_progress') en el proyecto. Inicia una con ozy_exploration(action=\"start\", task_description=\"...\")"
    ))
}

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
                .or_else(|| args.get("goal"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| anyhow!("[ERROR: MISSING_TASK_DESCRIPTION] Falta el parámetro 'task_description' o 'goal' para iniciar la trayectoria. Ejemplo: ozy_exploration(action=\"start\", goal=\"Implementar X\")"))?;
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

        "step" | "record_step" => {
            let trajectory_id = resolve_trajectory_id(backend, args)?;

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

            let rollback_snapshot = args
                .get("rollback_snapshot")
                .map(|v| if v.is_string() { v.as_str().unwrap().to_string() } else { v.to_string() });

            let execution_receipt = args
                .get("execution_receipt")
                .and_then(|v| serde_json::from_value::<ozymem_core::graph_backend::ObjectiveExecutionReceipt>(v.clone()).ok());

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
                rollback_snapshot,
                execution_receipt,
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
            let trajectory_id = resolve_trajectory_id(backend, args)?;

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

                let rollback_snapshot = s
                    .get("rollback_snapshot")
                    .map(|v| if v.is_string() { v.as_str().unwrap().to_string() } else { v.to_string() });

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
                    rollback_snapshot,
                    execution_receipt: None,
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

        "execute_and_record" => {
            let trajectory_id = resolve_trajectory_id(backend, args)?;
            let parent_id = args.get("parent_id").and_then(Value::as_str).map(String::from);
            let command = args.get("command").and_then(Value::as_str).unwrap_or("cargo");
            let cmd_args_raw = args.get("args").and_then(Value::as_array);
            let cmd_args: Vec<String> = if let Some(arr) = cmd_args_raw {
                arr.iter().filter_map(|v| v.as_str().map(String::from)).collect()
            } else {
                vec!["test".to_string()]
            };
            let cmd_args_refs: Vec<&str> = cmd_args.iter().map(|s| s.as_str()).collect();

            let working_dir_str = args.get("working_dir").and_then(Value::as_str).unwrap_or(".");
            let working_dir = std::path::Path::new(working_dir_str);

            let is_solution = args.get("is_solution").and_then(Value::as_bool);
            let observation = args.get("observation").and_then(Value::as_str).unwrap_or("Objective execution run").to_string();

            // Ejecución objetiva inmutable del comando en el sistema operativo
            let receipt = backend.execute_objective_evaluation(working_dir, command, &cmd_args_refs)?;

            let action_type = args.get("action_type").and_then(Value::as_str).unwrap_or("test_execution").to_string();
            let action_payload = json!({
                "command": command,
                "args": cmd_args,
                "working_dir": working_dir_str,
            }).to_string();

            let node = backend.record_exploration_node(RecordNodeParams {
                trajectory_id,
                parent_id,
                action_type,
                action_payload,
                observation,
                cost_tokens: Some(0),
                latency_ms: Some(receipt.duration_ms as i64),
                reward_score: None,
                is_solution,
                is_pruned: None,
                rollback_snapshot: None,
                execution_receipt: Some(receipt.clone()),
            })?;

            let res = json!({
                "status": "ok",
                "node": node,
                "receipt": receipt,
                "message": if receipt.passed {
                    "[OBJECTIVE: VERIFIED_EXECUTION_PASSED] Ejecución de tests verificada con éxito por el sistema operativo"
                } else {
                    "[ALERT: OBJECTIVE_EXECUTION_FAILED] Fallo de tests detectado objetivamente por código de salida del proceso"
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

        "shadow_metrics" | "shadow" => {
            let trajectory_id = resolve_trajectory_id(backend, args)?;
            let metrics = backend.get_shadow_metrics(&trajectory_id)?;

            let res = json!({
                "status": "ok",
                "trajectory_id": trajectory_id,
                "shadow_metrics": metrics,
                "message": if metrics.is_some() {
                    "[STATUS: SHADOW_MODE_ACTIVE] Métricas cuantitativas de exploración registradas en modo sombra"
                } else {
                    "[STATUS: NOT_FOUND] No se registraron métricas de modo sombra para esta trayectoria"
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

        "finish" | "complete" => {
            let trajectory_id = resolve_trajectory_id(backend, args)?;
            let status = args
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("completed");
            let final_reward = args.get("reward_score").and_then(Value::as_f64);

            backend.complete_trajectory(&trajectory_id, status, final_reward)?;
            let mermaid = backend.render_mermaid_tree(&trajectory_id).ok();

            let mut res = json!({
                "status": "ok",
                "trajectory_id": trajectory_id,
                "final_status": status,
                "message": "Trayectoria finalizada correctamente"
            });
            if let Some(ref m) = mermaid {
                res["mermaid"] = json!(m);
            }

            Ok(ToolCallResult {
                content: vec![ContentBlock {
                    kind: "text",
                    text: serde_json::to_string_pretty(&res)?,
                }],
                is_error: None,
            })
        }

        "get_tree" => {
            let trajectory_id = resolve_trajectory_id(backend, args)?;
            let lod = args
                .get("lod")
                .and_then(Value::as_str)
                .unwrap_or("contracts");

            let tree = backend.get_trajectory_tree(&trajectory_id)?;
            let filtered_tree: Vec<_> = tree.into_iter().map(|n| apply_lod_to_tree_node(n, lod)).collect();
            let mermaid = backend.render_mermaid_tree(&trajectory_id).unwrap_or_default();

            let res = json!({
                "status": "ok",
                "trajectory_id": trajectory_id,
                "lod": lod,
                "mermaid": mermaid,
                "tree": filtered_tree
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
            let trajectory_id = match resolve_trajectory_id(backend, args) {
                Ok(tid) => tid,
                Err(_) => {
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
            let trajectory_id = match resolve_trajectory_id(backend, args) {
                Ok(tid) => tid,
                Err(_) => {
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

        "rollback" => {
            let node_id = args
                .get("node_id")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("Falta el parámetro 'node_id' para ejecutar rollback"))?;

            let to_parent = args.get("to_parent").and_then(Value::as_bool).unwrap_or(false);
            let report = if to_parent {
                backend.rollback_to_parent(node_id)?
            } else {
                backend.rollback_node(node_id)?
            };

            let res = json!({
                "status": "ok",
                "rollback": report,
                "message": format!("[ROLLBACK] Proceso finalizado con estado '{}': {} archivos restaurados", report.status, report.restored_files.len())
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

fn apply_lod_to_tree_node(mut tree_node: ExplorationTreeNode, lod: &str) -> ExplorationTreeNode {
    match lod {
        "macro" => {
            tree_node.node.observation = String::new();
            if tree_node.node.action_payload.len() > 60 {
                tree_node.node.action_payload = format!("{}...", &tree_node.node.action_payload[..60]);
            }
            tree_node.node.rollback_snapshot = None;
        }
        "contracts" => {
            if tree_node.node.observation.len() > 200 {
                let omitted = tree_node.node.observation.len() - 200;
                tree_node.node.observation = format!("{}... [omitted {} chars]", &tree_node.node.observation[..200], omitted);
            }
            tree_node.node.rollback_snapshot = None;
        }
        _ => {}
    }
    tree_node.children = tree_node
        .children
        .into_iter()
        .map(|c| apply_lod_to_tree_node(c, lod))
        .collect();
    tree_node
}
