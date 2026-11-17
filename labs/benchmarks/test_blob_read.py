import subprocess
import json
import os
import sys

sys.stdout.reconfigure(line_buffering=True)

exe = r"C:\Users\Lenovo\Documents\ozygram\ozygram\target\release\ozymem-server.exe"
env = os.environ.copy()
env["OZYGRAM_PROJECT_ROOT"] = r"C:\Users\Lenovo\Documents\ozygram\ozygram"

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

send_and_wait({
    "jsonrpc": "2.0", "id": 1, "method": "initialize",
    "params": {"protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "t", "version": "1"}}
})

# Test ozy_read_blob
r_blob = send_and_wait({
    "jsonrpc": "2.0",
    "id": 2,
    "method": "tools/call",
    "params": {
        "name": "ozy_read_blob",
        "arguments": {
            "blob_id": "d95aeff7603e",
            "offset": 0,
            "limit": 5
        }
    }
})
text_blob = r_blob["result"]["content"][0]["text"]
print("=== BLOB READ OUTPUT ===")
print(text_blob)

proc.kill()
