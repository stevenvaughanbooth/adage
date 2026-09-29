#!/usr/bin/env python3
"""RepoBench v1 (30 turns) against v2 (60 turns), per repository and arm, from the runner roots.

  python3 evals/launch/rb_compare.py --v1 /tmp/adage-repobench/runs --v2 /tmp/adage-rb2/runs
        [--repos click,attrs,marshmallow,vue-core] [--jsonl out.jsonl]

Per (version, repository, arm): kept runs (infra and scorer errors left out, as the runner does),
resolve rate, mean cost, $ per resolved task, median turns and turns to first edit, runs that hit
the turn cap, mean dead-end hits, sessions that received at least one directive, directives per
session, and Adage's own follow judgements (followed / exposed).
"""

from __future__ import annotations

import argparse
import json
import re
import statistics
from pathlib import Path


def kept(state):
    for rid, r in state["runs"].items():
        if r.get("phase") == "train" or r.get("status") not in ("passed", "failed"):
            continue
        m = r.get("metrics") or {}
        if m.get("status") in ("infra_error", "scorer_error"):
            continue
        yield rid, r, m


def routes(r):
    out = []
    for d in r.get("directives") or []:
        out += sorted(set(re.findall(r'route=\\*"([A-Za-z0-9_.-]+)', d.get("raw", ""))))
    return out


def row(version, repo, arm, runs, cap):
    ms = [m for _, _, m in runs]
    n = len(ms)
    passed = sum(bool(m.get("passed")) for m in ms)
    cost = sum(m.get("cost_usd") or 0 for m in ms)
    turns = [min(m.get("turns") or 0, cap) for m in ms]
    ttfe = [m.get("turns_to_first_edit") if m.get("turns_to_first_edit") is not None else min(m.get("turns") or cap, cap)
            for m in ms]
    fol = [m.get("follow") or {} for m in ms]
    dirs = [routes(r) for _, r, _ in runs]
    return {"version": version, "repo": repo, "arm": arm, "runs": n, "resolve_rate": round(passed / n, 3) if n else None,
            "mean_cost": round(cost / n, 3) if n else None, "usd_per_resolved": round(cost / passed, 3) if passed else None,
            "median_turns": statistics.median(turns) if turns else None,
            "median_ttfe": statistics.median(ttfe) if ttfe else None,
            "hit_cap": sum(m.get("status") == "censored" for m in ms),
            "dead_end_hits": round(statistics.mean(m.get("dead_end_hits") or 0 for m in ms), 2) if n else None,
            "sessions_with_directive": sum(bool(d) for d in dirs),
            "directives_per_session": round(sum(len(d) for d in dirs) / n, 2) if n else None,
            "judged_followed": sum(f.get("followed", 0) for f in fol), "judged_exposed": sum(f.get("exposed", 0) for f in fol),
            "routes": sorted({x for d in dirs for x in d})}


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--v1", default="/tmp/adage-repobench/runs")
    ap.add_argument("--v2", default="/tmp/adage-rb2/runs")
    ap.add_argument("--repos", default="click,attrs,marshmallow,vue-core")
    ap.add_argument("--suffix", default="")
    ap.add_argument("--jsonl")
    a = ap.parse_args(argv)
    rows = []
    for repo in a.repos.split(","):
        for version, base in (("v1", a.v1), ("v2", a.v2)):
            p = Path(base) / (repo + a.suffix) / "state.json"
            if not p.exists():
                continue
            st = json.loads(p.read_text())
            cap = st.get("max_turns") or 30
            runs = list(kept(st))
            for arm in ("control", "adage", "agents-md"):
                rs = [x for x in runs if x[1].get("arm") == arm]
                if rs:
                    rows.append(row(version, repo, arm, rs, cap))
    cols = ["version", "repo", "arm", "runs", "resolve_rate", "mean_cost", "usd_per_resolved", "median_turns",
            "median_ttfe", "hit_cap", "dead_end_hits", "sessions_with_directive", "directives_per_session",
            "judged_followed", "judged_exposed"]
    print("| " + " | ".join(cols) + " |")
    print("|" + " --- |" * len(cols))
    for r in rows:
        print("| " + " | ".join(str(r[c]) for c in cols) + " |")
    if a.jsonl:
        Path(a.jsonl).write_text("\n".join(json.dumps(r) for r in rows) + "\n")


if __name__ == "__main__":
    main()
