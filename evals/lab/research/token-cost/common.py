"""Shared: pricing, loading, token calibration, carry-cost attribution, bootstrap."""
from __future__ import annotations

import pickle
import numpy as np

# $ per million tokens. Haiku 4.5 and Sonnet 5 first-party list prices (claude-api skill table,
# cached 2026-09-25); cache multipliers 1.25x (5m write), 2x (1h write), 0.1x (read).
# Validated against Claude Code's own `cost-state.totalCostUSD` in each transcript (see README).
PRICES = {
    "haiku": dict(inp=1.00, out=5.00, cw5m=1.25, cw1h=2.00, cr=0.10),
    "sonnet": dict(inp=2.00, out=10.00, cw5m=2.50, cw1h=4.00, cr=0.20),
}
# Sonnet 4.6 for a sensitivity row (the same traces priced at $3/$15)
PRICES_S46 = dict(inp=3.00, out=15.00, cw5m=3.75, cw1h=6.00, cr=0.30)


def fam(model: str) -> str:
    return "sonnet" if "sonnet" in (model or "") else "haiku"


def load():
    with open("/tmp/adage-research/token-cost/sessions.pkl", "rb") as fh:
        return pickle.load(fh)


def call_cost(c, p):
    return (c["inp"] * p["inp"] + c["out"] * p["out"] + c["cw1h"] * p["cw1h"] + c["cw5m"] * p["cw5m"]
            + (c["cw"] - c["cw1h"] - c["cw5m"]) * p["cw5m"] + c["cr"] * p["cr"]) / 1e6


def session_cost(s, p=None):
    p = p or PRICES[fam(s["model"])]
    return sum(call_cost(c, p) for c in s["calls"])


def fit_cpt(sessions):
    """Chars per token of tool-result text, per model family: least squares of the cache write
    at each call on (tool-result chars, other new chars, previous output tokens)."""
    out = {}
    for f in ("haiku", "sonnet"):
        X, y = [], []
        for s in sessions:
            if fam(s["model"]) != f:
                continue
            cs = s["calls"]
            for k in range(1, len(cs)):
                X.append([cs[k]["res_chars"], cs[k]["other_chars"], cs[k - 1]["out"], 1.0])
                y.append(cs[k]["cw"])
        X, y = np.array(X, float), np.array(y, float)
        coef, *_ = np.linalg.lstsq(X, y, rcond=None)
        pred = X @ coef
        r2 = 1 - ((y - pred) ** 2).sum() / ((y - y.mean()) ** 2).sum()
        out[f] = dict(tok_per_char=coef[0], other=coef[1], out=coef[2], const=coef[3], r2=r2, n=len(y))
    return out


def carry_multiplier(s, seen_call):
    """$ per token for content first seen at call `seen_call`: one cache write + a cache read on
    every later call. Content never sent (session ended) costs nothing."""
    p = PRICES[fam(s["model"])]
    n = len(s["calls"])
    if seen_call >= n:
        return 0.0
    w = p["cw1h"] if s["calls"][seen_call]["cw1h"] or not s["calls"][seen_call]["cw5m"] else p["cw5m"]
    return (w + p["cr"] * (n - 1 - seen_call)) / 1e6


def boot(vals, n=4000, seed=0, stat=np.mean):
    vals = np.asarray(vals, float)
    if len(vals) == 0:
        return (float("nan"),) * 3
    rng = np.random.default_rng(seed)
    idx = rng.integers(0, len(vals), (n, len(vals)))
    bs = np.array([stat(vals[i]) for i in idx])
    return float(stat(vals)), float(np.percentile(bs, 5)), float(np.percentile(bs, 95))


def boot_ratio(num, den, n=4000, seed=0):
    num, den = np.asarray(num, float), np.asarray(den, float)
    rng = np.random.default_rng(seed)
    idx = rng.integers(0, len(num), (n, len(num)))
    bs = num[idx].sum(1) / den[idx].sum(1)
    return float(num.sum() / den.sum()), float(np.percentile(bs, 5)), float(np.percentile(bs, 95))


def harness_sets(S, model):
    """Held-out control sessions and training sessions exactly as the common harness defines them
    (evals/lab/research/harness: one session per run, the one state.json names)."""
    import json
    ids = json.load(open("/tmp/adage-research/token-cost/harness_ids.json"))
    h, t = set(ids[model]), set(ids["train"])
    return [s for s in S if s["sid"] in h], [s for s in S if s["sid"] in t]


def boot_cluster(num, den=None, groups=None, n=4000, seed=0):
    """90% task-clustered percentile bootstrap of mean(num) (den None) or sum(num)/sum(den)."""
    num = np.asarray(num, float)
    den = None if den is None else np.asarray(den, float)
    groups = np.asarray(groups if groups is not None else np.arange(len(num)))
    ug = np.unique(groups)
    idx = [np.where(groups == g)[0] for g in ug]
    rng = np.random.default_rng(seed)
    stat = (lambda i: num[i].mean()) if den is None else (lambda i: num[i].sum() / den[i].sum())
    full = np.arange(len(num))
    bs = []
    for _ in range(n):
        pick = np.concatenate([idx[j] for j in rng.integers(0, len(ug), len(ug))])
        bs.append(stat(pick))
    return float(stat(full)), float(np.percentile(bs, 5)), float(np.percentile(bs, 95))
