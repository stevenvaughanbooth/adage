"""Local CPU text embedders, all behind `Embedder.encode(list[str]) -> np.ndarray` (L2-normalised).

Models are never committed. `ADAGE_LAB_MODELS` (default: `$TMPDIR/adage-lab-models`) holds them:

  potion-base-8M     npm @yarflam/potion-base-8m (MIT; byte-identical to the pinned
                     minishlab/potion-base-8M files Adage's runtime verifies)
  potion-base-32M    npm @yarflam/potion-base-32m (MIT)
  bge-small-en-v1.5  qdrant-fastembed GCS mirror, fast-bge-small-en-v1.5.tar.gz (MIT; ONNX)
  all-MiniLM-L6-v2   qdrant-fastembed GCS mirror, sentence-transformers-all-MiniLM-L6-v2.tar.gz
                     (Apache-2.0; ONNX)

`fetch_models.sh` downloads them. `tfidf` (word + char n-gram TF-IDF fitted on the training
situations) needs nothing and is the fallback when no model is present.
"""

from __future__ import annotations

import hashlib
import json
import os
import time
from pathlib import Path

import numpy as np

MODELS = Path(os.environ.get("ADAGE_LAB_MODELS", "/tmp/adage-lab-models"))
CACHE = Path(os.environ.get("ADAGE_LAB_CACHE", "/tmp/adage-lab-cache"))

SPECS = {
    "potion-8M": {"kind": "static", "dir": "yarflam-potion-base-8m-1.0.4/package/models", "licence": "MIT"},
    "potion-32M": {"kind": "static", "dir": "yarflam-potion-base-32m-1.1.5/package/models", "licence": "MIT"},
    "bge-small": {"kind": "onnx", "dir": "fast-bge-small-en-v1.5", "file": "model_optimized.onnx",
                  "pool": "cls", "licence": "MIT"},
    "minilm-L6": {"kind": "onnx", "dir": "fast-all-MiniLM-L6-v2", "file": "model.onnx", "pool": "mean",
                  "licence": "Apache-2.0"},
}


def _norm(x: np.ndarray) -> np.ndarray:
    n = np.linalg.norm(x, axis=1, keepdims=True)
    return x / np.maximum(n, 1e-9)


class Static:
    """model2vec: mean of the token vectors (tokenizer.json + model.safetensors)."""

    def __init__(self, d: Path):
        from safetensors.numpy import load_file
        from tokenizers import Tokenizer
        t = load_file(str(d / "model.safetensors"))
        self.E = t.get("embeddings", next(iter(t.values()))).astype(np.float32)
        self.tok = Tokenizer.from_file(str(d / "tokenizer.json"))
        self.dim = self.E.shape[1]

    def encode(self, texts):
        out = np.zeros((len(texts), self.dim), np.float32)
        for i, enc in enumerate(self.tok.encode_batch(list(texts), add_special_tokens=False)):
            ids = [j for j in enc.ids if j < len(self.E)][:512]
            if ids:
                out[i] = self.E[ids].mean(0)
        return _norm(out)


class Onnx:
    def __init__(self, d: Path, file: str, pool: str, max_len: int = 256):
        import onnxruntime as ort
        from tokenizers import Tokenizer
        so = ort.SessionOptions()
        so.intra_op_num_threads = int(os.environ.get("ADAGE_LAB_THREADS", "4"))
        self.sess = ort.InferenceSession(str(d / file), so, providers=["CPUExecutionProvider"])
        self.tok = Tokenizer.from_file(str(d / "tokenizer.json"))
        self.tok.enable_truncation(max_len)
        self.tok.enable_padding()
        self.pool = pool
        self.inputs = {i.name for i in self.sess.get_inputs()}

    def encode(self, texts, batch: int = 32):
        outs = []
        for s in range(0, len(texts), batch):
            enc = self.tok.encode_batch(list(texts[s:s + batch]))
            ids = np.array([e.ids for e in enc], np.int64)
            mask = np.array([e.attention_mask for e in enc], np.int64)
            feed = {"input_ids": ids, "attention_mask": mask}
            if "token_type_ids" in self.inputs:
                feed["token_type_ids"] = np.zeros_like(ids)
            h = self.sess.run(None, feed)[0]
            if self.pool == "cls":
                v = h[:, 0]
            else:
                m = mask[..., None].astype(np.float32)
                v = (h * m).sum(1) / np.maximum(m.sum(1), 1)
            outs.append(v.astype(np.float32))
        return _norm(np.concatenate(outs)) if outs else np.zeros((0, 1), np.float32)


