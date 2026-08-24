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
    args = parser.parse_args(argv)
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
