"""Live RepoBench v1 result of the agents-md arm vs control (same tasks, same round, same harness).

Reads each run's state.json metrics only (no transcripts). Paired by (task, round); task-clustered
90% bootstrap of the mean paired difference. Sessions were capped at 30 turns in v1 (censored), so
calls and $ are truncated for both arms alike.
"""
import json, re, sys
from collections import defaultdict
import numpy as np

BASE = "/tmp/adage-repobench/runs"
REPOS = ["attrs", "click", "marshmallow", "urfave-cli", "vue-core", "click-sonnet5", "vue-core-sonnet5"]


def rows():
    out = []
    for repo in REPOS:
        s = json.load(open(f"{BASE}/{repo}/state.json"))["runs"]
        for rid, r in s.items():
            m = r.get("metrics") or {}
            if r.get("phase") != "eval" or not m or r.get("arm") not in ("control", "agents-md", "adage"):
                continue
            out.append(dict(repo=repo, task=r["task"], round=re.search(r"-r(\d+)-", rid).group(1), arm=r["arm"],
                            cost=m.get("cost_usd"), calls=m.get("tool_calls"), turns=m.get("turns"),
                            passed=bool(m.get("hidden_passed", m.get("passed"))),
                            censored=m.get("status") == "censored",
                            first_edit=m.get("turns_to_first_edit"), explore=m.get("explore_calls_before_edit")))
    return out


def paired(rs, a, b, key, repos):
    by = defaultdict(dict)
    for r in rs:
        if r["repo"] in repos and r[key] is not None:
            by[(r["task"], r["round"])][r["arm"]] = float(r[key])
    diffs = defaultdict(list)
    base = defaultdict(list)
    for (t, _), d in by.items():
        if a in d and b in d:
            diffs[t].append(d[a] - d[b])
            base[t].append(d[b])
    tasks = sorted(diffs)
    if not tasks:
        return None
    rng = np.random.default_rng(7)
    per = np.array([np.mean(diffs[t]) for t in tasks])
    bs = [np.mean(per[rng.integers(0, len(per), len(per))]) for _ in range(5000)]
    bmean = np.mean([np.mean(base[t]) for t in tasks])
    return dict(n_tasks=len(tasks), n_pairs=sum(len(v) for v in diffs.values()), diff=float(per.mean()),
                ci90=[float(np.percentile(bs, 5)), float(np.percentile(bs, 95))], control_mean=float(bmean),
                rel=float(per.mean() / bmean) if bmean else None)


if __name__ == "__main__":
    rs = rows()
    res = {}
    for label, repos in [("haiku-all5", REPOS[:5]), ("haiku-python3", REPOS[:3]), ("sonnet5", REPOS[5:])] + \
            [(r, [r]) for r in REPOS]:
        res[label] = {k: paired(rs, "agents-md", "control", k, repos)
                      for k in ("cost", "calls", "passed", "first_edit", "explore")}
        c = sum(1 for r in rs if r["repo"] in repos and r["censored"] and r["arm"] == "control")
        n = sum(1 for r in rs if r["repo"] in repos and r["arm"] == "control")
        res[label]["control_censored"] = f"{c}/{n}"
    json.dump(res, open("results/live_agents_md.json", "w"), indent=1)
    for label, d in res.items():
        print(label, d["control_censored"])
        for k in ("cost", "calls", "passed", "first_edit", "explore"):
            x = d[k]
            if x:
                print(f"   {k:10s} ctrl {x['control_mean']:.3f}  diff {x['diff']:+.3f} [{x['ci90'][0]:+.3f},{x['ci90'][1]:+.3f}] rel {x['rel']:+.1%} tasks {x['n_tasks']} pairs {x['n_pairs']}")
