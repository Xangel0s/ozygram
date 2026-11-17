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

send_and_wait({
    "jsonrpc": "2.0", "id": 1, "method": "initialize",
    "params": {"protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "test", "version": "1"}}
})

candidate_code = """pub(crate) fn mark_legacy_tools_deprecated(value: &mut Value) {
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

print("Testing ozy_patch with target_symbol...")
r_sym = send_and_wait({
    "jsonrpc": "2.0", "id": 2, "method": "tools/call",
    "params": {
        "name": "ozy_patch",
        "arguments": {
            "action": "simulate",
            "target_symbol": "mark_legacy_tools_deprecated",
            "candidate_code": candidate_code
        }
    }
})
sym_text = r_sym["result"]["content"][0]["text"]
print("Symbol Simulation Result:")
print(sym_text[:500])

proc.kill()
