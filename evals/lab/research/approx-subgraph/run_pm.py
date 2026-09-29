"""Process mining over learned activities, evaluated like the mined patterns.

Heuristics-Miner edges become two-step patterns (usefulness and delivery replay on the held-out
fold, same folds as run.py `grown`); the Inductive Miner's tree is fitted on each repository's
first-fold training log and summarised (flower mass, sequence nodes).

Usage: python3 run_pm.py [alphabet ...]  -> results/pm.json
"""

from __future__ import annotations

import json
import sys
from collections import Counter

import data
import evaluate as ev
import pm
from run import GAP, OUT, UNSTEERED, smin_for


def main(alphas):
    S = data.load()
    res = {}
    for r in data.BASE_REPOS:
        pool = [s for s in S if s.repo == r and s.model == "haiku" and s.arm in UNSTEERED]
        tasks = sorted({s.task for s in pool})
        for a in alphas:
            use, rep, edges_n, trees = [], [], [], []
            for f in range(5):
                hold = set(tasks[f::5])
                tr = [s for s in pool if s.task not in hold]
                te = [s for s in pool if s.task in hold and s.arm == "control"]
                if not te:
                    continue
                lab = ev.make(a, tr)
                X = [lab.labels(s) for s in tr]
                Y = [lab.labels(s) for s in te]
                E = pm.heuristics(X, smin_for(len(X)))
                pats = [e[0] for e in E]
                edges_n.append(len(E))
                use += [{**{k: v for k, v in u.items() if k not in ("pattern", "prefix", "suffix")},
                         "prefix": list(u["prefix"]), "suffix": list(u["suffix"])} for u in ev.usefulness(pats, Y, GAP)]
                dirs = [((p[0],), (p[1],)) for p in pats if not p[1].endswith("!")]
                rep += ev.replay(dirs, te, Y, GAP)
                if f == 0:
                    freq = Counter(x for s in X for x in s)
                    tree = pm.im(X)
                    st = pm.tree_stats(tree, freq)
                    trees.append({"stats": {**st, "seq_nodes": [[lab.describe(x, 1) for x in q] for q in st["seq_nodes"][:8]]},
                                  "render": pm.render(tree, lambda x: lab.describe(x, 1)[:70]),
                                  "hm_top": [{"edge": [lab.describe(e[0][0], 1), lab.describe(e[0][1], 1)],
                                              "dep": e[1], "count": e[2], "sessions": e[3]} for e in E[:10]]})
            res[f"{r}|{a}"] = {"edges": edges_n, "use": use, "replay": rep, "trees": trees}
            fol = sum(u["n_follow"] for u in use)
            pre = sum(u["n_prefix"] for u in use)
            net = sum(x["net"]["all"] for x in rep) / max(1, len(rep))
            print(r, a, "edges", edges_n, "follow", fol, "/", pre, "net_all", round(net, 2),
                  "flower_mass", round(trees[0]["stats"]["flower_mass"], 2) if trees else None, flush=True)
    OUT.mkdir(exist_ok=True)
    (OUT / "pm.json").write_text(json.dumps(res, default=str))


if __name__ == "__main__":
    main(sys.argv[1:] or ["key", "emb@0.8", "emb@0.7"])
