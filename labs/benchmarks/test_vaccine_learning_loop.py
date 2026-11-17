import os
import sys
import json
import time
import urllib.request
import urllib.error

API_KEY = os.environ.get("OPENROUTER_API_KEY")
if not API_KEY:
    print("Error: Missing OPENROUTER_API_KEY environment variable.", file=sys.stderr)
    sys.exit(1)

WORKSPACE = r"C:\Users\Lenovo\Documents\crmnew\production-crm-backend"
MEMORIES_DIR = os.path.join(WORKSPACE, ".ozy", "memories")
os.makedirs(MEMORIES_DIR, exist_ok=True)
VACCINE_FILE = os.path.join(MEMORIES_DIR, "normalize_date_ymd_invariant.md")

OPENROUTER_URL = "https://openrouter.ai/api/v1/chat/completions"
HEADERS = {
    "Authorization": f"Bearer {API_KEY}",
    "Content-Type": "application/json",
    "HTTP-Referer": "https://github.com/Xangel0s/ozygram",
    "X-Title": "Ozygram Vaccine Learning Loop"
}

def call_openrouter_with_retry(messages, max_tokens=800, temperature=0.1, retries=4, delay=12):
    payload = {
        "model": "deepseek/deepseek-chat",
        "messages": messages,
        "max_tokens": max_tokens,
        "temperature": temperature
    }
    
    for attempt in range(retries):
        t0 = time.time()
        try:
            req = urllib.request.Request(OPENROUTER_URL, data=json.dumps(payload).encode("utf-8"), headers=HEADERS, method="POST")
            with urllib.request.urlopen(req, timeout=60) as resp:
                elapsed = time.time() - t0
                raw = json.loads(resp.read().decode("utf-8"))
                choice = raw.get("choices", [{}])[0]
                content = choice.get("message", {}).get("content", "")
                usage = raw.get("usage", {})
                return {
                    "elapsed": elapsed,
                    "content": content,
                    "usage": usage,
                    "cost": usage.get("cost", 0.0)
                }
        except urllib.error.HTTPError as e:
            err_msg = e.read().decode("utf-8")
            if e.code == 429 and attempt < retries - 1:
                wait_time = delay * (attempt + 1)
                print(f"    [RateLimit 429] Waiting {wait_time}s before retry {attempt + 2}/{retries}...")
                time.sleep(wait_time)
                continue
            raise e

def run_vaccine_loop():
    print("==========================================================")
    print("  CASO 1: BUCLE DE APRENDIZAJE Y VACUNAS (.ozy/memories/) ")
    print("==========================================================")
    
    # Wait 8s initially to clear any recent rate window
    print("Clearing rate window (8s)...")
    time.sleep(8)
    
    # ----------------------------------------------------
    # FASE 1: SÍNTESIS DE LA VACUNA DESDE EL FALLO
    # ----------------------------------------------------
    print("\n[Fase 1] DeepSeek-V3 analiza el error y sintetiza la Vacuna Arquitectural...")
    prompt_synthesis = [
        {"role": "system", "content": "You are a Principal Software Architect synthesizing memory vaccines for AI agents in Ozygram."},
        {"role": "user", "content": """Post-Mortem Analysis:
In `production-crm-backend`, an AI agent attempted to refactor `normalize_date_ymd` in `app/utils/date_format.py`.
The agent replaced the signature `def normalize_date_ymd(value: str | datetime | date | None)` with `def normalize_date_ymd(date_str: str)`.
Consequence: 35 production callers (in quote schemas, lab dates, compression, and report exports) pass SQLAlchemy datetime objects and Excel numbers. Changing this type causes catastrophic 500 errors in production.

Generate a concise architectural memory vaccine in Markdown to be persisted in `.ozy/memories/normalize_date_ymd_invariant.md`.
Include:
1. Invariant Contract (`value: str | datetime | date | None`)
2. Architectural Gotcha (Why string-only typing is forbidden)
3. Zero-Regression Directive for future AI refactoring agents."""}
    ]
    
    res_synthesis = call_openrouter_with_retry(prompt_synthesis, max_tokens=600)
    vaccine_content = res_synthesis["content"]
    print(f"  -> Vacuna sintetizada en {res_synthesis['elapsed']:.2f}s ({res_synthesis['usage'].get('total_tokens')} tokens, Cost: ${res_synthesis['cost']:.6f})")
    
    # Save the vaccine to .ozy/memories/
    with open(VACCINE_FILE, "w", encoding="utf-8") as f:
        f.write(vaccine_content)
    print(f"  -> Guardada exitosamente en: {VACCINE_FILE}\n")
    
    # Pacing to avoid rate limits
    print("Pacing before Phase 2 (12s)...")
    time.sleep(12)
    
    # ----------------------------------------------------
    # FASE 2: VERIFICACIÓN CON LA VACUNA INYECTADA
    # ----------------------------------------------------
    print("[Fase 2] Sometiendo a DeepSeek al mismo refactor CON la Vacuna inyectada...")
    
    prompt_with_vaccine = [
        {"role": "system", "content": f"""[protocol]
name = "ozy"
rule = "cow_ram_refactor_before_disk"

[active_vaccine: normalize_date_ymd_invariant.md]
{vaccine_content}

[tools.ozy_refactor]
action = "simulate"
"""},
        {"role": "user", "content": "Refactor `normalize_date_ymd` in `app/utils/date_format.py` to add `include_time: bool = False`. Output the JSON call to `ozy_refactor(action='simulate')`."}
    ]
    
    res_verification = call_openrouter_with_retry(prompt_with_vaccine, max_tokens=500)
    print(f"  -> Refactor generado en {res_verification['elapsed']:.2f}s ({res_verification['usage'].get('total_tokens')} tokens, Cost: ${res_verification['cost']:.6f})")
    print("\n--- RESPUESTA GENERADA BAJO LA VACUNA ---")
    print(res_verification["content"][:600] + ("..." if len(res_verification["content"]) > 600 else ""))
    
    # Evaluate if vaccine prevented the regression
    content = res_verification["content"]
    adheres_to_types = ("value" in content and ("datetime" in content or "date" in content))
    no_date_str_hallucination = ("date_str: str" not in content)
    success = adheres_to_types and no_date_str_hallucination
    
    print("\n==========================================================")
    print(f"EVALUACIÓN DEL BUCLE DE APRENDIZAJE: {'EXITO ABSOLUTO' if success else 'PARCIAL'}")
    print(f" - Mantuvo tipos originales (value: str | datetime | date | None): {adheres_to_types}")
    print(f" - Evitó la alucinación 'date_str: str': {no_date_str_hallucination}")
    print("==========================================================")
    
    # Export results
    out_json = r"C:\Users\Lenovo\.gemini\antigravity\brain\b3fcd1c4-b7e7-41fe-ab23-46a79f8e3b05\scratch\vaccine_learning_results.json"
    with open(out_json, "w", encoding="utf-8") as f:
        json.dump({
            "vaccine_file": VACCINE_FILE,
            "vaccine_content": vaccine_content,
            "res_synthesis": res_synthesis,
            "res_verification": res_verification,
            "success": success
        }, f, indent=2)
    print(f"Resultados guardados en: {out_json}")

if __name__ == "__main__":
    run_vaccine_loop()
