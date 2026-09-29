#!/usr/bin/env python3
"""Everything the data-expansion agent has spent: the sessions of every runner root under
/tmp/adage-research/data-expansion/runs (reported cost; a session still running or killed is
estimated from its stream's token usage, evals/repobench/spend.py), plus the tool-less `claude -p`
calls (prompt writing) in the cache's spend ledger.

  python3 spend_total.py [--quiet]
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parents[2] / "repobench"))
from spend import stream_cost  # noqa: E402

DX = Path("/tmp/adage-research/data-expansion")


def total() -> dict:
    roots, running = {}, 0.0
    for st in sorted((DX / "runs").glob("*/state.json")):
        s = json.loads(st.read_text())
        rep = 0.0
        for rid, r in s["runs"].items():
            c = (r.get("metrics") or {}).get("cost_usd")
            if c:
                rep += c
            elif r.get("status") == "running":
                running += stream_cost(st.parent / "runs" / rid / "stream.jsonl", s.get("model") or "")
        roots[st.parent.name] = round(rep, 3)
    ledger = DX / "cache" / "spend.jsonl"
    llm = sum(json.loads(x)["usd"] for x in ledger.read_text().splitlines()) if ledger.exists() else 0.0
    tot = sum(roots.values()) + running + llm
    return {"roots": roots, "running_est": round(running, 3), "claude_p": round(llm, 3), "total": round(tot, 3)}


if __name__ == "__main__":
    t = total()
    print(f"{t['total']:.3f}" if "--quiet" in sys.argv else json.dumps(t, indent=1))
