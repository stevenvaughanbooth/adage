"""Retrievers from a task prompt to the repository's files, at the task's base commit.

Every scorer returns a dense vector over `files` (the text files tracked at base). None of them
knows anything about a language, a tool or a file kind: they see paths, contents, git history and
past traces only.

- `bm25`: Okapi BM25 (k1=1.2, b=0.75) over identifier tokens of path + content, per file.
- `bm25c`: the same over 60-line chunks, max-pooled per file.
- `emb:<model>`: static model2vec embeddings (potion-base-8M, potion-base-32M, potion-code-16M-v2)
  of 60-line chunks prefixed by the path; score = max cosine to the prompt over a file's chunks.
- `cochange`: personalised-PageRank-style one-step propagation of a seed ranking over the co-change
  graph of the last 2,000 commits before base (edge weight = commits touching both / sqrt of each
  file's commit count; commits with > 40 files skipped), and `hot`: log(1 + commits touching f).
- `ref`: one-step propagation over the reference graph "f's content mentions g's file stem"
  (a language-agnostic proxy for imports).
- `knn`: past traces: the similarity-weighted vote of the files the k most similar past sessions of
  the same repository (by prompt embedding) edited or opened before their last edit.
"""

from __future__ import annotations

import math
import os
import pickle
import re
from collections import Counter, defaultdict
from functools import lru_cache
from pathlib import Path

import numpy as np
from scipy import sparse

import repo as R

CACHE = Path("/tmp/adage-research/context-prefetch")
MODELS = {
    "p8": Path(os.environ.get("ADAGE_LAB_MODELS", "/tmp/adage-lab-models")) / "yarflam-potion-base-8m-1.0.4/package/models",
    "p32": Path(os.environ.get("ADAGE_LAB_MODELS", "/tmp/adage-lab-models")) / "yarflam-potion-base-32m-1.1.5/package/models",
    "code16": CACHE / "pc16/package/assets",
}
CHUNK_LINES = 60
WORD = re.compile(r"[A-Za-z_][A-Za-z0-9_]*|\d+")
CAMEL = re.compile(r"[A-Z]+(?=[A-Z][a-z])|[A-Z]?[a-z]+|[A-Z]+|\d+")


_tok_cache: dict = {}


def cached_tokens(key, text: str) -> list:
    t = _tok_cache.get(key)
    if t is None:
        t = _tok_cache[key] = tokens(text)
    return t


def tokens(text: str) -> list:
    out = []
    for w in WORD.findall(text):
        lw = w.lower()
        out.append(lw)
        parts = [p.lower() for p in CAMEL.findall(w.replace("_", " ")) if p]
        parts = [p for q in parts for p in q.split()]
        if len(parts) > 1:
            out.extend(parts)
    return out


# ---------------------------------------------------------------- static embedding models
class Static:
    def __init__(self, d: Path):
        from safetensors.numpy import load_file
        from tokenizers import Tokenizer
        t = load_file(str(d / "model.safetensors"))
        self.E = t.get("embeddings", next(iter(t.values()))).astype(np.float32)
        tj = d / "tokenizer.json"
        if not tj.exists():
            tj = d / "tokenizer" / "tokenizer.json"
        self.tok = Tokenizer.from_file(str(tj))
        self.dim = self.E.shape[1]

    def encode(self, texts, max_tokens=512):
        out = np.zeros((len(texts), self.dim), np.float32)
        for i, enc in enumerate(self.tok.encode_batch(list(texts), add_special_tokens=False)):
            ids = [j for j in enc.ids if j < len(self.E)][:max_tokens]
            if ids:
                out[i] = self.E[ids].mean(0)
        n = np.linalg.norm(out, axis=1, keepdims=True)
        return out / np.maximum(n, 1e-9)


@lru_cache(None)
def model(name: str) -> Static:
    return Static(MODELS[name])


def chunks(path: str, text: str) -> list:
    lines = text.splitlines()
    return [f"{path}\n" + "\n".join(lines[i:i + CHUNK_LINES]) for i in range(0, max(len(lines), 1), CHUNK_LINES)]


_emb: dict = {}


def blob_chunks_emb(repo: str, name: str, path: str, oid: str) -> np.ndarray:
    key = (repo, name)
    if key not in _emb:
        f = CACHE / f"emb.{repo}.{name}.pkl"
        _emb[key] = pickle.loads(f.read_bytes()) if f.exists() else {}
    st = _emb[key]
    k = (path, oid)
    if k not in st:
        st[k] = model(name).encode(chunks(path, R.text(repo, oid))).astype(np.float16)
    return st[k]


def save_emb():
    for (repo, name), st in _emb.items():
        (CACHE / f"emb.{repo}.{name}.pkl").write_bytes(pickle.dumps(st))


