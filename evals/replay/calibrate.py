#!/usr/bin/env python3
"""Calibrate the replay savings metric against the one live measurement there is.

  python3 evals/replay/calibrate.py --manifests DIR [--adage target/debug/adage]
                                    [--deadends evals/launch/data/deadends-v2.jsonl]

The live measurement (`evals/launch/README.md` §1, `evals/launch/mechanism.py`): in RepoBench v2's
Haiku sessions on click, attrs and marshmallow that hit U11's dead end, the adage arm needed 1.85
fewer tool calls from the dead end to a test run that ran tests (task-clustered over the 21 tasks
with such sessions in both arms; 95% CI [1.17, 2.56]).

The replay's prediction of the same quantity: replay the **control** arm's sessions with the index
that was live (`private/adage-home/index.json`) restricted to U11's routes, post-hoc (as delivered
live), and take Σ_t S(d_t) per session that hit the dead end; per task the mean over its sessions;
the statistic is the mean over the same 21 tasks, with a 95% percentile bootstrap over tasks (5000
draws, seed 7, as mechanism.py). The replay is calibrated when its point estimate lies inside the
live CI. Also reported: the same replay of the **adage** arm (where U11 was delivered and
followed): what is left to save there should be about zero.
"""

from __future__ import annotations

import argparse
import json
import random
import statistics
import subprocess
import sys
from collections import defaultdict
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
ROOTS = ("click", "attrs", "marshmallow")
LIVE = (1.85, 1.17, 2.56)


def replay(adage: Path, manifest: Path, arm: str, *extra: str) -> dict:
    out = subprocess.run([str(adage), "eval", "replay-savings", str(manifest), "--arm", arm, "--only", "U11",
                          "--json", *extra], capture_output=True, text=True, check=True)
    return json.loads(out.stdout)


def live_tasks(deadends: Path) -> set[tuple[str, str]]:
    """The (repository, task) pairs mechanism.py pairs: dead-end sessions in both arms."""
    arms = defaultdict(set)
    for line in deadends.read_text().splitlines():
        if not line.strip():
            continue
        r = json.loads(line)
        repo = r["root"].split("/")[-1]
        if repo in ROOTS and r["u11_hits"] and r["recovery_calls"] is not None:
            arms[(repo, r["task"])].add(r["arm"])
    return {k for k, v in arms.items() if {"adage", "control"} <= v}


def task_means(reports: dict[str, dict], keep: set | None, field: str = "saved") -> dict:
    by = defaultdict(list)
    for repo, rep in reports.items():
        for s in rep["sessions"]:
            if s["dead_end"] and (keep is None or (repo, s["task"]) in keep):
                by[(repo, s["task"])].append(s[field])
    return {k: statistics.mean(v) for k, v in by.items() if v}


def clustered(means: dict, seed: int = 7) -> tuple[float, float, float, int]:
    vals = list(means.values())
    if not vals:
        return (0.0, 0.0, 0.0, 0)
    rng = random.Random(seed)
    boots = sorted(statistics.mean(rng.choice(vals) for _ in vals) for _ in range(5000))
    return (statistics.mean(vals), boots[124], boots[4874], len(vals))


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--manifests", type=Path, required=True)
    ap.add_argument("--adage", type=Path, default=REPO / "target" / "debug" / "adage")
    ap.add_argument("--deadends", type=Path, default=REPO / "evals" / "launch" / "data" / "deadends-v2.jsonl")
    ap.add_argument("--json", type=Path, help="write the calibration rows here")
    a = ap.parse_args(argv)
    keep = live_tasks(a.deadends)
    rows = []
    print(f"live (mechanism.py, {len(keep)} tasks): saving {LIVE[0]:.2f} calls [95% CI {LIVE[1]:.2f}, {LIVE[2]:.2f}]\n")
    print("| Sat rule | arm replayed | mode | tasks | directives | useful | predicted saving (calls) | 95% CI | inside live CI |")
    print("| --- | --- | --- | ---: | ---: | ---: | ---: | --- | --- |")
    for sat in ("strict", "goal"):
        for arm, mode in (("control", "post-hoc"), ("control", "pre-emptive"), ("adage", "post-hoc")):
            reps = {r: replay(a.adage, a.manifests / f"{r}.json", arm, "--sat", sat, "--mode", mode) for r in ROOTS}
            m, lo, hi, n = clustered(task_means(reps, keep))
            d = sum(x["summary"]["directives"] for x in reps.values())
            u = sum(x["summary"]["useful"] for x in reps.values())
            inside = LIVE[1] <= m <= LIVE[2]
            flag = ("**yes**" if inside else "no") if (arm, mode) == ("control", "post-hoc") else "–"
            print(f"| {sat} | {arm} | {mode} | {n} | {d} | {u} | {m:.2f} | [{lo:.2f}, {hi:.2f}] | {flag} |")
            rows.append({"sat": sat, "arm": arm, "mode": mode, "tasks": n, "directives": d, "useful": u,
                         "predicted": round(m, 3), "ci": [round(lo, 3), round(hi, 3)], "inside_live_ci": inside})
    if a.json:
        a.json.write_text(json.dumps({"live": LIVE, "rows": rows}, indent=1) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
