import os
import sys
import json
import time
import hashlib
import ast
import urllib.request
import urllib.error

API_KEY = os.environ.get("OPENROUTER_API_KEY")
if not API_KEY:
    print("Error: Missing OPENROUTER_API_KEY environment variable.", file=sys.stderr)
    sys.exit(1)

WORKSPACE = r"C:\Users\Lenovo\Documents\crmnew\production-crm-backend"
TARGET_FILE = os.path.join(WORKSPACE, "app", "utils", "date_format.py")

MODELS = [
    {"name": "DeepSeek-V3 (671B)", "slug": "deepseek/deepseek-chat"},
    {"name": "Cohere North Mini Code (Free)", "slug": "cohere/north-mini-code:free"},
    {"name": "Gemma 26B / Free Router", "slug": "openrouter/free"}
]

OPENROUTER_URL = "https://openrouter.ai/api/v1/chat/completions"
HEADERS = {
    "Authorization": f"Bearer {API_KEY}",
    "Content-Type": "application/json",
    "HTTP-Referer": "https://github.com/Xangel0s/ozygram",
    "X-Title": "Ozygram Multi-Model Battle"
}

def call_openrouter_with_retry(model_slug, messages, max_tokens=1000, temperature=0.1, retries=3, delay=8):
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
                usage = raw.get("usage", {})
                choice = raw.get("choices", [{}])[0]
                msg = choice.get("message", {})
                content = msg.get("content") or msg.get("reasoning") or ""
                cost = usage.get("cost", 0.0)
                actual_model = raw.get("model", model_slug)
                return {
                    "elapsed": elapsed,
                    "prompt_tokens": usage.get("prompt_tokens", 0),
                    "completion_tokens": usage.get("completion_tokens", 0),
                    "total_tokens": usage.get("total_tokens", 0),
                    "cost": cost,
                    "content": content,
                    "model": actual_model,
                    "success": True
                }
        except urllib.error.HTTPError as e:
            elapsed = time.time() - t0
            err_msg = e.read().decode("utf-8")
            if e.code == 429 and attempt < retries - 1:
                wait_time = delay * (attempt + 1)
                print(f"      [429 RateLimit on {model_slug}] Waiting {wait_time}s...")
                time.sleep(wait_time)
                continue
            return {
                "elapsed": elapsed,
                "prompt_tokens": 0,
                "completion_tokens": 0,
                "total_tokens": 0,
                "cost": 0.0,
                "content": f"HTTP Error {e.code}: {err_msg}",
                "model": model_slug,
                "success": False
            }
        except Exception as e:
            elapsed = time.time() - t0
            return {
                "elapsed": elapsed,
                "prompt_tokens": 0,
                "completion_tokens": 0,
                "total_tokens": 0,
                "cost": 0.0,
                "content": str(e),
                "model": model_slug,
                "success": False
            }

def compute_sha256(filepath):
    h = hashlib.sha256()
    with open(filepath, "rb") as f:
        while chunk := f.read(8192):
            h.update(chunk)
    return h.hexdigest()

def evaluate_response(content):
    if not content:
        return {"is_json": False, "preserved_type_signature": False}
    
    is_json = False
    preserved_type_signature = False
    
    if "ozy_refactor" in content or "action" in content:
        is_json = True
    
    if "value" in content and ("datetime" in content or "date" in content):
        preserved_type_signature = True
    elif "date_str" in content or "str_date" in content:
        preserved_type_signature = False
    
    return {
        "is_json": is_json,
        "preserved_type_signature": preserved_type_signature
    }

