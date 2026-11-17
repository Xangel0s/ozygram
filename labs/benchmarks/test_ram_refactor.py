import subprocess
import json
import os
import sys

sys.stdout.reconfigure(line_buffering=True)

exe = r"C:\Users\Lenovo\Documents\ozygram\ozygram\target\release\ozymem-server.exe"
project_root = r"C:\Users\Lenovo\Documents\ozygram\ozygram"

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

print("1. Initialize server...")
send_and_wait({
    "jsonrpc": "2.0", "id": 1, "method": "initialize",
    "params": {"protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "test", "version": "1"}}
})

target_file = "crates/ozymem-server/src/tools.rs"

print(f"\n2. [RAM Graph] Calculate Blast Radius for {target_file}...")
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
graph_impact = r_graph["result"]["content"][0]["text"]
print("Impact Output:")
print(graph_impact)

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
contracts_text = r_ctx["result"]["content"][0]["text"]
print("Contracts Output snippet:")
print(contracts_text[:400])

# Legacy code target
target_code = """pub(crate) fn mark_legacy_tools_deprecated(value: &mut Value) {
    let Some(tools) = value.get_mut("tools").and_then(Value::as_array_mut) else {
        return;
    };
    for tool in tools {
        let name = tool.get("name").and_then(Value::as_str).unwrap_or("");
        if name.starts_with("ozy_") {
            continue;
        }
        if let Some(desc) = tool.get("description").and_then(Value::as_str) {
            let new_desc = format!("[DEPRECATED: use one of the 5 unified ozy_* meta-tools] {desc}");
            tool["description"] = Value::String(new_desc);
        }
        tool["deprecated"] = Value::Bool(true);
    }
}"""

# Modern, idempotent, zero-duplicate replacement
replacement_code = """pub(crate) fn mark_legacy_tools_deprecated(value: &mut Value) {
    const DEPRECATION_PREFIX: &str = "[DEPRECATED: use one of the 5 unified ozy_* meta-tools]";

    let Some(tools) = value.get_mut("tools").and_then(Value::as_array_mut) else {
        return;
    };
    for tool in tools {
        let name = tool.get("name").and_then(Value::as_str).unwrap_or("");
        if name.starts_with("ozy_") {
            continue;
        }
        if let Some(desc) = tool.get("description").and_then(Value::as_str) {
            if !desc.starts_with(DEPRECATION_PREFIX) {
                let new_desc = format!("{DEPRECATION_PREFIX} {desc}");
                tool["description"] = Value::String(new_desc);
            }
        }
        tool["deprecated"] = Value::Bool(true);
    }
}"""

print(f"\n4. [Volatile RAM Simulation] Simulating refactored AST patch in memory...")
r_sim = send_and_wait({
    "jsonrpc": "2.0", "id": 4, "method": "tools/call",
    "params": {
        "name": "ozy_patch",
        "arguments": {
            "action": "simulate",
            "file_path": target_file,
            "target_code": target_code,
            "replacement": replacement_code
        }
    }
})
sim_output = r_sim["result"]["content"][0]["text"]
print("Simulation result snippet:")
print(sim_output[:350])

# Check if disk file was modified
with open(os.path.join(project_root, target_file), "r", encoding="utf-8") as f:
    disk_content = f.read()

is_disk_unmodified = "const DEPRECATION_PREFIX" not in disk_content
print("\n5. Disk integrity check:")
print(f"   Has disk file remained UNCHANGED? {is_disk_unmodified} (100% in-RAM simulation)")

proc.kill()
