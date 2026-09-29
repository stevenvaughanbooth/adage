"""Small statistics helpers: Wilson and Newcombe intervals, Mantel-Haenszel risk difference with a
task-cluster bootstrap, and two-proportion power."""

from __future__ import annotations

import math
from collections import defaultdict

import numpy as np
from scipy.stats import norm


def wilson(k: int, n: int, conf: float = 0.90) -> tuple[float, float]:
    if n == 0:
        return (float("nan"), float("nan"))
    z = norm.ppf(0.5 + conf / 2)
    p = k / n
    d = 1 + z * z / n
    c = (p + z * z / (2 * n)) / d
    h = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / d
    return (c - h, c + h)


def newcombe(k1, n1, k0, n0, conf=0.90) -> tuple[float, float, float]:
    """Difference p1 - p0 with Newcombe's hybrid-score interval (method 10)."""
    p1, p0 = k1 / n1, k0 / n0
    l1, u1 = wilson(k1, n1, conf)
    l0, u0 = wilson(k0, n0, conf)
    d = p1 - p0
    lo = d - math.sqrt((p1 - l1) ** 2 + (u0 - p0) ** 2)
    hi = d + math.sqrt((u1 - p1) ** 2 + (p0 - l0) ** 2)
    return d, lo, hi


def mh_rd(rows) -> tuple[float, float]:
    """rows: iterable of (stratum, x in {0,1}, y in {0,1}). Mantel-Haenszel risk difference
    (x=1 minus x=0) and its total weight."""
    g = defaultdict(lambda: [0, 0, 0, 0])  # n1, y1, n0, y0
    for s, x, y in rows:
        a = g[s]
        if x:
            a[0] += 1; a[1] += y
        else:
            a[2] += 1; a[3] += y
    num = den = 0.0
    for n1, y1, n0, y0 in g.values():
        if n1 and n0:
            w = n1 * n0 / (n1 + n0)
            num += w * (y1 / n1 - y0 / n0)
            den += w
    return (num / den if den else float("nan")), den


def boot_mh(rows, cluster_of, B=2000, conf=0.90, seed=0):
    """Point estimate and percentile CI, resampling clusters (tasks) with replacement."""
    rows = list(rows)
    est, w = mh_rd(rows)
    by = defaultdict(list)
    for r in rows:
        by[cluster_of(r)].append(r)
    keys = list(by)
    rng = np.random.default_rng(seed)
    bs = []
    for _ in range(B):
        pick = rng.integers(0, len(keys), len(keys))
        rr = []
        for j, i in enumerate(pick):
            rr += [((j, r[0]), r[1], r[2]) for r in by[keys[i]]]
        e, _ = mh_rd(rr)
        if not math.isnan(e):
            bs.append(e)
    a = (1 - conf) / 2
    lo, hi = (np.quantile(bs, [a, 1 - a]) if bs else (float("nan"), float("nan")))
    return est, float(lo), float(hi), w


def n_per_arm(p0: float, delta: float, alpha=0.05, power=0.80) -> int:
    """Two-sided two-proportion test, sessions per arm."""
    p1 = min(max(p0 + delta, 1e-6), 1 - 1e-6)
    z = norm.ppf(1 - alpha / 2) + norm.ppf(power)
    return math.ceil(z * z * (p0 * (1 - p0) + p1 * (1 - p1)) / (delta * delta))


def n_per_arm_stratified(sigma2_within: float, delta: float, alpha=0.05, power=0.80) -> int:
    """Task-stratified design (every task run in both arms): the task effect cancels, leaving the
    within-task Bernoulli variance."""
    z = norm.ppf(1 - alpha / 2) + norm.ppf(power)
    return math.ceil(z * z * 2 * sigma2_within / (delta * delta))
