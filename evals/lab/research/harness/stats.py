"""Statistics: cluster bootstrap (task-clustered; the Rust replay's exact generator for parity),
Wilson intervals, paired task-level differences.

Sessions of the same task are correlated (the same bug, repeated), so every CI here resamples
*tasks* (clusters) and carries all of a drawn task's sessions with it. The brief asks for 90%
intervals; `level=0.95` reproduces `adage eval replay-savings`.
"""

from __future__ import annotations

import math
import random
from collections import OrderedDict

MASK = (1 << 64) - 1


class SplitMix64:
    """The Rust replay's bootstrap generator (`savings_replay::Rng`)."""

    def __init__(self, seed: int = 0x5EED0ADA9E000001):
        self.x = seed & MASK

    def next(self) -> int:
        self.x = (self.x + 0x9E3779B97F4A7C15) & MASK
        z = self.x
        z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & MASK
        z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & MASK
        return z ^ (z >> 31)

    def below(self, n: int) -> int:
        return self.next() % n


def clustered(values, clusters, level: float = 0.90, draws: int = 2000, seed: int | None = None):
    """(mean over units, [lo, hi]) with a percentile bootstrap resampling clusters.
    `values[i]` belongs to `clusters[i]`. Deterministic (SplitMix64, the Rust seed by default)."""
    vals = list(values)
    if not vals:
        return 0.0, [0.0, 0.0]
    by: "OrderedDict[str, list]" = OrderedDict()
    for v, c in sorted(zip(vals, clusters), key=lambda x: str(x[1])):
        by.setdefault(str(c), []).append(v)
    groups = [(sum(g), len(g)) for g in by.values()]
    rng = SplitMix64() if seed is None else SplitMix64(seed)
    boots = []
    m = len(groups)
    for _ in range(draws):
        s = n = 0
        for _ in range(m):
            gs, gn = groups[rng.below(m)]
            s += gs
            n += gn
        boots.append(s / max(n, 1))
    boots.sort()
    a = (1 - level) / 2
    lo, hi = boots[int(math.floor(a * draws)) - 1 if a * draws >= 1 else 0], boots[int(math.ceil((1 - a) * draws)) - 1]
    return sum(vals) / len(vals), [lo, hi]


def wilson(k: int, n: int, level: float = 0.90):
    """(rate, lo, hi) Wilson score interval."""
    if n == 0:
        return 0.0, 0.0, 0.0
    z = {0.90: 1.6448536269514722, 0.95: 1.959963984540054, 0.80: 1.2815515655446004}.get(level) or _z(level)
    p = k / n
    d = 1 + z * z / n
    c = (p + z * z / (2 * n)) / d
    h = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / d
    return p, max(0.0, c - h), min(1.0, c + h)


def _z(level):
    from statistics import NormalDist
    return NormalDist().inv_cdf(0.5 + level / 2)


def paired_task_diff(a_by_task: dict, b_by_task: dict, level: float = 0.90, draws: int = 5000, seed: int = 7):
    """Mean over tasks present in both of (mean a − mean b), with a percentile bootstrap over tasks
    (as `evals/launch/mechanism.py`). Returns (diff, [lo, hi], n_tasks)."""
    keys = sorted(set(a_by_task) & set(b_by_task))
    diffs = [sum(a_by_task[k]) / len(a_by_task[k]) - sum(b_by_task[k]) / len(b_by_task[k]) for k in keys
             if a_by_task[k] and b_by_task[k]]
    if not diffs:
        return 0.0, [0.0, 0.0], 0
    rng = random.Random(seed)
    boots = sorted(sum(rng.choice(diffs) for _ in diffs) / len(diffs) for _ in range(draws))
    a = (1 - level) / 2
    return sum(diffs) / len(diffs), [boots[int(a * draws)], boots[int((1 - a) * draws) - 1]], len(diffs)


def clustered_ratio(num, den, clusters, level: float = 0.90, draws: int = 2000):
    """(Σnum/Σden, [lo, hi]) with the same task-cluster bootstrap: e.g. $ saved as a share of $ spent."""
    by: "OrderedDict[str, list]" = OrderedDict()
    for a, b, c in sorted(zip(num, den, clusters), key=lambda x: str(x[2])):
        g = by.setdefault(str(c), [0.0, 0.0])
        g[0] += a
        g[1] += b
    groups = list(by.values())
    if not groups:
        return 0.0, [0.0, 0.0]
    rng = SplitMix64()
    m = len(groups)
    boots = []
    for _ in range(draws):
        sa = sb = 0.0
        for _ in range(m):
            g = groups[rng.below(m)]
            sa += g[0]
            sb += g[1]
        boots.append(sa / sb if sb else 0.0)
    boots.sort()
    a = (1 - level) / 2
    tot_b = sum(g[1] for g in groups)
    return (sum(g[0] for g in groups) / tot_b if tot_b else 0.0), [boots[max(0, int(a * draws) - 1)], boots[int((1 - a) * draws) - 1]]
