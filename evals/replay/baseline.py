#!/usr/bin/env python3
"""The baseline table: Ŝ for each lever, per RepoBench repository, on the held-out control sessions.

  python3 evals/replay/baseline.py --manifests DIR [--adage target/debug/adage] [--json out.json]
                                   [--repos click,attrs,...]

Policies (all scored on the control arm's held-out sessions, which no directive steered):

  learned      the current engine: learn from the repository's training traces (default
               thresholds), then bind instincts against a fresh checkout, as `adage condition` does
  instincts    instincts only, bound against the checkout (day 0)
  u11-post     U11 alone, post-hoc (as delivered live; the calibration policy)
  u11-pre      U11 alone, pre-emptive: at PreToolUse of a call that would hit its dead end
               (hypothetical: the planned PreToolUse rewrite)
  cochange     a co-change file→test oracle: after a source edit, the top-3 test files that
               changed with it before the task's base commit (hypothetical)

Pooled rows resample (repository, task) clusters. Writes a Markdown table to stdout.
"""

from __future__ import annotations

import argparse
import json
import math
import random
import statistics
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent

POLICIES = {
    "learned": ["--policy", "learned"],
    "instincts": ["--policy", "instincts"],
    "u11-post": ["--policy", "instincts", "--only", "U11"],
    "u11-pre": ["--policy", "instincts", "--only", "U11", "--mode", "pre-emptive"],
    "cochange": ["--policy", "cochange"],
}
LABEL = {
    "learned": "(a) learned reflexes + instincts",
    "instincts": "(b) instincts only",
    "u11-post": "U11 post-hoc (as live)",
    "u11-pre": "(c) U11 pre-emptive (hypothetical)",
    "cochange": "(d) co-change file→test oracle (hypothetical)",
}


def replay(adage: Path, manifest: Path, args: list[str]) -> dict:
    out = subprocess.run([str(adage), "eval", "replay-savings", str(manifest), "--arm", "control", "--json", *args],
                         capture_output=True, text=True)
    if out.returncode != 0:
        raise SystemExit(f"{manifest.name} {' '.join(args)}: {out.stderr.strip()[-600:]}")
    return json.loads(out.stdout)


def wilson_lb(k: int, n: int, z: float = 1.6448536269514722) -> float:
    if n == 0:
        return 0.0
    p = k / n
    d = 1 + z * z / n
    return max(0.0, (p + z * z / (2 * n) - z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n))) / d)


def pooled(rows: list[dict], seed: int = 20260929) -> dict:
    """Ŝ over sessions with a bootstrap over (repository, task) clusters, and the rates."""
    by: dict[tuple, list[dict]] = {}
    for r in rows:
        by.setdefault((r["_repo"], r["task"]), []).append(r)
    clusters = list(by.values())
    rng = random.Random(seed)

    def mean(field, cl):
        xs = [r[field] for c in cl for r in c]
        return sum(xs) / len(xs) if xs else 0.0

    def ci(field):
        b = sorted(mean(field, [rng.choice(clusters) for _ in clusters]) for _ in range(2000))
        return [b[49], b[1949]]

    d = sum(r["directives"] for r in rows)
    u = sum(r["useful"] for r in rows)
    h = sum(r["harmful"] for r in rows)
    calls = sum(r["calls"] for r in rows)
    return {"sessions": len(rows), "tasks": len(clusters), "calls": calls, "directives": d, "useful": u,
            "harmful": h, "s_hat": mean("net", clusters), "s_hat_ci": ci("net"),
            "s_hat_usd": mean("net_usd", clusters), "s_hat_usd_ci": ci("net_usd"),
            "useful_rate": u / d if d else 0.0, "useful_lb90": wilson_lb(u, d),
            "harmful_rate": h / d if d else 0.0, "per_10_calls": 10 * d / calls if calls else 0.0}


def fmt(name: str, policy: str, s: dict) -> str:
    gates = []
    if s["directives"]:
        gates.append("LB≥0.8 " + ("✓" if s["useful_lb90"] >= 0.8 else "✗"))
        gates.append("harm≤1% " + ("✓" if s["harmful_rate"] <= 0.01 else "✗"))
        gates.append("≤1/10 " + ("✓" if s["per_10_calls"] <= 1 else "✗"))
    return (f"| {name} | {LABEL[policy]} | {s['sessions']} | {s['directives']} | {s['per_10_calls']:.2f} | "
            f"{s['useful_rate']:.2f} ({s['useful_lb90']:.2f}) | {s['harmful_rate']:.2f} | "
            f"**{s['s_hat']:+.2f}** [{s['s_hat_ci'][0]:+.2f}, {s['s_hat_ci'][1]:+.2f}] | "
            f"{s['s_hat_usd']:+.4f} | {', '.join(gates) or '–'} |")


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--manifests", type=Path, required=True)
    ap.add_argument("--adage", type=Path, default=REPO / "target" / "debug" / "adage")
    ap.add_argument("--repos", default="click,attrs,marshmallow,vue-core")
    ap.add_argument("--pools", default="python=click,attrs,marshmallow;haiku=click,attrs,marshmallow,vue-core")
    ap.add_argument("--json", type=Path)
    a = ap.parse_args(argv)
    repos = a.repos.split(",")
    rows: dict[str, dict[str, list[dict]]] = {p: {} for p in POLICIES}
    out = {"repos": {}, "pools": {}}
    print("| Repository | Policy | Sessions | Directives | per 10 calls | Useful (Wilson90 LB) | Harmful | "
          "Ŝ calls/session [95% CI] | Ŝ $/session | §5.2 gates |")
    print("| --- | --- | ---: | ---: | ---: | ---: | ---: | --- | ---: | --- |")
    for repo in repos:
        m = a.manifests / f"{repo}.json"
        for p, args in POLICIES.items():
            rep = replay(a.adage, m, args)
            for s in rep["sessions"]:
                s["_repo"] = repo
                s.pop("scored", None)
            rows[p][repo] = rep["sessions"]
            s = rep["summary"]
            out["repos"].setdefault(repo, {})[p] = {"summary": s, "routes": rep["routes"], "by_route": rep["by_route"]}
            print(fmt(repo, p, s))
    for pool in filter(None, a.pools.split(";")):
        name, members = pool.split("=")
        members = [r for r in members.split(",") if r in repos]
        if len(members) < 2:
            continue
        for p in POLICIES:
            s = pooled([r for repo in members for r in rows[p][repo]])
            out["pools"].setdefault(name, {})[p] = s
            print(fmt(f"**pooled: {name}**", p, s))
    if a.json:
        a.json.write_text(json.dumps(out, indent=1, default=float) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
