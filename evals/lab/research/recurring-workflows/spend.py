#!/usr/bin/env python3
"""Everything this study spent on live sessions: reported cost of finished sessions in every runner
root under /tmp/adage-research/recurring-workflows/runs, plus an estimate for sessions still running."""

from __future__ import annotations

import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parents[2] / "repobench"))
from spend import stream_cost  # noqa: E402

RUNS = Path("/tmp/adage-research/recurring-workflows/runs")


def total() -> dict:
    roots, running, n = {}, 0.0, 0
    for st in sorted(RUNS.glob("*/state.json")):
        s = json.loads(st.read_text())
        rep = 0.0
        for rid, r in s["runs"].items():
            c = (r.get("metrics") or {}).get("cost_usd")
            if c:
                rep += c
                n += 1
            elif r.get("status") == "running":
                running += stream_cost(st.parent / "runs" / rid / "stream.jsonl", s.get("model") or "")
        roots[st.parent.name] = round(rep, 3)
    return {"roots": roots, "sessions": n, "running_est": round(running, 3),
            "total": round(sum(roots.values()) + running, 3)}


if __name__ == "__main__":
    print(json.dumps(total(), indent=1))
