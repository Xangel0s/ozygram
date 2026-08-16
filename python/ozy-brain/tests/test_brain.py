import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from ozy_brain.main import run


class OzyBrainTests(unittest.TestCase):
    def test_plan_safe_mode(self):
        result = run("plan", {
            "project": "p",
            "goal": "auth refactor main",
            "files": ["src/main.rs", "README.md"],
            "git_context": {"dirty": True, "status_files": [{"status": "M", "path": "src/main.rs"}]},
            "memories": [{"title": "validate"}],
        })
        self.assertEqual(result["action"], "plan")
        self.assertTrue(result["safe_mode"])
        self.assertGreaterEqual(len(result["plan"]), 3)
        self.assertTrue(any("Security" in risk for risk in result["risks"]))
        self.assertTrue(any(call["tool"] == "ozy_context" for call in result["suggested_mcp_calls"]))
        self.assertEqual(result["structured_plan"]["autonomy_level"], "advisory")
        self.assertGreaterEqual(len(result["structured_plan"]["phases"]), 5)
        self.assertIn("cargo test", result["structured_plan"]["suggested_commands"])
        self.assertTrue(any("modify files" in item for item in result["execution_policy"]["requires_confirmation"]))
        self.assertTrue(any("borrar archivos" in item for item in result["execution_policy"]["forbidden_for_python_worker"]))
        self.assertEqual(result["brain_version"], "0.2.0")
        self.assertEqual(result["brain_schema_version"], "v1")
        self.assertGreaterEqual(len(result["provenance"]), 1)
        self.assertTrue(result["brain_context_pack"]["dirty"])
        self.assertEqual(result["brain_context_pack"]["risk_level"], "high")
        self.assertEqual(result["brain_context_pack"]["candidate_file_scores"][0]["path"], "src/main.rs")

    def test_recall_deep_combines_relevant_memory_and_lessons(self):
        result = run(
            "recall_deep",
            {
                "project": "p",
                "goal": "quote autosave",
                "relevant_memories": [{"title": "autosave must be silent"}],
                "relevant_lessons": [{"error_context": "quote numbering", "solution": "send numeric sequence"}],
            },
        )
        joined = "\n".join(result["plan"])
        self.assertIn("autosave must be silent", joined)
        self.assertIn("quote numbering", joined)
        self.assertGreaterEqual(result["confidence"], 0.78)
        self.assertEqual(result["structured_plan"], {})
        self.assertEqual(result["execution_policy"]["mode"], "recall_only")
        self.assertEqual(result["brain_context_pack"]["project"], "p")

    def test_reflect_analysis(self):
        result = run("reflect", {
            "project": "ozy-test",
            "goal": "fix auth login flow",
            "failures": ["Permission denied when accessing token cache"],
            "changes": ["src/auth.rs", "src/extra_file.rs"],
            "files": ["src/auth.rs"],
        })
        self.assertEqual(result["action"], "reflect")
        self.assertIn("reflection_report", result)
        report = result["reflection_report"]
        self.assertEqual(report["total_failures"], 1)
        self.assertTrue(any("Permission" in rc for rc in report["root_causes"]))
        self.assertTrue(report["scope_creep_detected"])
        self.assertIn("src/extra_file.rs", report["out_of_scope_files"])

    def test_risk_review_categories(self):
        result = run("risk_review", {
            "project": "crm-geofal",
            "goal": "drop table legacy_quotes and run database migration",
            "files": ["crates/ozymem-core/src/schema.rs"],
        })
        self.assertEqual(result["action"], "risk_review")
        self.assertIn("risk_assessment", result)
        assessment = result["risk_assessment"]
        self.assertEqual(assessment["risk_level"], "critical")
        self.assertIn("data_loss", assessment["risk_categories"])
        self.assertTrue(assessment["requires_user_confirmation"])
        self.assertTrue(any("backup" in check for check in assessment["verification_checklist"]))

    def test_build_mental_model(self):
        result = run("build_mental_model", {
            "project": "ozymem-partner",
            "files": ["crates/ozymem-core/src/lib.rs", "python/ozy-brain/ozy_brain/main.py"],
            "graph_summary": {"nodes": 45, "edges": 120},
        })
        self.assertEqual(result["action"], "build_mental_model")
        self.assertIn("mental_model", result)
        model = result["mental_model"]
        self.assertEqual(model["project"], "ozymem-partner")
        self.assertIn("crates", model["core_modules"])
        self.assertGreaterEqual(len(model["where_to_look_first"]), 1)

    def test_unknown_action_falls_back_to_plan(self):
        result = run("unknown", {"project": "p"})
        self.assertEqual(result["action"], "plan")

    def test_rank_memories_action(self):
        result = run("rank_memories", {
            "project": "ozymem",
            "memories": [
                {"id": 1, "title": "high touch", "touch_count": 10, "confidence_score": 0.95},
                {"id": 2, "title": "stale touch", "touch_count": 0, "confidence_score": 0.2},
            ],
        })
        self.assertEqual(result["action"], "rank_memories")
        self.assertIn("rank", result["summary"].lower())

    def test_detect_patterns_and_suggest_next_steps(self):
        patterns_res = run("detect_patterns", {
            "project": "ozymem",
            "files": ["src/api/auth.rs", "src/api/user.rs", "src/api/order.rs"],
            "memories": [{"title": "jwt validation"}],
        })
        self.assertEqual(patterns_res["action"], "detect_patterns")

        steps_res = run("suggest_next_steps", {
            "project": "ozymem",
            "goal": "implement mfa authentication",
            "files": ["src/api/auth.rs"],
        })
        self.assertEqual(steps_res["action"], "suggest_next_steps")
        self.assertGreaterEqual(len(steps_res["plan"]), 1)

    def test_summarize_project_and_compress_session(self):
        summary_res = run("summarize_project", {
            "project": "ozymem",
            "files": ["crates/ozymem-core/src/lib.rs"],
            "graph_summary": {"file_count": 10, "function_count": 45},
        })
        self.assertEqual(summary_res["action"], "summarize_project")

        compress_res = run("compress_session", {
            "project": "ozymem",
            "changes": ["crates/ozymem-core/src/sync.rs"],
            "failures": ["none"],
        })
        self.assertEqual(compress_res["action"], "compress_session")

    def test_consolidate_engrams_and_triads(self):
        result = run("consolidate_engrams", {
            "project": "ozymem",
            "trigger_event": "git_commit_passed",
            "memories": [
                {"title": "AuthModule -> requires -> RS256 token verification"},
                {"title": "Database: always use connection pooling"},
                {"title": "temporary debug line print output"},
            ],
        })
        self.assertEqual(result["action"], "consolidate_engrams")
        self.assertGreaterEqual(len(result["memory_updates"]), 2)
        self.assertTrue(any("AuthModule" in u for u in result["memory_updates"]))


if __name__ == "__main__":
    unittest.main()
