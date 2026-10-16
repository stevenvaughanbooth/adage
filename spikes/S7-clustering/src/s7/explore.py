"""Sanity check a scaled corpus: uniqueness after normalisation, and kNN similarity by kind."""
from __future__ import annotations

import sys
import time

import numpy as np

from s7 import corpus, embed

n = int(sys.argv[1]) if len(sys.argv) > 1 else 10_000
rows = corpus.scaled(n)
norm = [embed.normalize(r["text"]) for r in rows]
print("rows", len(rows), "unique raw", len({r["text"] for r in rows}), "unique normalised", len(set(norm)))
t = time.time()
E = embed.embed(norm)
print("embed s", round(time.time() - t, 2), E.shape)
S = E @ E.T
np.fill_diagonal(S, -1)
k = 10
kth = -np.sort(-S, axis=1)[:, k - 1]
for kind in ["family", "chatter", "oneoff"]:
    m = np.array([(r["family"] is not None) if kind == "family" else (r["noise_kind"] == kind) for r in rows])
    q = np.percentile(kth[m], [5, 25, 50, 75, 95])
    print(f"{kind:8s} n={m.sum():6d} 10th-NN cos p5/25/50/75/95 = {np.round(q, 3)}")
for r, t in list(zip(rows, norm))[:12]:
    print(r["family"] or r["noise_kind"], "|", t)