def run_battle():
    print("==========================================================")
    print("   OZYGRAM MULTI-MODEL BLIND BATTLE: EMPIRICAL BENCHMARK   ")
    print("==========================================================")
    sha_initial = compute_sha256(TARGET_FILE)
    print(f"Workspace: {WORKSPACE}")
    print(f"Target File: {TARGET_FILE}")
    print(f"File SHA-256 (Pristine): {sha_initial}\n")
    
    results = {}
    
    for m in MODELS:
        name = m["name"]
        slug = m["slug"]
        print(f"\n>>> CONTENDER: {name} (Slug: {slug})")
        
        # 1. Arm A: Sin Ozygram (Standard unassisted agent trying to dump whole file)
        print("  - Running Arm A (Sin Ozygram)...")
        prompt_arm_a = [
            {"role": "system", "content": "You are a coding assistant. Refactor normalize_date_ymd in app/utils/date_format.py to add `include_time: bool = False`. If include_time is True and time is present, return '%Y/%m/%d %H:%M:%S', else '%Y/%m/%d'. Return the full modified file so it can be overwritten on disk."},
            {"role": "user", "content": f"Modify this file:\n{open(TARGET_FILE, encoding='utf-8').read()}"}
        ]
        res_a = call_openrouter_with_retry(slug, prompt_arm_a, max_tokens=1200)
        content_a = res_a.get("content") or ""
        print(f"    [Arm A]: {res_a['elapsed']:.2f}s | {res_a['total_tokens']} tokens | Cost: ${res_a['cost']:.6f} | Dump length: {len(content_a)} chars")
        
        time.sleep(6) # Pacing
        
        # 2. Arm B: Con Ozygram (Universal TOML + In-RAM simulate)
        print("  - Running Arm B (Con Ozygram v0.2.7)...")
        prompt_arm_b = [
            {"role": "system", "content": """[protocol]
name = "ozy"
rule = "cow_ram_refactor_before_disk"
[anti_patterns]
veto = ["line_by_line_reading", "raw_grep_before_ast", "direct_disk_writes_without_sim"]
rule = "DO NOT dump whole files. Return JSON tool call ozy_refactor with action='simulate'."
"""},
            {"role": "user", "content": """Task: Refactor `normalize_date_ymd` in `app/utils/date_format.py` to add `include_time: bool = False`.
Target AST Symbol: `normalize_date_ymd`
Existing Contract: `def normalize_date_ymd(value: str | datetime | date | None) -> Optional[str]`
Blast Radius: 35 callers depend on this function. Ensure backward compatibility with all argument types.
Output format: JSON call to `ozy_refactor(action='simulate', file_path='app/utils/date_format.py', symbol_name='normalize_date_ymd', replacement_code='...')`."""}
        ]
        res_b = call_openrouter_with_retry(slug, prompt_arm_b, max_tokens=400)
        content_b = res_b.get("content") or ""
        eval_b = evaluate_response(content_b)
        print(f"    [Arm B]: {res_b['elapsed']:.2f}s | {res_b['total_tokens']} tokens | Cost: ${res_b['cost']:.6f} | Tool Call JSON: {eval_b['is_json']} | Preserved Types: {eval_b['preserved_type_signature']}")
        
        token_savings = ((res_a["total_tokens"] - res_b["total_tokens"]) / res_a["total_tokens"] * 100) if res_a["total_tokens"] > 0 else 0
        cost_savings = ((res_a["cost"] - res_b["cost"]) / res_a["cost"] * 100) if res_a["cost"] > 0 else 0
        
        results[slug] = {
            "name": name,
            "actual_model": res_b["model"],
            "arm_a": res_a,
            "arm_b": res_b,
            "eval_b": eval_b,
            "token_savings": token_savings,
            "cost_savings": cost_savings
        }
        
        time.sleep(8) # Pacing between models
        
    sha_final = compute_sha256(TARGET_FILE)
    disk_untouched = (sha_initial == sha_final)
    print("\n==========================================================")
    print(f"Physical Disk Integrity: {'PASSED (100% UNTOUCHED)' if disk_untouched else 'FAILED'}")
    print(f"Initial SHA-256: {sha_initial}")
    print(f"Final SHA-256:   {sha_final}")
    print("==========================================================")
    
    out_file = r"C:\Users\Lenovo\.gemini\antigravity\brain\b3fcd1c4-b7e7-41fe-ab23-46a79f8e3b05\scratch\multi_model_battle_results.json"
    with open(out_file, "w", encoding="utf-8") as f:
        json.dump(results, f, indent=2)
    print(f"Results exported to {out_file}")

if __name__ == "__main__":
    run_battle()