class Tfidf:
    """Word + char n-gram TF-IDF, fitted on whatever `fit` is given (the training situations)."""

    def __init__(self):
        from sklearn.feature_extraction.text import TfidfVectorizer
        from sklearn.pipeline import make_union
        self.v = make_union(TfidfVectorizer(token_pattern=r"[A-Za-z_][A-Za-z0-9_./-]+", sublinear_tf=True),
                            TfidfVectorizer(analyzer="char_wb", ngram_range=(3, 5), sublinear_tf=True,
                                            max_features=50000))
        self.fitted = False

    def fit(self, texts):
        self.v.fit(texts)
        self.fitted = True

    def encode(self, texts):
        """Sparse, L2-normalised rows (dense would be |texts| x 50k floats)."""
        from sklearn.preprocessing import normalize
        return normalize(self.v.transform(texts).astype(np.float32))


def available() -> list[str]:
    out = []
    for name, sp in SPECS.items():
        d = MODELS / sp["dir"]
        if d.is_dir():
            out.append(name)
    return out


def load(name: str):
    if name == "tfidf":
        return Tfidf()
    sp = SPECS[name]
    d = MODELS / sp["dir"]
    if sp["kind"] == "static":
        return Static(d)
    return Onnx(d, sp["file"], sp["pool"])


def size_mb(name: str) -> float:
    sp = SPECS[name]
    return sum(f.stat().st_size for f in (MODELS / sp["dir"]).rglob("*") if f.is_file()) / 1e6


class Cached:
    """Disk cache of embeddings keyed by (model, sha1(text)); the lab re-embeds the same situations
    across parameter sweeps."""

    def __init__(self, name: str, model):
        self.name, self.model = name, model
        CACHE.mkdir(parents=True, exist_ok=True)
        self.path = CACHE / f"{name}.npz"
        self.mem: dict[str, np.ndarray] = {}
        if self.path.exists():
            z = np.load(self.path)
            self.mem = {k: z[k] for k in z.files}
        self.dirty = False

    def encode(self, texts):
        keys = [hashlib.sha1(t.encode()).hexdigest() for t in texts]
        miss = [i for i, k in enumerate(keys) if k not in self.mem]
        if miss:
            uniq = {}
            for i in miss:
                uniq.setdefault(keys[i], texts[i])
            ks = list(uniq)
            V = self.model.encode([uniq[k] for k in ks])
            for k, v in zip(ks, V):
                self.mem[k] = v
            self.dirty = True
        return np.stack([self.mem[k] for k in keys]) if keys else np.zeros((0, 1), np.float32)

    def save(self):
        if self.dirty:
            np.savez(self.path, **self.mem)
            self.dirty = False


def latency(name: str, texts: list[str], n: int = 200) -> dict:
    """Single-text CPU latency (ms): median and p95 over `n` calls, after warm-up."""
    m = load(name)
    if name == "tfidf":
        m.fit(texts)
    m.encode(texts[:4])
    ts = []
    for t in texts[:n]:
        t0 = time.perf_counter()
        m.encode([t])
        ts.append((time.perf_counter() - t0) * 1000)
    ts.sort()
    return {"median_ms": round(ts[len(ts) // 2], 3), "p95_ms": round(ts[int(len(ts) * 0.95)], 3)}
