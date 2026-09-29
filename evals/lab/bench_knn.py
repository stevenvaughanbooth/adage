#!/usr/bin/env python3
"""Single-threaded brute-force kNN cost (cosine = dot on unit vectors, top-10), the hook's bound:

  python3 evals/lab/bench_knn.py

Prints n, dim, dtype, median ms, p95 ms. int16 is how Adage stores quantised vectors
(`adage_intent::quantize`); numpy has no int16 GEMV fast path, so it is widened to int32 here
(Rust would use i16 x i16 -> i32 SIMD and be faster than this)."""
import os
os.environ['OPENBLAS_NUM_THREADS'] = '1'; os.environ['OMP_NUM_THREADS'] = '1'; os.environ['MKL_NUM_THREADS'] = '1'
import numpy as np, time
rng = np.random.default_rng(0)
for n in (300, 1000, 5000, 20000):
    for dim, dt in ((256, np.float32), (512, np.float32), (256, np.int16)):
        X = rng.standard_normal((n, dim)).astype(np.float32)
        if dt == np.int16:
            X = (X * 1000).astype(np.int16).astype(np.int32)
        q = X[0].copy()
        ts = []
        for _ in range(300):
            t1 = time.perf_counter(); s = X @ q; np.argpartition(-s, 10)[:10]; ts.append((time.perf_counter() - t1) * 1000)
        ts.sort()
        print(n, dim, dt.__name__, round(ts[150], 3), round(ts[285], 3))
