"""Is within-task pass/fail predictable from behaviour at all? (a diagnostic, not a lever)

Features: every learned-cluster feature from `levers.py` (k=12 and k=40), centred within the
task x model x bench stratum, so only within-task differences remain. Model: L2 logistic
regression. Evaluation: leave-one-task-out; the score is the within-stratum AUC (the probability
that a passing session outranks a failing one of the same stratum), pooled over held-out mixed
strata, with a task bootstrap 90% CI. 0.5 = behaviour carries no within-task signal.
"""

from __future__ import annotations

import json
import sys
from collections import Counter, defaultdict

import numpy as np
from sklearn.linear_model import LogisticRegression

import clusters as CL
from levers import binarise, features, stratum
from load import load


def pairs_auc(scores, S):
    by = defaultdict(list)
    for sc, s in zip(scores, S):
        by[stratum(s)].append((sc, s["passed"], s["task"]))
    out = []  # (task, wins, pairs)
    for v in by.values():
        P = [x[0] for x in v if x[1]]; N = [x[0] for x in v if not x[1]]
        if P and N:
            w = sum((p > n) + 0.5 * (p == n) for p in P for n in N)
            out.append((v[0][2], w, len(P) * len(N)))
    return out


def run(pop: str):
    S = [s for s in load() if s["phase"] == "eval" and (pop == "all" or not s["censored"])]
    cnt = Counter(stratum(s) for s in S)
    S = [s for s in S if cnt[stratum(s)] >= 2]
    F = {}
    for k in (12, 40):
        lab = CL.assign(load(), CL.fit(load(), k), None)
        for s in S:
            F.setdefault(s["sid"], {}).update(features(s, lab[s["sid"]], k, pairs=(k == 12)))
    binarise(S, F)
    keys = sorted(F[S[0]["sid"]])
    X = np.array([[F[s["sid"]][k] for k in keys] for s in S], float)
    y = np.array([s["passed"] for s in S], int)
    # centre within stratum
    by = defaultdict(list)
    for i, s in enumerate(S):
        by[stratum(s)].append(i)
    Xc = X.copy()
    for idx in by.values():
        Xc[idx] -= X[idx].mean(0)
    scores = np.zeros(len(S))
    tasks = sorted({s["task"] for s in S})
    for t in tasks:
        te = np.array([s["task"] == t for s in S])
        tr = ~te
        if len(set(y[tr])) < 2:
            continue
        m = LogisticRegression(C=0.05, max_iter=2000).fit(Xc[tr], y[tr])
        scores[te] = m.decision_function(Xc[te])
    res = pairs_auc(scores, S)
    auc = sum(r[1] for r in res) / sum(r[2] for r in res)
    rng = np.random.default_rng(0)
    bs = []
    for _ in range(2000):
        pick = [res[i] for i in rng.integers(0, len(res), len(res))]
        bs.append(sum(r[1] for r in pick) / sum(r[2] for r in pick))
    lo, hi = np.quantile(bs, [0.05, 0.95])
    return dict(pop=pop, within_task_auc=round(auc, 3), ci90=[round(lo, 3), round(hi, 3)],
                mixed_strata=len(res), pairs=sum(r[2] for r in res), n_features=len(keys))


if __name__ == "__main__":
    out = [run("all"), run("uncensored")]
    print(json.dumps(out, indent=1))
    json.dump(out, open("separability.json", "w"), indent=1)
