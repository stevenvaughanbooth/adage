"""Subprocess entry point for Python clustering methods (see runner.py)."""
from __future__ import annotations

import json
import sys
import time

import numpy as np

from s7 import methods as M


def cluster(method, E, p):
    if method == "sk_hdbscan":
        return M.sk_hdbscan(E, **p)
    if method == "lib_hdbscan_pca32":
        return M.lib_hdbscan(E, pca=32, **p)
    if method == "knn_leiden":
        return M.knn_leiden(E, knn=M.exact_knn(E, p.get("k", 15)), **p)
    if method == "knn_hdbscan":
        return M.knn_hdbscan(E, **p)
    if method == "mknn_cc":
        return M.mknn_cc(E, **p)
    raise ValueError(method)


def main():
    emb, groups, out, method, params = sys.argv[1:6]
    E = np.load(emb)
    G = np.load(groups)
    p = json.loads(params)
    t = time.time()
    labels = np.full(len(E), -1, dtype=np.int64)
    nxt = 0
    for g in sorted(set(G.tolist())):
        ix = np.where(G == g)[0]
        lab = cluster(method, E[ix], p)
        labels[ix] = np.where(lab >= 0, lab + nxt, -1)
        nxt += int(lab.max()) + 1 if (lab >= 0).any() else 0
    open(out, "wb").write(labels.astype("<i4").tobytes())
    print(json.dumps({"cluster_ms": round((time.time() - t) * 1e3, 1), "clusters": nxt}))


if __name__ == "__main__":
    main()
