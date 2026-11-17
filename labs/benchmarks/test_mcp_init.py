import subprocess
import json
import os
import sys

exe = r"C:\Users\Lenovo\Documents\ozygram\ozygram\target\release\ozymem-server.exe"
env = os.environ.copy()
env["OZYGRAM_PROJECT_ROOT"] = r"C:\Users\Lenovo\Documents\crmnew"

proc = subprocess.Popen(
    [exe],
    stdin=subprocess.PIPE,
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
    text=True,
    env=env
)

init_req = {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "initialize",
    "params": {
        "protocolVersion": "2024-11-05",
        "capabilities": {},
        "clientInfo": {"name": "test-client", "version": "1.0.0"}
    }
}

try:
    stdout, stderr = proc.communicate(input=json.dumps(init_req) + "\n", timeout=10)
    print("RETURN CODE:", proc.returncode)
    print("STDOUT:", stdout[:500])
    print("STDERR:", stderr[:500])
except subprocess.TimeoutExpired:
    proc.kill()
    stdout, stderr = proc.communicate()
    print("TIMEOUT!")
    print("STDOUT:", stdout[:500])
    print("STDERR:", stderr[:500])
