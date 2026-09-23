"""The Rust Louvain must reproduce the Python reference exactly (same partition) on the dev corpus."""
from __future__ import annotations

from pathlib import Path

from s7 import corpus, embed, methods as M, metrics, runner
from s7.louvain_py import run as louvain_py

(Path(runner.ROOT) / ".cache").mkdir(exist_ok=True)
rows = corpus.scaled(10_000, seed=1)
E = embed.embed([embed.normalize(r["text"]) for r in rows])
py = louvain_py(E, M.exact_knn(E, 30), k=10, eps=0.6, resolution=2.0, min_size=10)
rs = runner.run("louvain", E, dict(k=10, eps=0.6, res=2.0, min_size=10))
print("rust stats", {k: v for k, v in rs.items() if k != "labels"})
print("python ref", metrics.score(rows, py))
print("rust      ", metrics.score(rows, rs["labels"]))
print("identical partition:", metrics.same_partition(py, rs["labels"]))
db_py = M.knn_dbscan(E, eps=0.75, min_pts=8, min_size=5, k=15)
db_rs = runner.run("knn_dbscan", E, dict(k=15, eps=0.75, min_pts=8, min_size=5))
print("knn_dbscan identical:", metrics.same_partition(db_py, db_rs["labels"]))
