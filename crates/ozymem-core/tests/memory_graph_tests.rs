use anyhow::Result;
use std::str::FromStr;
use tempfile::tempdir;

use ozymem_core::McpBackend;
use ozymem_core::graph_backend::types::{
    GraphBackend, GraphEntityType, MemoryEdgeRecord, MemoryEdgeType, MemoryNodeRecord,
};

#[tokio::test]
async fn test_memory_types_from_str_and_display() {
    assert_eq!(GraphEntityType::File.to_string(), "file");
    assert_eq!(GraphEntityType::Memory.to_string(), "memory");
    assert_eq!(GraphEntityType::Symbol.to_string(), "symbol");
    assert_eq!(GraphEntityType::TrajectoryNode.to_string(), "trajectory_node");

    assert_eq!(GraphEntityType::from_str("file").unwrap(), GraphEntityType::File);
    assert_eq!(GraphEntityType::from_str("memory").unwrap(), GraphEntityType::Memory);
    assert_eq!(GraphEntityType::from_str("symbol").unwrap(), GraphEntityType::Symbol);
    assert_eq!(GraphEntityType::from_str("trajectory_node").unwrap(), GraphEntityType::TrajectoryNode);

    assert_eq!(MemoryEdgeType::AppliesTo.to_string(), "applies_to");
    assert_eq!(MemoryEdgeType::CoupledWith.to_string(), "coupled_with");
    assert_eq!(MemoryEdgeType::CausesRegression.to_string(), "causes_regression");
    assert_eq!(MemoryEdgeType::Supersedes.to_string(), "supersedes");
    assert_eq!(MemoryEdgeType::Reinforces.to_string(), "reinforces");
    assert_eq!(MemoryEdgeType::DerivedFrom.to_string(), "derived_from");

    assert_eq!(MemoryEdgeType::from_str("applies_to").unwrap(), MemoryEdgeType::AppliesTo);
    assert_eq!(MemoryEdgeType::from_str("coupled_with").unwrap(), MemoryEdgeType::CoupledWith);
    assert_eq!(MemoryEdgeType::from_str("causes_regression").unwrap(), MemoryEdgeType::CausesRegression);
    assert_eq!(MemoryEdgeType::from_str("supersedes").unwrap(), MemoryEdgeType::Supersedes);
    assert_eq!(MemoryEdgeType::from_str("reinforces").unwrap(), MemoryEdgeType::Reinforces);
    assert_eq!(MemoryEdgeType::from_str("derived_from").unwrap(), MemoryEdgeType::DerivedFrom);
}

