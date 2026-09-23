"""Dev-corpus check of the purity step across base clusterings (tunes bar/sep/merge_tau)."""
from __future__ import annotations

import json
import sys

from s7 import corpus, embed, methods as M, purity
from s7.louvain_py import run as louvain_run


def main():
    rows = corpus.scaled(10_000, seed=1)
    E = embed.embed([embed.normalize(r["text"]) for r in rows])
    fns = {
        "louvain": lambda X: louvain_run(X, M.exact_knn(X, 30), k=10, eps=0.6, resolution=2.0, min_size=10),
        "knn_hdbscan": lambda X: M.knn_hdbscan(X, min_cluster_size=10, min_samples=5),
        "knn_dbscan": lambda X: M.knn_dbscan(X, eps=0.75, min_pts=5, min_size=5),
    }
    for name, fn in fns.items():
        for kw in [dict(bar=0.6, sep=0.85, merge_tau=0.8), dict(bar=0.7, sep=0.85, merge_tau=0.8),
                   dict(bar=0.6, sep=0.8, merge_tau=0.75)]:
            r = purity.evaluate(rows, E, fn, **kw)
            for mode, s in r.items():
                s = {k: v for k, v in s.items() if k != "split_report"}
                print(name, kw, mode, json.dumps(s), flush=True)


if __name__ == "__main__":
    main()
