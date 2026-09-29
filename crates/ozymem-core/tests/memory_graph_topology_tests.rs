use anyhow::Result;
use tempfile::tempdir;

use ozymem_core::graph_backend::types::{
    GraphBackend, GraphEntityType, MemoryEdgeRecord, MemoryEdgeType, MemoryNodeRecord,
};

#[tokio::test]
async fn test_multi_hop_bfs_traversal_with_decay() -> Result<()> {
    let tmp = tempdir()?;
    let db_path = tmp.path().join("memory.db");
    let backend = GraphBackend::open(Some(&db_path.to_string_lossy()))?;

    // 1. Create entities:
    // File A: api-geofal-crm/app/modules/marketing_publicidad/router.py
    // Memory 1: mem_campaign_rule
    // File B: crm-geofal/src/components/dashboard/recepcion-native/OrdenForm.tsx
    // Memory 2: mem_frontend_state_rule

    let mem1 = MemoryNodeRecord {
        id: "mem_campaign_rule".to_string(),
        kind: "module_rule".to_string(),
        title: "Campaign sync required".to_string(),
        content: "Campaign router requires sync before saving".to_string(),
        error_context: "".to_string(),
        solution: "Call sync_catalog first".to_string(),
        confidence_score: 1.0,
        touch_count: 0,
        stale: 0,
        stale_reason: None,
        created_at: "2026-09-29T12:00:00Z".to_string(),
        last_verified_at: "2026-09-29T12:00:00Z".to_string(),
        tenant_id: "local".to_string(),
        workspace_root: tmp.path().to_string_lossy().to_string(),
    };

    let mem2 = MemoryNodeRecord {
        id: "mem_frontend_state_rule".to_string(),
        kind: "convention".to_string(),
        title: "Optimistic UI update in OrdenForm".to_string(),
        content: "OrdenForm should update campaign state optimistically".to_string(),
        error_context: "".to_string(),
        solution: "Dispatch optimistic event".to_string(),
        confidence_score: 0.9,
        touch_count: 0,
        stale: 0,
        stale_reason: None,
        created_at: "2026-09-29T12:00:00Z".to_string(),
        last_verified_at: "2026-09-29T12:00:00Z".to_string(),
        tenant_id: "local".to_string(),
        workspace_root: tmp.path().to_string_lossy().to_string(),
    };

    backend.insert_memory_node(&mem1)?;
    backend.insert_memory_node(&mem2)?;

    // Edges:
    // mem1 --[applies_to]--> router.py (weight 1.0)
    // mem1 --[coupled_with]--> OrdenForm.tsx (weight 0.8)
    // mem2 --[applies_to]--> OrdenForm.tsx (weight 1.0)

    backend.insert_memory_edge(&MemoryEdgeRecord {
        source_type: GraphEntityType::Memory,
        source_id: "mem_campaign_rule".to_string(),
        target_type: GraphEntityType::File,
        target_id: "api-geofal-crm/app/modules/marketing_publicidad/router.py".to_string(),
        edge_type: MemoryEdgeType::AppliesTo,
        weight: 1.0,
        created_at: "2026-09-29T12:00:00Z".to_string(),
        tenant_id: "local".to_string(),
        workspace_root: tmp.path().to_string_lossy().to_string(),
    })?;

    backend.insert_memory_edge(&MemoryEdgeRecord {
        source_type: GraphEntityType::Memory,
        source_id: "mem_campaign_rule".to_string(),
        target_type: GraphEntityType::File,
        target_id: "crm-geofal/src/components/dashboard/recepcion-native/OrdenForm.tsx".to_string(),
        edge_type: MemoryEdgeType::CoupledWith,
        weight: 0.8,
        created_at: "2026-09-29T12:00:00Z".to_string(),
        tenant_id: "local".to_string(),
        workspace_root: tmp.path().to_string_lossy().to_string(),
    })?;

    backend.insert_memory_edge(&MemoryEdgeRecord {
        source_type: GraphEntityType::Memory,
        source_id: "mem_frontend_state_rule".to_string(),
        target_type: GraphEntityType::File,
        target_id: "crm-geofal/src/components/dashboard/recepcion-native/OrdenForm.tsx".to_string(),
        edge_type: MemoryEdgeType::AppliesTo,
        weight: 1.0,
        created_at: "2026-09-29T12:00:00Z".to_string(),
        tenant_id: "local".to_string(),
        workspace_root: tmp.path().to_string_lossy().to_string(),
    })?;

    // Multi-hop BFS from File: router.py with depth 2
    let neighborhood = backend.get_memory_neighborhood(
        &GraphEntityType::File,
        "api-geofal-crm/app/modules/marketing_publicidad/router.py",
        2,
    )?;

    // Should reach mem_campaign_rule at depth 1, and OrdenForm.tsx at depth 2
    assert!(!neighborhood.is_empty(), "Neighborhood should return connected entities");

    let depth1_items: Vec<_> = neighborhood.iter().filter(|n| n.depth == 1).collect();
    assert_eq!(depth1_items.len(), 1);
    assert_eq!(depth1_items[0].entity.id, "mem_campaign_rule");
    assert_eq!(depth1_items[0].effective_weight, 1.0);

    let depth2_items: Vec<_> = neighborhood.iter().filter(|n| n.depth == 2).collect();
    assert!(!depth2_items.is_empty());
    let orden_form = depth2_items.iter().find(|n| n.entity.id == "crm-geofal/src/components/dashboard/recepcion-native/OrdenForm.tsx");
    assert!(orden_form.is_some(), "2-hop BFS should discover coupled frontend component");
    let orden_form = orden_form.unwrap();
    // Decay factor for depth 2 is 0.6: 0.8 * 0.6 = 0.48
    assert!((orden_form.effective_weight - 0.48).abs() < 1e-4);

    Ok(())
}

