import json
import sys

if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8")

from ozy_brain.brain import run
from ozy_brain.schemas import DOMAIN_SYNONYMS

# 22 Archivos reales del repositorio crmnew
real_crmnew_files = [
    "crmnew/crm-geofal/src/components/dashboard/dashboard-home-module.tsx",
    "crmnew/crm-geofal/src/components/dashboard/tracing/custom-report-modal.tsx",
    "crmnew/crm-geofal/src/components/dashboard/tracing/tracing-table.tsx",
    "crmnew/crm-geofal/src/components/dashboard/tracing/columns.tsx",
    "crmnew/crm-geofal/src/components/dashboard/reception/reception-form.tsx",
    "crmnew/crm-geofal/src/components/dashboard/reception/reception-table.tsx",
    "crmnew/crm-geofal/src/components/dashboard/commercial/commercial-module.tsx",
    "crmnew/crm-geofal/src/components/dashboard/commercial/use-cartera-resolver.ts",
    "crmnew/crm-geofal/src/components/dashboard/reports/reports-view.tsx",
    "crmnew/crm-geofal/src/components/dashboard/settings/settings-tab.tsx",
    "crmnew/crm-geofal/src/lib/supabaseClient.ts",
    "crmnew/crm-geofal/src/lib/docqrApi.ts",
    "crmnew/api-geofal-crm/app/main.py",
    "crmnew/api-geofal-crm/app/routers/auth.py",
    "crmnew/api-geofal-crm/app/routers/probetas.py",
    "crmnew/api-geofal-crm/app/routers/informes.py",
    "crmnew/api-geofal-crm/app/routers/clientes.py",
    "crmnew/api-geofal-crm/app/routers/obras.py",
    "crmnew/api-geofal-crm/app/services/pdf_generator.py",
    "crmnew/api-geofal-crm/app/services/outbox.py",
    "crmnew/api-geofal-crm/app/models/database.py",
    "crmnew/api-geofal-crm/app/schemas/informe.py",
]

payload = {
    "project": "crmnew",
    "goal": "editar cambiar pdf cambiar numero de informe drag and drop",
    "files": real_crmnew_files,
    "git_context": {
        "status_files": [], # 0 archivos modificados en git (totalmente limpio)
    },
    "relevant_memories": [
        {
            "title": "DocQR PDF Drag and Drop Handling",
            "content": "Custom report modal implements PDF replacement and signed document synchronization.",
            "linked_files": ["crmnew/crm-geofal/src/components/dashboard/tracing/custom-report-modal.tsx"]
        }
    ]
}

res = run("plan", payload)

structured = res.get("structured_plan", {})
candidates = res.get("brain_context_pack", {}).get("candidate_file_scores", [])
topological = structured.get("topological_edit_sequence", [])

print("=" * 60)
print("[PRUEBA DE FUEGO REAL - FASE 2]")
print(f"Goal: {payload['goal']}")
print(f"Total candidate files evaluados: {len(real_crmnew_files)}")
print(f"Git status files: {len(payload['git_context']['status_files'])} (Git Limpio)")
print("=" * 60)

print("\n--- TOP CANDIDATOS PUNTUADOS ---")
for i, c in enumerate(candidates[:5], 1):
    print(f"#{i} {c['path']} (score: {c['score']}) -> {c['reasons']}")

print("\n--- SECUENCIA TOPOLOGICA GENERADA ---")
for s in topological[:5]:
    print(f"Step {s['step']}: {s['file']} -> {s['action']}")

# Simulación exacta del formateador ultracompacto de brain.rs
confidence = res.get("confidence", 0.90)
goal = structured.get("goal", payload["goal"])
blast = structured.get("blast_radius_analysis", {})
risk_tier = blast.get("risk_tier", "low")
total_impact = blast.get("total_blast_radius", 1)

body = f"# Ozy Brain: plan (Confidence: {confidence:.2f})\n**Goal**: {goal}\n\n"
if topological:
    body += "### Target Files & Edit Sequence\n"
    for item in topological[:5]:
        body += f"{item['step']}. `{item['file']}` → {item['action']}\n"
    body += "\n"

body += f"### Blast Radius & Risks\n- Risk Tier: {risk_tier.upper()} | Impact: {total_impact} file(s)\n"
for r in res.get("risks", [])[:2]:
    body += f"- Risk: {r}\n"

if structured.get("suggested_commands"):
    cmd = structured["suggested_commands"][0]
    body += f"\n### Validation\n- `{cmd}`\n"

print("\n" + "=" * 60)
print("--- OUTPUT ULTRACOMPACTO DEVUELTO AL MCP CLIENT ---")
print("=" * 60)
print(body)
print("=" * 60)

# Estimación de tokens: palabras / 0.75 y caracteres / 4
token_est_words = len(body.split()) / 0.75
token_est_chars = len(body) / 4.0
print(f"Caracteres totales: {len(body)}")
print(f"Palabras totales: {len(body.split())}")
print(f"Tokens estimados (método chars): {token_est_chars:.1f} tokens")
print(f"Tokens estimados (método words): {token_est_words:.1f} tokens")

assert token_est_chars < 250, f"ERROR: Output excede 250 tokens ({token_est_chars:.1f})"
assert token_est_words < 250, f"ERROR: Output excede 250 tokens ({token_est_words:.1f})"
print("[VALIDACION EXITOSA]: Output ultracompacto mide MENOS de 250 tokens (nunca se volcará a output.txt)!")

top_file = candidates[0]["path"] if candidates else ""
assert "custom-report-modal.tsx" in top_file, f"ERROR: Top 1 esperado 'custom-report-modal.tsx', pero fue '{top_file}'"
print("[VERIFICACION EXITOSA]: 'custom-report-modal.tsx' es TOP 1 sin sesgo de Git dirty!")
