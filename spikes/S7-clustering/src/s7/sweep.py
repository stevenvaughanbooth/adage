"""Parameter sweep on a *development* corpus (10k, seed 1); the chosen parameters are then frozen
and evaluated on separately-seeded test corpora by bench.py. Writes results/sweep.json."""
from __future__ import annotations

import itertools
import json
import time
from pathlib import Path

from s7 import corpus, embed, methods as M, metrics

ROOT = Path(__file__).resolve().parents[2]

GRIDS = {
    "knn_dbscan": dict(eps=[0.55, 0.6, 0.65, 0.7, 0.75], min_pts=[3, 5, 8], min_size=[5, 10]),
    "mknn_cc": dict(eps=[0.55, 0.6, 0.65, 0.7, 0.75], k=[5, 10, 15], min_size=[5, 10]),
    "knn_hdbscan": dict(min_cluster_size=[5, 10, 20, 40], min_samples=[3, 5, 10]),
    "knn_leiden": dict(eps=[0.5, 0.6, 0.65, 0.7], resolution=[0.5, 1.0, 2.0], min_size=[10]),
    "sk_hdbscan": dict(min_cluster_size=[5, 10, 20, 40], min_samples=[None, 5]),
    "lib_hdbscan_pca32": dict(min_cluster_size=[5, 10, 20, 40], min_samples=[None, 5]),
}


def run(name, E, knn, **p):
    if name == "knn_dbscan":
        return M.knn_dbscan(E, knn=knn, **p)
    if name == "mknn_cc":
        return M.mknn_cc(E, knn=knn, **p)
    if name == "knn_hdbscan":
        return M.knn_hdbscan(E, knn=knn, **p)
    if name == "knn_leiden":
        return M.knn_leiden(E, knn=knn, **p)
    if name == "sk_hdbscan":
        return M.sk_hdbscan(E, **p)
    if name == "lib_hdbscan_pca32":
        return M.lib_hdbscan(E, pca=32, **p)
    raise ValueError(name)


def main():
    rows = corpus.scaled(10_000, seed=1)
    E = embed.embed([embed.normalize(r["text"]) for r in rows])
    knn = M.exact_knn(E, 30)
    out = {}
    for name, grid in GRIDS.items():
        res = []
        keys = list(grid)
        for vals in itertools.product(*grid.values()):
            p = dict(zip(keys, vals))
            t = time.time()
            s = metrics.score(rows, run(name, E, knn, **p))
            s["seconds"] = round(time.time() - t, 2)
            res.append({"params": p, **s})
            print(name, p, s, flush=True)
        res.sort(key=lambda r: -r["ari"])
        out[name] = {"best": res[0], "all": res}
    (ROOT / "results").mkdir(exist_ok=True)
    (ROOT / "results" / "sweep.json").write_text(json.dumps(out, indent=1))
    for name, r in out.items():
        print(f"BEST {name}: {r['best']}")


if __name__ == "__main__":
    main()
