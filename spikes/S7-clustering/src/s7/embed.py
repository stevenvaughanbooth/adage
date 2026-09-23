"""Normalise (S2's normaliser, unchanged) and embed with potion-base-8M (static, S2)."""
from __future__ import annotations

import importlib.util
from pathlib import Path

import numpy as np

ROOT = Path(__file__).resolve().parents[2]
S2 = ROOT.parent / "S2-embeddings"
MODEL_DIR = ROOT / "models" / "potion-base-8m"


def _load_s2_normalize():
    spec = importlib.util.spec_from_file_location("s2_normalize", S2 / "src" / "s2" / "normalize.py")
    mod = importlib.util.module_from_spec(spec)
    import sys
    sys.modules["s2_normalize"] = mod
    spec.loader.exec_module(mod)
    return mod


_N = _load_s2_normalize()


def normalize(text: str) -> str:
    return _N.normalize(text).text


_model = None


def model():
    global _model
    if _model is None:
        from model2vec import StaticModel
        _model = StaticModel.from_pretrained(str(MODEL_DIR), force_download=False)
    return _model


def embed(texts: list[str]) -> np.ndarray:
    """Unit-normalised float32 embeddings; unique texts are embedded once."""
    uniq = sorted(set(texts))
    pos = {t: i for i, t in enumerate(uniq)}
    e = np.asarray(model().encode(uniq, batch_size=4096), dtype=np.float32)
    e /= np.clip(np.linalg.norm(e, axis=1, keepdims=True), 1e-9, None)
    return e[[pos[t] for t in texts]]
