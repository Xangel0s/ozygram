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
    assert_eq!(GraphEntityType::Endpoint.to_string(), "endpoint");
    assert_eq!(GraphEntityType::DatabaseTable.to_string(), "database_table");

    assert_eq!(GraphEntityType::from_str("file").unwrap(), GraphEntityType::File);
    assert_eq!(GraphEntityType::from_str("memory").unwrap(), GraphEntityType::Memory);
    assert_eq!(GraphEntityType::from_str("symbol").unwrap(), GraphEntityType::Symbol);
    assert_eq!(GraphEntityType::from_str("trajectory_node").unwrap(), GraphEntityType::TrajectoryNode);
    assert_eq!(GraphEntityType::from_str("endpoint").unwrap(), GraphEntityType::Endpoint);
    assert_eq!(GraphEntityType::from_str("database_table").unwrap(), GraphEntityType::DatabaseTable);

    assert_eq!(MemoryEdgeType::AppliesTo.to_string(), "applies_to");
    assert_eq!(MemoryEdgeType::CoupledWith.to_string(), "coupled_with");
    assert_eq!(MemoryEdgeType::CausesRegression.to_string(), "causes_regression");
    assert_eq!(MemoryEdgeType::Supersedes.to_string(), "supersedes");
    assert_eq!(MemoryEdgeType::Reinforces.to_string(), "reinforces");
    assert_eq!(MemoryEdgeType::DerivedFrom.to_string(), "derived_from");
    assert_eq!(MemoryEdgeType::ConsumesEndpoint.to_string(), "consumes_endpoint");
    assert_eq!(MemoryEdgeType::TouchesTable.to_string(), "touches_table");

    assert_eq!(MemoryEdgeType::from_str("applies_to").unwrap(), MemoryEdgeType::AppliesTo);
    assert_eq!(MemoryEdgeType::from_str("coupled_with").unwrap(), MemoryEdgeType::CoupledWith);
    assert_eq!(MemoryEdgeType::from_str("causes_regression").unwrap(), MemoryEdgeType::CausesRegression);
    assert_eq!(MemoryEdgeType::from_str("supersedes").unwrap(), MemoryEdgeType::Supersedes);
    assert_eq!(MemoryEdgeType::from_str("reinforces").unwrap(), MemoryEdgeType::Reinforces);
    assert_eq!(MemoryEdgeType::from_str("derived_from").unwrap(), MemoryEdgeType::DerivedFrom);
    assert_eq!(MemoryEdgeType::from_str("consumes_endpoint").unwrap(), MemoryEdgeType::ConsumesEndpoint);
    assert_eq!(MemoryEdgeType::from_str("touches_table").unwrap(), MemoryEdgeType::TouchesTable);
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

