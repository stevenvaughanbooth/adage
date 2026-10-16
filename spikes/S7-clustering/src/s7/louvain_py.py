"""Reference deterministic Louvain (pure Python, index-order local moving, lowest-id tie-break) —
the algorithm the Rust implementation follows — compared with leidenalg on the dev corpus."""
from __future__ import annotations

import sys
from collections import defaultdict

import numpy as np

from s7 import corpus, embed, methods as M, metrics


def graph(knn, n, k, eps):
    idx, sim = knn
    adj = defaultdict(dict)
    for i in range(n):
        for j, s in zip(idx[i, :k], sim[i, :k]):
            j = int(j)
            if s < eps:
                break
            w = float(s)
            adj[i][j] = max(adj[i].get(j, 0.0), w)
            adj[j][i] = max(adj[j].get(i, 0.0), w)
    return [sorted(adj[i].items()) for i in range(n)]


def louvain(nbrs, resolution=1.0, max_levels=10):
    n = len(nbrs)
    node_members = [[i] for i in range(n)]
    g = nbrs
    for _level in range(max_levels):
        N = len(g)
        deg = [sum(w for _, w in g[i]) + 0.0 for i in range(N)]
        m2 = sum(deg)
        if m2 == 0:
            break
        comm = list(range(N))
        tot = deg[:]
        moved_any = False
        improved = True
        while improved:
            improved = False
            for i in range(N):
                ci = comm[i]
                links = defaultdict(float)
                for j, w in g[i]:
                    if j != i:
                        links[comm[j]] += w
                tot[ci] -= deg[i]
                best_c, best_gain = ci, links.get(ci, 0.0) - resolution * deg[i] * tot[ci] / m2
                for c in sorted(links):
                    gain = links[c] - resolution * deg[i] * tot[c] / m2
                    if gain > best_gain + 1e-12:
                        best_c, best_gain = c, gain
                tot[best_c] += deg[i]
                if best_c != ci:
                    comm[i] = best_c
                    improved = True
                    moved_any = True
        if not moved_any:
            break
        # aggregate
        ids = {}
        for c in comm:
            ids.setdefault(c, len(ids))
        new_members = [[] for _ in ids]
        for i in range(N):
            new_members[ids[comm[i]]].extend(node_members[i])
        agg = [defaultdict(float) for _ in ids]
        for i in range(N):
            for j, w in g[i]:
                agg[ids[comm[i]]][ids[comm[j]]] += w
        g = [sorted(a.items()) for a in agg]
        node_members = new_members
    labels = np.full(n, -1, dtype=np.int64)
    for c, mem in enumerate(node_members):
        labels[mem] = c
    return labels


def run(E, knn, k=15, eps=0.6, resolution=1.0, min_size=10):
    n = len(E)
    lab = louvain(graph(knn, n, k, eps), resolution)
    uniq, counts = np.unique(lab, return_counts=True)
    small = set(uniq[counts < min_size])
    return M._canon(np.array([-1 if l in small else l for l in lab]))


if __name__ == "__main__":
    rows = corpus.scaled(10_000, seed=1)
    E = embed.embed([embed.normalize(r["text"]) for r in rows])
    knn = M.exact_knn(E, 30)
    out = []
    for eps in [0.55, 0.6, 0.65]:
        for res in [1.0, 2.0, 4.0]:
            for k in [10, 15]:
                s = metrics.score(rows, run(E, knn, k=k, eps=eps, resolution=res))
                out.append({"params": {"k": k, "eps": eps, "resolution": res, "min_size": 10}, **s})
                print("louvain", k, eps, res, s, flush=True)
    out.sort(key=lambda r: -r["ari"])
    import json
    from pathlib import Path
    (Path(__file__).resolve().parents[2] / "results" / "sweep_louvain.json").write_text(json.dumps(out, indent=1))
