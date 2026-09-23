#!/usr/bin/env python3
"""Cost benchmark for ONE model, run in its own process (so cold-load
timing is not polluted by another model's already-imported libraries).
Prints a single JSON object to stdout. Intended to be invoked via
subprocess by run_cost.py, which repeats this across models/thread counts
and aggregates.

Usage:
    .venv/bin/python src/s2/bench_cost.py --model bge-fp32 --threads 1 \
        --n-latency 300 --emit-embeddings /tmp/emb.npy
"""
from __future__ import annotations

import argparse
import json
import os
import sys
import time

# Thread-count env vars must be set BEFORE numpy/onnxruntime are imported.
parser = argparse.ArgumentParser()
parser.add_argument("--model", required=True)
parser.add_argument("--threads", type=int, default=None)
parser.add_argument("--n-latency", type=int, default=300)
parser.add_argument("--n-determinism", type=int, default=50)
parser.add_argument("--emit-embeddings", default=None)
parser.add_argument("--fixed-prompts", default=None, help="JSON file: list[str]")
args = parser.parse_args()

if args.threads is not None:
    for var in ("OMP_NUM_THREADS", "OPENBLAS_NUM_THREADS", "MKL_NUM_THREADS"):
        os.environ[var] = str(args.threads)

PROC_T0 = time.perf_counter()

from pathlib import Path  # noqa: E402

import numpy as np  # noqa: E402
import psutil  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "src"))

from s2.models import make_model  # noqa: E402
from s2.normalize import normalize  # noqa: E402

proc = psutil.Process()
rss_before_mb = proc.memory_info().rss / 1e6

model = make_model(args.model, intra_op_threads=args.threads)
load_s = model.load()  # excludes interpreter startup, includes import + weight load + first embed
rss_after_load_mb = proc.memory_info().rss / 1e6

# Load a slice of the real benchmark prompts (normalized) for realistic
# latency measurement -- not the training/eval split, just realistic text.
data_path = ROOT / "data" / "prompts.jsonl"
rows = [json.loads(l) for l in open(data_path)]
texts = [normalize(r["text"]).text for r in rows]
n_lat = max(args.n_latency, 1)
rng_texts = (texts * ((n_lat // len(texts)) + 1))[:n_lat]

p50 = p95 = p99 = float("nan")
rss_after_embed_mb = rss_after_load_mb
if args.n_latency >= 1:
    # Warm up (JIT/graph-optimization caches, allocator warmup) -- skipped
    # entirely for a pure cold-start measurement (n_latency == 1 below is
    # used exactly for that: process start -> first embedding ready).
    n_warmup = min(20, max(0, n_lat - 1))
    for t in rng_texts[:n_warmup]:
        model.encode([t])

    lat_s = []
    for t in rng_texts[n_warmup:]:
        t0 = time.perf_counter()
        model.encode([t])
        lat_s.append(time.perf_counter() - t0)
    if not lat_s:  # n_latency == 1: the single call above didn't happen yet
        t0 = time.perf_counter()
        model.encode([rng_texts[0]])
        lat_s.append(time.perf_counter() - t0)

    rss_after_embed_mb = proc.memory_info().rss / 1e6
    lat_ms = np.array(lat_s) * 1000
    p50 = float(np.percentile(lat_ms, 50))
    p95 = float(np.percentile(lat_ms, 95))
    p99 = float(np.percentile(lat_ms, 99))

determinism_max_abs_diff = float("nan")
e1 = None
if args.n_determinism > 0:
    fixed = rng_texts[: args.n_determinism] if len(rng_texts) >= args.n_determinism else rng_texts
    e1 = model.encode(fixed)
    e2 = model.encode(fixed)
    determinism_max_abs_diff = float(np.max(np.abs(e1 - e2)))

if args.emit_embeddings:
    np.save(args.emit_embeddings, e1 if e1 is not None else model.encode(rng_texts[:1]))

proc_wall_s = time.perf_counter() - PROC_T0

result = {
    "model": args.model,
    "threads": args.threads,
    "cold_load_ms": load_s * 1000,
    "script_wall_ms_after_argparse": proc_wall_s * 1000,
    "latency_p50_ms": p50,
    "latency_p95_ms": p95,
    "latency_p99_ms": p99,
    "latency_n": len(rng_texts),
    "rss_before_mb": rss_before_mb,
    "rss_after_load_mb": rss_after_load_mb,
    "rss_after_embed_mb": rss_after_embed_mb,
    "determinism_max_abs_diff_same_config": determinism_max_abs_diff,
}
print(json.dumps(result))
