"""Instrument: next-k *set* prediction on the split (train arm -> held-out control).

At each decision point, rank clusters by a model's next-k score (PPM order 3 over clusters, repo-conditioned,
target "c occurs in the next 5 calls") and report precision@5 / recall@5 against the set of clusters
actually taken in the next 5 calls, plus the same for the "eventually, after a related dead end" target.
These are proxies; the savings they translate to are in results/split-K128.json.
"""

from __future__ import annotations

import json

import numpy as np

from run import prep, OUT
from skip import PPM, Prior, load


def main(K=128):
    S = load()
    tr = [s for s in S if s["arm"] == "train"]
    te = [s for s in S if s["arm"] == "control"]
    sp, W, trd, ted = prep(tr, te, K)
    out = {}
    for name, m, tgt in [("ppm3-next5-repo", PPM(sp.K, 3, "nextk", repo=True), "nextk"),
                         ("prior-next5", Prior(sp.K, "nextk"), "nextk"),
                         ("ppm3-eventual-repo", PPM(sp.K, 3, "y", repo=True), "y")]:
        m.fit(trd)
        hit = tot_p = tot_r = 0
        for s, lab in ted:
            sc = m.score(s)
            top = np.argsort(-sc, 1)[:, :5]
            Y = lab[tgt]
            for r in range(len(sc)):
                h = Y[r, top[r]].sum()
                hit += h
                tot_p += 5
                tot_r += Y[r].sum()
        out[name] = {"precision@5": round(hit / tot_p, 3), "recall@5": round(hit / max(tot_r, 1), 3)}
        print(name, out[name], flush=True)
    (OUT / "nextk.json").write_text(json.dumps(out, indent=1))


if __name__ == "__main__":
    main()
