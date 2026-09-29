use anyhow::Result;
use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;
use rusqlite::params;
use std::collections::{HashMap, HashSet, VecDeque};
use std::str::FromStr;

use crate::graph_backend::types::{
    GraphBackend, GraphEntityNode, GraphEntityType, MemoryEdgeRecord, MemoryEdgeType,
    MemoryGraphEdge, MemoryNodeRecord, NeighborhoodResult,
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
}
