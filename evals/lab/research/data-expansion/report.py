#!/usr/bin/env python3
"""Markdown tables of the data-expansion corpus (from corpus.json), with Wilson 90% intervals on
pass rates. Costs $0.

  python3 report.py            # prints the tables README.md embeds
"""

from __future__ import annotations

import json
import math
from pathlib import Path

HERE = Path(__file__).resolve().parent


def wilson(k: int, n: int, z: float = 1.645) -> tuple[float, float]:
    if n == 0:
        return (0.0, 0.0)
    p = k / n
    d = 1 + z * z / n
    c = (p + z * z / (2 * n)) / d
    h = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / d
    return (max(0.0, c - h), min(1.0, c + h))


def main() -> int:
    d = json.loads((HERE / "corpus.json").read_text())
    done = [s for s in d["sessions"] if s["status"] in ("passed", "failed")]
    print("| repo | lang / tooling | validated tasks | sessions (train / control) | pass rate [90% Wilson] | "
          "$ / session | calls / session | censored at 60 turns |")
    print("|---|---|---|---|---|---|---|---|")
    order = []
    for s in done:
        if s["repo"] not in order:
            order.append(s["repo"])
    tot = {"n": 0, "k": 0, "usd": 0.0, "calls": 0}
    for rid in order:
        ss = [s for s in done if s["repo"] == rid]
        n, k = len(ss), sum(bool(s["passed"]) for s in ss)
        lo, hi = wilson(k, n)
        usd = sum(s["cost_usd"] or 0 for s in ss)
        calls = sum(s["tool_calls"] or 0 for s in ss)
        cens = sum(s["run_status"] == "censored" for s in ss)
        tr = sum(s["arm"] == "train" for s in ss)
        r = d["repos"].get(rid, {})
        lang = r.get("lang", "python (existing)")
        nval = r.get("n_validated", "(existing)")
        print(f"| {rid} | {lang} | {nval} | {n} ({tr} / {n - tr}) | {k}/{n} = {k / n:.0%} [{lo:.0%}, {hi:.0%}] | "
              f"${usd / n:.3f} | {calls / n:.1f} | {cens} |")
        tot["n"] += n
        tot["k"] += k
        tot["usd"] += usd
        tot["calls"] += calls
    lo, hi = wilson(tot["k"], tot["n"])
    print(f"| **all** | | | {tot['n']} | {tot['k']}/{tot['n']} = {tot['k'] / max(1, tot['n']):.0%} [{lo:.0%}, {hi:.0%}] | "
          f"${tot['usd'] / max(1, tot['n']):.3f} | {tot['calls'] / max(1, tot['n']):.1f} | |")
    print()
    sp = d["spend"]
    print(f"Spend: sessions ${sum(sp['roots'].values()):.2f} + tool-less `claude -p` (prompt writing, "
          f"killed-session estimates) ${sp['claude_p']:.2f} = **${sp['total']:.2f}** of the $45 cap.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
