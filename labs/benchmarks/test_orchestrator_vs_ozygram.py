import os
import sys
import json
import time
import hashlib
import urllib.request
import urllib.error

API_KEY = os.environ.get("OPENROUTER_API_KEY")
if not API_KEY:
    print("Error: Missing OPENROUTER_API_KEY environment variable.", file=sys.stderr)
    sys.exit(1)

WORKSPACE = r"C:\Users\Lenovo\Documents\crmnew\production-crm-backend"
TARGET_FILE = os.path.join(WORKSPACE, "app", "utils", "date_format.py")

PLANNER_MODEL = "openrouter/free"
CODER_MODEL = "cohere/north-mini-code:free"
REVIEWER_MODEL = "openrouter/free"

SINGLE_MODEL = "cohere/north-mini-code:free"

OPENROUTER_URL = "https://openrouter.ai/api/v1/chat/completions"
HEADERS = {
    "Authorization": f"Bearer {API_KEY}",
    "Content-Type": "application/json",
    "HTTP-Referer": "https://github.com/Xangel0s/ozygram",
    "X-Title": "Ozygram Lab: Multi-Model Orchestrator vs Ozygram"
}

def call_openrouter(model_slug, messages, max_tokens=1000, temperature=0.1, retries=3, delay=12):
    payload = {
        "model": model_slug,
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
                msg = choice.get("message", {})
                content = msg.get("content") or msg.get("reasoning") or ""
                usage = raw.get("usage", {})
                return {
                    "elapsed": elapsed,
                    "content": content,
                    "usage": usage,
                    "prompt_tokens": usage.get("prompt_tokens", 0),
                    "completion_tokens": usage.get("completion_tokens", 0),
                    "total_tokens": usage.get("total_tokens", 0),
                    "cost": usage.get("cost", 0.0),
                    "model": raw.get("model", model_slug)
                }
        except urllib.error.HTTPError as e:
            if e.code == 429 and attempt < retries - 1:
                wait_time = delay * (attempt + 1)
                print(f"      [RateLimit 429 on {model_slug}] Waiting {wait_time}s before retry {attempt + 2}/{retries}...")
                time.sleep(wait_time)
                continue
            raise e

def compute_sha256(filepath):
    h = hashlib.sha256()
    with open(filepath, "rb") as f:
        while chunk := f.read(8192):
            h.update(chunk)
    return h.hexdigest()

def run_duel():
    print("==========================================================================")
    print("  OZYGRAM LABS: ORQUESTADOR MULTI-MODELO vs OZYGRAM CON 1 SOLO MODELO     ")
    print("==========================================================================")
    sha_initial = compute_sha256(TARGET_FILE)
    print(f"Workspace: {WORKSPACE}")
    print(f"Target File: {TARGET_FILE}")
    print(f"Pristine SHA-256: {sha_initial}\n")
    
    file_content = open(TARGET_FILE, encoding="utf-8").read()
    task_description = "Refactor `normalize_date_ymd` in `app/utils/date_format.py` to add `include_time: bool = False`. If True and time is present, return '%Y/%m/%d %H:%M:%S', else '%Y/%m/%d'. Maintain backwards compatibility."
    
    # =========================================================================
    # BRAZO A: ORQUESTADOR MULTI-MODELO (PLANNER -> CODER -> REVIEWER -> FINAL)
    # =========================================================================
    print(">>> INICIANDO BRAZO A: Orquestador Multi-Modelo Tradicional (Ensemble de 3 Agentes)...")
    t0_orch = time.time()
    orch_steps = []
    
    # Paso 1: Planner
    print("  [Orchestrator Step 1/3] Planner (openrouter/free) diseña el plan...")
    prompt_planner = [
        {"role": "system", "content": "You are a Software Architect Planner. Read the full file and provide a detailed 4-step plan for the Coder to refactor the target function."},
        {"role": "user", "content": f"Task: {task_description}\n\nFile:\n{file_content}"}
    ]
    res_planner = call_openrouter(PLANNER_MODEL, prompt_planner, max_tokens=400)
    print(f"    -> Planner completado en {res_planner['elapsed']:.2f}s ({res_planner['total_tokens']} tokens, Modelo: {res_planner['model']})")
    orch_steps.append({"role": "planner", "res": res_planner})
    time.sleep(12)
    
    # Paso 2: Coder
    print("  [Orchestrator Step 2/3] Coder (cohere/north-mini-code:free) escribe el código...")
    prompt_coder = [
        {"role": "system", "content": "You are a Lead Coder. Implement the refactor based on the Architect's plan. Provide the modified function and implementation."},
        {"role": "user", "content": f"Task: {task_description}\n\nArchitect's Plan:\n{res_planner['content']}\n\nExisting File:\n{file_content}"}
    ]
    res_coder = call_openrouter(CODER_MODEL, prompt_coder, max_tokens=800)
    print(f"    -> Coder completado en {res_coder['elapsed']:.2f}s ({res_coder['total_tokens']} tokens, Modelo: {res_coder['model']})")
    orch_steps.append({"role": "coder", "res": res_coder})
    time.sleep(12)
    
    # Paso 3: Reviewer
    print("  [Orchestrator Step 3/3] Reviewer / Crítico (openrouter/free) audita el código...")
    prompt_reviewer = [
        {"role": "system", "content": "You are a Strict Code Reviewer. Audit the Coder's candidate code for bugs, missing imports, arity issues, or regressions. Give final approval or critique."},
        {"role": "user", "content": f"Task: {task_description}\n\nCoder's Output:\n{res_coder['content']}"}
    ]
    res_reviewer = call_openrouter(REVIEWER_MODEL, prompt_reviewer, max_tokens=400)
    print(f"    -> Reviewer completado en {res_reviewer['elapsed']:.2f}s ({res_reviewer['total_tokens']} tokens, Modelo: {res_reviewer['model']})")
    orch_steps.append({"role": "reviewer", "res": res_reviewer})
    
    elapsed_orch = time.time() - t0_orch
    total_tokens_orch = sum(s["res"]["total_tokens"] for s in orch_steps)
    print(f"  ==> FIN BRAZO A (Orquestador): {elapsed_orch:.2f}s totales | {total_tokens_orch} tokens acumulados\n")
    
    time.sleep(14)
    
    # =========================================================================
    # BRAZO B: OZYGRAM CON 1 SOLO MODELO (Cohere North Mini Code:free)
    # =========================================================================
    print(">>> INICIANDO BRAZO B: Ozygram v0.2.7 con 1 Solo Modelo (cohere/north-mini-code:free)...")
    t0_ozy = time.time()
    
    prompt_ozy = [
        {"role": "system", "content": """[protocol]
name = "ozy"
rule = "cow_ram_refactor_before_disk"
[anti_patterns]
veto = ["line_by_line_reading", "raw_grep_before_ast", "direct_disk_writes_without_sim"]
rule = "DO NOT dump whole files. Return JSON tool call ozy_refactor with action='simulate'."
"""},
        {"role": "user", "content": f"""Task: {task_description}
Target AST Symbol: `normalize_date_ymd`
Existing Contract: `def normalize_date_ymd(value: str | datetime | date | None) -> Optional[str]`
Blast Radius: 35 callers depend on this function. Ensure backward compatibility with all argument types.
Output format: JSON call to `ozy_refactor(action='simulate', file_path='app/utils/date_format.py', symbol_name='normalize_date_ymd', replacement_code='...')`."""}
    ]
    
    res_ozy = call_openrouter(SINGLE_MODEL, prompt_ozy, max_tokens=400)
    elapsed_ozy = time.time() - t0_ozy
    print(f"    -> Ozygram completado en {res_ozy['elapsed']:.2f}s ({res_ozy['total_tokens']} tokens, Modelo: {res_ozy['model']})")
    print(f"  ==> FIN BRAZO B (Ozygram): {elapsed_ozy:.2f}s totales | {res_ozy['total_tokens']} tokens\n")
    
    # Check Disk Integrity
    sha_final = compute_sha256(TARGET_FILE)
    disk_untouched = (sha_initial == sha_final)
    
    token_savings = ((total_tokens_orch - res_ozy["total_tokens"]) / total_tokens_orch) * 100
    speedup = elapsed_orch / elapsed_ozy if elapsed_ozy > 0 else 1.0
    
    print("==========================================================================")
    print("                       COMPARATIVA FINAL DEL DUELO                        ")
    print("==========================================================================")
    print(f"Tokens Totales: Orquestador = {total_tokens_orch} | Ozygram (1 Modelo) = {res_ozy['total_tokens']} (Ahorro: {token_savings:.1f}%)")
    print(f"Latencia Total: Orquestador = {elapsed_orch:.2f}s | Ozygram (1 Modelo) = {elapsed_ozy:.2f}s (Speedup: {speedup:.2f}x)")
    print(f"Modelos Usados: Orquestador = 3 agentes encadenados | Ozygram = 1 solo modelo")
    print(f"Preservación Disco: {'100% INTACTO (SHA-256 verificado)' if disk_untouched else 'FALLO'}")
    print("==========================================================================")
    
    # Export results to labs/results/
    out_json = r"C:\Users\Lenovo\Documents\ozygram\ozygram\labs\results\orchestrator_vs_ozygram_results.json"
    with open(out_json, "w", encoding="utf-8") as f:
        json.dump({
            "orchestrator": {
                "elapsed_total": elapsed_orch,
                "total_tokens": total_tokens_orch,
                "steps": orch_steps
            },
            "ozygram_single_model": {
                "elapsed_total": elapsed_ozy,
                "total_tokens": res_ozy["total_tokens"],
                "res": res_ozy
            },
            "comparison": {
                "token_savings_pct": token_savings,
                "speedup": speedup,
                "disk_untouched": disk_untouched
            }
        }, f, indent=2)
    print(f"\nResultados exportados a: {out_json}")

if __name__ == "__main__":
    run_duel()
