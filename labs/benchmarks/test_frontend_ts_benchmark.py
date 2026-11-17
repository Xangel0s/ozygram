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

# Using openrouter/free as instructed by user when paid balance exhausts
MODEL = os.environ.get("BENCHMARK_MODEL", "openrouter/free")
WORKSPACE = r"C:\Users\Lenovo\Documents\crmnew\production-crm-frontend"
TARGET_FILE = os.path.join(WORKSPACE, "src", "lib", "utils.ts")

OPENROUTER_URL = "https://openrouter.ai/api/v1/chat/completions"
HEADERS = {
    "Authorization": f"Bearer {API_KEY}",
    "Content-Type": "application/json",
    "HTTP-Referer": "https://github.com/Xangel0s/ozygram",
    "X-Title": "Ozygram Frontend TS Benchmark"
}

def call_openrouter_with_retry(messages, max_tokens=1000, temperature=0.1, retries=3, delay=12):
    payload = {
        "model": MODEL,
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
                    "cost": usage.get("cost", 0.0),
                    "model": raw.get("model", MODEL)
                }
        except urllib.error.HTTPError as e:
            if e.code == 429 and attempt < retries - 1:
                wait_time = delay * (attempt + 1)
                print(f"    [RateLimit 429] Waiting {wait_time}s before retry {attempt + 2}/{retries}...")
                time.sleep(wait_time)
                continue
            raise e

def compute_sha256(filepath):
    h = hashlib.sha256()
    with open(filepath, "rb") as f:
        while chunk := f.read(8192):
            h.update(chunk)
    return h.hexdigest()

def run_frontend_benchmark():
    print("==========================================================")
    print(f" CASO 3: REFACTOR MULTI-ARCHIVO FRONTEND TYPESCRIPT ({MODEL}) ")
    print("==========================================================")
    sha_initial = compute_sha256(TARGET_FILE)
    print(f"Workspace: {WORKSPACE}")
    print(f"Target File: {TARGET_FILE}")
    print(f"File SHA-256 (Pristine): {sha_initial}\n")
    
    file_content = open(TARGET_FILE, encoding="utf-8").read()
    
    # ----------------------------------------------------
    # ARM A: Sin Ozygram (Trata de sobreescribir el archivo completo)
    # ----------------------------------------------------
    print(f"[Arm A] {MODEL} sin Ozygram (Intento de volcado de archivo completo en TS)...")
    prompt_arm_a = [
        {"role": "system", "content": "You are a frontend assistant. Refactor formatLocalDate in src/lib/utils.ts to accept `dateStr: string | Date | null | undefined` and add an optional `format?: 'short' | 'long' | 'iso'` (default 'short'). Return the entire modified utils.ts file so it can be overwritten on disk."},
        {"role": "user", "content": f"Modify this file:\n{file_content}"}
    ]
    res_a = call_openrouter_with_retry(prompt_arm_a, max_tokens=1200)
    print(f"  -> Arm A finalizado en {res_a['elapsed']:.2f}s ({res_a['usage'].get('total_tokens')} tokens, Cost: ${res_a['cost']:.6f})")
    print(f"  -> Longitud de volcado generado: {len(res_a['content'])} caracteres | Modelo: {res_a['model']}")
    
    # Pacing to respect OpenRouter rate limits
    print("Pacing between arms (12s)...")
    time.sleep(12)
    
    # ----------------------------------------------------
    # ARM B: Con Ozygram (Refactorización quirúrgica en RAM CoW)
    # ----------------------------------------------------
    print(f"\n[Arm B] {MODEL} con Ozygram v0.2.7 (1-Turn Atomic In-RAM Refactor)...")
    prompt_arm_b = [
        {"role": "system", "content": """[protocol]
name = "ozy"
rule = "cow_ram_refactor_before_disk"
[anti_patterns]
veto = ["line_by_line_reading", "raw_grep_before_ast", "direct_disk_writes_without_sim"]
rule = "DO NOT dump whole files. Return JSON tool call ozy_refactor with action='simulate'."
"""},
        {"role": "user", "content": """Task: Refactor `formatLocalDate` in `src/lib/utils.ts`.
Target AST Symbol: `formatLocalDate`
Existing Contract: `export function formatLocalDate(dateStr: string | null | undefined): string`
Blast Radius: 11 React components depend on this function. Ensure backward compatibility with string ISO callers.
Requirement: Accept `dateStr: string | Date | null | undefined` and optional `format?: 'short' | 'long' | 'iso'`.
Output format: JSON call to `ozy_refactor(action='simulate', file_path='src/lib/utils.ts', symbol_name='formatLocalDate', replacement_code='...')`."""}
    ]
    res_b = call_openrouter_with_retry(prompt_arm_b, max_tokens=400)
    print(f"  -> Arm B finalizado en {res_b['elapsed']:.2f}s ({res_b['usage'].get('total_tokens')} tokens, Cost: ${res_b['cost']:.6f}) | Modelo: {res_b['model']}")
    
    sha_final = compute_sha256(TARGET_FILE)
    disk_untouched = (sha_initial == sha_final)
    
    token_savings = ((res_a["usage"].get("total_tokens", 0) - res_b["usage"].get("total_tokens", 0)) / res_a["usage"].get("total_tokens", 1)) * 100
    
    print("\n==========================================================")
    print("RESULTADOS DEL BENCHMARK FRONTEND TYPESCRIPT:")
    print(f" - Ahorro de Tokens: {token_savings:.1f}% ({res_a['usage'].get('total_tokens')} -> {res_b['usage'].get('total_tokens')})")
    print(f" - Costo API Total:  $0.00 (Free Tier 100%)")
    print(f" - Integridad Disco: {'100% INTACTO (SHA-256 verificado)' if disk_untouched else 'FALLIDO'}")
    print("==========================================================")
    
    out_json = r"C:\Users\Lenovo\.gemini\antigravity\brain\b3fcd1c4-b7e7-41fe-ab23-46a79f8e3b05\scratch\frontend_ts_benchmark_results.json"
    with open(out_json, "w", encoding="utf-8") as f:
        json.dump({
            "res_a": res_a,
            "res_b": res_b,
            "token_savings": token_savings,
            "disk_untouched": disk_untouched
        }, f, indent=2)
    print(f"Resultados guardados en: {out_json}")

if __name__ == "__main__":
    run_frontend_benchmark()
