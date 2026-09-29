"""What are the learned deterministic stretches? Calibration of the learner's held-out confidence,
the share of model turns that are predictable at each confidence, what those turns are (harness
instrument categories: MEASUREMENT ONLY), and how often predictable turns come in chains.

    python3 stretches.py   # → results/stretches.json
"""
from __future__ import annotations

import json
import re
from collections import Counter, defaultdict

import numpy as np

from data import corpus, records
from learner import Model, authoring, label_window
from verifier import files_of  # noqa: F401  (instrument helpers live there)

GIT = re.compile(r"^git\b")
BINS = (0.0, 0.1, 0.3, 0.5, 0.7, 0.8, 0.9, 1.01)


def category(s, k):
    """HAND-WRITTEN MEASUREMENT INSTRUMENT: what the predicted call is."""
    r, a = records(s)[k], s.acts[k]
    if a.run is not None:
        return "test run"
    if r.tool == "Bash" and GIT.match(r.text):
        return "git"
    if a.env_probe:
        return "env probe/install"
    if r.tool == "Read":
        return "Read"
    if r.tool in ("Grep", "Glob") or (r.tool == "Bash" and r.text.split()[:1] in (["find"], ["grep"], ["ls"])):
        return "search/list"
    return "other " + r.tool


def main():
    c = corpus()
    m = Model(c.train())
    out = {}
    for model in ("haiku", "sonnet"):
        S = c.held_out(model=model)
        rows = []
        reach = 0
        turns = 0
        for s in S:
            R = records(s)
            for t, key, src, cand, p in m.top(s):
                turns += 1
                k = label_window(R, t, key) if key else None
                nxt_in = t + 1 < len(R)
                rows.append((p, k is not None, category(s, k) if k is not None else None, s.sid, t, k))
        # candidate-set recall (is the next call among any candidate?) is the structural ceiling
        P = np.array([r[0] for r in rows])
        Y = np.array([r[1] for r in rows])
        calib = []
        for lo, hi in zip(BINS[:-1], BINS[1:]):
            sel = (P >= lo) & (P < hi)
            calib.append({"bin": [lo, min(hi, 1.0)], "points": int(sel.sum()),
                          "share_of_turns": float(sel.mean()), "hit_rate": float(Y[sel].mean()) if sel.any() else None})
        by_theta = {}
        for th in (0.5, 0.7, 0.8, 0.9):
            sel = P >= th
            cats = Counter(r[2] for r in rows if r[0] >= th and r[1])
            # chains: consecutive decision points that are both hits at ≥ θ
            chain = 0
            hitpts = {(r[3], r[4]) for r in rows if r[0] >= th and r[1]}
            for sid, t in hitpts:
                if (sid, t + 1) in hitpts:
                    chain += 1
            by_theta[th] = {"deliveries_per_session": float(sel.sum() / len(S)),
                            "precision": float(Y[sel].mean()) if sel.any() else None,
                            "hits_per_session": float((sel & Y).sum() / len(S)),
                            "share_of_turns_removed": float((sel & Y).sum() / len(rows)),
                            "hit_categories_per_session": {k: v / len(S) for k, v in cats.most_common()},
                            "chained_hits_per_session": chain / len(S)}
        out[model] = {"decision_points": len(rows), "sessions": len(S), "calibration": calib, "by_theta": by_theta,
                      "any_hit_possible_share": float(Y.mean())}
        print(model, json.dumps(out[model], indent=1)[:3000])
    json.dump(out, open("results/stretches.json", "w"), indent=1)


if __name__ == "__main__":
    main()
