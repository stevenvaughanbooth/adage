"""Behavioural purity split/merge (Design §8.2.1 step 5): embeddings propose, behaviour decides.

Given clusters and each prompt's *next route* (what its session did next):

1. **Split.** Within a cluster, group members by route (groups of at least ``min_group`` members,
   route != "none"). If there are two or more groups and the text can tell them apart —
   leave-one-out nearest-group-centroid accuracy >= ``sep`` — split the cluster: every member goes
   to its nearest group centroid. If the text cannot tell them apart, do not split (a split the
   runtime could not reproduce is useless); the purity bar then decides.
2. **Merge.** Clusters with the same dominant route whose centroids are similar (cosine >=
   ``merge_tau``) merge, most similar pair first.
3. **Bar.** A family is usable only if its purity (share of members whose route is the dominant
   route) >= ``bar``, the dominant route is a real route, and it has >= ``min_size`` members.

Everything is ordered by cluster id and ties break on the lower id, so the step is deterministic.
"""
from __future__ import annotations

from collections import Counter

import numpy as np

from s7 import methods as M


def _unit(v):
    return v / max(np.linalg.norm(v), 1e-9)


def _separable(E, groups, sep):
    """LOO nearest-centroid accuracy among route groups."""
    sums = {g: E[ix].sum(0) for g, ix in groups.items()}
    cnt = {g: len(ix) for g, ix in groups.items()}
    keys = sorted(groups)
    correct = total = 0
    for g, ix in groups.items():
        for i in ix:
            best, bs = None, -9.0
            for h in keys:
                c = sums[h] - (E[i] if h == g else 0)
                s = float(E[i] @ _unit(c)) if cnt[h] - (h == g) > 0 else -9.0
                if s > bs:
                    best, bs = h, s
            correct += best == g
            total += 1
    return correct / max(1, total)


def split(E, labels, routes, sep=0.85, min_group=10, share=0.15):
    labels = labels.copy()
    nxt = labels.max() + 1
    report = []
    for c in sorted(set(labels.tolist()) - {-1}):
        ix = np.where(labels == c)[0]
        cnt = Counter(routes[i] for i in ix)
        keep = sorted(r for r, n in cnt.items() if r != "none" and n >= max(min_group, share * len(ix)))
        if len(keep) < 2:
            continue
        groups = {r: [i for i in ix if routes[i] == r] for r in keep}
        acc = _separable(E, groups, sep)
        report.append({"cluster": int(c), "groups": {r: len(g) for r, g in groups.items()}, "loo_acc": round(acc, 3)})
        if acc < sep:
            continue
        C = np.stack([_unit(E[g].mean(0)) for g in groups.values()])
        nearest = (E[ix] @ C.T).argmax(1)
        for gi in range(1, len(keep)):
            labels[ix[nearest == gi]] = nxt
            nxt += 1
        report[-1]["split"] = True
    return labels, report


def dominant(labels, routes):
    out = {}
    for c in sorted(set(labels.tolist()) - {-1}):
        cnt = Counter(routes[i] for i in np.where(labels == c)[0])
        r, n = sorted(cnt.items(), key=lambda kv: (-kv[1], kv[0]))[0]
        out[c] = (r, n / sum(cnt.values()), sum(cnt.values()))
    return out


def merge(E, labels, routes, merge_tau=0.8):
    labels = labels.copy()
    while True:
        dom = dominant(labels, routes)
        ids = [c for c in dom if dom[c][0] != "none"]
        best = None
        cents = {c: _unit(E[labels == c].mean(0)) for c in ids}
        for a in range(len(ids)):
            for b in range(a + 1, len(ids)):
                x, y = ids[a], ids[b]
                if dom[x][0] != dom[y][0]:
                    continue
                s = float(cents[x] @ cents[y])
                if s >= merge_tau and (best is None or s > best[0]):
                    best = (s, x, y)
        if best is None:
            return labels
        labels[labels == best[2]] = best[1]


def usable(labels, routes, bar=0.6, min_size=10):
    dom = dominant(labels, routes)
    ok = {c: r for c, (r, p, n) in dom.items() if r != "none" and p >= bar and n >= min_size}
    out = np.array([l if l in ok else -1 for l in labels])
    return out, ok


def pipeline(E, labels, routes, mode="full", bar=0.6, min_size=10, sep=0.85, merge_tau=0.8):
    """mode: 'none' (every cluster, dominant route, no bar), 'bar' (purity bar only),
    'full' (split, merge, bar)."""
    rep = []
    if mode == "none":
        dom = dominant(labels, routes)
        fams = {c: r for c, (r, p, n) in dom.items() if r != "none"}
        return np.array([l if l in fams else -1 for l in labels]), fams, rep
    if mode == "full":
        labels, rep = split(E, labels, routes, sep=sep, min_group=min_size)
        labels = merge(E, labels, routes, merge_tau)
    lab, fams = usable(labels, routes, bar, min_size)
    return lab, fams, rep


def deploy(E_train, fam_labels, fams, E_test, q=0.10, floor=0.5):
    """Runtime matching: nearest usable-family centroid, per-family threshold (q-quantile of its
    training members' similarity). Returns the predicted route per test prompt (or None)."""
    ids = sorted(fams)
    if not ids:
        return [None] * len(E_test)
    C = np.stack([_unit(E_train[fam_labels == c].mean(0)) for c in ids])
    thr = np.array([max(floor, float(np.quantile(E_train[fam_labels == c] @ C[k], q))) for k, c in enumerate(ids)])
    S = E_test @ C.T
    best = S.argmax(1)
    ok = S[np.arange(len(E_test)), best] >= thr[best]
    return [fams[ids[b]] if o else None for b, o in zip(best, ok)]


def behaviour_score(truth_routes, pred_routes, rows):
    t = np.array([x if x is not None else "" for x in truth_routes], dtype=object)
    p = np.array([x if x is not None else "" for x in pred_routes], dtype=object)
    directed = p != ""
    has = t != ""
    correct = directed & (p == t)
    noise = np.array([r["family"] is None for r in rows])
    impure = np.array([r["family"] == "update_dependencies" for r in rows])
    return {
        "directive_precision": round(float(correct.sum() / max(1, directed.sum())), 4),
        "directive_recall": round(float(correct[has].sum() / max(1, has.sum())), 4),
        "noise_directed": round(float(directed[noise].mean()), 4),
        "impure_family_directed": round(float(directed[impure].mean()), 4) if impure.any() else None,
        "wrong_directives": int((directed & (p != t)).sum()),
        "directives": int(directed.sum()),
    }


def evaluate(rows, E, cluster_fn, modes=("none", "bar", "full"), train_share=0.7, seed=0, **kw):
    """Cluster + purify on a training split; deploy to the held-out split; score behaviour."""
    from s7 import corpus
    rng = np.random.default_rng(seed)
    perm = rng.permutation(len(rows))
    cut = int(train_share * len(rows))
    tr, te = np.sort(perm[:cut]), np.sort(perm[cut:])
    labels = cluster_fn(E[tr])
    routes = [rows[i]["route"] for i in tr]
    out = {}
    for mode in modes:
        fam_labels, fams, rep = pipeline(E[tr], labels, routes, mode=mode, **kw)
        pred = deploy(E[tr], fam_labels, fams, E[te])
        truth = [corpus.behaviour_truth(rows[i]["family"]) for i in te]
        out[mode] = {"families": len(fams), **behaviour_score(truth, pred, [rows[i] for i in te])}
        if rep:
            out[mode]["split_report"] = rep
    return out
