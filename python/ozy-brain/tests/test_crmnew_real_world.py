import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from ozy_brain.main import run


class RealWorldCrmnewStressTests(unittest.TestCase):
    def setUp(self):
        self.crmnew_git_status = [
            {"status": "M", "path": "Comercial-Control-CRM"},
            {"status": "M", "path": "api-geofal-crm"},
            {"status": "M", "path": "crm-geofal"},
            {"status": "M", "path": "scripts/record_playwright_video_verificacion.py"},
            {"status": "??", "path": ".agents/rules/"},
            {"status": "??", "path": "DEVELOPMENT_AND_DEPLOYMENT_GUIDE.md"},
            {"status": "??", "path": "Ingenieria/"},
            {"status": "??", "path": "backups/"},
            {"status": "??", "path": "docs/"},
            {"status": "??", "path": "downloads/"},
            {"status": "??", "path": "incidentes/"},
            {"status": "??", "path": "marketing_publicidad.md"},
            {"status": "??", "path": "scripts/test_crm_docqr_e2e_video.py"},
            {"status": "??", "path": "start-dev.bat"},
            {"status": "??", "path": "stop-dev.bat"},
            {"status": "??", "path": "supabase/"},
        ]

    def test_sulfatos_quimicos_goal_on_real_crmnew(self):
        """Simulate real user request: Sulfatos Solubles NTP 339.178 with messy crmnew workspace."""
        payload = {
            "project": "crmnew",
            "goal": "Ajustar el redondeo de Sulfatos Solubles (NTP 339.178) en SulfatosSolublesForm.tsx y schemas.py",
            "files": [
                "crm-geofal/src/components/dashboard/SulfatosSolublesForm.tsx",
                "api-geofal-crm/app/modules/schemas.py",
                "api-geofal-crm/app/services/excel.py",
                "crm-geofal/package.json",
                "api-geofal-crm/pyproject.toml",
            ],
            "git_context": {
                "dirty": True,
                "status_files": self.crmnew_git_status + [
                    {"status": "M", "path": "crm-geofal/src/components/dashboard/SulfatosSolublesForm.tsx"}
                ],
            },
            "changes": [
                "crm-geofal/src/components/dashboard/SulfatosSolublesForm.tsx",
                "api-geofal-crm/app/modules/schemas.py",
            ],
            "memories": [
                {"title": "Validación de Ensayos Químicos en Geofal CRM", "content": "Sulfatos Solubles NTP 339.178 debe redondear PPM a enteros"}
            ],
        }

        result = run("plan", payload)
        struct_plan = result["structured_plan"]
        candidates = result["brain_context_pack"]["candidate_file_scores"]
        cand_paths = [c["path"] for c in candidates]

        # 1. No Rust leak
        self.assertFalse(any("cargo" in c for c in struct_plan["suggested_commands"]))
        self.assertTrue(any("npm test" in c or "pnpm" in c for c in struct_plan["suggested_commands"]))
        self.assertTrue(any("pytest" in c for c in struct_plan["suggested_commands"]))

        # 2. Candidate precision: Sulfatos must be #1
        self.assertIn("SulfatosSolublesForm.tsx", cand_paths[0])
        self.assertTrue(any("schemas.py" in p for p in cand_paths[:3]))

        # 3. Satellite directories must NOT crowd out code
        self.assertNotIn("backups/", cand_paths[:3])
        self.assertNotIn(".agents/rules/", cand_paths[:3])
        self.assertNotIn("DEVELOPMENT_AND_DEPLOYMENT_GUIDE.md", cand_paths[:3])
        self.assertNotIn("Ingenieria/", cand_paths[:3])

        # 4. Topological sequence must prioritize schemas/contracts before UI
        seq = struct_plan["topological_edit_sequence"]
        seq_files = [item["file"] for item in seq]
        if "api-geofal-crm/app/modules/schemas.py" in seq_files and "crm-geofal/src/components/dashboard/SulfatosSolublesForm.tsx" in seq_files:
            schema_idx = seq_files.index("api-geofal-crm/app/modules/schemas.py")
            form_idx = seq_files.index("crm-geofal/src/components/dashboard/SulfatosSolublesForm.tsx")
            self.assertLess(schema_idx, form_idx, "Base schemas must be refactored before frontend forms")

    def test_fastapi_backend_only_migration_goal(self):
        """Test backend-only DB migration goal in api-geofal-crm."""
        payload = {
            "project": "crmnew",
            "goal": "Crear migracion 071_agregar_indice_usuarios.sql para tabla usuarios en api-geofal-crm",
            "files": [
                "api-geofal-crm/migrations/070_actualizar_ejecutivocomercial3_telefono.sql",
                "api-geofal-crm/app/models/usuarios.py",
                "api-geofal-crm/pyproject.toml",
            ],
            "git_context": {
                "dirty": True,
                "status_files": self.crmnew_git_status,
            },
        }

        result = run("plan", payload)
        struct_plan = result["structured_plan"]
        commands = struct_plan["suggested_commands"]

        # Backend-only should suggest pytest
        self.assertTrue(any("pytest" in c for c in commands))
        # Should not suggest npm test when only python/sql files are touched
        self.assertFalse(any("cargo" in c for c in commands))
        self.assertEqual(result["brain_context_pack"]["risk_level"], "high")  # "migration" in goal -> high risk

    def test_frontend_only_component_goal(self):
        """Test frontend-only component goal in crm-geofal."""
        payload = {
            "project": "crmnew",
            "goal": "Corregir renderizado de sales-solubles-module.tsx en crm-geofal",
            "files": [
                "crm-geofal/src/components/dashboard/sales-solubles-module.tsx",
                "crm-geofal/package.json",
                "crm-geofal/tsconfig.json",
            ],
            "git_context": {
                "dirty": True,
                "status_files": self.crmnew_git_status,
            },
        }

        result = run("plan", payload)
        struct_plan = result["structured_plan"]
        commands = struct_plan["suggested_commands"]

        # Frontend-only should suggest npm test and npm run build
        self.assertTrue(any("npm test" in c or "pnpm test" in c for c in commands))
        self.assertTrue(any("npm run build" in c for c in commands))
        # Should NOT suggest pytest or cargo test
        self.assertFalse(any("cargo" in c for c in commands))
        self.assertFalse(any("pytest" in c for c in commands))


if __name__ == "__main__":
    unittest.main()
