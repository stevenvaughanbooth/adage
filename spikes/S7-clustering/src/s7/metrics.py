"""Clustering metrics against intent labels. Noise (true or predicted) is scored as singletons, so
lumping all noise into one bucket earns nothing."""
from __future__ import annotations

import numpy as np
from sklearn.metrics import adjusted_rand_score


def _with_singletons(labels, offset):
    out = np.asarray(labels, dtype=np.int64).copy()
    noise = out < 0
    out[noise] = offset + np.arange(noise.sum())
    return out


def truth_ids(rows, key="family"):
    names = sorted({r[key] for r in rows if r[key] is not None})
    m = {f: i for i, f in enumerate(names)}
    return np.array([m[r[key]] if r[key] is not None else -1 for r in rows])


def score(rows, pred) -> dict:
    t = truth_ids(rows)
    pred = np.asarray(pred)
    n = len(pred)
    big = 10_000_000
    ari = adjusted_rand_score(_with_singletons(t, big), _with_singletons(pred, 2 * big))
    fam = t >= 0
    ari_fam = adjusted_rand_score(t[fam], _with_singletons(pred[fam], 2 * big))
    clustered = pred >= 0
    # purity over predicted clusters (noise truth counts as its own label)
    pur_num = 0
    for c in np.unique(pred[clustered]):
        _, counts = np.unique(t[pred == c], return_counts=True)
        pur_num += counts.max()
    purity = pur_num / max(1, clustered.sum())
    kinds = np.array([r.get("noise_kind") or "family" for r in rows])
    out = {
        "ari": round(float(ari), 4),
        "ari_families": round(float(ari_fam), 4),
        "purity": round(float(purity), 4),
        "coverage": round(float(clustered[fam].mean()), 4),
        "noise_recall": round(float((~clustered[~fam]).mean()) if (~fam).any() else 1.0, 4),
        "clusters": int(len(np.unique(pred[clustered]))),
        "n": n,
    }
    for k in ("oneoff", "chatter"):
        m = kinds == k
        if m.any():
            out[f"noise_recall_{k}"] = round(float((~clustered[m]).mean()), 4)
    return out


def canon(labels) -> np.ndarray:
    out = np.full(len(labels), -1, dtype=np.int64)
    ids = {}
    for i, l in enumerate(labels):
        if l >= 0:
            out[i] = ids.setdefault(int(l), len(ids))
    return out


def same_partition(a, b) -> bool:
    return bool(np.array_equal(canon(a), canon(b)))