#[tokio::test]
async fn test_endpoint_dependents_cross_language() -> Result<()> {
    let tmp = tempdir()?;
    let root = tmp.path();
    let db_path = root.join("memory.db");
    let backend = GraphBackend::open(Some(&db_path.to_string_lossy()))?;
    backend.set_project_path(Some(&root.to_string_lossy()));

    // 1. Create FastAPI backend file
    let py_dir = root.join("api-geofal-crm").join("routers");
    std::fs::create_dir_all(&py_dir)?;
    let py_file = py_dir.join("sig.py");
    std::fs::write(
        &py_file,
        r#"from fastapi import APIRouter
router = APIRouter(prefix="/api/sig")

@router.get("/sctr")
def get_sctr_records():
    return [{"id": 1, "status": "active"}]

@router.post("/sctr")
def create_sctr_record():
    return {"status": "created"}
"#,
    )?;

    // 2. Create Next.js frontend consumer files
    let tsx_dir = root.join("crm-geofal").join("src").join("app").join("control-sig");
    std::fs::create_dir_all(&tsx_dir)?;
    let tsx_file = tsx_dir.join("page.tsx");
    std::fs::write(
        &tsx_file,
        r#"import React, { useEffect, useState } from 'react';
export default function ControlSigPage() {
    useEffect(() => {
        fetch("/api/sig/sctr").then(r => r.json());
    }, []);
    return <div>Control SIG</div>;
}
"#,
    )?;

    let comp_dir = root.join("crm-geofal").join("src").join("components");
    std::fs::create_dir_all(&comp_dir)?;
    let comp_file = comp_dir.join("SctrForm.tsx");
    std::fs::write(
        &comp_file,
        r#"import { apiClient } from '@/lib/api';
export function SctrForm() {
    const submit = async () => {
        await apiClient.post("/api/sig/sctr", { name: "Test" });
    };
    return <button onClick={submit}>Save</button>;
}
"#,
    )?;

    // 3. Index the files via index_file_delta
    backend.index_file_delta(&py_file, root)?;
    backend.index_file_delta(&tsx_file, root)?;
    backend.index_file_delta(&comp_file, root)?;

    // 4. Query endpoint dependents for /api/sig/sctr
    let report = backend.get_endpoint_dependents("/api/sig/sctr")?;
    assert_eq!(report.canonical_path, "/api/sig/sctr");
    assert_eq!(report.backend_definitions.len(), 2);
    assert_eq!(report.total_consumer_files, 2);
    assert_eq!(report.consumers.len(), 2);

    // Verify consumer files were found
    assert!(report.consumer_files.iter().any(|f| f.contains("page.tsx")));
    assert!(report.consumer_files.iter().any(|f| f.contains("SctrForm.tsx")));

    // 5. Query endpoint dependents by Python backend file
    let file_report = backend.get_endpoint_dependents(&py_file.to_string_lossy())?;
    assert_eq!(file_report.backend_definitions.len(), 2);
    assert_eq!(file_report.total_consumer_files, 2);

    Ok(())
}

#[tokio::test]
async fn test_table_lineage_ddl_and_models() -> Result<()> {
    let tmp = tempdir()?;
    let root = tmp.path();
    let db_path = root.join("memory.db");
    let backend = GraphBackend::open(Some(&db_path.to_string_lossy()))?;
    backend.set_project_path(Some(&root.to_string_lossy()));

    // 1. Create SQL migration file
    let mig_dir = root.join("migrations");
    std::fs::create_dir_all(&mig_dir)?;
    let sql_file = mig_dir.join("067_clasificar_jinshi_antiguo_silvia.sql");
    std::fs::write(
        &sql_file,
        r#"-- Migration 067
CREATE TABLE IF NOT EXISTS public.control_probetas (
    id SERIAL PRIMARY KEY,
    codigo VARCHAR(50) NOT NULL
);

ALTER TABLE control_probetas ADD COLUMN costo_servicio NUMERIC(10, 2);
"#,
    )?;

    // 2. Create Python SQLAlchemy model file
    let mod_dir = root.join("api").join("models");
    std::fs::create_dir_all(&mod_dir)?;
    let py_model = mod_dir.join("probeta.py");
    std::fs::write(
        &py_model,
        r#"from sqlmodel import SQLModel, Field

class ControlProbeta(SQLModel, table=True):
    __tablename__ = "control_probetas"
    id: int = Field(primary_key=True)
"#,
    )?;

    // 3. Index the files
    backend.index_file_delta(&sql_file, root)?;
    backend.index_file_delta(&py_model, root)?;

    // 4. Query table lineage for control_probetas
    let lineage = backend.get_table_lineage("control_probetas")?;
    assert_eq!(lineage.table_name, "control_probetas");
    assert_eq!(lineage.total_referencing_files, 2);
    assert_eq!(lineage.migration_definitions.len(), 2);
    assert_eq!(lineage.backend_models.len(), 1);

    assert_eq!(lineage.migration_definitions[0].operation, "CREATE");
    assert_eq!(lineage.migration_definitions[1].operation, "ALTER");
    assert_eq!(lineage.backend_models[0].operation, "MODEL");

    Ok(())
}

