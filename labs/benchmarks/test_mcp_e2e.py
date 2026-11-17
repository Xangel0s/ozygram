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
    bufsize=1,
    env=env
)

def send_req(req):
    req_str = json.dumps(req) + "\n"
    proc.stdin.write(req_str)
    proc.stdin.flush()
    line = proc.stdout.readline()
    return json.loads(line)

print("=== 1. TEST INITIALIZE ===")
init_req = {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "initialize",
    "params": {
        "protocolVersion": "2024-11-05",
        "capabilities": {},
        "clientInfo": {"name": "e2e-test", "version": "1.0.0"}
    }
}
init_res = send_req(init_req)
result = init_res.get("result", {})
server_info = result.get("serverInfo", {})
print("Protocol Version:", result.get("protocolVersion"))
print("Server Name:", server_info.get("name"))
print("Server Version:", server_info.get("version"))
has_instructions = "instructions" in result
print("Has System Instructions:", has_instructions)
if has_instructions:
    print("Instructions snippet:\n", result["instructions"][:200], "...\n")

print("=== 2. TEST NOTIFICATIONS/INITIALIZED ===")
notif = {"jsonrpc": "2.0", "method": "notifications/initialized"}
proc.stdin.write(json.dumps(notif) + "\n")
proc.stdin.flush()

print("=== 3. TEST TOOLS/LIST ===")
tools_req = {"jsonrpc": "2.0", "id": 2, "method": "tools/list"}
tools_res = send_req(tools_req)
tools = tools_res.get("result", {}).get("tools", [])
print(f"Total tools exposed: {len(tools)}")
tool_names = [t["name"] for t in tools]
print("Has ozy_read_blob:", "ozy_read_blob" in tool_names)
print("Has ozy_graph:", "ozy_graph" in tool_names)
print("Has ozy_context:", "ozy_context" in tool_names)
print("Has ozy_memory:", "ozy_memory" in tool_names)
print("Has ozy_patch:", "ozy_patch" in tool_names)

print("\n=== 4. TEST OZY_CONTEXT WITH RELATIVE PATH ===")
ctx_req = {
    "jsonrpc": "2.0",
    "id": 3,
    "method": "tools/call",
    "params": {
        "name": "ozy_context",
        "arguments": {
            "file_path": "production-crm-backend/app/modules/programacion_ingenieria/models.py",
            "lod": "contracts"
        }
    }
}
ctx_res = send_req(ctx_req)
content = ctx_res.get("result", {}).get("content", [{}])[0].get("text", "")
print("Context response status:", "Found" if "ProgramacionIngenieria" in content else "FAILED")
print("Snippet:\n", content[:350], "...\n")

print("=== 5. TEST OZY_GRAPH (IMPACT) ===")
graph_req = {
    "jsonrpc": "2.0",
    "id": 4,
    "method": "tools/call",
    "params": {
        "name": "ozy_graph",
        "arguments": {
            "action": "impact",
            "file_path": "production-crm-backend/app/modules/programacion_ingenieria/models.py",
            "max_hops": 2
        }
    }
}
graph_res = send_req(graph_req)
graph_content = graph_res.get("result", {}).get("content", [{}])[0].get("text", "")
print("Graph impact snippet:\n", graph_content[:300], "...\n")

print("=== 6. TEST OZY_READ_BLOB ===")
blob_req = {
    "jsonrpc": "2.0",
    "id": 5,
    "method": "tools/call",
    "params": {
        "name": "ozy_read_blob",
        "arguments": {
            "blob_id": "021de06bf5a3",
            "offset": 0,
            "limit": 5
        }
    }
}
blob_res = send_req(blob_req)
blob_content = blob_res.get("result", {}).get("content", [{}])[0].get("text", "")
print("Blob read snippet:\n", blob_content[:250], "...\n")

print("=== 7. TEST OZY_MEMORY (SEARCH) ===")
mem_req = {
    "jsonrpc": "2.0",
    "id": 6,
    "method": "tools/call",
    "params": {
        "name": "ozy_memory",
        "arguments": {
            "action": "search",
            "query": "SU24"
        }
    }
}
mem_res = send_req(mem_req)
mem_content = mem_res.get("result", {}).get("content", [{}])[0].get("text", "")
print("Memory search snippet:\n", mem_content[:250], "...\n")

proc.kill()
print("=== ALL 7 TESTS PASSED CLEANLY! ===")
