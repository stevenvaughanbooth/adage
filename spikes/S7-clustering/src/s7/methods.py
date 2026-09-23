"""Candidate clustering methods. Every method takes unit-normalised embeddings (n x d, float32) and
returns int labels, -1 = noise.

Rust-portable candidates (exact kNN + simple graph algorithms, index tie-breaks):
  * ``knn_dbscan``  — density: a point is *core* when its (min_pts)-th nearest neighbour has cosine
                      >= eps; clusters are connected components of core points joined by kNN edges
                      with cosine >= eps; a border point joins its most similar core neighbour.
  * ``mknn_cc``     — mutual-kNN edges with cosine >= eps, connected components, min size.
  * ``knn_hdbscan`` — HDBSCAN on the kNN graph (mutual reachability over kNN edges, Kruskal MST
                      with index tie-breaks, condensed tree, excess-of-mass selection).
Python-library candidates: sklearn/hdbscan HDBSCAN (raw, PCA), kNN + Leiden.
"""
from __future__ import annotations

import numpy as np


# ------------------------------------------------------------------ exact kNN (deterministic)

def exact_knn(E: np.ndarray, k: int, block: int = 1024) -> tuple[np.ndarray, np.ndarray]:
    """Top-k neighbours by cosine, self excluded. Ties broken by lower index (stable order)."""
    n = E.shape[0]
    k = min(k, n - 1)
    idx = np.empty((n, k), dtype=np.int64)
    sim = np.empty((n, k), dtype=np.float32)
    extra = min(n - 1, k + 8)
    for s in range(0, n, block):
        e = min(n, s + block)
        S = E[s:e] @ E.T
        S[np.arange(e - s), np.arange(s, e)] = -np.inf
        cand = np.argpartition(-S, extra - 1, axis=1)[:, :extra]
        cs = np.take_along_axis(S, cand, axis=1)
        # sort candidates by (-sim, index)
        order = np.lexsort((cand, -cs), axis=1)
        cand = np.take_along_axis(cand, order, axis=1)[:, :k]
        cs = np.take_along_axis(cs, order, axis=1)[:, :k]
        idx[s:e] = cand
        sim[s:e] = cs
    return idx, sim


class UF:
    def __init__(self, n):
        self.p = np.arange(n)

    def find(self, x):
        p = self.p
        r = x
        while p[r] != r:
            r = p[r]
        while p[x] != r:
            p[x], x = r, p[x]
        return r

    def union(self, a, b):
        ra, rb = self.find(a), self.find(b)
        if ra == rb:
            return
        # smaller root index wins: deterministic
        if ra < rb:
            self.p[rb] = ra
        else:
            self.p[ra] = rb


def _components(n, edges, members_mask=None):
    uf = UF(n)
    for a, b in edges:
        uf.union(a, b)
    roots = np.array([uf.find(i) for i in range(n)])
    return roots


def _relabel(roots: np.ndarray, keep: np.ndarray, min_size: int) -> np.ndarray:
    labels = np.full(len(roots), -1, dtype=np.int64)
    r = roots[keep]
    uniq, counts = np.unique(r, return_counts=True)
    big = {u for u, c in zip(uniq, counts) if c >= min_size}
    ids = {}
    for i in np.where(keep)[0]:
        if roots[i] in big:
            labels[i] = ids.setdefault(roots[i], len(ids))
    return labels


def knn_dbscan(E, eps=0.7, min_pts=5, k=15, knn=None, min_size=5):
    idx, sim = knn if knn is not None else exact_knn(E, k)
    n = E.shape[0]
    core = sim[:, min_pts - 1] >= eps
    edges = [(i, int(j)) for i in np.where(core)[0] for j, s in zip(idx[i], sim[i]) if s >= eps and core[j]]
    roots = _components(n, edges)
    labels = _relabel(roots, core, 1)
    # border points: most similar core neighbour within eps (list is sorted by -sim, index)
    for i in np.where(~core)[0]:
        for j, s in zip(idx[i], sim[i]):
            if s < eps:
                break
            if core[j]:
                labels[i] = labels[j]
                break
    # small clusters -> noise
    uniq, counts = np.unique(labels[labels >= 0], return_counts=True)
    small = set(uniq[counts < min_size])
    labels = np.array([-1 if l in small else l for l in labels])
    return _canon(labels)


