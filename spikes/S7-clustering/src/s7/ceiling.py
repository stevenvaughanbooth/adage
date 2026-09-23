"""How much can any clustering method get out of these embeddings? Supervised references:

* nearest-true-centroid top-1 over family rows;
* an *oracle partition*: every row goes to its nearest true centroid if the similarity clears that
  family's threshold (calibrated at 90% recall of its own rows), else noise. No clustering method
  can be expected to beat this; it bounds ARI for this embedding + normalisation.
Writes results/ceiling.json."""
from __future__ import annotations

import json
import sys
from pathlib import Path

import numpy as np

from s7 import corpus, embed, metrics

ROOT = Path(__file__).resolve().parents[2]


def ceiling(rows, E):
    t = metrics.truth_ids(rows)
    F = t.max() + 1
    C = np.stack([E[t == f].mean(0) for f in range(F)])
    C /= np.linalg.norm(C, axis=1, keepdims=True)
    S = E @ C.T
    nearest = S.argmax(1)
    fam = t >= 0
    top1 = float((nearest[fam] == t[fam]).mean())
    thr = np.array([np.quantile(S[t == f, f], 0.10) for f in range(F)])
    best = S[np.arange(len(E)), nearest]
    oracle = np.where(best >= thr[nearest], nearest, -1)
    return {"top1": round(top1, 4), "oracle": metrics.score(rows, oracle)}


def main():
    out = {}
    for name, rows in [("s2_benchmark", corpus.s2_benchmark()), ("scaled_10k", corpus.scaled(10_000))]:
        E = embed.embed([embed.normalize(r["text"]) for r in rows])
        out[name] = ceiling(rows, E)
        print(name, out[name])
    (ROOT / "results" / "ceiling.json").write_text(json.dumps(out, indent=1))


if __name__ == "__main__":
    main()
