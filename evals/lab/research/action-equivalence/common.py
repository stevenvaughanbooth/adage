"""Shared: fit sets, baselines that need the abstraction lab's per-call labels, statistics."""
from __future__ import annotations

import math
import pickle
import sys
from collections import Counter
from pathlib import Path

import numpy as np

import data
import methods as M

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "abstraction"))

FIT_ARMS = ("train", "adage", "agents-md")


def sessions():
    return data.load()


def fit_calls(S, exclude_repo=None):
    return M.bash_calls([s for s in S if s.arm in FIT_ARMS and s.repo != exclude_repo])


def baseline_tables(S):
    """cmd -> label for the hand-written baselines (taxonomy = action.rs's equivalence via grammar.py;
    intent; lgg-loose from evals/lab/abstraction)."""
    p = data.CACHE / "baselines.pkl"
    if p.exists():
        return pickle.load(open(p, "rb"))
    import lgg
    L = pickle.load(open("/tmp/adage-abstraction-cache.pkl", "rb"))
    fx = {}
    for lab in L:
        fx.setdefault(lab.sess.repo, lab.facts)
    tax, intent, loose = {}, {}, {}
    for s in S:
        for r in s.recs:
            if r.tool != "Bash" or not r.cmd.strip():
                continue
            tax.setdefault(r.cmd, f"{r.role}|{r.runner}|{r.envclass}")
            intent.setdefault(r.cmd, r.intent)
            try:
                loose.setdefault(r.cmd, lgg.skeleton(r.cmd, "", fx[s.repo], loose=True))
            except Exception:
                loose.setdefault(r.cmd, "<err>")
    out = {"taxonomy": tax, "intent": intent, "lgg-loose": loose}
    pickle.dump(out, open(p, "wb"))
    return out


def table_rule(name, table):
    return M.Rule(name, lambda c: table.get(c, "?:" + c))


def wilson(k, n, z=1.645):
    if n == 0:
        return (0.0, 0.0)
    p = k / n
    d = 1 + z * z / n
    c = (p + z * z / (2 * n)) / d
    h = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / d
    return (c - h, c + h)


def boot(values, groups=None, B=2000, seed=7, q=(5, 95)):
    """Mean and 90% percentile bootstrap CI over sessions (or over groups when given)."""
    v = np.asarray(values, float)
    rng = np.random.default_rng(seed)
    if groups is None:
        idx = rng.integers(0, len(v), (B, len(v)))
        bs = v[idx].mean(1)
    else:
        g = np.asarray(groups)
        ug = np.unique(g)
        members = [np.where(g == x)[0] for x in ug]
        bs = []
        for _ in range(B):
            pick = rng.integers(0, len(ug), len(ug))
            ii = np.concatenate([members[j] for j in pick])
            bs.append(v[ii].mean())
        bs = np.array(bs)
    return float(v.mean()), float(np.percentile(bs, q[0])), float(np.percentile(bs, q[1]))


def clustered(name, S, exclude_repo=None):
    """A fitted `methods.Clustered` for embedder `name`, cached on disk (potion is re-loaded)."""
    p = data.CACHE / f"clustered-{name}-{exclude_repo or 'all'}.pkl"
    if p.exists() and name != "potion":
        return pickle.load(open(p, "rb"))
    m = M.Clustered(M.EMBEDDERS[name]()).fit(fit_calls(S, exclude_repo))
    if name != "potion":
        pickle.dump(m, open(p, "wb"))
    return m
