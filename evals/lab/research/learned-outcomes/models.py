"""The learned outcome models, and the hand-labelled evaluation.

Scores compared on the 277 hand-labelled control-arm calls (labels.txt; the instrument):

  baselines (hand-written or harness-only; the things to beat)
    B0 `is_error`            the harness flag alone
    B1 `hand-coded`          replay/adage-core: failed = is_error or env signature; success =
                             tests executed (EXECUTED regex), then not failed
  learned (no hand-written strings; trained on sessions that are NOT control-arm, or for
  leave-one-repo-out on no session of the held-out repository)
    L1 `drain→exit`          Drain line templates (hashed) → logistic regression, taught by the
                             harness's exit status (`is_error`) on the unlabelled pool
    L2 `potion→exit`         potion-8M output embedding (+ action embedding, tool) → LR, same teacher
    L3 `drain+potion→exit`   both
    L4 `retry-excess`        self-supervision from the agent's next move: LR predicts `retry` from
                             output+action; score = logit(output+action) − logit(action only), the
                             part of the retry odds the output explains
    L5 `kmeans-cluster`      k-means (64) of output embeddings; a cluster's exit-failure rate and
                             retry excess on the pool
    L6 `td-advantage`        see value.py (TD(λ) over output-cluster states, session reward)
    L7 `combined`            L3 and L4 stacked (sum of standardised logits)
    L3h / L7h                the same, at runtime OR'd with the harness flag (max(is_error, p))
  success score: LR on the self-supervised `recover` target (last attempt of a retry chain,
  then the agent moved on), times (1 − failure score).
"""

from __future__ import annotations

import json
from pathlib import Path

import numpy as np
from scipy.sparse import csr_matrix, hstack
from sklearn.cluster import MiniBatchKMeans
from sklearn.linear_model import LogisticRegression
from sklearn.metrics import roc_auc_score, f1_score

import features as F
import instrument as I
import load

HERE = Path(__file__).resolve().parent


def labels(a):
    idx = {(s, int(i)): k for k, (s, i) in enumerate(zip(a["sid"], a["idx"]))}
    todo = {json.loads(l)["n"]: json.loads(l) for l in (HERE / "labels_todo.jsonl").open()}
    out = []
    for l in (HERE / "labels.txt").open():
        if l.startswith("#") or not l.strip():
            continue
        n, code = l.split()
        sid, i = todo[int(n)]["id"].rsplit(":", 1)
        out.append((idx[(sid, int(i))], code))
    return np.array([k for k, _ in out]), np.array([c for _, c in out])


def tool_onehot(tools, vocab):
    M = np.zeros((len(tools), len(vocab) + 1), np.float32)
    for i, t in enumerate(tools):
        M[i, vocab.get(t, len(vocab))] = 1
    return M


def lr(X, y, C=1.0):
    m = LogisticRegression(C=C, max_iter=2000, class_weight="balanced")
    m.fit(X, y)
    return m


def logit(p):
    p = np.clip(p, 1e-6, 1 - 1e-6)
    return np.log(p / (1 - p))


def zs(x, ref):
    return (x - ref.mean()) / (ref.std() + 1e-9)


class Fold:
    """Everything learned from one training pool (a boolean mask over the call table)."""

    def __init__(self, a, H, fit, seed=0):
        self.a, self.fit = a, fit
        EO, EA = a["EO"].astype(np.float32), a["EA"].astype(np.float32)
        vocab = {t: i for i, t in enumerate(sorted(set(a["tool"][fit])))}
        T = tool_onehot(a["tool"], vocab)
        ln = np.c_[np.log1p(a["out_len"]) / 10, (a["out_len"] == 0)].astype(np.float32)
        y_exit, y_retry, y_rec = a["is_error"], a["retry"], a["recover"]
        Xd = H
        Xp = np.c_[EO, EA, T, ln]
        Xdp = hstack([H, csr_matrix(Xp)]).tocsr()
        Xact = np.c_[EA, T]
        self.s = {}
        self.s["drain→exit"] = lr(Xd[fit], y_exit[fit]).predict_proba(Xd)[:, 1]
        self.s["potion→exit"] = lr(Xp[fit], y_exit[fit]).predict_proba(Xp)[:, 1]
        m3 = lr(Xdp[fit], y_exit[fit], C=0.5)
        self.m3 = m3
        self.s["drain+potion→exit"] = m3.predict_proba(Xdp)[:, 1]
        full = lr(Xdp[fit], y_retry[fit], C=0.5).predict_proba(Xdp)[:, 1]
        act = lr(Xact[fit], y_retry[fit]).predict_proba(Xact)[:, 1]
        self.s["retry-excess"] = logit(full) - logit(act)
        km = MiniBatchKMeans(64, random_state=seed, n_init=3, batch_size=2048).fit(EO[fit])
        cl = km.predict(EO)
        self.cluster = cl
        rate = np.array([y_exit[fit][cl[fit] == c].mean() if (cl[fit] == c).any() else 0 for c in range(64)])
        rex = np.array([self.s["retry-excess"][fit][cl[fit] == c].mean() if (cl[fit] == c).any() else 0
                        for c in range(64)])
        self.s["kmeans-cluster"] = zs(rate[cl], rate[cl][fit]) + zs(rex[cl], rex[cl][fit])
        l3 = logit(self.s["drain+potion→exit"])
        self.s["combined"] = zs(l3, l3[fit]) + zs(self.s["retry-excess"], self.s["retry-excess"][fit])
        self.s["drain+potion→exit | is_error"] = np.maximum(self.s["drain+potion→exit"], a["is_error"])
        c = self.s["combined"]
        cz = (c - c[fit].min()) / (c[fit].max() - c[fit].min())
        self.s["combined | is_error"] = np.maximum(cz, a["is_error"])
        # success: the last attempt of a retry chain, after which the agent moved on
        p_rec = lr(Xdp[fit], y_rec[fit], C=0.5).predict_proba(Xdp)[:, 1]
        self.success = p_rec * (1 - self.s["drain+potion→exit | is_error"])
        self.p_fail = self.s["drain+potion→exit | is_error"]


def hand_coded(a, calls):
    fail, succ = np.zeros(len(calls)), np.zeros(len(calls))
    for k, c in enumerate(calls):
        x = I.act(c)
        f = I.failed(x)
        fail[k] = f
        succ[k] = 2 if x["executed"] else (0 if f else 1)
    return fail, succ


def all_calls():
    return [c for s in load.sessions() for c in s["calls"]]


def boot_auc(y, s, groups, n=1000, seed=7):
    """AUC with a 90% bootstrap that resamples sessions."""
    rng = np.random.default_rng(seed)
    g = np.unique(groups)
    by = {x: np.nonzero(groups == x)[0] for x in g}
    vals = []
    for _ in range(n):
        idx = np.concatenate([by[x] for x in rng.choice(g, len(g))])
        if len(set(y[idx])) < 2:
            continue
        vals.append(roc_auc_score(y[idx], s[idx]))
    return roc_auc_score(y, s), np.percentile(vals, 5), np.percentile(vals, 95)
