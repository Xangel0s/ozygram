use anyhow::Result;
use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;
use rusqlite::params;
use std::collections::{HashMap, HashSet, VecDeque};
use std::str::FromStr;

use crate::graph_backend::types::{
    AutoWireReport, EndpointDependentsReport, GraphBackend, GraphEntityNode, GraphEntityType,
    MemoryEdgeRecord, MemoryEdgeType, MemoryGraphEdge, MemoryNodeRecord, NeighborhoodResult,
    TableLineageReport,
};

impl GraphBackend {
    /// Clave canónica única para indexar entidades en RAM: "tipo:id"
    #[inline]
    fn make_entity_key(entity_type: &GraphEntityType, id: &str) -> String {
        format!("{}:{}", entity_type, id)
    }

    /// Carga el grafo de memorias y aristas desde SQLite directamente a la memoria RAM (petgraph).
    pub fn load_memory_graph_to_ram(&self) -> Result<()> {
        let mut inner = self.inner.lock().unwrap();
        inner.memory_graph.clear();
        inner.memory_index.clear();

        // 1. Cargar nodos de memoria dentro de un scope para liberar el borrow de SQLite
        let nodes: Vec<(String, String, String, f64, bool)> = {
            let mut stmt = inner.sqlite.prepare(
                "SELECT id, kind, title, confidence_score, stale
                 FROM memory_nodes WHERE tenant_id = ?1",
            )?;

            let rows = stmt.query_map(params![self.tenant_id], |row| {
                let id: String = row.get(0)?;
                let kind: String = row.get(1)?;
                let title: String = row.get(2)?;
                let confidence_score: f64 = row.get(3)?;
                let stale: i64 = row.get(4)?;
                Ok((id, kind, title, confidence_score, stale != 0))
            })?;
            let items: Vec<_> = rows.filter_map(|r| r.ok()).collect();
            items
        };

        for (id, kind, title, conf, stale) in nodes {
            let key = Self::make_entity_key(&GraphEntityType::Memory, &id);
            let idx = inner.memory_graph.add_node(GraphEntityNode {
                entity_type: GraphEntityType::Memory,
                id,
                title,
                kind,
                confidence_score: conf,
                stale,
            });
            inner.memory_index.insert(key, idx);
        }

        // 2. Cargar aristas de memoria dentro de un scope para liberar el borrow de SQLite
        let raw_edges: Vec<(String, String, String, String, String, f64)> = {
            let mut edge_stmt = inner.sqlite.prepare(
                "SELECT source_type, source_id, target_type, target_id, edge_type, weight
                 FROM memory_edges WHERE tenant_id = ?1",
            )?;

            let rows = edge_stmt.query_map(params![self.tenant_id], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            })?;
            let items: Vec<_> = rows.filter_map(|r| r.ok()).collect();
            items
        };

        for (src_type_str, src_id, dst_type_str, dst_id, edge_type_str, weight) in raw_edges {
            let src_type = GraphEntityType::from_str(&src_type_str).unwrap_or(GraphEntityType::Memory);
            let dst_type = GraphEntityType::from_str(&dst_type_str).unwrap_or(GraphEntityType::File);
            let edge_type = MemoryEdgeType::from_str(&edge_type_str).unwrap_or(MemoryEdgeType::AppliesTo);

            let src_key = Self::make_entity_key(&src_type, &src_id);
            let dst_key = Self::make_entity_key(&dst_type, &dst_id);

            // Asegurar que el nodo origen exista en RAM
            let src_idx = match inner.memory_index.get(&src_key) {
                Some(&idx) => idx,
                None => {
                    let idx = inner.memory_graph.add_node(GraphEntityNode {
                        entity_type: src_type,
                        id: src_id.clone(),
                        title: src_id.clone(),
                        kind: "unknown".to_string(),
                        confidence_score: 1.0,
                        stale: false,
                    });
                    inner.memory_index.insert(src_key, idx);
                    idx
                }
            };

            // Asegurar que el nodo destino exista en RAM
            let dst_idx = match inner.memory_index.get(&dst_key) {
                Some(&idx) => idx,
                None => {
                    let idx = inner.memory_graph.add_node(GraphEntityNode {
                        entity_type: dst_type,
                        id: dst_id.clone(),
                        title: dst_id.clone(),
                        kind: "unknown".to_string(),
                        confidence_score: 1.0,
                        stale: false,
                    });
                    inner.memory_index.insert(dst_key, idx);
                    idx
                }
            };

            inner.memory_graph.add_edge(
                src_idx,
                dst_idx,
                MemoryGraphEdge {
                    edge_type,
                    weight,
                },
            );
        }

