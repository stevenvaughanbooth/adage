import numpy as np


def boot(x, groups=None, n=4000, seed=7, q=(5, 95)):
    """Mean and 90% percentile bootstrap CI; resamples groups (e.g. tasks) when given."""
    x = np.asarray(x, float)
    rng = np.random.default_rng(seed)
    if groups is None:
        idx = [np.array([i]) for i in range(len(x))]
    else:
        g = {}
        for i, k in enumerate(groups):
            g.setdefault(k, []).append(i)
        idx = [np.array(v) for v in g.values()]
    m = len(idx)
    bs = []
    for _ in range(n):
        pick = rng.integers(0, m, m)
        sel = np.concatenate([idx[p] for p in pick])
        bs.append(x[sel].mean())
    return float(x.mean()), [float(np.percentile(bs, q[0])), float(np.percentile(bs, q[1]))]


def wilson(k, n, z=1.645):
    if n == 0:
        return (0.0, 0.0, 1.0)
    p = k / n
    d = 1 + z * z / n
    c = (p + z * z / (2 * n)) / d
    h = z * np.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / d
    return (p, c - h, c + h)