#[tokio::test]
async fn test_ghost_patch_simulation_cow_and_arity() -> Result<()> {
    let tmp = tempdir()?;
    let root = tmp.path();
    let db_path = root.join("memory.db");
    let backend = GraphBackend::open(Some(&db_path.to_string_lossy()))?;
    backend.set_project_path(Some(&root.to_string_lossy()));

    // 1. Defining file
    let def_file = root.join("math_service.py");
    let orig_def = r#"def compute_discount(subtotal: float, rate: float = 0.1) -> float:
    return subtotal * rate
"#;
    std::fs::write(&def_file, orig_def)?;

    // 2. Caller files
    let caller1 = root.join("order_service.py");
    std::fs::write(
        &caller1,
        r#"from math_service import compute_discount

def process_order(price: float):
    d = compute_discount(price)
    return price - d
"#,
    )?;

    let caller2 = root.join("billing_service.py");
    std::fs::write(
        &caller2,
        r#"from math_service import compute_discount

def generate_invoice(amount: float, discount_rate: float):
    d = compute_discount(amount, discount_rate)
    return amount - d
"#,
    )?;

    backend.index_file_delta(&def_file, root)?;
    backend.index_file_delta(&caller1, root)?;
    backend.index_file_delta(&caller2, root)?;

    // 3. Test non-breaking patch (maintains required arity = 1)
    let non_breaking_patch = r#"def compute_discount(subtotal: float, rate: float = 0.15) -> float:
    return subtotal * (rate if rate > 0 else 0.05)
"#;
    let sim1 = backend.simulate_patch("compute_discount", non_breaking_patch)?;
    assert_eq!(sim1.status, ozymem_core::graph_backend::types::SimulationStatus::Success);
    assert!(!sim1.breaks_dependents, "Should not break callers with default param");
    assert_eq!(sim1.breakages.len(), 0);
    assert_eq!(sim1.isolation_guarantee, "SANDBOX_INVARIANT_COW_RAM_ONLY");
    assert!(sim1.execution_time_ms < 100);

    // 4. Test breaking patch (increases required arity from 1 to 3)
    let breaking_patch = r#"def compute_discount(subtotal: float, rate: float, tax_category: str) -> float:
    return subtotal * rate
"#;
    let sim2 = backend.simulate_patch("compute_discount", breaking_patch)?;
    assert_eq!(sim2.status, ozymem_core::graph_backend::types::SimulationStatus::Success);
    assert!(sim2.breaks_dependents, "Should flag breakage when required arity increases to 3");
    assert!(sim2.arity_changed);
    assert_eq!(sim2.old_required_param_count, 1);
    assert_eq!(sim2.new_required_param_count, 3);
    assert_eq!(sim2.breakages.len(), 2, "Both caller1 (passes 1) and caller2 (passes 2) must be flagged");

    // 5. Verify strict Sandbox Invariant: disk file must be completely untouched!
    let current_disk = std::fs::read_to_string(&def_file)?;
    assert_eq!(current_disk, orig_def, "Disk file must NEVER be modified by simulate_patch");

    // 6. Test edge case: Symbol not found
    let sim_not_found = backend.simulate_patch("non_existent_func", "def non_existent_func(): pass")?;
    assert_eq!(sim_not_found.status, ozymem_core::graph_backend::types::SimulationStatus::SymbolNotFound);

    // 7. Test edge case: Syntax error in candidate code
    let sim_syntax_err = backend.simulate_patch("compute_discount", "def broken_code(((((") ?;
    assert!(matches!(sim_syntax_err.status, ozymem_core::graph_backend::types::SimulationStatus::SyntaxError(_)));

    Ok(())
}

