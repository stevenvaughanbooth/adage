"""Model wrappers with a common `.encode(list[str]) -> np.ndarray` interface,
and a common `.load()` step that is timed separately from encoding so cost
benchmarks can isolate "cold load" from "per-prompt embed".
"""
from __future__ import annotations

import os
import time
from pathlib import Path
from typing import Optional

import numpy as np

ROOT = Path(__file__).resolve().parents[2]
MODELS_DIR = ROOT / "models"

MAX_TOKENS = 128


class BaseEncoder:
    name: str
    dim: int

    def load(self) -> float:
        """Load weights/session. Returns elapsed wall time in seconds."""
        raise NotImplementedError

    def encode(self, texts: list[str]) -> np.ndarray:
        raise NotImplementedError


class StaticEncoder(BaseEncoder):
    """model2vec (potion) static embedding: lookup table + mean pooling."""

    def __init__(self, subdir: str = "potion-base-8m"):
        self.name = f"static:{subdir}"
        self.subdir = subdir
        self.dim = None
        self._model = None

    def load(self) -> float:
        from model2vec import StaticModel

        t0 = time.perf_counter()
        self._model = StaticModel.from_pretrained(
            str(MODELS_DIR / self.subdir), force_download=False
        )
        # Touch the model with a dummy encode so lazy init (if any) counts
        # toward load time, matching "process start -> first embedding
        # ready" semantics from the SPIKE.
        _ = self._model.encode(["warmup"])
        t1 = time.perf_counter()
        self.dim = self._model.encode(["x"]).shape[-1]
        return t1 - t0

    def encode(self, texts: list[str]) -> np.ndarray:
        return np.asarray(self._model.encode(texts))


class OnnxEncoder(BaseEncoder):
    """Generic BERT-style ONNX encoder: tokenizers + onnxruntime, mean
    pooling over the attention mask, L2 normalization.
    """

    def __init__(
        self,
        subdir: str,
        onnx_filename: str,
        label: str,
        intra_op_threads: Optional[int] = None,
    ):
        self.name = label
        self.subdir = subdir
        self.onnx_filename = onnx_filename
        self.intra_op_threads = intra_op_threads
        self.dim = 384
        self._sess = None
        self._tok = None
        self._input_names: set[str] = set()

    def load(self) -> float:
        import onnxruntime as ort
        from tokenizers import Tokenizer

        t0 = time.perf_counter()
        tok_path = MODELS_DIR / self.subdir / "tokenizer.json"
        self._tok = Tokenizer.from_file(str(tok_path))
        self._tok.enable_truncation(max_length=MAX_TOKENS)
        self._tok.enable_padding(length=None)

        so = ort.SessionOptions()
        if self.intra_op_threads is not None:
            so.intra_op_num_threads = self.intra_op_threads
        so.graph_optimization_level = ort.GraphOptimizationLevel.ORT_ENABLE_ALL
        model_path = MODELS_DIR / self.subdir / self.onnx_filename
        self._sess = ort.InferenceSession(
            str(model_path), sess_options=so, providers=["CPUExecutionProvider"]
        )
        self._input_names = {i.name for i in self._sess.get_inputs()}
        _ = self.encode(["warmup"])
        t1 = time.perf_counter()
        return t1 - t0

    def encode(self, texts: list[str]) -> np.ndarray:
        encodings = self._tok.encode_batch(texts)
        max_len = max(len(e.ids) for e in encodings)
        input_ids = np.zeros((len(texts), max_len), dtype=np.int64)
        attn = np.zeros((len(texts), max_len), dtype=np.int64)
        type_ids = np.zeros((len(texts), max_len), dtype=np.int64)
        for i, e in enumerate(encodings):
            n = len(e.ids)
            input_ids[i, :n] = e.ids
            attn[i, :n] = e.attention_mask
            if hasattr(e, "type_ids"):
                type_ids[i, :n] = e.type_ids

        feed = {"input_ids": input_ids, "attention_mask": attn}
        if "token_type_ids" in self._input_names:
            feed["token_type_ids"] = type_ids

        (last_hidden,) = self._sess.run(["last_hidden_state"], feed)
        mask = attn[:, :, None].astype(np.float32)
        summed = (last_hidden * mask).sum(axis=1)
        counts = np.clip(mask.sum(axis=1), 1e-9, None)
        mean_pooled = summed / counts
        norms = np.linalg.norm(mean_pooled, axis=1, keepdims=True)
        norms = np.clip(norms, 1e-9, None)
        return mean_pooled / norms


def make_model(key: str, *, intra_op_threads: Optional[int] = None) -> BaseEncoder:
    if key == "potion-8m":
        return StaticEncoder("potion-base-8m")
    if key == "bge-fp32":
        return OnnxEncoder(
            "bge-small-en-v1.5", "model.onnx", "bge-small-en-v1.5:fp32",
            intra_op_threads=intra_op_threads,
        )
    if key == "bge-int8":
        return OnnxEncoder(
            "bge-small-en-v1.5", "model_int8_dynamic.onnx", "bge-small-en-v1.5:int8-dynamic",
            intra_op_threads=intra_op_threads,
        )
    if key == "minilm-int8":
        return OnnxEncoder(
            "all-MiniLM-L6-v2", "model_qint8_arm64.onnx", "all-MiniLM-L6-v2:int8-arm64",
            intra_op_threads=intra_op_threads,
        )
    raise ValueError(f"unknown model key: {key}")


ALL_MODEL_KEYS = ["potion-8m", "bge-fp32", "bge-int8", "minilm-int8"]
