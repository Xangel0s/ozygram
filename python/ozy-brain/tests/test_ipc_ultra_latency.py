import json
import socket
import threading
import time
import pytest
from ozy_brain.server import ThreadedTCPServer, JSONRPCHandler


def test_ipc_server_tcp_nodelay_ultra_low_latency():
    # Start server on dynamic port
    server = ThreadedTCPServer(("127.0.0.1", 0), JSONRPCHandler)
    host, port = server.server_address
    server_thread = threading.Thread(target=server.serve_forever, daemon=True)
    server_thread.start()

    try:
        # Connect client with TCP_NODELAY
        client = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        client.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
        client.connect((host, port))

        # Benchmark 50 consecutive health calls
        req_bytes = json.dumps({"jsonrpc": "2.0", "id": 1, "method": "health"}).encode("utf-8") + b"\n"

        start_time = time.perf_counter()
        count = 50
        for _ in range(count):
            client.sendall(req_bytes)
            line = b""
            while not line.endswith(b"\n"):
                chunk = client.recv(1024)
                if not chunk:
                    break
                line += chunk

            res = json.loads(line.decode("utf-8").strip())
            assert res.get("result", {}).get("status") == "ok"

        total_secs = time.perf_counter() - start_time
        avg_ms = (total_secs / count) * 1000.0

        # Sub-millisecond SLA: average request must be well under 1.5ms
        assert avg_ms < 1.5, f"Expected average latency < 1.5ms, got {avg_ms:.3f}ms"
        client.close()
    finally:
        server.shutdown()
        server.server_close()