#[test]
fn test_prune_decayed_memories_and_vaccinate_postmortems() -> anyhow::Result<()> {
    let tmp = tempfile::tempdir()?;
    let root = tmp.path();
    let backend = GraphBackend::open_for_project(root)?;

    // 1. Test Temporal Decay Pruning
    let active_node = ozymem_core::graph_backend::types::MemoryNodeRecord {
        id: "mem_active_1".to_string(),
        kind: "lesson".to_string(),
        title: "Active robust lesson".to_string(),
        content: "High confidence".to_string(),
        error_context: "".to_string(),
        solution: "Solution".to_string(),
        confidence_score: 0.95,
        touch_count: 5,
        stale: 0,
        stale_reason: None,
        created_at: "100000".to_string(),
        last_verified_at: "".to_string(),
        tenant_id: "local".to_string(),
        workspace_root: root.to_string_lossy().to_string(),
    };
    let decayed_node = ozymem_core::graph_backend::types::MemoryNodeRecord {
        id: "mem_decayed_1".to_string(),
        kind: "observation".to_string(),
        title: "Temporary low confidence note".to_string(),
        content: "Stale note".to_string(),
        error_context: "".to_string(),
        solution: "None".to_string(),
        confidence_score: 0.20,
        touch_count: 0,
        stale: 0,
        stale_reason: None,
        created_at: "100000".to_string(),
        last_verified_at: "".to_string(),
        tenant_id: "local".to_string(),
        workspace_root: root.to_string_lossy().to_string(),
    };

    backend.insert_memory_node(&active_node)?;
    backend.insert_memory_node(&decayed_node)?;

    let pruned_count = backend.prune_decayed_memories(0.35, 7)?;
    assert_eq!(pruned_count, 1, "Should prune exactly the 1 decayed memory node");

    let node1 = backend.get_memory_node("mem_active_1")?.unwrap();
    assert_eq!(node1.stale, 0);

    let node2 = backend.get_memory_node("mem_decayed_1")?.unwrap();
    assert_eq!(node2.stale, 1);
    assert_eq!(node2.stale_reason.as_deref(), Some("TEMPORAL_MEMORY_DECAY"));

    // 2. Test Postmortem Regression Vaccination
    // Initialize git repo in root
    let _ = std::process::Command::new("git").current_dir(root).args(["init"]).output()?;
    let _ = std::process::Command::new("git").current_dir(root).args(["config", "user.name", "Ozy"]).output()?;
    let _ = std::process::Command::new("git").current_dir(root).args(["config", "user.email", "ozy@example.com"]).output()?;

    let coti_file = root.join("cotizacion.py");
    std::fs::write(&coti_file, "def calc(): pass\n")?;
    let _ = std::process::Command::new("git").current_dir(root).args(["add", "cotizacion.py"]).output()?;
    let _ = std::process::Command::new("git").current_dir(root).args(["commit", "-m", "feat: initial commit"]).output()?;

    // Make fix commit
    std::fs::write(&coti_file, "def calc(): return 100\n")?;
    let _ = std::process::Command::new("git").current_dir(root).args(["add", "cotizacion.py"]).output()?;
    let _ = std::process::Command::new("git").current_dir(root).args(["commit", "-m", "fix(cotizacion): prevent total price overwrite"]).output()?;

    let vaccinated = backend.vaccinate_postmortems(Some(root))?;
    assert!(vaccinated >= 1, "Should vaccinate the fix commit");

    let invariants = backend.list_memory_nodes(Some("invariant"), false)?;
    assert!(!invariants.is_empty(), "Invariant memory node must be present");
    let inv = invariants.iter().find(|i| i.id.contains("prevent_total_price_overwrite")).expect("Must find vaccinated invariant node");
    assert_eq!(inv.kind, "invariant");
    assert!(inv.content.contains("[POSTMORTEM_INVARIANT]"));

    // Verify AppliesTo edge connects invariant to cotizacion.py
    let neighborhood = backend.get_memory_neighborhood(
        &ozymem_core::graph_backend::types::GraphEntityType::Memory,
        &inv.id,
        1,
    )?;
    assert!(!neighborhood.is_empty(), "Invariant must be connected to touched file in memory graph");
    assert!(neighborhood.iter().any(|n| n.edge_type == ozymem_core::graph_backend::types::MemoryEdgeType::AppliesTo));

    Ok(())
}



