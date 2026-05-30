#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any


def load_trace(path: Path) -> dict[str, dict[str, Any]]:
    events: dict[str, dict[str, Any]] = {}
    with path.open(encoding="utf8") as file:
        for line in file:
            event = json.loads(line)
            events[event["name"]] = event
    return events


def short(value: Any, limit: int = 120) -> str:
    text = json.dumps(value, ensure_ascii=False)
    return text if len(text) <= limit else text[: limit - 3] + "..."


def main() -> None:
    parser = argparse.ArgumentParser(description="Compare Python and Rust Qwen3-TTS parity traces.")
    parser.add_argument("python_trace", type=Path)
    parser.add_argument("rust_trace", type=Path)
    parser.add_argument("--prefix", default="")
    args = parser.parse_args()

    left = load_trace(args.python_trace)
    right = load_trace(args.rust_trace)
    names = sorted(name for name in set(left) | set(right) if name.startswith(args.prefix))
    for name in names:
        if name not in left:
            print(f"RUST_ONLY {name}")
            continue
        if name not in right:
            print(f"PY_ONLY   {name}")
            continue
        a = left[name]
        b = right[name]
        mismatches = []
        for key in ("kind", "shape", "len", "size", "values", "indices"):
            if a.get(key) != b.get(key):
                mismatches.append(key)
        if a.get("kind") == "tensor" and not mismatches:
            for key in ("first", "last", "probes"):
                av = a.get(key)
                bv = b.get(key)
                if av is None or bv is None:
                    continue
                if key == "probes":
                    differs = len(av) != len(bv) or any(
                        int(x[0]) != int(y[0]) or abs(float(x[1]) - float(y[1])) > 1e-3 for x, y in zip(av, bv)
                    )
                else:
                    differs = len(av) != len(bv) or any(abs(float(x) - float(y)) > 1e-3 for x, y in zip(av, bv))
                if differs:
                    mismatches.append(key)
        if mismatches:
            print(f"DIFF     {name}: {', '.join(mismatches)}")
            for key in mismatches[:4]:
                print(f"  py   {key}: {short(a.get(key))}")
                print(f"  rust {key}: {short(b.get(key))}")
            break
        print(f"OK       {name}")


if __name__ == "__main__":
    main()
