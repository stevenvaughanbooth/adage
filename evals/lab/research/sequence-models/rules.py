"""Which skip-ahead rules does the sequence model discover on its own?

Rule a => b: "at a decision point whose last call is in cluster a, the agent will eventually take
cluster b, after at least one failed call" (the learned target y). Fitted on every non-control
session (train arm + adage + agents-md arms: 380 sessions), scored on the held-out control sessions
(Wilson 90% CI on the rule's precision there). Rules are ranked by held-in P(y | a, b) with support
>= 8 sessions and lift over P(y | b) >= 1.5. Clusters are shown by their most frequent members.
"""

from __future__ import annotations

import collections
import json
from pathlib import Path

import numpy as np

from run import prep
from skip import load, wilson

HERE = Path(__file__).resolve().parent


def table(data, K):
    n_a = collections.Counter()
    n_ab = collections.Counter()
    sess_ab = collections.defaultdict(set)
    n_b = np.zeros(K)
    n_tot = 0
    dist = collections.defaultdict(list)
    inst = collections.defaultdict(list)
    for s, lab in data:
        for r, i in enumerate(s.dp):
            if i == 0:
                continue
            a = int(s.c[i - 1])
            n_a[a] += 1
            n_tot += 1
            ys = np.where(lab["y"][r])[0]
            n_b[ys] += 1
            for b in ys:
                n_ab[a, int(b)] += 1
                sess_ab[a, int(b)].add(s.sid)
                t = lab["t"][r, b]
                dist[a, int(b)].append(t - i)
                inst[a, int(b)].append(int(s.inst[i:t].sum()))
    return n_a, n_ab, sess_ab, n_b / max(n_tot, 1), dist, inst


def main(K=128, top=25):
    S = load()
    tr = [s for s in S if s["arm"] != "control"]
    te = [s for s in S if s["arm"] == "control"]
    sp, W, trd, ted = prep(tr, te, K)
    n_a, n_ab, sess_ab, pb, dist, inst = table(trd, sp.K)
    t_a, t_ab, _, _, t_dist, t_inst = table(ted, sp.K)
    rules = []
    for (a, b), k in n_ab.items():
        if len(sess_ab[a, b]) < 8:
            continue
        p = k / n_a[a]
        lift = p / max(pb[b], 1e-9)
        if lift < 1.5:
            continue
        rules.append({
            "a": sp.describe(a, 3), "b": sp.describe(b, 3), "p_train": round(p, 3), "lift": round(lift, 2),
            "support_sessions": len(sess_ab[a, b]),
            "train_skip_dist": round(float(np.mean(dist[a, b])), 1),
            "train_inst_waste": round(float(np.mean(inst[a, b])), 2),
            "held_out_n": t_a[a], "held_out_precision": round(t_ab[a, b] / t_a[a], 3) if t_a[a] else None,
            "held_out_ci": wilson(t_ab[a, b], t_a[a]),
            "held_out_inst_waste": round(float(np.mean(t_inst[a, b])), 2) if t_inst[a, b] else None,
            "repos": sorted({s.repo for s, _ in trd if s.sid in sess_ab[a, b]}),
        })
    rules.sort(key=lambda r: -r["p_train"])
    for r in rules[:top]:
        print(json.dumps(r))
    (HERE / "results").mkdir(exist_ok=True)
    (HERE / "results" / "rules.json").write_text(json.dumps(rules[:60], indent=1, default=float))
    return rules


if __name__ == "__main__":
    main()
