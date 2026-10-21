from __future__ import annotations

import argparse
import json
import sys

from ozy_brain.brain import run


def main(argv: list[str] | None = None) -> int:
    # Force UTF-8 on Windows standard streams if supported
    if hasattr(sys.stdin, "reconfigure"):
        try:
            sys.stdin.reconfigure(encoding="utf-8")
            sys.stdout.reconfigure(encoding="utf-8")
            sys.stderr.reconfigure(encoding="utf-8")
        except Exception:
            pass

    parser = argparse.ArgumentParser(description="Ozy Brain local worker")
    parser.add_argument("--action", default="plan")
    parser.add_argument("--server", action="store_true", help="Run as persistent TCP/JSON-RPC server")
    parser.add_argument("--host", default="127.0.0.1", help="TCP server host")
    parser.add_argument("--port", type=int, default=9473, help="TCP server port")
    args = parser.parse_args(argv)

    if args.server:
        from ozy_brain.server import start_server
        start_server(args.host, args.port)
        return 0
    try:
        raw = sys.stdin.read().strip() or "{}"
        payload = json.loads(raw)
        if not isinstance(payload, dict):
            raise ValueError("payload must be a JSON object")
        output_bytes = json.dumps(run(args.action, payload), ensure_ascii=False).encode("utf-8")
        sys.stdout.buffer.write(output_bytes + b"\n")
        sys.stdout.buffer.flush()
        return 0
    except Exception as exc:  # noqa: BLE001 - CLI boundary returns structured error
        err_bytes = json.dumps(
            {"error": str(exc), "action": args.action, "engine": "ozy-brain-python"},
            ensure_ascii=False,
        ).encode("utf-8")
        sys.stdout.buffer.write(err_bytes + b"\n")
        sys.stdout.buffer.flush()
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
