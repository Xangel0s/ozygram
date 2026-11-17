import subprocess
import json
import os
import sys
import time

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

def read_response(req_id):
    """Read lines until we get the response with the matching id."""
    while True:
        line = proc.stdout.readline()
        if not line:
            return None
        line_clean = line.strip()
        if not line_clean:
            continue
        try:
            data = json.loads(line_clean)
            if data.get("id") == req_id:
                return data
            else:
                print(f"  [Ignored intermediate notification/msg]: method={data.get('method')} id={data.get('id')}")
        except Exception as e:
            print("  [Non-JSON output]:", line_clean[:100])

def send_req(req):
    req_str = json.dumps(req) + "\n"
    proc.stdin.write(req_str)
    proc.stdin.flush()
    return read_response(req["id"])

print("--- 1. Initialize ---")
init_res = send_req({
    "jsonrpc": "2.0",
    "id": 1,
    "method": "initialize",
    "params": {
        "protocolVersion": "2024-11-05",
        "capabilities": {},
        "clientInfo": {"name": "self-test", "version": "1.0"}
    }
})
print("Init result version:", init_res["result"]["serverInfo"]["version"])
print("Instructions present:", "instructions" in init_res["result"])

# Let background scan finish on ozygram
print("Waiting 5s for background scan to index ozygram...")
time.sleep(5)

print("\n--- 2. Tools List ---")
tools_res = send_req({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})
tools = tools_res["result"]["tools"]
print(f"Tools count: {len(tools)}")

print("\n--- 3. Context on crates/ozymem-server/src/dispatch.rs ---")
ctx_res = send_req({
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
text3 = ctx_res["result"]["content"][0]["text"]
print("dispatch.rs context snippet:")
print(text3[:300])

print("\n--- 4. Graph Impact on crates/ozymem-core/src/graph_backend/sqlite_backend.rs ---")
graph_res = send_req({
    "jsonrpc": "2.0",
    "id": 4,
    "method": "tools/call",
    "params": {
        "name": "ozy_graph",
        "arguments": {
            "action": "impact",
            "file_path": "crates/ozymem-core/src/graph_backend/sqlite_backend.rs",
            "max_hops": 2
        }
    }
})
text4 = graph_res["result"]["content"][0]["text"]
print("Graph impact snippet:")
print(text4[:300])

print("\n--- 5. Simulation Patch on crates/ozymem-server/src/dispatch.rs ---")
patch_res = send_req({
    "jsonrpc": "2.0",
    "id": 5,
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
text5 = patch_res["result"]["content"][0]["text"]
print("Simulation result:")
print(text5[:250])

print("\n--- 6. Memory gotcha save & search ---")
mem_save_res = send_req({
    "jsonrpc": "2.0",
    "id": 6,
    "method": "tools/call",
    "params": {
        "name": "ozy_memory",
        "arguments": {
            "action": "save",
            "kind": "gotcha",
            "topic_key": "testing_audit",
            "title": "E2E Self-Test Verification",
            "content": "Verified that ozymem-server runs clean self-tests against itself."
        }
    }
})
print("Memory save result:", mem_save_res["result"]["content"][0]["text"][:200])

mem_search_res = send_req({
    "jsonrpc": "2.0",
    "id": 7,
    "method": "tools/call",
    "params": {
        "name": "ozy_memory",
        "arguments": {
            "action": "search",
            "query": "E2E Self-Test Verification"
        }
    }
})
print("Memory search result:", mem_search_res["result"]["content"][0]["text"][:200])

proc.kill()
print("\n=== TEST COMPLETED SUCCESSFULLY ===")
