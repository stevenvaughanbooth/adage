"""Method (c): embedding similarity over command (and output) text, clustered.

Two static embedders, both deterministic and CPU-cheap:

- `potion`: potion-base-8M (model2vec: WordPiece tokens of bge-base, mean of 256-d static
  vectors, L2-normalised), the model Adage already ships for intent. Loaded from a local copy
  (`POTION_DIR`); huggingface is not reachable here.
- `hash`: 512-d signed feature hashing (SHA-1 seeded) of word tokens, word bigrams and character
  4-grams: the Track G §1.2 structural half, without the model.

Clustering: average-linkage agglomerative clustering on cosine distance with a threshold 1 − τ
(τ ∈ {0.80, 0.85, 0.90, 0.95}), per repository (Adage learns per repository), over distinct texts.
Held-out texts are assigned to the nearest fitted centroid when its cosine is ≥ τ, else they are
singletons (unseen labels), as `proto.rs` assigns new nodes to a frozen epoch.
"""
from __future__ import annotations

import hashlib
import os
import re
from functools import lru_cache

import numpy as np

POTION_DIR = os.environ.get("POTION_DIR", "/tmp/claude-0/-home-user-adage/d6ae021f-492b-5125-b4ff-8e8a0676a66b/scratchpad/models/yarflam-potion-base-8m-1.0.4/package/models")


@lru_cache(maxsize=1)
def _potion():
    from safetensors.numpy import load_file
    from tokenizers import Tokenizer
    tok = Tokenizer.from_file(os.path.join(POTION_DIR, "tokenizer.json"))
    emb = load_file(os.path.join(POTION_DIR, "model.safetensors"))["embeddings"]
    return tok, emb


def potion(texts: list[str]) -> np.ndarray:
    tok, emb = _potion()
    out = np.zeros((len(texts), emb.shape[1]), dtype=np.float32)
    encs = tok.encode_batch([t[:2000] for t in texts], add_special_tokens=False)
    for i, e in enumerate(encs):
        ids = [x for x in e.ids if x < emb.shape[0]]
        if ids:
            v = emb[ids].mean(axis=0)
            out[i] = v / (np.linalg.norm(v) + 1e-9)
    return out


def _h(s: str) -> tuple[int, float]:
    d = hashlib.sha1(s.encode()).digest()
    return int.from_bytes(d[:4], "little"), (1.0 if d[4] & 1 else -1.0)


def hashed(texts: list[str], dim: int = 512) -> np.ndarray:
    out = np.zeros((len(texts), dim), dtype=np.float32)
    for i, t in enumerate(texts):
        words = re.findall(r"[A-Za-z_][A-Za-z0-9_.\-]*|\S", t[:600])
        feats = ["w:" + w for w in words] + ["b:" + a + " " + b for a, b in zip(words, words[1:])]
        s = re.sub(r"\s+", " ", t[:300])
        feats += ["c:" + s[j:j + 4] for j in range(max(0, len(s) - 3))]
        for f in feats:
            k, sg = _h(f)
            out[i, k % dim] += sg
        n = np.linalg.norm(out[i])
        if n:
            out[i] /= n
    return out


def cluster(X: np.ndarray, tau: float) -> np.ndarray:
    from sklearn.cluster import AgglomerativeClustering
    if len(X) == 1:
        return np.zeros(1, dtype=int)
    X = X.copy()
    X[np.linalg.norm(X, axis=1) == 0, 0] = 1.0  # empty texts form one group
    ac = AgglomerativeClustering(n_clusters=None, metric="cosine", linkage="average", distance_threshold=1 - tau)
    return ac.fit_predict(X)


def centroids(X: np.ndarray, labels: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    ids = np.unique(labels)
    C = np.stack([X[labels == i].mean(axis=0) for i in ids])
    C /= np.linalg.norm(C, axis=1, keepdims=True) + 1e-9
    return ids, C


def assign(Xnew: np.ndarray, ids: np.ndarray, C: np.ndarray, tau: float) -> list[int | None]:
    S = Xnew @ C.T
    best = S.argmax(axis=1)
    return [int(ids[b]) if S[i, b] >= tau else None for i, b in enumerate(best)]