#[tokio::test]
async fn test_memory_nodes_and_edges_crud() -> Result<()> {
    let tmp = tempdir()?;
    let db_path = tmp.path().join("memory.db");
    let backend = GraphBackend::open(Some(&db_path.to_string_lossy()))?;

    // 1. Insert a memory node
    let node = MemoryNodeRecord {
        id: "mem_order_form_sync".to_string(),
        kind: "convention".to_string(),
        title: "OrdenForm validation rule".to_string(),
        content: "OrdenForm must validate roles before calling marketing router".to_string(),
        error_context: "Missing SUPER_ADMIN role".to_string(),
        solution: "Check user.role in session prior to request".to_string(),
        confidence_score: 0.95,
        touch_count: 1,
        stale: 0,
        stale_reason: None,
        created_at: "2026-09-29T12:00:00Z".to_string(),
        last_verified_at: "2026-09-29T12:00:00Z".to_string(),
        tenant_id: "local".to_string(),
        workspace_root: tmp.path().to_string_lossy().to_string(),
    };

    let id = backend.insert_memory_node(&node)?;
    assert_eq!(id, "mem_order_form_sync");

    // 2. Fetch memory node
    let fetched = backend.get_memory_node("mem_order_form_sync")?;
    assert!(fetched.is_some());
    let fetched = fetched.unwrap();
    assert_eq!(fetched.kind, "convention");
    assert_eq!(fetched.confidence_score, 0.95);
    assert_eq!(fetched.title, "OrdenForm validation rule");

    // 3. List memory nodes
    let all = backend.list_memory_nodes(None, false)?;
    assert!(!all.is_empty());
    let conventions = backend.list_memory_nodes(Some("convention"), false)?;
    assert_eq!(conventions.len(), 1);
    let decisions = backend.list_memory_nodes(Some("decision"), false)?;
    assert_eq!(decisions.len(), 0);

    // 4. Insert memory edges (structural and cross-layer)
    let edge1 = MemoryEdgeRecord {
        source_type: GraphEntityType::Memory,
        source_id: "mem_order_form_sync".to_string(),
        target_type: GraphEntityType::File,
        target_id: "crm-geofal/src/components/dashboard/recepcion-native/OrdenForm.tsx".to_string(),
        edge_type: MemoryEdgeType::AppliesTo,
        weight: 1.0,
        created_at: "2026-09-29T12:00:00Z".to_string(),
        tenant_id: "local".to_string(),
        workspace_root: tmp.path().to_string_lossy().to_string(),
    };

    let edge2 = MemoryEdgeRecord {
        source_type: GraphEntityType::Memory,
        source_id: "mem_order_form_sync".to_string(),
        target_type: GraphEntityType::File,
        target_id: "api-geofal-crm/app/modules/marketing_publicidad/router.py".to_string(),
        edge_type: MemoryEdgeType::CoupledWith,
        weight: 0.9,
        created_at: "2026-09-29T12:00:00Z".to_string(),
        tenant_id: "local".to_string(),
        workspace_root: tmp.path().to_string_lossy().to_string(),
    };

    backend.insert_memory_edge(&edge1)?;
    backend.insert_memory_edge(&edge2)?;

    // 5. Query edges for node
    let edges_for_mem = backend.list_edges_for_node(&GraphEntityType::Memory, "mem_order_form_sync")?;
    assert_eq!(edges_for_mem.len(), 2);

    let edges_for_router = backend.list_edges_for_node(
        &GraphEntityType::File,
        "api-geofal-crm/app/modules/marketing_publicidad/router.py",
    )?;
    assert_eq!(edges_for_router.len(), 1);
    assert_eq!(edges_for_router[0].edge_type, MemoryEdgeType::CoupledWith);

    // 6. Delete edge
    let deleted_edge = backend.delete_memory_edge(
        &GraphEntityType::Memory,
        "mem_order_form_sync",
        &GraphEntityType::File,
        "api-geofal-crm/app/modules/marketing_publicidad/router.py",
        &MemoryEdgeType::CoupledWith,
    )?;
    assert!(deleted_edge);

    let edges_after = backend.list_edges_for_node(&GraphEntityType::Memory, "mem_order_form_sync")?;
    assert_eq!(edges_after.len(), 1);

    // 7. Delete node and ensure cascade cleanup of remaining edges
    let deleted_node = backend.delete_memory_node("mem_order_form_sync")?;
    assert!(deleted_node);

    let edges_after_delete = backend.list_edges_for_node(&GraphEntityType::Memory, "mem_order_form_sync")?;
    assert_eq!(edges_after_delete.len(), 0);

    Ok(())
}

#[tokio::test]
async fn test_migration_and_sync_triggers_from_lessons() -> Result<()> {
    let tmp = tempdir()?;
    let db_path = tmp.path().join("memory.db");
    let backend = GraphBackend::open(Some(&db_path.to_string_lossy()))?;

    // Record a lesson using legacy lessons API via McpBackend
    backend.record_lesson(
        "api-geofal-crm/app/modules/roles/router.py",
        Some("verify_role"),
        "Role not found in DB",
        "Return 403 Forbidden with structured detail",
    ).await?;

    let lessons = backend.recent_lessons(None, 5).await?;
    assert_eq!(lessons.len(), 1);
    let lesson_id = lessons[0].id;

    // Verify trigger populated memory_nodes
    let legacy_node_id = format!("legacy_lesson_{}", lesson_id);
    let node = backend.get_memory_node(&legacy_node_id)?;
    assert!(node.is_some(), "Trigger should automatically mirror new lesson into memory_nodes");
    let node = node.unwrap();
    assert_eq!(node.title, "verify_role");
    assert!(node.content.contains("403 Forbidden"));

    // Verify trigger populated memory_edges
    let edges = backend.list_edges_for_node(&GraphEntityType::Memory, &legacy_node_id)?;
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].edge_type, MemoryEdgeType::AppliesTo);
    assert_eq!(edges[0].target_id, "api-geofal-crm/app/modules/roles/router.py");

    // Test FTS search on lessons
    let search_res = backend.search_lessons("Forbidden", None, 10).await?;
    assert!(!search_res.is_empty());

    // Prune/delete memory_node and verify cleanup of memory_nodes and memory_edges
    let deleted = backend.delete_memory_node(&legacy_node_id)?;
    assert!(deleted);

    let node_after = backend.get_memory_node(&legacy_node_id)?;
    assert!(node_after.is_none(), "Delete should remove memory_nodes record");

    let edges_after = backend.list_edges_for_node(&GraphEntityType::Memory, &legacy_node_id)?;
    assert_eq!(edges_after.len(), 0, "Delete should remove associated memory_edges");

    Ok(())
}
