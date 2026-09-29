use anyhow::Result;
use rusqlite::params;
use std::str::FromStr;

use crate::graph_backend::types::{
    GraphBackend, GraphEntityType, MemoryEdgeRecord, MemoryEdgeType, MemoryNodeRecord,
};

impl GraphBackend {
    /// Inserta o actualiza un nodo de memoria en SQLite.
    pub fn insert_memory_node(&self, node: &MemoryNodeRecord) -> Result<String> {
        let inner = self.inner.lock().unwrap();
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

    /// Elimina un nodo de memoria y sus aristas asociadas.
    pub fn delete_memory_node(&self, id: &str) -> Result<bool> {
        let inner = self.inner.lock().unwrap();
        let affected = inner.sqlite.execute(
            "DELETE FROM memory_nodes WHERE id = ?1 AND tenant_id = ?2",
            params![id, self.tenant_id],
        )?;
        inner.sqlite.execute(
            "DELETE FROM memory_edges WHERE ((source_type = 'memory' AND source_id = ?1) OR (target_type = 'memory' AND target_id = ?1)) AND tenant_id = ?2",
            params![id, self.tenant_id],
        )?;
        Ok(affected > 0)
    }

    /// Inserta o actualiza una arista de memoria tipada.
    pub fn insert_memory_edge(&self, edge: &MemoryEdgeRecord) -> Result<()> {
        let inner = self.inner.lock().unwrap();
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
        Ok(())
    }

    /// Elimina una arista de memoria específica.
    pub fn delete_memory_edge(
        &self,
        source_type: &GraphEntityType,
        source_id: &str,
        target_type: &GraphEntityType,
        target_id: &str,
        edge_type: &MemoryEdgeType,
    ) -> Result<bool> {
        let inner = self.inner.lock().unwrap();
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
        Ok(affected > 0)
    }

    /// Obtiene las aristas conectadas (entrantes y salientes) a una entidad.
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
}
