from __future__ import annotations

import argparse
import json
import os
import socketserver
import sys
import threading
import time
from typing import Any

from ozy_brain.brain import run
from ozy_brain.schemas import BRAIN_SCHEMA_VERSION, BRAIN_VERSION


class JSONRPCHandler(socketserver.StreamRequestHandler):
    """Low-latency persistent JSON-RPC TCP/Socket handler for Ozy Brain."""

    def handle(self) -> None:
        while True:
            line = self.rfile.readline()
            if not line:
                break
            raw = line.decode("utf-8").strip()
            if not raw:
                continue

            try:
                req = json.loads(raw)
                req_id = req.get("id")
                method = req.get("method") or req.get("action")
                params = req.get("params") or req.get("payload") or {}

                if method in ("/health", "health", "ping"):
                    res = {
                        "jsonrpc": "2.0",
                        "id": req_id,
                        "result": {
                            "status": "ok",
                            "ready": True,
                            "brain_version": BRAIN_VERSION,
                            "schema_version": BRAIN_SCHEMA_VERSION,
                            "mode": "persistent",
                            "pid": os.getpid(),
                            "uptime_secs": round(time.time() - START_TIME, 2),
                        },
                    }
                else:
                    action = method or params.get("action") or "plan"
                    payload = params if isinstance(params, dict) else {}
                    result = run(action, payload)
                    res = {"jsonrpc": "2.0", "id": req_id, "result": result}

            except Exception as exc:  # noqa: BLE001
                res = {
                    "jsonrpc": "2.0",
                    "id": req.get("id") if "req" in locals() and isinstance(req, dict) else None,
                    "error": {"code": -32603, "message": str(exc)},
                }

            out = json.dumps(res, ensure_ascii=False) + "\n"
            self.wfile.write(out.encode("utf-8"))
            self.wfile.flush()


class ThreadedTCPServer(socketserver.ThreadingMixIn, socketserver.TCPServer):
    allow_reuse_address = True
    daemon_threads = True


START_TIME = time.time()


def start_server(host: str = "127.0.0.1", port: int = 9473) -> None:
    server = ThreadedTCPServer((host, port), JSONRPCHandler)
    server_thread = threading.Thread(target=server.serve_forever)
    server_thread.daemon = True
    server_thread.start()
    print(f"[ozy-brain-server] Persistent worker running on {host}:{port} (PID: {os.getpid()})", flush=True)

    try:
        while True:
            time.sleep(1)
    except KeyboardInterrupt:
        print("[ozy-brain-server] Shutting down...", flush=True)
        server.shutdown()
        server.server_close()


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Ozy Brain Persistent TCP/JSON-RPC Server")
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=9473)
    args = parser.parse_args(argv)
    start_server(args.host, args.port)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