#[tokio::test]
async fn test_cyclic_graph_termination_in_bfs() -> Result<()> {
    let tmp = tempdir()?;
    let db_path = tmp.path().join("memory.db");
    let backend = GraphBackend::open(Some(&db_path.to_string_lossy()))?;

    // Create cyclic graph: NodeA -> NodeB -> NodeC -> NodeA
    for id in &["node_a", "node_b", "node_c"] {
        backend.insert_memory_node(&MemoryNodeRecord {
            id: id.to_string(),
            kind: "convention".to_string(),
            title: id.to_string(),
            content: "content".to_string(),
            error_context: "".to_string(),
            solution: "".to_string(),
            confidence_score: 1.0,
            touch_count: 0,
            stale: 0,
            stale_reason: None,
            created_at: "2026-09-29T12:00:00Z".to_string(),
            last_verified_at: "2026-09-29T12:00:00Z".to_string(),
            tenant_id: "local".to_string(),
            workspace_root: tmp.path().to_string_lossy().to_string(),
        })?;
    }

    let pairs = [("node_a", "node_b"), ("node_b", "node_c"), ("node_c", "node_a")];
    for (src, dst) in pairs {
        backend.insert_memory_edge(&MemoryEdgeRecord {
            source_type: GraphEntityType::Memory,
            source_id: src.to_string(),
            target_type: GraphEntityType::Memory,
            target_id: dst.to_string(),
            edge_type: MemoryEdgeType::Reinforces,
            weight: 1.0,
            created_at: "2026-09-29T12:00:00Z".to_string(),
            tenant_id: "local".to_string(),
            workspace_root: tmp.path().to_string_lossy().to_string(),
        })?;
    }

    let start = std::time::Instant::now();
    let res = backend.get_memory_neighborhood(&GraphEntityType::Memory, "node_a", 3)?;
    let elapsed = start.elapsed();

    assert!(elapsed.as_millis() < 50, "BFS on cyclic graph must complete quickly");
    assert!(!res.is_empty());

    Ok(())
}

#[tokio::test]
async fn test_cascade_stale_invalidation() -> Result<()> {
    let tmp = tempdir()?;
    let db_path = tmp.path().join("memory.db");
    let backend = GraphBackend::open(Some(&db_path.to_string_lossy()))?;

    // Create File and two connected memories (mem_v1 and mem_v2 chained by SUPERSEDES)
    backend.insert_memory_node(&MemoryNodeRecord {
        id: "mem_cache_v1".to_string(),
        kind: "decision".to_string(),
        title: "Legacy cache rule".to_string(),
        content: "Cache in memory".to_string(),
        error_context: "".to_string(),
        solution: "".to_string(),
        confidence_score: 1.0,
        touch_count: 0,
        stale: 0,
        stale_reason: None,
        created_at: "2026-09-29T12:00:00Z".to_string(),
        last_verified_at: "2026-09-29T12:00:00Z".to_string(),
        tenant_id: "local".to_string(),
        workspace_root: tmp.path().to_string_lossy().to_string(),
    })?;

    backend.insert_memory_node(&MemoryNodeRecord {
        id: "mem_cache_v2".to_string(),
        kind: "decision".to_string(),
        title: "Updated cache rule".to_string(),
        content: "Cache in redis".to_string(),
        error_context: "".to_string(),
        solution: "".to_string(),
        confidence_score: 1.0,
        touch_count: 0,
        stale: 0,
        stale_reason: None,
        created_at: "2026-09-29T12:00:00Z".to_string(),
        last_verified_at: "2026-09-29T12:00:00Z".to_string(),
        tenant_id: "local".to_string(),
        workspace_root: tmp.path().to_string_lossy().to_string(),
    })?;

    // mem_cache_v1 applies_to src/service.py
    backend.insert_memory_edge(&MemoryEdgeRecord {
        source_type: GraphEntityType::Memory,
        source_id: "mem_cache_v1".to_string(),
        target_type: GraphEntityType::File,
        target_id: "src/service.py".to_string(),
        edge_type: MemoryEdgeType::AppliesTo,
        weight: 1.0,
        created_at: "2026-09-29T12:00:00Z".to_string(),
        tenant_id: "local".to_string(),
        workspace_root: tmp.path().to_string_lossy().to_string(),
    })?;

    // mem_cache_v1 supersedes mem_cache_v2
    backend.insert_memory_edge(&MemoryEdgeRecord {
        source_type: GraphEntityType::Memory,
        source_id: "mem_cache_v1".to_string(),
        target_type: GraphEntityType::Memory,
        target_id: "mem_cache_v2".to_string(),
        edge_type: MemoryEdgeType::Supersedes,
        weight: 1.0,
        created_at: "2026-09-29T12:00:00Z".to_string(),
        tenant_id: "local".to_string(),
        workspace_root: tmp.path().to_string_lossy().to_string(),
    })?;

    // Trigger cascade invalidation when src/service.py changes
    let invalidated = backend.propagate_stale_invalidation("src/service.py", "mtime changed on disk")?;
    assert_eq!(invalidated.len(), 2, "Cascade should invalidate both mem_cache_v1 and downstream mem_cache_v2");
    assert!(invalidated.contains(&"mem_cache_v1".to_string()));
    assert!(invalidated.contains(&"mem_cache_v2".to_string()));

    // Verify SQLite reflects stale state
    let v1 = backend.get_memory_node("mem_cache_v1")?.unwrap();
    assert_eq!(v1.stale, 1);
    assert!(v1.stale_reason.unwrap().contains("mtime changed on disk"));

    let v2 = backend.get_memory_node("mem_cache_v2")?.unwrap();
    assert_eq!(v2.stale, 1);

    Ok(())
}
