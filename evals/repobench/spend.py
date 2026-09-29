#!/usr/bin/env python3
"""Spend accounting across every root: reported costs, plus estimates for sessions that never
reported one (killed mid-session), from the token usage in their streams.

  python3 evals/repobench/spend.py [--runs DIR] [--record-killed LABEL]

`--record-killed LABEL` appends the estimate for sessions still marked `running` to
$REPOBENCH_CACHE/extra-spend.json (read by make_report.py), so a rerun that replaces them does not
make their spend disappear.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

import common
import llm

# USD per token (list prices, 2026-09): input, output, cache read, cache write (5 min).
PRICES = {"haiku": (1e-6, 5e-6, 0.1e-6, 1.25e-6), "sonnet": (3e-6, 15e-6, 0.3e-6, 3.75e-6)}


def stream_cost(path: Path, model: str) -> float:
    pi, po, pr, pw = PRICES["sonnet" if "sonnet" in model else "haiku"]
    seen, usd = set(), 0.0
    try:
        lines = path.read_text().splitlines()
    except OSError:
        return 0.0
    for line in lines:
        try:
            m = json.loads(json.loads(line)["line"])
        except (ValueError, KeyError, TypeError):
            continue
        if m.get("type") != "assistant":
            continue
        msg = m.get("message") or {}
        if msg.get("id") in seen:
            continue
        seen.add(msg.get("id"))
        u = msg.get("usage") or {}
        usd += (u.get("input_tokens", 0) * pi + u.get("output_tokens", 0) * po
                + u.get("cache_read_input_tokens", 0) * pr + u.get("cache_creation_input_tokens", 0) * pw)
    return usd


def tally(runs_dir: Path) -> dict:
    out = {"roots": {}, "killed": [], "reported": 0.0}
    for st in sorted(runs_dir.glob("*/state.json")):
        s = json.loads(st.read_text())
        rep = sum((r.get("metrics") or {}).get("cost_usd") or 0 for r in s["runs"].values())
        out["roots"][st.parent.name] = round(rep, 3)
        out["reported"] += rep
        for rid, r in s["runs"].items():
            if r.get("status") == "running":
                c = stream_cost(st.parent / "runs" / rid / "stream.jsonl", s.get("model") or "")
                out["killed"].append((st.parent.name, rid, round(c, 4)))
    return out


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--runs", default=str(common.cache_dir() / "runs"))
    ap.add_argument("--curves", default=str(common.cache_dir() / "curves"))
    ap.add_argument("--record-killed")
    a = ap.parse_args(argv)
    t = tally(Path(a.runs))
    curves = 0.0
    for p in Path(a.curves).glob("*/*/state.json"):
        s = json.loads(p.read_text())
        curves += sum((r.get("metrics") or {}).get("cost_usd") or 0 for r in s["runs"].values())
    extra_p = common.cache_dir() / "extra-spend.json"
    extra = json.loads(extra_p.read_text()) if extra_p.exists() else []
    killed = sum(x[2] for x in t["killed"])
    if a.record_killed and t["killed"]:
        extra.append([a.record_killed, len(t["killed"]), round(killed, 3)])
        extra_p.write_text(json.dumps(extra))
    for k, v in t["roots"].items():
        print(f"  {k:<32} ${v:.2f}")
    print(f"reported (roots) ${t['reported']:.2f}; curves ${curves:.2f}; claude -p calls ${llm.spent():.2f}; "
          f"recorded extras ${sum(x[2] for x in extra):.2f}; running now/killed (est.) {len(t['killed'])} ${killed:.2f}")
    print(f"TOTAL ${t['reported'] + curves + llm.spent() + sum(x[2] for x in extra):.2f}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
