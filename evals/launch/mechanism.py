#!/usr/bin/env python3
"""The instinct's mechanism, task-clustered: in sessions that hit U11's dead end, the tool calls to a
test run that ran, and whether the next test command used the project's environment.

  python3 evals/launch/mechanism.py deadends-v2.jsonl [--roots adage-rb2/click,...]

Reads evals/launch/deadends.py's --json rows. For each (root, task) with sessions in both arms that
hit the dead end: the per-task mean of each arm; the statistic is the mean over tasks of
(adage - control); a 95% percentile bootstrap resamples tasks (5000), and a sign-flip permutation
test over tasks gives p (10000 draws, seeded).
"""

from __future__ import annotations

import argparse
import json
import random
import statistics
from collections import defaultdict


def by_task(rows, field, pred):
    out = defaultdict(lambda: {"adage": [], "control": []})
    for r in rows:
        if r["arm"] in ("adage", "control") and r["u11_hits"] and pred(r):
            v = r[field]
            if v is None:
                continue
            out[(r["root"], r["task"])][r["arm"]].append(float(v))
    return {k: (statistics.mean(v["adage"]), statistics.mean(v["control"])) for k, v in out.items()
            if v["adage"] and v["control"]}


def stat(pairs):
    return statistics.mean(a - c for a, c in pairs)


def summarise(name, d, seed=7):
    pairs = list(d.values())
    if not pairs:
        return f"{name}: no tasks"
    rng = random.Random(seed)
    obs = stat(pairs)
    boots = sorted(stat([rng.choice(pairs) for _ in pairs]) for _ in range(5000))
    lo, hi = boots[124], boots[4874]
    diffs = [a - c for a, c in pairs]
    extreme = sum(abs(statistics.mean(x * rng.choice((1, -1)) for x in diffs)) >= abs(obs) - 1e-12 for _ in range(10000))
    return (f"{name}: tasks {len(pairs)}, adage {statistics.mean(a for a, _ in pairs):.2f}, control "
            f"{statistics.mean(c for _, c in pairs):.2f}, diff {obs:+.2f} [95% CI {lo:+.2f}, {hi:+.2f}], "
            f"sign-flip p = {(extreme + 1) / 10001:.4f}")


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("rows")
    ap.add_argument("--roots")
    a = ap.parse_args(argv)
    rows = [json.loads(x) for x in open(a.rows) if x.strip()]
    if a.roots:
        keep = set(a.roots.split(","))
        rows = [r for r in rows if r["root"] in keep]
    print(summarise("tool calls from the dead end to a test run that ran",
                    by_task(rows, "recovery_calls", lambda r: True)))
    print(summarise("next test command used the project's environment (share)",
                    by_task(rows, "next_test_in_env", lambda r: True)))


if __name__ == "__main__":
    main()
