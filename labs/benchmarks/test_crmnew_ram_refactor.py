import subprocess
import json
import os
import sys

sys.stdout.reconfigure(line_buffering=True)

exe = r"C:\Users\Lenovo\Documents\ozygram\ozygram\target\release\ozymem-server.exe"
project_root = r"C:\Users\Lenovo\Documents\crmnew"

env = os.environ.copy()
env["OZYGRAM_PROJECT_ROOT"] = project_root

proc = subprocess.Popen(
    [exe],
    stdin=subprocess.PIPE,
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
    text=True,
    bufsize=1,
    env=env
)

def send_and_wait(req):
    req_str = json.dumps(req) + "\n"
    proc.stdin.write(req_str)
    proc.stdin.flush()
    while True:
        line = proc.stdout.readline()
        if not line: return None
        line = line.strip()
        if not line: continue
        val = json.loads(line)
        if val.get("id") == req.get("id"):
            return val

print("1. Initialize server for crmnew...")
send_and_wait({
    "jsonrpc": "2.0", "id": 1, "method": "initialize",
    "params": {"protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "test", "version": "1"}}
})

target_file = "production-crm-backend/app/utils/date_format.py"

print(f"\n2. [RAM Graph] Calculate Blast Radius for {target_file} in crmnew...")
r_graph = send_and_wait({
    "jsonrpc": "2.0", "id": 2, "method": "tools/call",
    "params": {
        "name": "ozy_graph",
        "arguments": {
            "action": "impact",
            "file_path": target_file,
            "max_hops": 2
        }
    }
})
print("Impact Output:", r_graph["result"]["content"][0]["text"])

print(f"\n3. [RAM Contracts] Extract interface contract for {target_file}...")
r_ctx = send_and_wait({
    "jsonrpc": "2.0", "id": 3, "method": "tools/call",
    "params": {
        "name": "ozy_context",
        "arguments": {
            "file_path": target_file,
            "lod": "contracts"
        }
    }
})
print("Contracts Output snippet:\n", r_ctx["result"]["content"][0]["text"][:400])

# Legacy code snippet in date_format.py
legacy_target = """def normalize_date_ymd(value: str | datetime | date | None) -> Optional[str]:
    dt = parse_flexible_date(value)
    if not dt:
        return None
    return dt.strftime("%Y/%m/%d")"""

# Refactored modern snippet with typeguard and fast-path
refactored_replacement = """def normalize_date_ymd(value: str | datetime | date | None) -> Optional[str]:
    # Fast path for already formatted string
    if isinstance(value, str) and len(value) == 10 and value[4] == '/' and value[7] == '/':
        return value
    dt = parse_flexible_date(value)
    return dt.strftime("%Y/%m/%d") if dt is not None else None"""

print(f"\n4. [Volatile RAM Simulation] Simulating refactored AST patch in memory for crmnew...")
r_sim = send_and_wait({
    "jsonrpc": "2.0", "id": 4, "method": "tools/call",
    "params": {
        "name": "ozy_patch",
        "arguments": {
            "action": "simulate",
            "file_path": target_file,
            "target_code": legacy_target,
            "replacement": refactored_replacement
        }
    }
})
sim_output = r_sim["result"]["content"][0]["text"]
print("Simulation result:\n", sim_output[:450])

# Verify disk integrity in crmnew
with open(os.path.join(project_root, target_file), "r", encoding="utf-8") as f:
    disk_content = f.read()

is_disk_unmodified = "Fast path for already formatted string" not in disk_content
print("\n5. Disk integrity check in crmnew:")
print(f"   Has crmnew disk file remained 100% UNCHANGED? {is_disk_unmodified}")

proc.kill()
