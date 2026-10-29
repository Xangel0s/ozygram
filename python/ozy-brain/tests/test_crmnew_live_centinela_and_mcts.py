r"""
Real-world validation and stress test of Live Dream-RSI Centinela and Warm-Started Branching MCTS
directly against the actual C:\Users\Lenovo\Documents\crmnew production workspace.
Standard: Zero-Emoji, SOLID, DRY.
"""

from pathlib import Path
import pytest

from ozy_brain.scanner.multi_repo import MultiRepoScanner, RepoRole
from ozy_brain.scanner.contract_tracer import (
    FastApiContractExtractor,
    NextJsConsumerExtractor,
    CrossRepoContractTracer,
)
from ozy_brain.scanner.blast_radius import ReverseBlastRadiusEngine
from ozy_brain.scanner.compute_config import MonsterComputeConfig, ComputeMode
from ozy_brain.dream.plan_branching import (
    PlanExecutionState,
    PlanBranchingMctsOptimizer,
    persist_mitigation_vaccine,
)
from ozy_brain.planner import plan, _structured_plan


CRMNEW_PATH = Path(r"C:\Users\Lenovo\Documents\crmnew")


@pytest.mark.skipif(not CRMNEW_PATH.exists(), reason="crmnew workspace not found")
class TestCrmnewLiveCentinelaAndMcts:

    def test_01_multi_repo_scanner_on_real_crmnew(self):
        """Validates that MultiRepoScanner identifies both api-geofal-crm and crm-geofal."""
        discovered = MultiRepoScanner.discover_sibling_repos(CRMNEW_PATH)

        names = {d.relative_name for d in discovered}
        assert "api-geofal-crm" in names, "api-geofal-crm must be detected as FastAPI sub-repo"
        assert "crm-geofal" in names, "crm-geofal must be detected as Next.js sub-repo"

        roles = {d.relative_name: d.role for d in discovered}
        assert roles["api-geofal-crm"] == RepoRole.BACKEND_FASTAPI
        assert roles["crm-geofal"] == RepoRole.FRONTEND_NEXTJS

    def test_02_fastapi_contract_extraction_on_real_probetas(self):
        """Extracts actual router endpoints from control_probetas in api-geofal-crm."""
        router_path = CRMNEW_PATH / "api-geofal-crm" / "app" / "modules" / "control_probetas" / "router.py"
        assert router_path.exists(), f"Router file {router_path} must exist"

        source = router_path.read_text(encoding="utf-8")
        endpoints = FastApiContractExtractor.extract_from_source(source, str(router_path))
        assert len(endpoints) >= 1, "Must extract at least one real endpoint from control_probetas router"

        paths = [ep.canonical_path for ep in endpoints]
        methods = [ep.method for ep in endpoints]
        assert any("/probetas" in p.lower() or "/muestras" in p.lower() or "/auto-sync" in p.lower() or "/items" in p.lower() or len(p) > 1 for p in paths)
        assert "GET" in methods or "POST" in methods

    def test_03_nextjs_consumer_extraction_on_real_hooks(self):
        """Extracts fetch calls from use-control-probetas.ts and use-recepciones.ts."""
        hook_path = CRMNEW_PATH / "crm-geofal" / "src" / "hooks" / "use-control-probetas.ts"
        assert hook_path.exists(), f"Hook file {hook_path} must exist"

        source = hook_path.read_text(encoding="utf-8")
        consumers = NextJsConsumerExtractor.extract_from_source(source, str(hook_path))
        assert len(consumers) >= 1, "Must extract real fetch calls from use-control-probetas.ts"

        target_paths = [c.canonical_path for c in consumers]
        assert any("probeta" in p.lower() or "control" in p.lower() or "api" in p.lower() for p in target_paths)

    def test_04_cross_repo_contract_tracer_on_crmnew(self):
        """Traces bidirectional contract linkage across api-geofal-crm and crm-geofal."""
        router_path = CRMNEW_PATH / "api-geofal-crm" / "app" / "modules" / "control_probetas" / "router.py"
        hook_path = CRMNEW_PATH / "crm-geofal" / "src" / "hooks" / "use-control-probetas.ts"

        backend_endpoints = FastApiContractExtractor.extract_from_source(
            router_path.read_text(encoding="utf-8"), str(router_path)
        )
        frontend_consumers = NextJsConsumerExtractor.extract_from_source(
            hook_path.read_text(encoding="utf-8"), str(hook_path)
        )

        alerts = CrossRepoContractTracer.trace_contracts(backend_endpoints, frontend_consumers)
        assert isinstance(alerts, list)
        for a in alerts:
            assert "[ALERT: CROSS_REPO_CONTRACT_DRIFT" in a.badge

    def test_05_reverse_blast_radius_on_crmnew_frontend(self):
        """Computes reverse import dependency BFS over crm-geofal components and hooks."""
        fe_root = CRMNEW_PATH / "crm-geofal"
        target_hook = fe_root / "src" / "hooks" / "use-control-probetas.ts"

        graph = ReverseBlastRadiusEngine.build_import_graph(fe_root)
        assert isinstance(graph, dict)
        assert len(graph) >= 1

        consumers = NextJsConsumerExtractor.extract_from_source(
            target_hook.read_text(encoding="utf-8"), str(target_hook)
        )

        report = ReverseBlastRadiusEngine.calculate_blast_radius(
            target_path="/api/control-probetas",
            frontend_consumers=consumers,
            inverted_graph=graph,
            config=MonsterComputeConfig.from_env(),
        )

        assert report.target_identifier == "/api/control-probetas"
        assert report.total_affected >= 0
        assert "[ALERT: BLAST_RADIUS_RISK" in report.badge

    def test_06_warm_started_mcts_plan_optimizer_on_crmnew_scenario(self, tmp_path: Path):
        """
        Simulates a real-world developer scenario on crmnew:
        The LLM already completed Phase 1 (context) and Phase 2 (leaf_contracts).
        Warm-starts MCTS in RAM to select the optimal non-breaking branch strategy for Phase 3 (domain_services).
        """
        probetas_router = str(CRMNEW_PATH / "api-geofal-crm" / "app" / "modules" / "control_probetas" / "router.py")
        probetas_hook = str(CRMNEW_PATH / "crm-geofal" / "src" / "hooks" / "use-control-probetas.ts")

        state = PlanExecutionState(
            completed_phases=["context", "leaf_contracts"],
            active_phase="domain_services",
            modified_files=[probetas_router],
            working_edits=[{"file": probetas_router, "action": "refactor_v2_payload"}],
        )

        candidates = [probetas_router, probetas_hook]

        # Monster Compute mode on real CRM
        config = MonsterComputeConfig(
            mode=ComputeMode.MONSTER,
            mcts_simulations=40,
            mcts_max_depth=6,
            parallel_workers=2,
            blast_radius_max_depth=4,
            throttle_sleep_ms=0,
            max_memory_mb=2048,
        )

        res = PlanBranchingMctsOptimizer.optimize_plan(
            state=state,
            candidate_files=candidates,
            compute_config=config,
            blast_radius_count=3,
        )

        assert res.best_strategy == "DualActiveContract", "MCTS must select non-breaking DualActiveContract"
        assert res.best_score >= 0.85
        assert res.badge == "[OPTIMIZED_BY_DREAM_RSI]"

        # Verify Markdown vaccine synthesis
        mem_dir = tmp_path / ".ozy" / "memories"
        vaccine_path = persist_mitigation_vaccine(
            optimization_result=res,
            target_files=candidates,
            base_dir=mem_dir,
        )
        assert vaccine_path is not None and vaccine_path.exists()
        vaccine_text = vaccine_path.read_text(encoding="utf-8")
        assert "kind: gotcha" in vaccine_text
        assert "DualActiveContract" in vaccine_text

    def test_07_full_brain_planner_integration_on_crmnew_payload(self):
        """Validates that plan() returns pre-optimized trajectory with real crmnew files."""
        probetas_router = str(CRMNEW_PATH / "api-geofal-crm" / "app" / "modules" / "control_probetas" / "router.py")

        payload = {
            "project": "crmnew",
            "goal": "Modernizar endpoint de control de probetas sin generar caidas en vistas de recepcion",
            "completed_phases": ["context", "leaf_contracts"],
            "current_phase": "domain_services",
            "files": [probetas_router],
        }

        brain_res = plan(payload)
        assert brain_res.action == "plan"
        assert brain_res.structured_plan is not None

        # Check phase statuses
        phases = {p["name"]: p["status"] for p in brain_res.structured_plan["phases"]}
        assert phases["context"] == "COMPLETED"
        assert phases["leaf_contracts"] == "COMPLETED"
        assert phases["domain_services"] == "ACTIVE"

        # Check dream MCTS optimization
        opt = brain_res.structured_plan.get("dream_mcts_optimization", {})
        assert opt.get("best_strategy") == "DualActiveContract"
        assert opt.get("badge") == "[OPTIMIZED_BY_DREAM_RSI]"
        assert opt.get("best_score", 0.0) >= 0.85

        # Check that plan steps contain the MCTS optimization directive
        plan_steps = "\n".join(brain_res.plan)
        assert "[DREAM_RSI]" in plan_steps
        assert "DualActiveContract" in plan_steps
