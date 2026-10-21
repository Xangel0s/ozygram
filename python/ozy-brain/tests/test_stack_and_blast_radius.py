import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from ozy_brain.main import run


class StackDetectionAndBlastRadiusTests(unittest.TestCase):
    def test_geofal_crm_stack_detection_and_semantic_blast_radius(self):
        """Recreates the Geofal CRM Sulfatos NTP 339.178 task with dirty unrelated files."""
        payload = {
            "project": "crmnew",
            "goal": "Ajustar el redondeo de Sulfatos Solubles (NTP 339.178) en SulfatosSolublesForm.tsx y schemas.py",
            "files": [
                "crm-geofal/src/components/dashboard/SulfatosSolublesForm.tsx",
                "api-geofal-crm/app/modules/schemas.py",
                "package.json",
                "pyproject.toml",
            ],
            "git_context": {
                "dirty": True,
                "status_files": [
                    {"status": "M", "path": "Comercial-Control-CRM/lead.tsx"},
                    {"status": "M", "path": "Ingenieria/perforacion.py"},
                    {"status": "??", "path": "backups/db_backup.sql"},
                    {"status": "M", "path": ".agents/rules/coding_rules.md"},
                    {"status": "M", "path": "DEVELOPMENT_AND_DEPLOYMENT_GUIDE.md"},
                    {"status": "M", "path": "crm-geofal/src/components/dashboard/SulfatosSolublesForm.tsx"},
                ],
            },
            "memories": [],
        }

        result = run("plan", payload)
        struct_plan = result["structured_plan"]
        commands = struct_plan["suggested_commands"]
        recommendations = result["recommendations"]

        # 1. Verification of Stack Autodetection: ZERO cargo test
        self.assertFalse(
            any("cargo" in cmd for cmd in commands),
            f"Cargo test leaked into Next.js/FastAPI project: {commands}",
        )
        self.assertTrue(
            any("npm test" in cmd or "pnpm test" in cmd for cmd in commands),
            f"npm test missing from Node/TS project: {commands}",
        )
        self.assertTrue(
            any("pytest" in cmd for cmd in commands),
            f"pytest missing from FastAPI project: {commands}",
        )

        # 2. Verification of Authority Recommendation
        joined_recs = " ".join(recommendations)
        self.assertNotIn("Maintain Rust as the validation authority", joined_recs)
        self.assertTrue(
            "TypeScript" in joined_recs and "pytest" in joined_recs,
            f"Expected TypeScript and pytest authority recommendation, got: {joined_recs}",
        )

        # 3. Verification of Candidate File Scoring & Semantic Blast Radius
        candidates = result["brain_context_pack"]["candidate_file_scores"]
        candidate_paths = [c["path"] for c in candidates]

        # SulfatosSolublesForm.tsx must be candidate #1
        self.assertIn("SulfatosSolublesForm.tsx", candidate_paths[0])

        # Backups and markdown guide must NOT dominate candidate scores
        self.assertNotIn("backups/db_backup.sql", candidate_paths[:3])
        self.assertNotIn("DEVELOPMENT_AND_DEPLOYMENT_GUIDE.md", candidate_paths[:3])
        self.assertNotIn(".agents/rules/coding_rules.md", candidate_paths[:3])

        # Unrelated dirty files (Comercial, Ingenieria) must have lower score than Sulfatos form
        sulfatos_entry = next(c for c in candidates if "SulfatosSolublesForm.tsx" in c["path"])
        comercial_entry = next((c for c in candidates if "Comercial-Control-CRM" in c["path"]), None)
        if comercial_entry:
            self.assertLess(comercial_entry["score"], sulfatos_entry["score"])

        # 4. Verification that risk is NOT erroneously marked as HIGH (8 files)
        self.assertNotEqual(result["brain_context_pack"]["risk_level"], "high")

    def test_pure_rust_stack_detection(self):
        """Verifies that Rust projects still correctly detect and recommend cargo test."""
        payload = {
            "project": "ozymem",
            "goal": "Optimize tree-sitter AST queries",
            "files": ["crates/ozymem-core/src/lib.rs", "Cargo.toml"],
            "git_context": {"dirty": False, "status_files": []},
        }

        result = run("plan", payload)
        commands = result["structured_plan"]["suggested_commands"]
        recommendations = result["recommendations"]

        self.assertTrue(any("cargo test" in cmd for cmd in commands))
        self.assertFalse(any("npm test" in cmd for cmd in commands))
        self.assertFalse(any("pytest" in cmd for cmd in commands))
        self.assertTrue(any("Rust" in rec for rec in recommendations))

    def test_doc_goal_allows_markdown_candidates(self):
        """Verifies that when the goal explicitly targets documentation, markdown files are scored."""
        payload = {
            "project": "docs-project",
            "goal": "Update README.md documentation and architecture guide",
            "files": ["README.md", "docs/architecture.md", "src/index.ts"],
            "git_context": {"dirty": True, "status_files": [{"status": "M", "path": "README.md"}]},
        }

        result = run("plan", payload)
        candidate_paths = [c["path"] for c in result["brain_context_pack"]["candidate_file_scores"]]
        self.assertIn("README.md", candidate_paths[:2])


if __name__ == "__main__":
    unittest.main()
