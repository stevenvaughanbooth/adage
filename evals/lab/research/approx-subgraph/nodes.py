"""Learned node labels: embed each call, cluster, assign held-out calls by similarity.

Nothing here knows a tool name, a command or an error string.

1. **Abstraction by recurrence (learned).** A call's text is split into word pieces at
   non-alphanumeric boundaries (delimiters kept). A piece is kept verbatim if it occurs in the
   training calls of at least `min_groups` distinct groups (tasks within a repository; repositories
   when pooling for leave-one-repo-out), otherwise it becomes `·`. So `tests/test_options.py` stays
   within click, and becomes `tests/test_·.py` when the vocabulary must hold across repositories;
   task-specific identifiers and heredoc bodies wash out. This is document frequency, the idea
   Drain and every log-template miner use, learned from the traces.
2. **Embedding.** potion-base-8M (the model Adage ships) of the abstracted text, concatenated with a
   256-d signed feature hash of its first-5-piece n-grams (Track G §1.2's structural half),
   weighted 0.5 / 0.5 and renormalised.
3. **Clustering.** Average-linkage agglomerative clustering of the distinct abstracted training
   texts (weighted by count) at cosine distance 1 − τ.
4. **Label** = cluster id + `!` when the harness reported the call as an error (`is_error`).
   Held-out calls take the nearest centroid if its cosine is ≥ τ, else `?` (unknown).
"""

from __future__ import annotations

import hashlib
import os
import re
import sys
from collections import Counter, defaultdict
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parents[1]))
os.environ.setdefault("ADAGE_LAB_CACHE", "/tmp/adage-research/approx-subgraph/emb")

import embed  # noqa: E402

PIECE = re.compile(r"[A-Za-z0-9_]+|[^A-Za-z0-9_\s]|\s+")
_MODEL = None


def model():
    global _MODEL
    if _MODEL is None:
        _MODEL = embed.Cached("potion-8M", embed.load("potion-8M"))
    return _MODEL


def pieces(text: str) -> list[str]:
    return PIECE.findall(text.split("\n")[0][:160])


def vocab(events_by_group: dict[str, list[str]], min_groups: int = 2) -> set[str]:
    seen = defaultdict(set)
    for g, texts in events_by_group.items():
        for t in texts:
            for p in set(pieces(t)):
                if p.strip() and (p[0].isalnum() or p[0] == "_"):
                    seen[p].add(g)
    return {p for p, gs in seen.items() if len(gs) >= min_groups}


def abstract(text: str, voc: set[str]) -> str:
    out = []
    for p in pieces(text):
        if p.isspace():
            out.append(" ")
        elif (p[0].isalnum() or p[0] == "_") and p not in voc:
            if not out or out[-1] != "·":
                out.append("·")
        else:
            out.append(p)
    return re.sub(r"\s+", " ", "".join(out)).strip()


def _hash_vec(t: str, dim: int = 256) -> np.ndarray:
    v = np.zeros(dim, np.float32)
    toks = [p for p in pieces(t) if not p.isspace()][:12]
    feats = [f"u:{x}" for x in toks] + [f"b:{a}|{b}" for a, b in zip(toks, toks[1:])] + [f"h:{'|'.join(toks[:2])}"]
    for f in feats:
        h = hashlib.sha256(f.encode()).digest()
        i = int.from_bytes(h[:4], "little") % dim
        v[i] += 1.0 if h[4] & 1 else -1.0
    n = np.linalg.norm(v)
    return v / n if n else v


def vectors(texts: list[str]) -> np.ndarray:
    a = model().encode(texts)
    b = np.stack([_hash_vec(t) for t in texts]) if texts else np.zeros((0, 256), np.float32)
    v = np.concatenate([0.7071 * a, 0.7071 * b], 1)
    return v / np.maximum(np.linalg.norm(v, axis=1, keepdims=True), 1e-9)


def _agglomerate(V: np.ndarray, w: np.ndarray, tau: float) -> np.ndarray:
    """Average linkage on cosine distance, cut at 1 - tau (scipy, weighted by duplicating nothing:
    the distinct texts are clustered; weights only set the centroids)."""
    from scipy.cluster.hierarchy import fcluster, linkage
    from scipy.spatial.distance import pdist
    if len(V) == 1:
        return np.array([0])
    D = pdist(V.astype(np.float64), "cosine")
    Z = linkage(D, "average")
    return fcluster(Z, t=1 - tau, criterion="distance") - 1


class Labeler:
    def __init__(self, train_sessions, tau: float, group: str = "task", min_groups: int = 2,
                 cluster_cache: dict | None = None):
        self.tau = tau
        by = defaultdict(list)
        for s in train_sessions:
            by[getattr(s, group)] += [e.text for e in s.events]
        self.voc = vocab(by, min_groups)
        cnt = Counter(abstract(e.text, self.voc) for s in train_sessions for e in s.events)
        self.texts = sorted(cnt)
        w = np.array([cnt[t] for t in self.texts], np.float32)
        V = vectors(self.texts)
        self.members: dict[int, Counter] = defaultdict(Counter)
        self.raw: dict[int, Counter] = defaultdict(Counter)
        lab = _agglomerate(V, w, tau)
        # Deterministic ids: rank clusters by total count desc, then first text.
        order = sorted(set(lab), key=lambda c: (-w[lab == c].sum(), min(t for t, l in zip(self.texts, lab) if l == c)))
        remap = {c: i for i, c in enumerate(order)}
        self.of_text = {t: remap[l] for t, l in zip(self.texts, lab)}
        k = len(order)
        C = np.zeros((k, V.shape[1]), np.float32)
        for t, v, wt in zip(self.texts, V, w):
            C[self.of_text[t]] += wt * v
            self.members[self.of_text[t]][t] += int(wt)
        self.C = C / np.maximum(np.linalg.norm(C, axis=1, keepdims=True), 1e-9)
        self.count = Counter()
        for s in train_sessions:
            for e in s.events:
                c = self.of_text[abstract(e.text, self.voc)]
                self.count[c] += 1
                self.raw[c][e.text.split("\n")[0][:100]] += 1
        self._memo: dict[str, int] = {}

    @property
    def k(self) -> int:
        return len(self.C)

    def cluster_of(self, texts: list[str]) -> list[int]:
        abs_ = [abstract(t, self.voc) for t in texts]
        todo = sorted({a for a in abs_ if a not in self._memo and a not in self.of_text})
        if todo:
            V = vectors(todo)
            sims = V @ self.C.T
            best = sims.argmax(1)
            for a, b, sv in zip(todo, best, sims[np.arange(len(todo)), best]):
                self._memo[a] = int(b) if sv >= self.tau else -1
        return [self.of_text.get(a, self._memo.get(a, -1)) for a in abs_]

    def labels(self, session) -> list[str]:
        cs = self.cluster_of([e.text for e in session.events])
        return [("?" if c < 0 else f"c{c}") + ("!" if e.fail else "") for c, e in zip(cs, session.events)]

    def describe(self, label: str, n: int = 2) -> str:
        if label.startswith("?"):
            return "<unseen>" + ("!" if label.endswith("!") else "")
        c = int(label.strip("c!"))
        top = [t for t, _ in self.raw[c].most_common(n)]
        tag = " FAILED" if label.endswith("!") else ""
        return " | ".join(t[:60] for t in top) + tag
