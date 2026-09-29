"""Instrument checks for the learned parts, leave-one-repository-out (day 0: no own sessions).

1. The dead-end detector against its own behavioural label on the held-out repository (AUC), and
   against the replay's hand-written environment signatures (a measurement instrument only:
   "No module named", "ModuleNotFoundError", "command not found", "Cannot find module"): recall of
   the signature calls, including the exit-0 piped ones the harness cannot flag.
2. The action clusters: which heads share the test-runner intent with `python -m pytest`.
"""

from __future__ import annotations

import collections
import json
import re
import sys
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import data  # noqa: E402
import learn  # noqa: E402

SIG = re.compile(r"No module named|ModuleNotFoundError|command not found|Cannot find module")  # instrument


def auc(y, p):
    from sklearn.metrics import roc_auc_score
    y = np.asarray(y)
    return float(roc_auc_score(y, p)) if 0 < y.sum() < len(y) else None


def main():
    allss = data.load()
    usable = [s for s in allss if s.arm in ("train", "control")]
    trees = learn.trees_for(allss)
    out = {}
    for X in data.REPOS:
        others = [s for s in usable if s.repo != X]
        L = learn.Learner(others, trees)
        test = [s for s in allss if s.repo == X and s.arm == "control"]
        ys, ps, sig, sig_piped = [], [], [], []
        for s in test:
            rs = L.prepare(s)
            # behavioural label on the held-out session, computed exactly as on the pool
            tmp = learn.Learner.__new__(learn.Learner)
            tmp.recs = {s.sid: rs}
            learn.Learner._label(tmp)
            for r, c in zip(rs, s.calls):
                if r.K is None:
                    continue
                ys.append(int(r.dead_beh))
                ps.append(r.pdead)
                if SIG.search(r.out):
                    sig.append(r.pdead >= 0.5)
                    if not c.is_error:
                        sig_piped.append(r.pdead >= 0.5)
        pos = [p >= 0.5 for p in ps]
        K = L.cluster_of("python -m pytest")
        same = sorted(h for h, k in L.hK.items() if k == K)
        out[X] = dict(bash_calls=len(ys), auc_behaviour=auc(ys, ps), flagged_share=float(np.mean(pos)),
                      signature_calls=len(sig), signature_recall=float(np.mean(sig)) if sig else None,
                      signature_exit0_calls=len(sig_piped),
                      signature_exit0_recall=float(np.mean(sig_piped)) if sig_piped else None,
                      pytest_intent=same[:25])
        print(X, {k: v for k, v in out[X].items() if k != "pytest_intent"}, flush=True)
    (HERE / "results" / "diag.json").write_text(json.dumps(out, indent=1))


if __name__ == "__main__":
    main()
