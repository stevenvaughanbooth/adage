"""Second sweep (dev corpus): fine clusters -> centroid merge -> centroid assignment ("as
deployed": what the runtime matcher would do with the families). Writes results/sweep2.json."""
from __future__ import annotations

import itertools
import json
from pathlib import Path

from s7 import corpus, embed, methods as M, metrics

ROOT = Path(__file__).resolve().parents[2]


def main():
    rows = corpus.scaled(10_000, seed=1)
    E = embed.embed([embed.normalize(r["text"]) for r in rows])
    knn = M.exact_knn(E, 30)
    bases = {
        "knn_dbscan(eps=.7,mp=5)": M.knn_dbscan(E, knn=knn, eps=0.7, min_pts=5, min_size=5),
        "knn_dbscan(eps=.75,mp=5)": M.knn_dbscan(E, knn=knn, eps=0.75, min_pts=5, min_size=5),
        "knn_dbscan(eps=.8,mp=5)": M.knn_dbscan(E, knn=knn, eps=0.8, min_pts=5, min_size=5),
        "mknn_cc(eps=.75,k=10)": M.mknn_cc(E, knn=knn, eps=0.75, k=10, min_size=5),
        "mknn_cc(eps=.8,k=10)": M.mknn_cc(E, knn=knn, eps=0.8, k=10, min_size=5),
        "knn_hdbscan(mcs=10,ms=5)": M.knn_hdbscan(E, knn=knn, min_cluster_size=10, min_samples=5),
        "knn_hdbscan(mcs=5,ms=3)": M.knn_hdbscan(E, knn=knn, min_cluster_size=5, min_samples=3),
        "knn_leiden(eps=.6,res=2)": M.knn_leiden(E, knn=knn, eps=0.6, resolution=2.0, min_size=10),
        "knn_leiden(eps=.65,res=4)": M.knn_leiden(E, knn=knn, eps=0.65, resolution=4.0, min_size=10),
    }
    out = []
    for name, lab in bases.items():
        for tau, q in itertools.product([None, 0.7, 0.75, 0.8, 0.85, 0.9], [None, 0.05, 0.1, 0.2]):
            l2 = lab if tau is None else M.merge_centroids(E, lab, tau)
            l3 = l2 if q is None else M.centroid_assign(E, l2, q=q)
            out.append({"base": name, "tau": tau, "q": q, **metrics.score(rows, l3)})
    out.sort(key=lambda r: -r["ari"])
    for r in out[:30]:
        print(r)
    (ROOT / "results" / "sweep2.json").write_text(json.dumps(out, indent=1))


if __name__ == "__main__":
    main()
