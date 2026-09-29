"""Early-stop ceiling vs a realistic doom predictor.

48% (Haiku) and 72% (Sonnet) of control $ is spent in sessions that fail the hidden tests, and
failed sessions run +14 calls longer. The oracle that stops every failing session at the passing
sessions' median length saves 22% / 38% of $ with no pass-rate loss. Is doom predictable online?

At a checkpoint m (call index), features use only what happened so far (no hindsight): calls, context
tokens, edits, failed shell calls (learned failure detector), test runs (hand-coded role
instrument, declared), explore calls, calls since the last edit, failures in the last 10 calls.
Logistic regression, trained leave-one-repo-out over held-out control sessions (Haiku v1+v2; the
train split is too small and nearly all-pass). Policy: at m, stop if P(fail) > theta. Cost: $
after m for stopped sessions. Harm: stopped sessions that would have passed (pass-rate points lost).
"""

from __future__ import annotations

import json
import pickle
import sys
from pathlib import Path

import numpy as np
from sklearn.linear_model import LogisticRegression
from sklearn.metrics import roc_auc_score

sys.path.insert(0, str(Path(__file__).resolve().parent))
from ceilings import reprice  # noqa: E402

CACHE = Path("/tmp/adage-research/waste-anatomy")
HERE = Path(__file__).resolve().parent
RNG = np.random.default_rng(3)


def feats(s, m):
    cs = s["calls"][:m]
    k = cs[-1]["k"] if cs else 0
    edits = [c for c in cs if c["role"] == "edit" and not c["err"]]
    last_edit = max([c["idx"] for c in edits], default=-1)
    return [len(cs), s["C"][min(k, len(s["C"]) - 1)] / 1e4, len(edits), sum(c["failed"] for c in cs),
            sum(c["role"] == "test" for c in cs), sum(c["role"] == "test" and c["failed"] for c in cs),
            sum(c["role"] == "explore" for c in cs), m - 1 - last_edit, sum(c["failed"] for c in cs[-10:]),
            len({c["file"] for c in edits})]


def main():
    S = pickle.load((CACHE / "calls.pkl").open("rb"))
    out = {}
    for m in (15, 20, 25):
        H = [s for s in S if s["arm"] == "control" and s["fam"] == "haiku" and s["cap"] == 60]
        # only sessions still running at m can be stopped
        live = [s for s in H if len(s["calls"]) > m]
        X = np.array([feats(s, m) for s in live])
        y = np.array([0 if s["passed"] else 1 for s in live])
        p = np.zeros(len(live))
        for repo in sorted({s["repo"] for s in live}):
            tr = np.array([s["repo"] != repo for s in live])
            if len(set(y[tr])) < 2:
                continue
            clf = LogisticRegression(C=1.0, max_iter=2000, class_weight="balanced").fit(X[tr], y[tr])
            p[~tr] = clf.predict_proba(X[~tr])[:, 1]
        auc = roc_auc_score(y, p)
        base = np.array([reprice(s)[2] for s in H])
        rows = []
        for th in (0.5, 0.6, 0.7, 0.8, 0.9):
            saved, lost = [], []
            for s, pi in zip(live, p):
                stop = pi > th
                saved.append(base[H.index(s)] - reprice(s, remove=frozenset(c["idx"] for c in s["calls"] if c["idx"] >= m))[2]
                             if stop else 0.0)
                lost.append(1.0 if stop and s["passed"] else 0.0)
            saved = np.array(saved + [0.0] * (len(H) - len(live)))
            lost = np.array(lost + [0.0] * (len(H) - len(live)))
            idx = RNG.integers(0, len(H), (2000, len(H)))
            sh = saved[idx].sum(1) / base[idx].sum(1)
            pl = lost[idx].mean(1)
            rows.append({"theta": th, "usd_saved_share": [float(saved.sum() / base.sum()), float(np.percentile(sh, 5)), float(np.percentile(sh, 95))],
                         "pass_rate_points_lost": [float(100 * lost.mean()), float(100 * np.percentile(pl, 5)), float(100 * np.percentile(pl, 95))],
                         "stopped": int(sum(pi > th for pi in p))})
        out[f"checkpoint_{m}"] = {"n_sessions": len(H), "n_live": len(live), "fail_rate_live": float(y.mean()),
                                  "auc_loro": float(auc), "policy": rows}
    (HERE / "stop.json").write_text(json.dumps(out, indent=1))
    for k, v in out.items():
        print(k, v["n_live"], "fail", round(v["fail_rate_live"], 2), "AUC", round(v["auc_loro"], 3))
        for r in v["policy"]:
            print("   ", r["theta"], "stopped", r["stopped"], "usd", [round(a, 3) for a in r["usd_saved_share"]],
                  "pts lost", [round(a, 1) for a in r["pass_rate_points_lost"]])


if __name__ == "__main__":
    main()
