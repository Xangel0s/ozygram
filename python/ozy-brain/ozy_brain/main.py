from __future__ import annotations

import argparse
import json
import sys

from ozy_brain.brain import run


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Ozy Brain local worker")
    parser.add_argument("--action", default="plan")
    args = parser.parse_args(argv)
    try:
        raw = sys.stdin.read().strip() or "{}"
        payload = json.loads(raw)
        if not isinstance(payload, dict):
            raise ValueError("payload must be a JSON object")
        print(json.dumps(run(args.action, payload), ensure_ascii=False))
        return 0
    except Exception as exc:  # noqa: BLE001 - CLI boundary returns structured error
        print(json.dumps({"error": str(exc), "action": args.action, "engine": "ozy-brain-python"}, ensure_ascii=False))
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
