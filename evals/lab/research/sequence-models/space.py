"""Learned action space: potion-8M embedding of the call's own text + argv-token TF-IDF (SVD),
clustered with k-means. No hand categories: the text is `<tool name> <argument text>` exactly as
the agent issued it (sandbox root stripped).

`Space.fit(train_sessions, K)` fits the token vocabulary, SVD and k-means on training occurrences
only; `assign(texts)` maps any text (including a new repository's) to its nearest centroid.
"""

from __future__ import annotations

import hashlib
import re
import sys
from pathlib import Path

import numpy as np

LAB = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(LAB))
import embed  # noqa: E402

CACHE = Path("/tmp/adage-research/sequence-models")
_MODEL = None
_EMB: dict[str, np.ndarray] = {}


def step_text(st) -> str:
    return f"{st['tool']} {st['arg']}"


def tokens(t: str) -> str:
    return " ".join(x for x in re.split(r"[\s/=:,;'\"()|<>&\[\]{}]+", t) if x)


def potion(texts: list[str]) -> np.ndarray:
    global _MODEL, _EMB
    p = CACHE / "potion8m.npz"
    if not _EMB and p.exists():
        z = np.load(p)
        _EMB = dict(zip(z["keys"].tolist(), z["vecs"]))
    keys = [hashlib.sha1(t.encode()).hexdigest() for t in texts]
    miss = sorted({k: t for k, t in zip(keys, texts) if k not in _EMB}.items())
    if miss:
        if _MODEL is None:
            _MODEL = embed.load("potion-8M")
        V = _MODEL.encode([t for _, t in miss])
        for (k, _), v in zip(miss, V):
            _EMB[k] = v
        np.savez(p, keys=np.array(list(_EMB)), vecs=np.stack(list(_EMB.values())))
    return np.stack([_EMB[k] for k in keys]).astype(np.float32)


class Space:
    def __init__(self, K: int = 128, svd_dim: int = 64, w_tok: float = 1.0, seed: int = 0):
        self.K, self.svd_dim, self.w_tok, self.seed = K, svd_dim, w_tok, seed

    def vec(self, texts: list[str]) -> np.ndarray:
        P = potion(texts)
        T = self.svd.transform(self.tfidf.transform([tokens(t) for t in texts]))
        T = T / np.maximum(np.linalg.norm(T, axis=1, keepdims=True), 1e-9)
        X = np.hstack([P, self.w_tok * T])
        return X / np.maximum(np.linalg.norm(X, axis=1, keepdims=True), 1e-9)

    def fit(self, sessions):
        from collections import Counter
        from sklearn.cluster import KMeans
        from sklearn.decomposition import TruncatedSVD
        from sklearn.feature_extraction.text import TfidfVectorizer
        cnt = Counter(step_text(st) for s in sessions for st in s["steps"])
        texts = list(cnt)
        self.tfidf = TfidfVectorizer(token_pattern=r"\S+", min_df=2, sublinear_tf=True).fit(
            [tokens(t) for t in texts])
        self.svd = TruncatedSVD(min(self.svd_dim, len(self.tfidf.vocabulary_) - 1), random_state=self.seed).fit(
            self.tfidf.transform([tokens(t) for t in texts]))
        X = self.vec(texts)
        w = np.array([cnt[t] for t in texts], float)
        K = min(self.K, len(texts))
        km = KMeans(K, n_init=2, random_state=self.seed).fit(X, sample_weight=w)
        self.C = km.cluster_centers_ / np.maximum(np.linalg.norm(km.cluster_centers_, axis=1, keepdims=True), 1e-9)
        self.K = K
        lab = km.labels_
        # representative per cluster: most frequent text (global) and per repo
        self.members: dict[int, Counter] = {c: Counter() for c in range(K)}
        for t, c in zip(texts, lab):
            self.members[int(c)][t] += cnt[t]
        self._cache = dict(zip(texts, lab.tolist()))
        # intra-cluster tightness (instrument)
        sims = (X * self.C[lab]).sum(1)
        self.tightness = float(np.average(sims, weights=w))
        return self

    def assign(self, texts: list[str]) -> np.ndarray:
        out = np.empty(len(texts), int)
        miss = [i for i, t in enumerate(texts) if t not in self._cache]
        for i, t in enumerate(texts):
            if t in self._cache:
                out[i] = self._cache[t]
        if miss:
            X = self.vec([texts[i] for i in miss])
            lab = (X @ self.C.T).argmax(1)
            for i, c in zip(miss, lab):
                out[i] = c
                self._cache[texts[i]] = int(c)
        return out

    def describe(self, c: int, n: int = 3) -> list[str]:
        return [t for t, _ in self.members[c].most_common(n)]
