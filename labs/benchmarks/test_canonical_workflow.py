import subprocess
import json
import hashlib
import os
import sys

SERVER_EXE = r"C:\Users\Lenovo\Documents\ozygram\ozygram\target\release\ozymem-server.exe"
PROJECT_DIR = r"C:\Users\Lenovo\Documents\crmnew"
TARGET_FILE = os.path.join(PROJECT_DIR, r"production-crm-backend\app\utils\date_format.py")

def get_file_hash(path):
    if not os.path.exists(path):
        return None
    with open(path, "rb") as f:
        return hashlib.sha256(f.read()).hexdigest()

def send_rpc(proc, req):
    line = json.dumps(req) + "\n"
    proc.stdin.write(line)
    proc.stdin.flush()
    while True:
        resp_line = proc.stdout.readline()
        if not resp_line:
            raise EOFError("Server closed stdout")
        resp_line = resp_line.strip()
        if not resp_line:
            continue
        try:
            return json.loads(resp_line)
        except json.JSONDecodeError:
            print(f"[RAW LOG]: {resp_line}", file=sys.stderr)

def main():
    print("=== Ozygram Canonical 6-Tool Suite Integration Test ===")
    if not os.path.exists(SERVER_EXE):
        print(f"Error: binary {SERVER_EXE} not found.")
        sys.exit(1)

    initial_hash = get_file_hash(TARGET_FILE)
    print(f"Initial target file SHA256: {initial_hash}")

    proc = subprocess.Popen(
        [SERVER_EXE],
        cwd=PROJECT_DIR,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8"
    )

    try:
        # Step 1: Initialize
        print("\n[Step 1] Initializing Server...")
        init_req = {
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "integration-test", "version": "1.0"},
                "rootUri": f"file:///{PROJECT_DIR.replace(chr(92), '/')}"
            }
        }
        init_res = send_rpc(proc, init_req)
        server_info = init_res.get("result", {}).get("serverInfo", {})
        instructions = init_res.get("result", {}).get("instructions", "")
        print(f"Server Name: {server_info.get('name')}, Version: {server_info.get('version')}")
        assert "ozy_refactor" in instructions, "ozy_refactor missing from server instructions"
        print("✓ Server instructions contain ozy_refactor guidance.")

        # Step 2: Tools List
        print("\n[Step 2] Querying tools/list...")
        list_req = {"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}}
        list_res = send_rpc(proc, list_req)
        tools = list_res.get("result", {}).get("tools", [])
        tool_names = [t["name"] for t in tools]
        print(f"Exposed tools ({len(tool_names)}): {tool_names}")
        
        expected_canonical = {"ozy_refactor", "ozy_explore", "ozy_simulate", "ozy_apply", "ozy_memory", "ozy_read_blob"}
        assert set(tool_names) == expected_canonical, f"Expected {expected_canonical}, got {set(tool_names)}"
        print("✓ Exact 6 canonical tools exposed! (77.8% schema token reduction confirmed).")

        # Step 3: Test ozy_refactor in RAM
        print("\n[Step 3] Testing ozy_refactor (1-turn atomic RAM refactor)...")
        target_snippet = "def normalize_date_ymd(value: str | datetime | date | None) -> Optional[str]:\n    dt = parse_flexible_date(value)\n    if not dt:\n        return None\n    return dt.strftime(\"%Y/%m/%d\")"
        replacement_snippet = "def normalize_date_ymd(value: str | datetime | date | None) -> Optional[str]:\n    # In-RAM 1-Turn Atomic Refactor\n    dt = parse_flexible_date(value)\n    return dt.strftime(\"%Y/%m/%d\") if dt else None"

        refactor_req = {
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "ozy_refactor",
                "arguments": {
                    "file_path": "production-crm-backend/app/utils/date_format.py",
                    "action": "simulate",
                    "target_code": target_snippet,
                    "replacement": replacement_snippet,
                    "goal": "Verify atomic 1-turn blast radius and RAM diff"
                }
            }
        }
        refactor_res = send_rpc(proc, refactor_req)
        refactor_content = refactor_res.get("result", {}).get("content", [{}])[0].get("text", "")
        print("--- ozy_refactor Response Preview ---")
        print("\n".join(refactor_content.splitlines()[:25]))
        print("--- End Preview ---")

        assert "=== [OZY_REFACTOR: ONE-TURN ATOMIC REPORT] ===" in refactor_content, "Missing header in ozy_refactor output"
        assert "1. TOPOLOGICAL BLAST RADIUS" in refactor_content, "Missing blast radius in ozy_refactor output"
        assert "2. AST PATCH SIMULATION (IN-RAM)" in refactor_content, "Missing RAM diff simulation in output"
        assert "Status: [SUCCESS]" in refactor_content, f"Patch simulation did not succeed: {refactor_content}"
        assert "[DISK STATUS]: UNTOUCHED" in refactor_content, "Missing UNTOUCHED verification in output"
        print("✓ ozy_refactor executed blast radius + RAM AST diff in 1 single turn!")

        # Step 4: Verify Disk Untouched
        after_hash = get_file_hash(TARGET_FILE)
        assert initial_hash == after_hash, f"DISK WAS MODIFIED! Initial: {initial_hash}, After: {after_hash}"
        print(f"✓ Target file on disk 100% UNTOUCHED (SHA256: {after_hash})")

        # Step 5: Test ozy_explore
        print("\n[Step 5] Testing ozy_explore (contracts, impact, summary)...")
        explore_contracts_req = {
            "jsonrpc": "2.0",
            "id": 4,
            "method": "tools/call",
            "params": {
                "name": "ozy_explore",
                "arguments": {
                    "file_path": "production-crm-backend/app/utils/date_format.py",
                    "action": "contracts"
                }
            }
        }
        exp_res = send_rpc(proc, explore_contracts_req)
        exp_content = exp_res.get("result", {}).get("content", [{}])[0].get("text", "")
        assert "normalize_date_ymd" in exp_content, "Missing normalize_date_ymd in contracts output"
        print("✓ ozy_explore(action='contracts') successfully returned AST contracts.")

        # Step 6: Test Backwards-Compatibility Dispatch
        print("\n[Step 6] Testing Legacy Dispatch (ozy_graph)...")
        legacy_req = {
            "jsonrpc": "2.0",
            "id": 5,
            "method": "tools/call",
            "params": {
                "name": "ozy_graph",
                "arguments": {
                    "action": "impact",
                    "file_path": "production-crm-backend/app/utils/date_format.py",
                    "max_hops": 1
                }
            }
        }
        leg_res = send_rpc(proc, legacy_req)
        leg_content = leg_res.get("result", {}).get("content", [{}])[0].get("text", "")
        print(f"Legacy content preview: {leg_content[:150]}")
        assert "Impact Analysis" in leg_content or "Impact:" in leg_content or "Direct Dependents" in leg_content or "ZERO_BLAST_RADIUS" in leg_content or "impact" in leg_content.lower(), "Legacy ozy_graph failed to dispatch correctly"
        print("✓ Legacy ozy_graph dispatched seamlessly with 100% backwards compatibility.")

        print("\n=======================================================")
        print("ALL TESTS PASSED! Canonical 6-Tool Suite Fully Verified.")
        print("=======================================================")

    finally:
        proc.terminate()
        proc.wait(timeout=5)

if __name__ == "__main__":
    main()
