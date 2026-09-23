#!/usr/bin/env python3
"""Freeze a finished demo or live run into a results directory:

  index.html     the dashboard with the final state embedded (no server needed)
  artifact.html  the same page without its document skeleton, for publishing
  summary.json   per-arm statistics for the held-out runs, and what Adage learned
  state.json     the full run state (timelines, directives, metrics)

  python3 demo/report.py [ROOT] [--out DIR]

ROOT is a demo root (default /tmp/adage-demo) or a live-runner root (evals/live/runner.py). The
default output is demo/results/<stamp>-<model>/. The live runner calls build() itself and writes
<root>/results/.
"""

import json
import random
import re
import statistics
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent


def med(v):
    v = [x for x in v if x is not None]
    return statistics.median(v) if v else None


def avg(v):
    v = [x for x in v if x is not None]
    return sum(v) / len(v) if v else None


def perm_p(a, b, stat, n=10000, seed=7):
    """Two-sided permutation test for a difference in `stat` between samples a and b."""
    a = [x for x in a if x is not None]
    b = [x for x in b if x is not None]
    if len(a) < 2 or len(b) < 2:
        return None
    rng = random.Random(seed)
    obs = abs(stat(a) - stat(b))
    pool = a + b
    hits = 0
    for _ in range(n):
        rng.shuffle(pool)
        if abs(stat(pool[: len(a)]) - stat(pool[len(a):])) >= obs - 1e-12:
            hits += 1
    return (hits + 1) / (n + 1)


def flag(key):
    return lambda m: None if m.get(key) is None else (1.0 if m[key] else 0.0)


def follow_rate(m):
    f = m.get("follow") or {}
    return f["followed"] / f["exposed"] if f.get("exposed") else None


METRICS = {
    "turns_to_first_edit": (lambda m: m["turns_to_first_edit"], med),
    "time_to_first_edit_s": (lambda m: m["time_to_first_edit"], med),
    "total_turns": (lambda m: m["turns"], med),
    "elapsed_s": (lambda m: m["elapsed"], med),
    "explore_calls_before_edit": (lambda m: m.get("explore_calls_before_edit"), med),
    "dead_end_hits": (lambda m: m.get("dead_end_hits"), avg),
    "pass_rate": (flag("passed"), avg),
    # Newer runs record these; older states (the first demo) do not, and report None.
    "hidden_pass_rate": (flag("hidden_passed"), avg),
    "obligation_met_rate": (flag("obligation_met"), avg),
    "follow_rate": (follow_rate, avg),
    "stale_generated_rate": (flag("stale_generated"), avg),
    "cost_usd": (lambda m: m["cost_usd"], avg),
}


def stats_for(arm_runs):
    out = {}
    for name, (f, agg) in METRICS.items():
        h = [f(m) for m in arm_runs["holdout"]]
        a = [f(m) for m in arm_runs["adage"]]
        out[name] = {"without_adage": agg(h), "with_adage": agg(a), "aggregate": agg.__name__,
                     "p_value": perm_p(a, h, agg), "n": [sum(x is not None for x in a), sum(x is not None for x in h)]}
    return out


def summarise(state):
    runs = [r for r in state["runs"].values() if r.get("metrics")]
    all_ev = [r for r in runs if r["phase"] == "eval"]
    rounds = sorted({r.get("round", 1) for r in all_ev})
    latest = rounds[-1] if rounds else 1
    ev = [r for r in all_ev if r.get("round", 1) == latest]
    arms = {a: [r["metrics"] for r in ev if r["arm"] == a] for a in ("holdout", "adage")}
    summary = {
        "config": state.get("config"),
        "model": state.get("model"),
        "generated": time.strftime("%Y-%m-%dT%H:%M:%S"),
        "training_runs": sum(1 for r in runs if r["phase"] == "train"),
        "training_pass_rate": avg([1.0 if r["metrics"]["passed"] else 0.0 for r in runs if r["phase"] == "train"]),
        "eval_runs": {a: len(v) for a, v in arms.items()},
        "learned": state.get("learn"),
        "routes": [
            {"track": r["track"], "id": r["route"]["id"], "aspect": r["route"]["aspect"], "effect": r["route"]["effect"],
             "trigger": r["route"]["trigger"], "step": r["route"]["step"], "evidence": r["route"]["evidence"]}
            for r in state.get("tracks", [])
        ],
    }
    summary["latest_round"] = latest
    summary["metrics"] = stats_for(arms)
    summary["rounds"] = {}
    for rd in rounds:
        rr = [r for r in all_ev if r.get("round", 1) == rd]
        summary["rounds"][str(rd)] = {
            "metrics": stats_for({a: [r["metrics"] for r in rr if r["arm"] == a] for a in ("holdout", "adage")}),
            "routes": [x["route"]["id"] + ":" + x["route"]["aspect"] for x in ((state.get("rounds") or {}).get(str(rd), {}).get("tracks") or [])],
        }
    followed = sum(1 for r in ev if r["arm"] == "adage" for d in r["metrics"].get("adage", []) if d.get("kind") == "followed")
    emitted = sum(1 for r in ev if r["arm"] == "adage" for d in r["metrics"].get("adage", []) if d.get("kind") == "directives")
    summary["directives"] = {"emitted": emitted, "followed": followed,
                             "stop_requests": sum(1 for r in ev for d in r["metrics"].get("adage", []) if d.get("kind") == "stop_request")}
    return summary


def build(state, out):
    """Write the results directory for `state` into `out`; return `out`."""
    out = Path(out)
    out.mkdir(parents=True, exist_ok=True)
    summary = summarise(state)
    (out / "summary.json").write_text(json.dumps(summary, indent=2))
    (out / "state.json").write_text(json.dumps(state))
    page = (HERE / "dashboard" / "index.html").read_text()
    embed = "<script>window.__STATE__ = " + json.dumps(state).replace("</", "<\\/") + ";</script>\n<script>"
    page = page.replace("<script>", embed, 1).replace("live proving run", "proving run results", 1)
    labels = state.get("labels") or {}
    if labels.get("regen"):
        page = page.replace("</i>make gen</span>", f"</i>{labels['regen']}</span>", 1)
    if labels.get("lookup"):
        page = page.replace("</i>work-item lookup</span>", f"</i>{labels['lookup']}</span>", 1)
    (out / "index.html").write_text(page)
    # Artifact variant: the publisher supplies its own document skeleton.
    art = re.sub(r"<!doctype html>\s*<html[^>]*>\s*<head>\s*", "", page, flags=re.I)
    art = re.sub(r'<meta charset="utf-8">\s*<meta name="viewport"[^>]*>\s*', "", art)
    art = art.replace("</head>\n<body>", "").replace("</body>\n</html>", "")
    (out / "artifact.html").write_text(art)
    return out


def main(argv):
    args = list(argv)
    out = None
    if "--out" in args:
        i = args.index("--out")
        out = Path(args[i + 1])
        del args[i:i + 2]
    root = Path(args[0] if args else "/tmp/adage-demo")
    state = json.loads((root / "state.json").read_text())
    if out is None:
        model = (state.get("model") or "model").replace("/", "-")
        out = HERE / "results" / f"{time.strftime('%Y%m%d-%H%M')}-{model}"
    build(state, out)
    print(json.dumps(summarise(state)["metrics"], indent=2))
    print(f"\nwrote {out}")


if __name__ == "__main__":
    main(sys.argv[1:])
