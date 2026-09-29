"""Uptake calibration against the live agents-md arm (RepoBench v1, Haiku 4.5, same tasks and rounds).

The offline scorer says AGENTS.md "obviates" a discovery when it contains every atom the discovery
needed. If agents fully used the file, the agents-md arm would make none of those discoveries.
Measured here, per session: calls spent on discoveries that the repository's AGENTS.md covers,
in control (v1) vs agents-md sessions of the same (task, round). uptake = 1 - agents-md / control.
Also the same for each fact kind, and for uncovered discoveries (a check that the arms are
otherwise comparable). Paired on (task, round), task-clustered 90% bootstrap.
"""
import json, re
from collections import defaultdict
from pathlib import Path
import numpy as np
import facts, kd_load, os
RES = "results/strict" if os.environ.get("KD_STRICT") == "1" else "results"
from common import events_for, dollars

HERE = Path(__file__).resolve().parent
CTX = HERE.parents[2] / "repobench" / "context"
REPOS = ["attrs", "click", "marshmallow", "urfave-cli", "vue-core"]
S = kd_load.load()
E = events_for(S)


def cov_calls(s, A, kind=None, covered=True):
    ks = set()
    for e in E[s.sid]:
        if kind and e.kind != kind:
            continue
        if facts.covers(A, e.need) == covered:
            ks |= set(e.calls)
    return ks


def rnd(s):
    return re.search(r"-r(\d+)-", s.run + "-").group(1)


res = {}
pairs = defaultdict(dict)
for s in S:
    if getattr(s, "version", "") == "v1" and s.arm in ("control", "agents-md"):
        pairs[(s.repo, s.task, rnd(s))][s.arm] = s
rows = []
for (repo, task, _), d in pairs.items():
    if len(d) < 2:
        continue
    A = facts.atoms((CTX / f"{repo}.md").read_text())
    row = dict(repo=repo, task=task)
    for arm, s in d.items():
        n30 = len(s.calls)
        row[f"{arm}_cov"] = len(cov_calls(s, A))
        row[f"{arm}_cov_loc"] = len(cov_calls(s, A, "locator"))
        row[f"{arm}_cov_chain"] = len(cov_calls(s, A, "chain"))
        row[f"{arm}_uncov"] = len(cov_calls(s, A, covered=False))
        row[f"{arm}_calls"] = n30
        row[f"{arm}_fail"] = sum(c.error for c in s.calls)
    rows.append(row)


def boot_pair(a, b, groups, n=4000, seed=7):
    a, b = np.array(a, float), np.array(b, float)
    g = defaultdict(list)
    for i, k in enumerate(groups):
        g[k].append(i)
    idx = list(g.values())
    rng = np.random.default_rng(seed)
    bs = []
    for _ in range(n):
        sel = np.concatenate([idx[p] for p in rng.integers(0, len(idx), len(idx))])
        bs.append((a[sel].mean(), b[sel].mean(), 1 - b[sel].sum() / max(a[sel].sum(), 1e-9)))
    bs = np.array(bs)
    return dict(control=round(a.mean(), 3), agents_md=round(b.mean(), 3), diff=round(b.mean() - a.mean(), 3),
                diff_ci90=[round(float(np.percentile(bs[:, 1] - bs[:, 0], 5)), 3), round(float(np.percentile(bs[:, 1] - bs[:, 0], 95)), 3)],
                uptake=round(1 - b.sum() / max(a.sum(), 1e-9), 3),
                uptake_ci90=[round(float(np.percentile(bs[:, 2], 5)), 3), round(float(np.percentile(bs[:, 2], 95)), 3)])


if __name__ == "__main__":
    for label, sub in [("all", rows)] + [(r, [x for x in rows if x["repo"] == r]) for r in REPOS]:
        g = [x["task"] for x in sub]
        res[label] = {k: boot_pair([x[f"control_{k}"] for x in sub], [x[f"agents-md_{k}"] for x in sub], g)
                      for k in ("cov", "cov_loc", "cov_chain", "uncov", "calls", "fail")}
        res[label]["n_pairs"] = len(sub)
    json.dump(res, open(Path(RES) / "calibration.json", "w"), indent=1)
    for label, d in res.items():
        print(label, d["n_pairs"])
        for k, v in d.items():
            if isinstance(v, dict):
                print(f"   {k:10s} ctrl {v['control']:.2f} agents-md {v['agents_md']:.2f} diff {v['diff']:+.2f} {v['diff_ci90']} uptake {v['uptake']:.2f} {v['uptake_ci90']}")
