"""Two more harness numbers:
  1. does the learned reflex add anything on top of U11 (U11 predictive first, then learned)?
  2. oracle ceilings under the cumulative-context cost model, with the *learned* outcome model as
     the predicate: every call it scores as a failure, and every dead-end window (a learned
     failure through to the next same-intent learned success), next to the hand-coded classes.

  python3 extras.py -> results/extras.json
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))
sys.path.insert(0, str(HERE))

from harness import load  # noqa: E402
from harness.drivers import split  # noqa: E402
from harness.oracle import ceiling  # noqa: E402
from harness.policies.u11 import U11  # noqa: E402

import policy as P  # noqa: E402


class Combo:
    events = {"PreToolUse", "PostToolUse"}

    def __init__(self, *pols):
        self.pols = pols

    def __call__(self, sit):
        for p in self.pols:
            d = p(sit)
            if d is not None:
                return d
        return None


def main():
    corpus = load(include_dx=False)  # pinned: RepoBench v1+v2 only (test set 6a795a362ccb)
    test = corpus.held_out(model="haiku")
    res = {}
    r = split(lambda tr: Combo(U11("predictive", tr), P.LearnedOutcomePolicy(tr)), corpus, model="haiku",
              name="U11 predictive + learned post-hoc [split]")
    res["U11 predictive + learned post-hoc split"] = r.to_json() | {"headline": r.headline()}
    print(r.headline(), flush=True)

    # learned outcome model fitted on every repository's training sessions (no held-out session)
    tr = corpus.train()
    m = P.model()
    rows = [c for s in tr for c in s.calls]
    eo = m.encode([P.out_text(c.output) for c in rows])
    ea = m.encode([P.act_text(c) for c in rows])
    om = P.OutcomeModel().fit(eo, ea, [c.tool for c in rows], [c.out_chars or len(c.output) for c in rows],
                              np.array([int(c.is_error) for c in rows]))
    pf, win = {}, {}
    for s in test:
        e_o = m.encode([P.out_text(c.output) for c in s.calls])
        e_a = m.encode([P.act_text(c) for c in s.calls])
        p = np.maximum(om.p(e_o, e_a, [c.tool for c in s.calls], [c.out_chars or len(c.output) for c in s.calls]),
                       [c.is_error for c in s.calls])
        pf[s.sid] = p
        w = set()
        for i in range(len(p)):
            if p[i] <= 0.5:
                continue
            for j in range(i + 1, min(len(p), i + 16)):
                if float(e_a[i] @ e_a[j]) >= 0.8 and p[j] < 0.5:
                    w.update(range(i, j))
                    break
        win[s.sid] = w
    sid_of = {id(c): (s.sid, k) for s in test for k, c in enumerate(s.calls)}

    def learned_fail(a, c):
        sid, k = sid_of[id(c)]
        return pf[sid][k] > 0.5

    def learned_window(a, c):
        sid, k = sid_of[id(c)]
        return k in win[sid]

    for name, pred in [("hand-coded failed (is_error or env signature)", lambda a, c: a.failed),
                       ("hand-coded env friction (U11's W)", lambda a, c: a.env_friction),
                       ("learned failure (p_fail > 0.5, or is_error)", learned_fail),
                       ("learned dead-end windows (failure → same-intent success)", learned_window)]:
        res[f"oracle: {name}"] = ceiling(test, pred)
        print(name, json.dumps(res[f"oracle: {name}"])[:400], flush=True)
    (HERE / "results" / "extras.json").write_text(json.dumps(res, indent=1, default=str))


if __name__ == "__main__":
    main()
