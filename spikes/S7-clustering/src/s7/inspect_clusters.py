"""Print what each cluster contains (debugging aid)."""
from __future__ import annotations

import sys
from collections import Counter

from s7 import corpus, embed, methods as M

rows = corpus.scaled(10_000, seed=1)
norm = [embed.normalize(r["text"]) for r in rows]
E = embed.embed(norm)
knn = M.exact_knn(E, 30)
method = sys.argv[1] if len(sys.argv) > 1 else "leiden"
if method == "leiden":
    lab = M.knn_leiden(E, knn=knn, eps=0.6, resolution=2.0, min_size=10)
elif method == "hdb":
    lab = M.knn_hdbscan(E, knn=knn, min_cluster_size=10, min_samples=5)
else:
    lab = M.knn_dbscan(E, knn=knn, eps=0.75, min_pts=5, min_size=5)
for c in sorted(set(lab.tolist()) - {-1}):
    idx = [i for i in range(len(rows)) if lab[i] == c]
    cnt = Counter(rows[i]["family"] or rows[i]["noise_kind"] for i in idx)
    ex = [norm[i] for i in idx[:3]]
    print(c, len(idx), cnt.most_common(4), ex)
fam = Counter(rows[i]["family"] for i in range(len(rows)) if lab[i] == -1 and rows[i]["family"])
print("family rows left as noise:", fam.most_common())