        Ok(())
    }

    /// Inserta o actualiza un nodo de memoria en SQLite y en el grafo RAM.
    pub fn insert_memory_node(&self, node: &MemoryNodeRecord) -> Result<String> {
        let mut inner = self.inner.lock().unwrap();
        inner.sqlite.execute(
            "INSERT INTO memory_nodes (
                id, kind, title, content, error_context, solution,
                confidence_score, touch_count, stale, stale_reason,
                created_at, last_verified_at, tenant_id, workspace_root
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
            ON CONFLICT(id) DO UPDATE SET
                kind = excluded.kind,
                title = excluded.title,
                content = excluded.content,
                error_context = excluded.error_context,
                solution = excluded.solution,
                confidence_score = excluded.confidence_score,
                touch_count = excluded.touch_count,
                stale = excluded.stale,
                stale_reason = excluded.stale_reason,
                last_verified_at = excluded.last_verified_at,
                workspace_root = excluded.workspace_root",
            params![
                node.id,
                node.kind,
                node.title,
                node.content,
                node.error_context,
                node.solution,
                node.confidence_score,
                node.touch_count,
                node.stale,
                node.stale_reason,
                node.created_at,
                node.last_verified_at,
                node.tenant_id,
                node.workspace_root
            ],
        )?;

        // Actualizar en el grafo en RAM
        let key = Self::make_entity_key(&GraphEntityType::Memory, &node.id);
        if let Some(&existing_idx) = inner.memory_index.get(&key) {
            if let Some(weight) = inner.memory_graph.node_weight_mut(existing_idx) {
                weight.title = node.title.clone();
                weight.kind = node.kind.clone();
                weight.confidence_score = node.confidence_score;
                weight.stale = node.stale != 0;
            }
        } else {
            let idx = inner.memory_graph.add_node(GraphEntityNode {
                entity_type: GraphEntityType::Memory,
                id: node.id.clone(),
                title: node.title.clone(),
                kind: node.kind.clone(),
                confidence_score: node.confidence_score,
                stale: node.stale != 0,
            });
            inner.memory_index.insert(key, idx);
        }

        Ok(node.id.clone())
    }

    /// Obtiene un nodo de memoria por su ID.
    pub fn get_memory_node(&self, id: &str) -> Result<Option<MemoryNodeRecord>> {
        let inner = self.inner.lock().unwrap();
        let mut stmt = inner.sqlite.prepare(
            "SELECT id, kind, title, content, error_context, solution,
                    confidence_score, touch_count, stale, stale_reason,
                    created_at, last_verified_at, tenant_id, workspace_root
             FROM memory_nodes WHERE id = ?1 AND tenant_id = ?2",
        )?;

        let mut rows = stmt.query(params![id, self.tenant_id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(MemoryNodeRecord {
                id: row.get(0)?,
                kind: row.get(1)?,
                title: row.get(2)?,
                content: row.get(3)?,
                error_context: row.get(4)?,
                solution: row.get(5)?,
                confidence_score: row.get(6)?,
                touch_count: row.get(7)?,
                stale: row.get(8)?,
                stale_reason: row.get(9)?,
                created_at: row.get(10)?,
                last_verified_at: row.get(11)?,
                tenant_id: row.get(12)?,
                workspace_root: row.get(13)?,
            }))
        } else {
            Ok(None)
        }
    }

    /// Lista nodos de memoria aplicando filtros opcionales.
    pub fn list_memory_nodes(&self, kind_filter: Option<&str>, stale_only: bool) -> Result<Vec<MemoryNodeRecord>> {
        let inner = self.inner.lock().unwrap();
        let mut sql = "SELECT id, kind, title, content, error_context, solution,
                              confidence_score, touch_count, stale, stale_reason,
                              created_at, last_verified_at, tenant_id, workspace_root
                       FROM memory_nodes WHERE tenant_id = ?1".to_string();

        if let Some(k) = kind_filter {
            sql.push_str(&format!(" AND kind = '{}'", k.replace('\'', "''")));
        }
        if stale_only {
            sql.push_str(" AND stale != 0");
        }
        sql.push_str(" ORDER BY created_at DESC");

        let mut stmt = inner.sqlite.prepare(&sql)?;
        let rows = stmt.query_map(params![self.tenant_id], |row| {
            Ok(MemoryNodeRecord {
                id: row.get(0)?,
                kind: row.get(1)?,
                title: row.get(2)?,
                content: row.get(3)?,
                error_context: row.get(4)?,
                solution: row.get(5)?,
                confidence_score: row.get(6)?,
                touch_count: row.get(7)?,
                stale: row.get(8)?,
                stale_reason: row.get(9)?,
                created_at: row.get(10)?,
                last_verified_at: row.get(11)?,
                tenant_id: row.get(12)?,
                workspace_root: row.get(13)?,
            })
        })?;

        let mut result = Vec::new();
        for r in rows {
            result.push(r?);
        }
        Ok(result)
    }

    /// Elimina un nodo de memoria y sus aristas asociadas tanto en SQLite como en RAM.
    pub fn delete_memory_node(&self, id: &str) -> Result<bool> {
        let mut inner = self.inner.lock().unwrap();
        let affected = inner.sqlite.execute(
            "DELETE FROM memory_nodes WHERE id = ?1 AND tenant_id = ?2",
            params![id, self.tenant_id],
        )?;
        inner.sqlite.execute(
            "DELETE FROM memory_edges WHERE ((source_type = 'memory' AND source_id = ?1) OR (target_type = 'memory' AND target_id = ?1)) AND tenant_id = ?2",
            params![id, self.tenant_id],
        )?;

        // Eliminar del grafo en RAM
        let key = Self::make_entity_key(&GraphEntityType::Memory, id);
        if let Some(node_idx) = inner.memory_index.remove(&key) {
            inner.memory_graph.remove_node(node_idx);
        }

        Ok(affected > 0)
    }

    /// Inserta o actualiza una arista de memoria tipada en SQLite y en RAM.
    pub fn insert_memory_edge(&self, edge: &MemoryEdgeRecord) -> Result<()> {
        let mut inner = self.inner.lock().unwrap();
        inner.sqlite.execute(
            "INSERT INTO memory_edges (
                source_type, source_id, target_type, target_id, edge_type, weight, created_at, tenant_id, workspace_root
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            ON CONFLICT(source_type, source_id, target_type, target_id, edge_type, tenant_id) DO UPDATE SET
                weight = excluded.weight,
                workspace_root = excluded.workspace_root",
            params![
                edge.source_type.to_string(),
                edge.source_id,
                edge.target_type.to_string(),
                edge.target_id,
                edge.edge_type.to_string(),
                edge.weight,
                edge.created_at,
                edge.tenant_id,
                edge.workspace_root
            ],
        )?;

        // Actualizar en el grafo en RAM
        let src_key = Self::make_entity_key(&edge.source_type, &edge.source_id);
        let dst_key = Self::make_entity_key(&edge.target_type, &edge.target_id);

        let src_idx = match inner.memory_index.get(&src_key) {
            Some(&idx) => idx,
            None => {
                let idx = inner.memory_graph.add_node(GraphEntityNode {
                    entity_type: edge.source_type.clone(),
                    id: edge.source_id.clone(),
                    title: edge.source_id.clone(),
                    kind: "entity".to_string(),
                    confidence_score: 1.0,
                    stale: false,
                });
                inner.memory_index.insert(src_key, idx);
                idx
            }
        };

        let dst_idx = match inner.memory_index.get(&dst_key) {
            Some(&idx) => idx,
            None => {
                let idx = inner.memory_graph.add_node(GraphEntityNode {
                    entity_type: edge.target_type.clone(),
                    id: edge.target_id.clone(),
                    title: edge.target_id.clone(),
                    kind: "entity".to_string(),
                    confidence_score: 1.0,
                    stale: false,
                });
                inner.memory_index.insert(dst_key, idx);
                idx
            }
        };

        // Eliminar arista previa idéntica si existía para evitar duplicados en RAM
        let existing_edge = inner
            .memory_graph
            .edges_connecting(src_idx, dst_idx)
            .find(|e| e.weight().edge_type == edge.edge_type)
            .map(|e| e.id());

        if let Some(edge_id) = existing_edge {
            inner.memory_graph.remove_edge(edge_id);
        }

        inner.memory_graph.add_edge(
            src_idx,
            dst_idx,
            MemoryGraphEdge {
                edge_type: edge.edge_type.clone(),
                weight: edge.weight,
            },
        );

        Ok(())
    }

    /// Elimina una arista de memoria específica en SQLite y en RAM.
    pub fn delete_memory_edge(
        &self,
        source_type: &GraphEntityType,
        source_id: &str,
        target_type: &GraphEntityType,
        target_id: &str,
        edge_type: &MemoryEdgeType,
    ) -> Result<bool> {
        let mut inner = self.inner.lock().unwrap();
        let affected = inner.sqlite.execute(
            "DELETE FROM memory_edges 
             WHERE source_type = ?1 AND source_id = ?2 
               AND target_type = ?3 AND target_id = ?4 
               AND edge_type = ?5 AND tenant_id = ?6",
            params![
                source_type.to_string(),
                source_id,
                target_type.to_string(),
                target_id,
                edge_type.to_string(),
                self.tenant_id
            ],
        )?;

        let src_key = Self::make_entity_key(source_type, source_id);
        let dst_key = Self::make_entity_key(target_type, target_id);

        if let (Some(&src_idx), Some(&dst_idx)) = (inner.memory_index.get(&src_key), inner.memory_index.get(&dst_key)) {
            let edge_to_remove = inner
                .memory_graph
                .edges_connecting(src_idx, dst_idx)
                .find(|e| e.weight().edge_type == *edge_type)
                .map(|e| e.id());

            if let Some(edge_id) = edge_to_remove {
                inner.memory_graph.remove_edge(edge_id);
            }
        }

        Ok(affected > 0)
    }

    /// Obtiene las aristas conectadas (entrantes y salientes) a una entidad desde SQLite.
    pub fn list_edges_for_node(&self, entity_type: &GraphEntityType, entity_id: &str) -> Result<Vec<MemoryEdgeRecord>> {
        let inner = self.inner.lock().unwrap();
        let type_str = entity_type.to_string();
        let mut stmt = inner.sqlite.prepare(
            "SELECT source_type, source_id, target_type, target_id, edge_type, weight, created_at, tenant_id, workspace_root
             FROM memory_edges 
             WHERE ((source_type = ?1 AND source_id = ?2) OR (target_type = ?1 AND target_id = ?2))
               AND tenant_id = ?3
             ORDER BY weight DESC, created_at DESC",
        )?;

        let rows = stmt.query_map(params![type_str, entity_id, self.tenant_id], |row| {
            let src_t: String = row.get(0)?;
            let dst_t: String = row.get(2)?;
            let edge_t: String = row.get(4)?;
            Ok(MemoryEdgeRecord {
                source_type: GraphEntityType::from_str(&src_t).unwrap_or(GraphEntityType::Memory),
                source_id: row.get(1)?,
                target_type: GraphEntityType::from_str(&dst_t).unwrap_or(GraphEntityType::File),
                target_id: row.get(3)?,
                edge_type: MemoryEdgeType::from_str(&edge_t).unwrap_or(MemoryEdgeType::AppliesTo),
                weight: row.get(5)?,
                created_at: row.get(6)?,
                tenant_id: row.get(7)?,
                workspace_root: row.get(8)?,
            })
        })?;

        let mut result = Vec::new();
        for r in rows {
            result.push(r?);
        }
        Ok(result)
    }

    /// [TASK 2.2] Algoritmo de Recorrido BFS con Decaimiento de Relevancia (profundidad <= 2).
    ///
    /// Realiza un recorrido multi-hop en memoria RAM desde la entidad objetivo, atenuando el peso
    /// según la distancia (factor 0.6 por salto) y evitando ciclos o explosión combinatoria.
    pub fn get_memory_neighborhood(
        &self,
        entity_type: &GraphEntityType,
        entity_id: &str,
        max_depth: usize,
    ) -> Result<Vec<NeighborhoodResult>> {
        let inner = self.inner.lock().unwrap();
        let key = Self::make_entity_key(entity_type, entity_id);

        let Some(&start_idx) = inner.memory_index.get(&key) else {
            return Ok(vec![]);
        };

        let clamped_depth = max_depth.clamp(1, 3);
        let mut results = Vec::new();
        let mut visited: HashMap<NodeIndex, usize> = HashMap::new();
        let mut queue: VecDeque<(NodeIndex, usize, f64)> = VecDeque::new();

        visited.insert(start_idx, 0);
        queue.push_back((start_idx, 0, 1.0));

        while let Some((curr_idx, curr_depth, _parent_weight)) = queue.pop_front() {
            if curr_depth >= clamped_depth {
                continue;
            }

            let next_depth = curr_depth + 1;
            let decay_factor = 0.6_f64.powi((next_depth - 1) as i32);

            // 1. Explorar aristas salientes
            for edge in inner.memory_graph.edges_directed(curr_idx, petgraph::Direction::Outgoing) {
                let target_idx = edge.target();
                let edge_weight = edge.weight().weight;
                let effective_weight = edge_weight * decay_factor;

                let is_new = !visited.contains_key(&target_idx) || visited.get(&target_idx).copied().unwrap_or(usize::MAX) > next_depth;
                if is_new {
                    visited.insert(target_idx, next_depth);
                    queue.push_back((target_idx, next_depth, effective_weight));

                    if let Some(target_entity) = inner.memory_graph.node_weight(target_idx) {
                        results.push(NeighborhoodResult {
                            entity: target_entity.clone(),
                            edge_type: edge.weight().edge_type.clone(),
                            depth: next_depth,
                            effective_weight,
                            direction: "outgoing".to_string(),
                        });
                    }
                }
            }

            // 2. Explorar aristas entrantes (bidireccional para descubrimiento contextual)
            for edge in inner.memory_graph.edges_directed(curr_idx, petgraph::Direction::Incoming) {
                let source_idx = edge.source();
                let edge_weight = edge.weight().weight;
                let effective_weight = edge_weight * decay_factor;

                let is_new = !visited.contains_key(&source_idx) || visited.get(&source_idx).copied().unwrap_or(usize::MAX) > next_depth;
                if is_new {
                    visited.insert(source_idx, next_depth);
                    queue.push_back((source_idx, next_depth, effective_weight));

                    if let Some(source_entity) = inner.memory_graph.node_weight(source_idx) {
                        results.push(NeighborhoodResult {
                            entity: source_entity.clone(),
                            edge_type: edge.weight().edge_type.clone(),
                            depth: next_depth,
                            effective_weight,
                            direction: "incoming".to_string(),
                        });
                    }
                }
            }
        }

        // Ordenar por peso efectivo descendente (los más fuertemente vinculados primero)
        results.sort_by(|a, b| {
            b.effective_weight
                .partial_cmp(&a.effective_weight)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        Ok(results)
    }

    /// [TASK 2.3] Propagación de Invalidación en Cascada (Stale Propagation).
    ///
    /// Cuando un archivo es modificado físicamente en disco o eliminado:
    /// 1. Localiza el nodo de archivo en el grafo en RAM.
    /// 2. Propaga el estado `stale = true` a todas las memorias que lo gobiernan (`APPLIES_TO`, `COUPLED_WITH`).
    /// 3. Propaga recursivamente a memorias aguas abajo (`SUPERSEDES`, `DERIVED_FROM`).
    /// 4. Sincroniza atómicamente el estado en SQLite.
    pub fn propagate_stale_invalidation(&self, file_path: &str, reason: &str) -> Result<Vec<String>> {
        let mut inner = self.inner.lock().unwrap();
        let file_key = Self::make_entity_key(&GraphEntityType::File, file_path);

        let Some(&file_idx) = inner.memory_index.get(&file_key) else {
            return Ok(vec![]);
        };

        let mut invalidated_ids = Vec::new();
        let mut queue: VecDeque<NodeIndex> = VecDeque::new();
        let mut visited: HashSet<NodeIndex> = HashSet::new();

        // Encontrar memorias que apuntan directamente al archivo
        for edge in inner.memory_graph.edges_directed(file_idx, petgraph::Direction::Incoming) {
            let src_idx = edge.source();
            if let Some(entity) = inner.memory_graph.node_weight(src_idx) {
                if entity.entity_type == GraphEntityType::Memory {
                    queue.push_back(src_idx);
                    visited.insert(src_idx);
                }
            }
        }

        // También revisar si hay aristas salientes hacia el archivo
        for edge in inner.memory_graph.edges_directed(file_idx, petgraph::Direction::Outgoing) {
            let dst_idx = edge.target();
            if let Some(entity) = inner.memory_graph.node_weight(dst_idx) {
                if entity.entity_type == GraphEntityType::Memory {
                    queue.push_back(dst_idx);
                    visited.insert(dst_idx);
                }
            }
        }

        let alert_msg = format!("[ALERT: STALE_MEMORY: {}]", reason);

        while let Some(curr_idx) = queue.pop_front() {
            let mem_id = if let Some(entity) = inner.memory_graph.node_weight_mut(curr_idx) {
                entity.stale = true;
                entity.confidence_score = (entity.confidence_score * 0.5).max(0.1);
                entity.id.clone()
            } else {
                continue;
            };

            invalidated_ids.push(mem_id.clone());

            // Propagar a memorias dependientes vinculadas por SUPERSEDES o DERIVED_FROM
            for edge in inner.memory_graph.edges_directed(curr_idx, petgraph::Direction::Outgoing) {
                let edge_type = &edge.weight().edge_type;
                if *edge_type == MemoryEdgeType::Supersedes || *edge_type == MemoryEdgeType::DerivedFrom {
                    let next_idx = edge.target();
                    if visited.insert(next_idx) {
                        queue.push_back(next_idx);
                    }
                }
            }
        }

        // Actualizar en SQLite en lote
        for id in &invalidated_ids {
            let _ = inner.sqlite.execute(
                "UPDATE memory_nodes 
                 SET stale = 1, stale_reason = ?1 
                 WHERE id = ?2 AND tenant_id = ?3",
                params![alert_msg, id, self.tenant_id],
            );
        }

        Ok(invalidated_ids)
    }

    /// [TASK 4.1] Auto-Wiring Autónomo durante Dream-RSI.
    ///
    /// Analiza pares de nodos de memoria activos, calcula su similitud semántica
    /// e infiere automáticamente aristas tipadas REINFORCES o SUPERSEDES.
    pub fn auto_wire_memories(&self, similarity_threshold: f64) -> Result<AutoWireReport> {
        let memories = self.list_memory_nodes(None, false)?;
        if memories.len() < 2 {
            return Ok(AutoWireReport {
                edges_created: 0,
                reinforces_count: 0,
                supersedes_count: 0,
                details: vec!["Menos de 2 memorias registradas, omision de auto-wiring.".to_string()],
            });
        }

        let threshold = similarity_threshold.clamp(0.2, 0.99);
        let mut report = AutoWireReport {
            edges_created: 0,
            reinforces_count: 0,
            supersedes_count: 0,
            details: Vec::new(),
        };

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            .to_string();

        for i in 0..memories.len() {
            for j in (i + 1)..memories.len() {
                let m1 = &memories[i];
                let m2 = &memories[j];

                let text1 = format!("{} {} {}", m1.title, m1.error_context, m1.solution).to_lowercase();
                let text2 = format!("{} {} {}", m2.title, m2.error_context, m2.solution).to_lowercase();

                let words1: HashSet<&str> = text1.split_whitespace().filter(|w| w.len() > 3).collect();
                let words2: HashSet<&str> = text2.split_whitespace().filter(|w| w.len() > 3).collect();

                let intersection = words1.intersection(&words2).count();
                let union = words1.union(&words2).count();

                let sim = if union > 0 {
                    intersection as f64 / union as f64
                } else {
                    0.0
                };

                if sim >= threshold {
                    let text1_has_obsolete = text1.contains("deprecated") || text1.contains("obsoleto") || text1.contains("antiguo") || text1.contains("no usar");
                    let text2_has_obsolete = text2.contains("deprecated") || text2.contains("obsoleto") || text2.contains("antiguo") || text2.contains("no usar");

                    let (source, target, edge_type, weight, desc) = if text2_has_obsolete && !text1_has_obsolete {
                        (
                            m1.id.clone(),
                            m2.id.clone(),
                            MemoryEdgeType::Supersedes,
                            1.0,
                            format!("[SUPERSEDES] {} reemplaza a {}", m1.id, m2.id)
                        )
                    } else if text1_has_obsolete && !text2_has_obsolete {
                        (
                            m2.id.clone(),
                            m1.id.clone(),
                            MemoryEdgeType::Supersedes,
                            1.0,
                            format!("[SUPERSEDES] {} reemplaza a {}", m2.id, m1.id)
                        )
                    } else {
                        (
                            m1.id.clone(),
                            m2.id.clone(),
                            MemoryEdgeType::Reinforces,
                            sim,
                            format!("[REINFORCES] {} refuerza a {} (sim: {:.2})", m1.id, m2.id, sim)
                        )
                    };

                    let edge = MemoryEdgeRecord {
                        source_type: GraphEntityType::Memory,
                        source_id: source,
                        target_type: GraphEntityType::Memory,
                        target_id: target,
                        edge_type: edge_type.clone(),
                        weight,
                        created_at: now.clone(),
                        tenant_id: self.tenant_id.clone(),
                        workspace_root: m1.workspace_root.clone(),
                    };

                    if self.insert_memory_edge(&edge).is_ok() {
                        report.edges_created += 1;
                        if edge_type == MemoryEdgeType::Reinforces {
                            report.reinforces_count += 1;
                        } else if edge_type == MemoryEdgeType::Supersedes {
                            report.supersedes_count += 1;
                        }
                        report.details.push(desc);
                    }
                }
            }
        }

        Ok(report)
    }

    /// [TASK 8.3] Purga y atenuación de memorias decaídas temporalmente.
    /// Marca como obsoletas en SQLite y en RAM las notas temporales con baja confianza o inactivas.
    pub fn prune_decayed_memories(&self, decay_threshold: f64, older_than_days: i64) -> Result<usize> {
        let mut inner = self.inner.lock().unwrap();
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let max_age_secs = (older_than_days.max(1) as u64) * 86400;

        let to_stale: Vec<String> = {
            let mut stmt = inner.sqlite.prepare(
                "SELECT id, created_at, confidence_score FROM memory_nodes 
                 WHERE tenant_id = ?1 AND stale = 0",
            )?;
            let rows = stmt.query_map(params![self.tenant_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, f64>(2)?,
                ))
            })?;

            let mut list = Vec::new();
            for item in rows.flatten() {
                let (id, created_at, conf) = item;
                let created_secs = created_at.parse::<u64>().unwrap_or(0);
                let age_secs = if created_secs > 0 {
                    now_secs.saturating_sub(created_secs)
                } else {
                    0
                };
                if conf < decay_threshold || (age_secs > max_age_secs && conf <= 0.5) {
                    list.push(id);
                }
            }
            list
        };

        let mut count = 0;
        for id in to_stale {
            inner.sqlite.execute(
                "UPDATE memory_nodes SET stale = 1, stale_reason = 'TEMPORAL_MEMORY_DECAY' WHERE id = ?1 AND tenant_id = ?2",
                params![id, self.tenant_id],
            )?;
            let key = Self::make_entity_key(&GraphEntityType::Memory, &id);
            if let Some(&idx) = inner.memory_index.get(&key) {
                if let Some(w) = inner.memory_graph.node_weight_mut(idx) {
                    w.stale = true;
                }
            }
            count += 1;
        }

        let _ = inner.sqlite.execute(
            "DELETE FROM ozy_observations WHERE tenant_id = ?1 AND is_deleted = 1",
            params![self.tenant_id],
        );

        Ok(count)
    }

    /// [TASK 8.3] Vacunación Postmortem de Regresiones desde commits fix: y revert.
    /// Extrae causas raíz e inyecta nodos permanentes INVARIANT[...] conectados por AppliesTo.
    pub fn vaccinate_postmortems(&self, project_path: Option<&std::path::Path>) -> Result<usize> {
        let root = project_path
            .map(|p| p.to_path_buf())
            .or_else(|| self.project_path().map(std::path::PathBuf::from))
            .unwrap_or_else(|| std::path::PathBuf::from("."));

        let output = std::process::Command::new("git")
            .current_dir(&root)
            .args([
                "log",
                "-n",
                "30",
                "--grep=^fix",
                "--grep=^revert",
                "--format=%h|%s",
            ])
            .output();

        let Ok(out) = output else {
            return Ok(0);
        };
        if !out.status.success() {
            return Ok(0);
        }

        let stdout = String::from_utf8_lossy(&out.stdout);
        let mut vaccinated = 0;

        for line in stdout.lines() {
            let parts: Vec<&str> = line.splitn(2, '|').collect();
            if parts.len() < 2 {
                continue;
            }
            let hash = parts[0].trim();
            let subject = parts[1].trim();
            if subject.is_empty() {
                continue;
            }

            let slug: String = subject
                .chars()
                .map(|c| if c.is_alphanumeric() || c == '_' || c == ':' { c } else { '_' })
                .collect();
            let invariant_id = format!("INVARIANT[{}]", slug);

            if self.get_memory_node(&invariant_id)?.is_some() {
                continue;
            }

            let diff_files_output = std::process::Command::new("git")
                .current_dir(&root)
                .args(["diff-tree", "--no-commit-id", "--name-only", "-r", "--root", hash])
                .output();

            let touched_files: Vec<String> = if let Ok(d_out) = diff_files_output {
                String::from_utf8_lossy(&d_out.stdout)
                    .lines()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            } else {
                Vec::new()
            };

            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs()
                .to_string();

            let node = MemoryNodeRecord {
                id: invariant_id.clone(),
                kind: "invariant".to_string(),
                title: subject.to_string(),
                content: format!("[POSTMORTEM_INVARIANT] Regresion vacunada desde commit {}: {}", hash, subject),
                error_context: format!("Git commit: {}", hash),
                solution: subject.to_string(),
                confidence_score: 1.0,
                touch_count: 1,
                stale: 0,
                stale_reason: None,
                created_at: now.clone(),
                last_verified_at: now.clone(),
                tenant_id: self.tenant_id.clone(),
                workspace_root: self.project_path().unwrap_or_default(),
            };

            self.insert_memory_node(&node)?;
            vaccinated += 1;

            for file_path in touched_files {
                let norm_file = crate::normalize_path(&file_path);
                let edge = MemoryEdgeRecord {
                    source_type: GraphEntityType::Memory,
                    source_id: invariant_id.clone(),
                    target_type: GraphEntityType::File,
                    target_id: norm_file,
                    edge_type: MemoryEdgeType::AppliesTo,
                    weight: 1.0,
                    created_at: now.clone(),
                    tenant_id: self.tenant_id.clone(),
                    workspace_root: self.project_path().unwrap_or_default(),
                };
                let _ = self.insert_memory_edge(&edge);
            }
        }

        Ok(vaccinated)
    }

    /// [TASK 4.3] Renderizado Visual Mermaid del Grafo de Memorias.
    ///
    /// Genera un diagrama Mermaid sintacticamente valido sin emojis, utilizando insignias
    /// textuales ([CONVENTION], [APPLIES_TO], etc.) y filtrado opcional por modulo.
    pub fn render_mermaid_memory_graph(&self, module_filter: Option<&str>) -> Result<String> {
        let inner = self.inner.lock().unwrap();
        let mut mermaid = String::from("graph TD\n");
        let mut node_mermaid_ids: HashMap<NodeIndex, String> = HashMap::new();
        let mut counter = 0;

        // 1. Renderizar nodos
        for idx in inner.memory_graph.node_indices() {
            if let Some(node) = inner.memory_graph.node_weight(idx) {
                if let Some(mf) = module_filter {
                    let needle = mf.to_lowercase();
                    if !node.id.to_lowercase().contains(&needle)
                        && !node.title.to_lowercase().contains(&needle) {
                        continue;
                    }
                }

                let safe_id = format!("node_{}", counter);
                counter += 1;
                node_mermaid_ids.insert(idx, safe_id.clone());

                let kind_tag = node.kind.to_uppercase();
                let status_tag = if node.stale { "[STALE] " } else { "" };
                let sanitized_title = node.title.replace('"', "'").replace(['\n', '\r'], " ");
                let truncated_title = if sanitized_title.len() > 40 {
                    format!("{}...", &sanitized_title[..37])
                } else {
                    sanitized_title
                };

                let label = format!("[{}]{} {}", kind_tag, status_tag, truncated_title);
                mermaid.push_str(&format!("    {}[\"{}\"]\n", safe_id, label));
            }
        }

        // 2. Renderizar aristas entre nodos incluidos
        for edge in inner.memory_graph.edge_references() {
            let src_idx = edge.source();
            let dst_idx = edge.target();

            if let (Some(src_id), Some(dst_id)) = (node_mermaid_ids.get(&src_idx), node_mermaid_ids.get(&dst_idx)) {
                let edge_type_label = edge.weight().edge_type.to_string().to_uppercase();
                mermaid.push_str(&format!("    {} -->|{}| {}\n", src_id, edge_type_label, dst_id));
            }
        }

        if node_mermaid_ids.is_empty() {
            mermaid.push_str("    empty[\"[EMPTY: No matching graph nodes found]\"]\n");
        }

        Ok(mermaid)
    }

    /// Render a dependency-impact Mermaid diagram for `file_path`.
    ///
    /// Produces a Zero-Emoji compliant textual `graph TD` showing:
    /// - The **focal file** (hexagon node, labelled `[FOCAL]`)
    /// - **Outgoing deps** (files the focal imports) up to `max_hops` — labelled `[IMPORTS]`
    /// - **Incoming deps** (files that import the focal) up to `max_hops` — labelled `[IMPACTED]`
    ///
    /// Severity badges on impacted nodes: `[STATUS: BREAKING]`, `[STATUS: WARNING]`,
    /// `[STATUS: ACTIVE]` — determined from `ImpactEntry.severity`.
    ///
    /// All node labels are capped at 50 chars and stripped of quotes/newlines to
    /// remain valid Mermaid syntax.  No emojis are used anywhere (Zero-Emoji standard).
    pub fn render_mermaid_impact_graph(
        &self,
        file_path: &str,
        max_hops: usize,
    ) -> Result<String> {
        // ---------------------------------------------------------------
        // 1. Collect outgoing (what file_path imports) via analyze_impact
        // ---------------------------------------------------------------
        let outgoing: Vec<crate::graph_backend::types::ImpactEntry> =
            self.analyze_impact(file_path, max_hops as u32);

        // ---------------------------------------------------------------
        // 2. Collect incoming (who imports file_path) via BFS reverse
        // ---------------------------------------------------------------
        let incoming: Vec<crate::graph_backend::types::IncomingDependencyDetail> =
            self.get_incoming_dependencies_detailed(file_path, max_hops);

        // ---------------------------------------------------------------
        // 3. Build safe node IDs (Mermaid does not accept slashes / dots)
        // ---------------------------------------------------------------
        let sanitize_id = |s: &str| -> String {
            s.replace(['/', '\\', '.', '-', ' ', ':'], "_")
                .trim_matches('_')
                .to_string()
        };

        let sanitize_label = |s: &str, max: usize| -> String {
            let cleaned = s.replace('"', "'").replace(['\n', '\r'], " ");
            if cleaned.len() > max {
                format!("{}...", &cleaned[..max.saturating_sub(3)])
            } else {
                cleaned
            }
        };

        // Derive a short display name from the full path (last 2 path segments)
        let short_name = |path: &str| -> String {
            let segs: Vec<&str> = path.split(['/', '\\']).filter(|s| !s.is_empty()).collect();
            match segs.len() {
                0 => path.to_string(),
                1 => segs[0].to_string(),
                n => format!("{}/{}", segs[n - 2], segs[n - 1]),
            }
        };

        let focal_id = format!("focal_{}", sanitize_id(file_path));
        let focal_label = sanitize_label(&short_name(file_path), 50);

        let mut mermaid = String::from("graph TD\n");
        let mut declared: HashSet<String> = HashSet::new();
        let mut edges: Vec<String> = Vec::new();

        // ---------------------------------------------------------------
        // 4. Focal node — hexagon shape {{...}}
        // ---------------------------------------------------------------
        mermaid.push_str(&format!(
            "    {}{{\"[FOCAL] {}\"}}\n",
            focal_id, focal_label
        ));
        declared.insert(focal_id.clone());

        // ---------------------------------------------------------------
        // 5. Outgoing nodes (what focal imports) — rectangle shape [...]
        //    Edge: focal -->[IMPORTS]--> dep
        // ---------------------------------------------------------------
        for entry in &outgoing {
            let dep_id = format!("out_{}", sanitize_id(&entry.file_path));
            let dep_label = sanitize_label(&short_name(&entry.file_path), 50);

            let severity_badge = match entry.severity.as_str() {
                "breaking" => "[STATUS: BREAKING]",
                "warning"  => "[STATUS: WARNING]",
                _          => "[STATUS: ACTIVE]",
            };

            if !declared.contains(&dep_id) {
                mermaid.push_str(&format!(
                    "    {}[\"{} {}\"]\n",
                    dep_id, severity_badge, dep_label
                ));
                declared.insert(dep_id.clone());
            }

            let hop_tag = if entry.depth <= 1 { "[IMPORTS]" } else { "[IMPORTS_INDIRECT]" };
            edges.push(format!(
                "    {} -->|{}| {}\n",
                focal_id, hop_tag, dep_id
            ));
        }

        // ---------------------------------------------------------------
        // 6. Incoming nodes (who imports focal) — diamond shape{...}
        //    Edge: importer -->[IMPACTED]--> focal
        // ---------------------------------------------------------------
        for dep in &incoming {
            let dep_id = format!("in_{}", sanitize_id(&dep.file_path));
            let dep_label = sanitize_label(&short_name(&dep.file_path), 50);

            if !declared.contains(&dep_id) {
                mermaid.push_str(&format!(
                    "    {}{{\"[IMPORTER] {}\"}}\n",
                    dep_id, dep_label
                ));
                declared.insert(dep_id.clone());
            }

            let hop_tag = if dep.depth <= 1 { "[IMPACTED]" } else { "[IMPACTED_INDIRECT]" };
            edges.push(format!(
                "    {} -->|{}| {}\n",
                dep_id, hop_tag, focal_id
            ));
        }

        // ---------------------------------------------------------------
        // 7. Write edges (after all nodes so Mermaid parses cleanly)
        // ---------------------------------------------------------------
        for edge in &edges {
            mermaid.push_str(edge);
        }

        // ---------------------------------------------------------------
        // 8. Empty guard
        // ---------------------------------------------------------------
        if declared.len() <= 1 {
            mermaid.push_str(&format!(
                "    {}[\"[INFO: NO_DEPENDENCIES_INDEXED] No dependency data found for this file.\"]\n",
                format!("empty_{}", sanitize_id(file_path))
            ));
        }

        // ---------------------------------------------------------------
        // 9. Stats footer as a Mermaid comment
        // ---------------------------------------------------------------
        mermaid.push_str(&format!(
            "%% [STATS] focal=1 outgoing={} incoming={} max_hops={}\n",
            outgoing.len(),
            incoming.len(),
            max_hops,
        ));

        Ok(mermaid)
    }

    /// Returns all frontend consumer files and backend definitions for a given API route endpoint or backend file.
    /// Bridges the Python/FastAPI backend and TypeScript/Next.js frontend.
    pub fn get_endpoint_dependents(&self, target: &str) -> Result<EndpointDependentsReport> {
        let is_file_query = target.ends_with(".py") || target.ends_with(".ts") || target.ends_with(".js");
        
        let mut target_endpoints: Vec<String> = Vec::new();
        let mut backend_definitions: Vec<ozymem_parser::ApiRouteDefinition> = Vec::new();

        if is_file_query {
            // Target is a file path: find all routes defined in that file
            let file_routes = self.map_api_routes(Some(target)).unwrap_or_default();
            for r in file_routes {
                target_endpoints.push(ozymem_parser::canonicalize_endpoint_path(&r.path));
                backend_definitions.push(r);
            }
        } else {
            let canon = ozymem_parser::canonicalize_endpoint_path(target);
            target_endpoints.push(canon.clone());
            let all_routes = self.map_api_routes(None).unwrap_or_default();
            for r in all_routes {
                let r_canon = ozymem_parser::canonicalize_endpoint_path(&r.path);
                if r_canon == canon || r.path == target {
                    backend_definitions.push(r);
                }
            }
        }

        let primary_endpoint = if target_endpoints.is_empty() {
            ozymem_parser::canonicalize_endpoint_path(target)
        } else {
            target_endpoints[0].clone()
        };

        // Fetch or scan frontend consumer files
        let mut consumers: Vec<ozymem_parser::EndpointConsumer> = Vec::new();
        let mut consumer_files_set: HashSet<String> = HashSet::new();

        // 1. Query files table for frontend files
        let frontend_files: Vec<(String, String)> = {
            let inner = self.inner.lock().unwrap();
            let mut stmt = inner.sqlite.prepare(
                "SELECT path, language FROM files 
                 WHERE tenant_id = ?1 
                   AND (language IN ('typescript', 'tsx', 'javascript', 'jsx') 
                        OR path LIKE '%.tsx' OR path LIKE '%.ts' OR path LIKE '%.jsx' OR path LIKE '%.js')",
            )?;
            let rows = stmt.query_map(params![self.tenant_id], |row| Ok((row.get(0)?, row.get(1)?)))?;
            rows.filter_map(|r| r.ok()).collect()
        };

        for (fp, _lang_str) in frontend_files {
            let path = std::path::Path::new(&fp);
            if path.exists() {
                if let Ok(source) = std::fs::read_to_string(path) {
                    let file_consumers = ozymem_parser::parse_frontend_endpoint_consumers(&source, &fp);
                    for c in file_consumers {
                        let matches = target_endpoints.iter().any(|te| te == &c.canonical_path || te == &c.raw_endpoint);
                        if matches {
                            consumer_files_set.insert(c.file_path.clone());
                            consumers.push(c);
                        }
                    }
                }
            }
        }

        let mut consumer_files: Vec<String> = consumer_files_set.into_iter().collect();
        consumer_files.sort();

        // Also ensure edges are recorded in SQLite/RAM for fast graph queries
        let now = chrono::Utc::now().to_rfc3339();
        for c in &consumers {
            let edge = MemoryEdgeRecord {
                source_type: GraphEntityType::File,
                source_id: c.file_path.clone(),
                target_type: GraphEntityType::Endpoint,
                target_id: c.canonical_path.clone(),
                edge_type: MemoryEdgeType::ConsumesEndpoint,
                weight: 1.0,
                created_at: now.clone(),
                tenant_id: self.tenant_id.clone(),
                workspace_root: self.project_path().unwrap_or_default(),
            };
            let _ = self.insert_memory_edge(&edge);
        }

        Ok(EndpointDependentsReport {
            endpoint: target.to_string(),
            canonical_path: primary_endpoint,
            backend_definitions,
            consumers,
            total_consumer_files: consumer_files.len(),
            consumer_files,
        })
    }

    /// Returns the DDL migration history and backend model references for a given database table.
    /// Connects SQL migration lineage with FastAPI SQLAlchemy/SQLModel models.
    pub fn get_table_lineage(&self, target_table: &str) -> Result<TableLineageReport> {
        let norm_table = ozymem_parser::normalize_table_name(target_table);

        let mut migration_definitions: Vec<ozymem_parser::SqlTableReference> = Vec::new();
        let mut backend_models: Vec<ozymem_parser::SqlTableReference> = Vec::new();
        let mut referencing_files_set: HashSet<String> = HashSet::new();

        // 1. Fetch relevant files from files table
        let target_files: Vec<(String, String)> = {
            let inner = self.inner.lock().unwrap();
            let mut stmt = inner.sqlite.prepare(
                "SELECT path, language FROM files 
                 WHERE tenant_id = ?1 
                   AND (path LIKE '%.sql' OR path LIKE '%.py' OR path LIKE '%.ts' OR path LIKE '%.js' OR path LIKE '%.prisma')",
            )?;
            let rows = stmt.query_map(params![self.tenant_id], |row| Ok((row.get(0)?, row.get(1)?)))?;
            rows.filter_map(|r| r.ok()).collect()
        };

        for (fp, _lang_str) in target_files {
            let path = std::path::Path::new(&fp);
            if path.exists() {
                if let Ok(source) = std::fs::read_to_string(path) {
                    if fp.ends_with(".sql") {
                        let sql_refs = ozymem_parser::parse_sql_ddl_tables(&source, &fp);
                        for r in sql_refs {
                            if r.table_name == norm_table {
                                referencing_files_set.insert(r.file_path.clone());
                                migration_definitions.push(r);
                            }
                        }
                    } else {
                        let model_refs = ozymem_parser::parse_model_tables(&source, &fp);
                        for r in model_refs {
                            if r.table_name == norm_table {
                                referencing_files_set.insert(r.file_path.clone());
                                backend_models.push(r);
                            }
                        }
                    }
                }
            }
        }

        let mut referencing_files: Vec<String> = referencing_files_set.into_iter().collect();
        referencing_files.sort();

        // Record TouchesTable edges in SQLite and memory graph
        let now = chrono::Utc::now().to_rfc3339();
        for r in migration_definitions.iter().chain(backend_models.iter()) {
            let edge = MemoryEdgeRecord {
                source_type: GraphEntityType::File,
                source_id: r.file_path.clone(),
                target_type: GraphEntityType::DatabaseTable,
                target_id: norm_table.clone(),
                edge_type: MemoryEdgeType::TouchesTable,
                weight: 1.0,
                created_at: now.clone(),
                tenant_id: self.tenant_id.clone(),
                workspace_root: self.project_path().unwrap_or_default(),
            };
            let _ = self.insert_memory_edge(&edge);
        }

        Ok(TableLineageReport {
            table_name: norm_table,
            migration_definitions,
            backend_models,
            total_referencing_files: referencing_files.len(),
            referencing_files,
        })
    }
}