def mknn_cc(E, eps=0.7, k=10, min_size=5, knn=None):
    idx, sim = knn if knn is not None else exact_knn(E, k)
    idx, sim = idx[:, :k], sim[:, :k]
    n = E.shape[0]
    nbr = [set(row.tolist()) for row in idx]
    edges = [(i, int(j)) for i in range(n) for j, s in zip(idx[i], sim[i]) if s >= eps and i in nbr[j]]
    roots = _components(n, edges)
    return _canon(_relabel(roots, np.ones(n, bool), min_size))


def _canon(labels: np.ndarray) -> np.ndarray:
    """Relabel clusters by their smallest member index (noise stays -1)."""
    out = np.full(len(labels), -1, dtype=np.int64)
    ids = {}
    for i, l in enumerate(labels):
        if l >= 0:
            out[i] = ids.setdefault(l, len(ids))
    return out


# ------------------------------------------------------------------ HDBSCAN on the kNN graph

def knn_hdbscan(E, min_cluster_size=10, min_samples=5, k=15, knn=None):
    """HDBSCAN restricted to kNN edges. Distances are 1 - cosine. Deterministic: edges sorted by
    (weight, a, b); union-find with index tie-breaks; the condensed tree and EOM selection are
    pure functions of the MST."""
    idx, sim = knn if knn is not None else exact_knn(E, k)
    n = E.shape[0]
    d = 1.0 - sim.astype(np.float64)
    core = d[:, min_samples - 1]
    a = np.repeat(np.arange(n), idx.shape[1])
    b = idx.reshape(-1)
    w = np.maximum(np.maximum(core[a], core[b]), d.reshape(-1))
    lo, hi = np.minimum(a, b), np.maximum(a, b)
    order = np.lexsort((hi, lo, w))
    uf = UF(n)
    mst = []
    for e in order:
        x, y = lo[e], hi[e]
        rx, ry = uf.find(x), uf.find(y)
        if rx != ry:
            uf.union(rx, ry)
            mst.append((x, y, w[e]))
    return _canon(_hdbscan_from_mst(n, mst, min_cluster_size))


def _hdbscan_from_mst(n, mst, mcs):
    """Single-linkage dendrogram from MST edges (ascending), condensed tree, EOM selection.
    With a kNN graph the MST may be a forest; each component then acts as a child of a virtual
    root (as it would in the full MST) and is itself selectable."""
    kids, dist, size = {}, {}, [1] * n
    uf = list(range(n))

    def find(x):
        while uf[x] != x:
            uf[x] = uf[uf[x]]
            x = uf[x]
        return x

    for x, y, w in mst:
        rx, ry = find(x), find(y)
        v = len(size)
        kids[v] = (rx, ry)
        dist[v] = max(float(w), 1e-4)
        size.append(size[rx] + size[ry])
        uf.append(v)
        uf[rx] = uf[ry] = v
    roots = sorted({find(i) for i in range(n)})

    def leaves(node):
        out, st = [], [node]
        while st:
            v = st.pop()
            if v < n:
                out.append(v)
            else:
                st.extend(kids[v])
        return out

    birth, parent, stab, children = [], [], [], []
    point_cluster = np.full(n, -1, dtype=np.int64)

    def new_cluster(par, lam):
        birth.append(lam); parent.append(par); stab.append(0.0); children.append([])
        if par >= 0:
            children[par].append(len(birth) - 1)
        return len(birth) - 1

    top = [r for r in roots if size[r] >= mcs]
    for root in top:
        c0 = new_cluster(-1, 0.0)
        stack = [(root, c0)]
        while stack:
            node, c = stack.pop()
            if node < n:
                point_cluster[node] = c
                continue
            lam = 1.0 / dist[node]
            l, r = kids[node]
            if size[l] >= mcs and size[r] >= mcs:
                for ch in (l, r):
                    stab[c] += size[ch] * (lam - birth[c])
                    stack.append((ch, new_cluster(c, lam)))
            else:
                for ch in (l, r):
                    if size[ch] >= mcs:
                        stack.append((ch, c))
                    else:
                        for p in leaves(ch):
                            point_cluster[p] = c
                            stab[c] += lam - birth[c]
    C = len(birth)
    best, sel = [0.0] * C, [False] * C
    for c in reversed(range(C)):
        ks = sum(best[k] for k in children[c])
        if children[c] and ks > stab[c]:
            best[c], sel[c] = ks, False
        else:
            best[c], sel[c] = stab[c], True
    single_root = len(top) == 1
    chosen = [False] * C
    st = [c for c in range(C) if parent[c] == -1]
    while st:
        c = st.pop()
        root_blocked = parent[c] == -1 and single_root and children[c]
        if sel[c] and not root_blocked:
            chosen[c] = True
        else:
            st.extend(children[c])
    labels = np.full(n, -1, dtype=np.int64)
    for p in range(n):
        c = point_cluster[p]
        while c >= 0 and not chosen[c]:
            c = parent[c]
        if c >= 0:
            labels[p] = c
    return labels


