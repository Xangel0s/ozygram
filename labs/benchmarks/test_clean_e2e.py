import subprocess
import json
import os
import sys
import time

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

def read_line_with_timeout():
    line = proc.stdout.readline()
    return line

def send_and_wait(req):
    req_str = json.dumps(req) + "\n"
    print(f"--> SEND: method={req.get('method')} id={req.get('id')}")
    proc.stdin.write(req_str)
    proc.stdin.flush()
    while True:
        line = read_line_with_timeout()
        if not line:
            print("EOF from server!")
            return None
        line = line.strip()
        if not line:
            continue
        try:
            val = json.loads(line)
            if val.get("id") == req.get("id"):
                print(f"<-- GOT RESPONSE id={req.get('id')}")
                return val
            else:
                print(f"  [STRAY]: {line[:120]}")
        except Exception as e:
            print(f"  [RAW]: {line[:120]}")

print("=== 1. Init ===")
r1 = send_and_wait({
    "jsonrpc": "2.0",
    "id": 1,
    "method": "initialize",
    "params": {
        "protocolVersion": "2024-11-05",
        "capabilities": {},
        "clientInfo": {"name": "test", "version": "1.0"}
    }
})

print("=== 2. Tools List ===")
r2 = send_and_wait({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})

print("=== 3. Context ===")
r3 = send_and_wait({
    "jsonrpc": "2.0",
    "id": 3,
    "method": "tools/call",
    "params": {
        "name": "ozy_context",
        "arguments": {
            "file_path": "crates/ozymem-server/src/dispatch.rs",
            "lod": "contracts"
        }
    }
})
text3 = r3["result"]["content"][0]["text"]
print("Context output snippet:\n", text3[:250])

print("=== 4. Simulate Patch ===")
r4 = send_and_wait({
    "jsonrpc": "2.0",
    "id": 4,
    "method": "tools/call",
    "params": {
        "name": "ozy_patch",
        "arguments": {
            "action": "simulate",
            "file_path": "crates/ozymem-server/src/dispatch.rs",
            "target_code": "let Some(ref id_ref) = request.id else {",
            "replacement": "// Validated check\n    let Some(ref id_ref) = request.id else {"
        }
    }
})
text4 = r4["result"]["content"][0]["text"]
print("Simulate output snippet:\n", text4[:250])

print("=== 5. Memory Save ===")
r5 = send_and_wait({
    "jsonrpc": "2.0",
    "id": 5,
    "method": "tools/call",
    "params": {
        "name": "ozy_memory",
        "arguments": {
            "action": "save",
            "kind": "gotcha",
            "topic_key": "protocol_fix",
            "title": "Stdout sanitation and atomic indexing",
            "content": "Verified that stdout is pure JSON-RPC without stray progress spam."
        }
    }
})
text5 = r5["result"]["content"][0]["text"]
print("Save output snippet:\n", text5[:250])

print("=== 6. Memory Search ===")
r6 = send_and_wait({
    "jsonrpc": "2.0",
    "id": 6,
    "method": "tools/call",
    "params": {
        "name": "ozy_memory",
        "arguments": {
            "action": "search",
            "query": "Stdout sanitation"
        }
    }
})
text6 = r6["result"]["content"][0]["text"]
print("Search output snippet:\n", text6[:250])

proc.kill()
print("=== ALL COMPLETED CLEANLY ===")
