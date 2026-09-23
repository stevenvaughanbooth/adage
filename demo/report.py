#!/usr/bin/env python3
"""Freeze a finished demo run into demo/results/<stamp>-<model>/:

  index.html    the dashboard with the final state embedded (no server needed)
  summary.json  per-arm statistics for the held-out runs, and what Adage learned
  state.json    the full run state (timelines, directives, metrics)

  python3 demo/report.py [DEMO_ROOT]
"""

import json
import random
import statistics
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = Path(sys.argv[1] if len(sys.argv) > 1 else "/tmp/adage-demo")


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


def main():
    state = json.loads((ROOT / "state.json").read_text())
    runs = [r for r in state["runs"].values() if r.get("metrics")]
    all_ev = [r for r in runs if r["phase"] == "eval"]
    rounds = sorted({r.get("round", 1) for r in all_ev})
    latest = rounds[-1] if rounds else 1
    ev = [r for r in all_ev if r.get("round", 1) == latest]
    arms = {a: [r["metrics"] for r in ev if r["arm"] == a] for a in ("holdout", "adage")}
    metrics = {
        "turns_to_first_edit": (lambda m: m["turns_to_first_edit"], med),
        "time_to_first_edit_s": (lambda m: m["time_to_first_edit"], med),
        "total_turns": (lambda m: m["turns"], med),
        "elapsed_s": (lambda m: m["elapsed"], med),
        "explore_calls_before_edit": (lambda m: m["explore_calls_before_edit"], med),
        "dead_end_hits": (lambda m: m["dead_end_hits"], avg),
        "pass_rate": (lambda m: 1.0 if m["passed"] else 0.0, avg),
        "stale_generated_rate": (lambda m: 1.0 if m["stale_generated"] else 0.0, avg),
        "cost_usd": (lambda m: m["cost_usd"], avg),
    }
    summary = {
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
        "metrics": {},
    }
    def stats_for(arm_runs):
        out = {}
        for name, (f, agg) in metrics.items():
            h = [f(m) for m in arm_runs["holdout"]]
            a = [f(m) for m in arm_runs["adage"]]
            out[name] = {"without_adage": agg(h), "with_adage": agg(a), "aggregate": agg.__name__,
                         "p_value": perm_p(a, h, agg), "n": [len(a), len(h)]}
        return out
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

    model = (state.get("model") or "model").replace("/", "-")
    out = HERE / "results" / f"{time.strftime('%Y%m%d-%H%M')}-{model}"
    out.mkdir(parents=True, exist_ok=True)
    (out / "summary.json").write_text(json.dumps(summary, indent=2))
    (out / "state.json").write_text(json.dumps(state))
    page = (HERE / "dashboard" / "index.html").read_text()
    embed = "<script>window.__STATE__ = " + json.dumps(state).replace("</", "<\\/") + ";</script>\n<script>"
    page = page.replace("<script>", embed, 1).replace("live proving run", "proving run results", 1)
    (out / "index.html").write_text(page)
    # Artifact variant: the publisher supplies its own document skeleton.
    import re as _re
    art = _re.sub(r"<!doctype html>\s*<html[^>]*>\s*<head>\s*", "", page, flags=_re.I)
    art = _re.sub(r'<meta charset="utf-8">\s*<meta name="viewport"[^>]*>\s*', "", art)
    art = art.replace("</head>\n<body>", "").replace("</body>\n</html>", "")
    (out / "artifact.html").write_text(art)
    print(json.dumps(summary["metrics"], indent=2))
    print(f"\nwrote {out}")


if __name__ == "__main__":
    main()
