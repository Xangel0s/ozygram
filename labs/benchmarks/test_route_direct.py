import subprocess
import json
import os

SERVER_EXE = r"C:\Users\Lenovo\Documents\ozygram\ozygram\target\release\ozymem-server.exe"
PROJECT_DIR = r"C:\Users\Lenovo\Documents\crmnew"

proc = subprocess.Popen(
    [SERVER_EXE],
    cwd=PROJECT_DIR,
    stdin=subprocess.PIPE,
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
    text=True,
    encoding="utf-8"
)

def rpc(req):
    proc.stdin.write(json.dumps(req) + "\n")
    proc.stdin.flush()
    while True:
        line = proc.stdout.readline()
        if not line:
            raise EOFError("EOF")
        line = line.strip()
        if not line:
            continue
        try:
            return json.loads(line)
        except Exception as e:
            print("NON-JSON:", line)

# 1. Initialize
init_req = {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "initialize",
    "params": {
        "protocolVersion": "2024-11-05",
        "capabilities": {},
        "clientInfo": {"name": "test", "version": "1.0"},
        "rootUri": f"file:///{PROJECT_DIR.replace(chr(92), '/')}"
    }
}
print("Initializing...", flush=True)
res = rpc(init_req)
print("Initialized! Instructions length:", len(res.get("result", {}).get("instructions", "")), flush=True)

# 2. Test action="route"
route_req = {
    "jsonrpc": "2.0",
    "id": 2,
    "method": "tools/call",
    "params": {
        "name": "ozy_explore",
        "arguments": {
            "action": "route",
            "query": "Refactoriza normalize_date_ymd para aceptar timestamps en date_format"
        }
    }
}
print("Calling ozy_explore(action='route')...", flush=True)
route_res = rpc(route_req)
print("Route response received!", flush=True)
print(route_res.get("result", {}).get("content", [{}])[0].get("text", ""))

proc.terminate()
print("Done!", flush=True)
