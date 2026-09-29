"""How well does each follow judge agree with the measurement instrument, leave-one-repo-out?

Unit-level: "followed within K calls of delivery" (K = 5 after a post-hoc hook; for a pre-emptive
UserPromptSubmit/SessionStart directive the window is the whole session, since the need comes later).
The learned judges' threshold tau is fitted on the other repositories (max accuracy against the
instrument), then applied to the held-out repository. Output: results/judges.json and the per-unit
cache used by the other scripts.
"""
from __future__ import annotations

import json
import pickle
from pathlib import Path

import numpy as np

import judge
import units

HERE = Path(__file__).resolve().parent
CACHE = Path("/tmp/adage-research/uptake-delivery")
PRE = {"UserPromptSubmit", "SessionStart"}
K = 5


def window(u):
    return None if u.hook in PRE else K


def trigger_text(u):
    if u.hook.startswith("PostToolUse") and u.pos > 0:
        return units.action_text(u.s.calls[u.pos - 1], u.s.cwd)
    return ""


def profiles(U):
    A = pickle.load(open(CACHE / "train_actions.pkl", "rb")) if (CACHE / "train_actions.pkl").exists() \
        else judge.train_actions()
    pot = judge.Sim("potion-8M")
    con = judge.Containment(A, boost=1.0)
    ctr = judge.Containment(A, boost=3.0)
    tf = judge.Sim("tfidf", fit_texts=[t for _, t in A])
    P = []
    for u in U:
        texts, _ = judge.lags(u, window(u))
        trig = trigger_text(u)
        P.append({
            "potion-8M": pot.sims(u.directed, texts) if texts else np.zeros(0),
            "tfidf": tf.sims(u.directed, texts) if texts else np.zeros(0),
            "containment": con.sims(u.directed, texts),
            "contrastive": ctr.sims(u.directed, texts, trig),
        })
    return P


def main():
    U = [u for u in units.build() if u.directed]
    P = profiles(U)
    inst = [judge.instrument_lag(u, window(u)) for u in U]
    y = np.array([l is not None for l in inst])
    repos = sorted({u.s.repo for u in U})
    out = {"n_units": len(U), "K": K, "judges": {}}
    grid = np.round(np.arange(0.30, 1.001, 0.01), 2)
    for name in ["potion-8M", "tfidf", "containment", "contrastive"]:
        mx = np.array([p[name].max() if len(p[name]) else 0.0 for p in P])
        pred = np.zeros(len(U), bool)
        taus = {}
        for r in repos:
            te = np.array([u.s.repo == r for u in U])
            acc = [((mx[~te] >= t) == y[~te]).mean() for t in grid]
            t = float(grid[int(np.argmax(acc))])
            taus[r] = t
            pred[te] = mx[te] >= t
        agree = float((pred == y).mean())
        fp = int((pred & ~y).sum())
        fn = int((~pred & y).sum())
        out["judges"][name] = {"loro_agreement": round(agree, 3), "false_follow": fp, "missed_follow": fn,
                               "tau_by_heldout_repo": taus}
        pickle.dump((U, P, inst), open(CACHE / "units_profiles.pkl", "wb"))
    lit = np.array([u.literal == 1 for u in U])
    out["judges"]["literal (runtime)"] = {"loro_agreement": round(float((lit == y).mean()), 3),
                                          "false_follow": int((lit & ~y).sum()), "missed_follow": int((~lit & y).sum())}
    (HERE / "results").mkdir(exist_ok=True)
    json.dump(out, open(HERE / "results" / "judges.json", "w"), indent=1)
    print(json.dumps(out, indent=1))


if __name__ == "__main__":
    main()