# ------------------------------------------------------------------ library methods

def sk_hdbscan(E, min_cluster_size=10, min_samples=None):
    from sklearn.cluster import HDBSCAN
    return _canon(HDBSCAN(min_cluster_size=min_cluster_size, min_samples=min_samples, metric="euclidean").fit_predict(E))


def lib_hdbscan(E, min_cluster_size=10, min_samples=None, pca=None):
    import hdbscan
    X = E
    if pca:
        from sklearn.decomposition import PCA
        X = PCA(n_components=pca, svd_solver="full").fit_transform(E).astype(np.float64)
    c = hdbscan.HDBSCAN(min_cluster_size=min_cluster_size, min_samples=min_samples, core_dist_n_jobs=1,
                        algorithm="boruvka_kdtree" if pca else "best")
    return _canon(c.fit_predict(X))


def knn_leiden(E, k=15, eps=0.6, resolution=1.0, min_size=10, seed=0, knn=None):
    import igraph as ig
    import leidenalg as la
    idx, sim = knn if knn is not None else exact_knn(E, k)
    n = E.shape[0]
    edges, weights = [], []
    seen = set()
    for i in range(n):
        for j, s in zip(idx[i, :k], sim[i, :k]):
            j = int(j)
            if s < eps:
                break
            key = (min(i, j), max(i, j))
            if key not in seen:
                seen.add(key)
                edges.append(key)
                weights.append(float(s))
    g = ig.Graph(n=n, edges=edges)
    g.es["weight"] = weights
    part = la.find_partition(g, la.RBConfigurationVertexPartition, weights="weight",
                             resolution_parameter=resolution, seed=seed)
    labels = np.array(part.membership)
    uniq, counts = np.unique(labels, return_counts=True)
    small = set(uniq[counts < min_size])
    return _canon(np.array([-1 if l in small else l for l in labels]))


# ------------------------------------------------------------------ post-processing (Rust-portable)

def centroids(E, labels):
    ids = sorted(set(labels.tolist()) - {-1})
    C = np.stack([E[labels == c].mean(0) for c in ids]) if ids else np.zeros((0, E.shape[1]), E.dtype)
    C /= np.clip(np.linalg.norm(C, axis=1, keepdims=True), 1e-9, None)
    return ids, C


def merge_centroids(E, labels, tau=0.85):
    """Centroid linkage: repeatedly merge the most similar pair of clusters while their centroid
    cosine >= tau. Ties by lower index. Deterministic; O(C^2) per merge, C = clusters."""
    labels = labels.copy()
    ids, _ = centroids(E, labels)
    sums = {c: E[labels == c].sum(0).astype(np.float64) for c in ids}
    live = list(ids)
    while len(live) > 1:
        Mx = np.stack([sums[c] / np.linalg.norm(sums[c]) for c in live])
        S = Mx @ Mx.T
        np.fill_diagonal(S, -np.inf)
        i, j = np.unravel_index(np.argmax(S), S.shape)
        if S[i, j] < tau:
            break
        a, b = sorted((live[i], live[j]))
        labels[labels == b] = a
        sums[a] = sums[a] + sums[b]
        live.remove(b)
    return _canon(labels)


def centroid_assign(E, labels, q=0.10, floor=0.5):
    """As deployed: every prompt goes to the most similar family centroid if it clears that
    family's threshold (the q-quantile of its members' similarity, never below floor)."""
    ids, C = centroids(E, labels)
    if not ids:
        return labels.copy()
    S = E @ C.T
    thr = np.array([max(floor, float(np.quantile(S[labels == c, k], q))) for k, c in enumerate(ids)])
    best = S.argmax(1)
    ok = S[np.arange(len(E)), best] >= thr[best]
    return _canon(np.where(ok, np.array(ids)[best], -1))