# ---------------------------------------------------------------- per-base index
class Index:
    def __init__(self, repo: str, base: str):
        self.repo, self.base = repo, base
        self.tree = R.snapshot(repo, base)
        self.files = sorted(self.tree)
        self.pos = {f: i for i, f in enumerate(self.files)}
        self._bm25 = None
        self._bm25c = None
        self._co = None
        self._ref = None

    # --- BM25
    def _build_bm25(self, docs):
        vocab, rows, cols, vals = {}, [], [], []
        lens = []
        for i, d in enumerate(docs):
            c = Counter(d)
            lens.append(len(d))
            for w, n in c.items():
                j = vocab.setdefault(w, len(vocab))
                rows.append(i); cols.append(j); vals.append(n)
        tf = sparse.csr_matrix((vals, (rows, cols)), shape=(len(docs), len(vocab)), dtype=np.float32)
        lens = np.asarray(lens, np.float32)
        avg = max(lens.mean(), 1.0)
        df = np.asarray((tf > 0).sum(0)).ravel()
        idf = np.log(1 + (len(docs) - df + 0.5) / (df + 0.5)).astype(np.float32)
        k1, b = 1.2, 0.75
        tf = tf.tocoo()
        denom = tf.data + k1 * (1 - b + b * lens[tf.row] / avg)
        w = tf.data * (k1 + 1) / denom * idf[tf.col]
        M = sparse.csr_matrix((w, (tf.row, tf.col)), shape=tf.shape)
        return vocab, M

    def bm25(self, q: str) -> np.ndarray:
        if self._bm25 is None:
            docs = [tokens(f) * 3 + cached_tokens(self.tree[f], R.text(self.repo, self.tree[f])) for f in self.files]
            self._bm25 = self._build_bm25(docs)
        return self._score(self._bm25, q)

    def bm25c(self, q: str) -> np.ndarray:
        if self._bm25c is None:
            owner, docs = [], []
            for i, f in enumerate(self.files):
                for j, ch in enumerate(chunks(f, R.text(self.repo, self.tree[f]))):
                    owner.append(i); docs.append(cached_tokens((f, self.tree[f], j), ch))
            self._bm25c = (np.asarray(owner), self._build_bm25(docs))
        owner, idx = self._bm25c
        s = self._score(idx, q)
        out = np.zeros(len(self.files), np.float32)
        np.maximum.at(out, owner, s)
        return out

    @staticmethod
    def _score(idx, q):
        vocab, M = idx
        qv = np.zeros(M.shape[1], np.float32)
        for w in set(tokens(q)):
            j = vocab.get(w)
            if j is not None:
                qv[j] = 1.0
        return np.asarray(M @ qv).ravel()

    # --- embeddings
    def emb(self, name: str, q: str, qv=None) -> np.ndarray:
        if qv is None:
            qv = model(name).encode([q])[0]
        out = np.zeros(len(self.files), np.float32)
        for i, f in enumerate(self.files):
            E = blob_chunks_emb(self.repo, name, f, self.tree[f])
            if len(E):
                out[i] = float((E.astype(np.float32) @ qv).max())
        return out

    # --- git co-change
    def cochange(self):
        if self._co is None:
            n = len(self.files)
            cnt = np.zeros(n, np.float32)
            rows, cols = [], []
            for _, fs in R.history(self.repo, self.base):
                ids = [self.pos[f] for f in fs if f in self.pos]
                for a in ids:
                    cnt[a] += 1
                for a in ids:
                    for b in ids:
                        if a != b:
                            rows.append(a); cols.append(b)
            C = sparse.csr_matrix((np.ones(len(rows), np.float32), (rows, cols)), shape=(n, n))
            C.sum_duplicates()
            d = 1 / np.sqrt(np.maximum(cnt, 1))
            C = sparse.diags(d) @ C @ sparse.diags(d)
            self._co = (C.tocsr(), np.log1p(cnt))
        return self._co

    def ref(self):
        if self._ref is None:
            stems = defaultdict(list)
            for i, f in enumerate(self.files):
                st = f.rsplit("/", 1)[-1].split(".")[0].lower()
                if len(st) >= 4:
                    stems[st].append(i)
            rows, cols = [], []
            for i, f in enumerate(self.files):
                toks = set(cached_tokens(self.tree[f], R.text(self.repo, self.tree[f])))
                for st, js in stems.items():
                    if st in toks:
                        for j in js:
                            if j != i:
                                rows.append(i); cols.append(j)
            n = len(self.files)
            A = sparse.csr_matrix((np.ones(len(rows), np.float32), (rows, cols)), shape=(n, n))
            A = A + A.T
            deg = np.asarray(A.sum(1)).ravel()
            d = 1 / np.sqrt(np.maximum(deg, 1))
            self._ref = (sparse.diags(d) @ A @ sparse.diags(d)).tocsr()
        return self._ref


@lru_cache(2)
def index(repo: str, base: str) -> Index:
    return Index(repo, base)


def norm(x: np.ndarray) -> np.ndarray:
    """Per-query standardisation: z-score, so scores are comparable across tasks."""
    s = x.std()
    return (x - x.mean()) / s if s > 0 else x * 0


def propagate(M, seed: np.ndarray, top: int = 10) -> np.ndarray:
    """One step of propagation from the top-`top` seed files (softmax-weighted)."""
    z = norm(seed)
    idx = np.argsort(-z)[:top]
    w = np.zeros_like(z)
    w[idx] = np.exp(z[idx] - z[idx].max())
    w /= w.sum() or 1
    return np.asarray(M @ w).ravel()


def rank_of(x: np.ndarray) -> np.ndarray:
    r = np.empty(len(x), np.float32)
    r[np.argsort(-x, kind="stable")] = np.arange(len(x))
    return r
