"""Intervals: task-clustered percentile bootstrap (90%) and Wilson (90%)."""

from __future__ import annotations

import math
from collections import defaultdict

import numpy as np


def boot(values, clusters=None, reps: int = 2000, seed: int = 7, level: float = 0.90, stat=np.mean):
    v = np.asarray(values, float)
    if len(v) == 0:
        return (float("nan"), float("nan"), float("nan"))
    rng = np.random.default_rng(seed)
    if clusters is None:
        idx = [np.array([i]) for i in range(len(v))]
    else:
        g = defaultdict(list)
        for i, c in enumerate(clusters):
            g[c].append(i)
        idx = [np.array(x) for x in g.values()]
    k = len(idx)
    out = []
    for _ in range(reps):
        pick = rng.integers(0, k, k)
        sel = np.concatenate([idx[j] for j in pick])
        out.append(stat(v[sel]))
    a = (1 - level) / 2
    return (float(stat(v)), float(np.quantile(out, a)), float(np.quantile(out, 1 - a)))


def ratio_boot(num, den, clusters=None, reps=2000, seed=7, level=0.90):
    """sum(num)/sum(den) with a clustered bootstrap."""
    num, den = np.asarray(num, float), np.asarray(den, float)
    rng = np.random.default_rng(seed)
    g = defaultdict(list)
    for i, c in enumerate(clusters if clusters is not None else range(len(num))):
        g[c].append(i)
    idx = [np.array(x) for x in g.values()]
    k = len(idx)
    out = []
    for _ in range(reps):
        sel = np.concatenate([idx[j] for j in rng.integers(0, k, k)])
        d = den[sel].sum()
        out.append(num[sel].sum() / d if d else 0.0)
    a = (1 - level) / 2
    d = den.sum()
    return (float(num.sum() / d if d else 0.0), float(np.quantile(out, a)), float(np.quantile(out, 1 - a)))


def wilson(k: int, n: int, z: float = 1.645):
    if n == 0:
        return (float("nan"), float("nan"), float("nan"))
    p = k / n
    d = 1 + z * z / n
    c = (p + z * z / (2 * n)) / d
    h = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / d
    return (p, max(0.0, c - h), min(1.0, c + h))
