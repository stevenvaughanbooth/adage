"""Fix for the exit-status teacher: a pipeline's exit status is its last stage's, so for a piped
Bash command (`… 2>&1 | tail`) `is_error` says nothing about the output the model reads. Those
calls are dropped from the teacher's training set (their labels are noise exactly where the
learned signal matters: piped dead ends). The pipe test is one shell-grammar token (an unquoted
`|` that is not `||`), not an error-string list; it is listed as a hard-coded part.

Re-evaluates the exit-taught models with the clean teacher (hand labels; held-out and LORO) and
the dead-end recall at the teacher's own 0.5 (no label tuning), using the cached Drain templates.

  python3 eval2.py -> results/eval2.json, caches pfail2_*.npy / cluster2_*.npy for downstream use
"""

from __future__ import annotations

import json
import pickle
import re

import numpy as np
from scipy.sparse import csr_matrix, hstack
from sklearn.cluster import MiniBatchKMeans

import features as F
import instrument as I
import models as M

CACHE = F.load.CACHE
PIPE = re.compile(r"(?<![|])[|](?![|])")
REPOS = ["attrs", "click", "marshmallow", "urfave-cli", "vue-core"]


def piped(a, calls):
    return np.array([c["tool"] == "Bash" and bool(PIPE.search(c["arg"])) for c in calls])


def fit_scores(a, H, fit):
    EO, EA = a["EO"].astype(np.float32), a["EA"].astype(np.float32)
    vocab = {t: i for i, t in enumerate(sorted(set(a["tool"][fit])))}
    T = M.tool_onehot(a["tool"], vocab)
    ln = np.c_[np.log1p(a["out_len"]) / 10, (a["out_len"] == 0)].astype(np.float32)
    Xp = np.c_[EO, EA, T, ln]
    Xdp = hstack([H, csr_matrix(Xp)]).tocsr()
    y = a["is_error"]
    s = {"drain→exit (clean teacher)": M.lr(H[fit], y[fit]).predict_proba(H)[:, 1],
         "potion→exit (clean teacher)": M.lr(Xp[fit], y[fit]).predict_proba(Xp)[:, 1],
         "drain+potion→exit (clean teacher)": M.lr(Xdp[fit], y[fit], C=0.5).predict_proba(Xdp)[:, 1]}
    km = MiniBatchKMeans(64, random_state=0, n_init=3, batch_size=2048).fit(EO[fit])
    return s, km.predict(EO)


def main():
    a = F.table()
    calls = M.all_calls()
    pip = piped(a, calls)
    li, lab = M.labels(a)
    y_fail = np.isin(lab, ["E", "T"]).astype(int)
    groups = a["sid"][li]
    res = {"piped_bash_calls": int(pip.sum())}
    pool = a["arm"] != "control"
    H = F.hash_templates(pickle.loads((CACHE / "tpl_main.pkl").read_bytes()))
    s, cl = fit_scores(a, H, pool & ~pip)
    res["failure_auc"] = {}
    for k, v in s.items():
        auc, lo, hi = M.boot_auc(y_fail, v[li], groups)
        res["failure_auc"][k] = {"auc": round(auc, 3), "ci90": [round(lo, 3), round(hi, 3)]}
    masked = a["is_error"][li] == 0
    res["failure_auc_is_error_false"] = {k: round(M.roc_auc_score(y_fail[masked], v[li][masked]), 3)
                                         for k, v in s.items()}
    best = "drain→exit (clean teacher)"
    pf = np.maximum(s[best], a["is_error"])
    np.save(CACHE / "pfail2_main.npy", pf)
    np.save(CACHE / "cluster2_main.npy", cl)
    # F1 at 0.5, no label tuning
    res["f1_failure_at_0.5"] = {k: round(M.f1_score(y_fail, v[li] > 0.5), 3) for k, v in s.items()}
    res["f1_failure_at_0.5"]["drain→exit (clean) | is_error"] = round(M.f1_score(y_fail, pf[li] > 0.5), 3)
    hc = M.hand_coded(a, [calls[i] for i in li])[0]
    res["f1_failure_at_0.5"]["B1 hand-coded"] = round(M.f1_score(y_fail, hc), 3)
    res["f1_failure_at_0.5"]["B0 is_error"] = round(M.f1_score(y_fail, a["is_error"][li]), 3)
    # dead-end recall on all control calls (the instrument's U11 dead ends and env signatures)
    ctrl = a["arm"] == "control"
    ie = a["is_error"].astype(bool)
    u11 = np.array([ctrl[i] and I.u11_dead_end(I.act(c)) for i, c in enumerate(calls)])
    env = np.array([ctrl[i] and c["tool"] == "Bash" and bool(I.ENV_SIG.search(c["output"])) for i, c in enumerate(calls)])
    fl = pf > 0.5
    rec = {}
    for name, m in [("U11 dead ends", u11), ("U11 dead ends, piped (is_error False)", u11 & ~ie),
                    ("env-signature calls", env), ("env-signature calls, is_error False", env & ~ie)]:
        rec[name] = {"n": int(m.sum()), "learned_flags": int((fl & m).sum())}
    rec["control calls flagged, is_error False"] = int((fl & ctrl & ~ie).sum())
    rec["control calls with is_error"] = int((ctrl & ie).sum())
    res["deadend_recall_at_0.5"] = rec
    # leave-one-repo-out
    pooled = {k: np.zeros(len(li)) for k in s}
    by = {}
    for r in REPOS:
        fit = (a["base_repo"] != r) & ~pip
        H = F.hash_templates(pickle.loads((CACHE / f"tpl_loro-{r}.pkl").read_bytes()))
        sr, clr = fit_scores(a, H, fit)
        sel = a["base_repo"][li] == r
        by[r] = {k: round(M.roc_auc_score(y_fail[sel], v[li][sel]), 3) for k, v in sr.items()}
        for k in pooled:
            pooled[k][sel] = sr[k][li][sel]
        np.save(CACHE / f"pfail2_loro-{r}.npy", np.maximum(sr[best], a["is_error"]))
        np.save(CACHE / f"cluster2_loro-{r}.npy", clr)
    res["loro_by_repo"] = by
    res["loro_pooled_failure_auc"] = {}
    for k, v in pooled.items():
        auc, lo, hi = M.boot_auc(y_fail, v, groups)
        res["loro_pooled_failure_auc"][k] = {"auc": round(auc, 3), "ci90": [round(lo, 3), round(hi, 3)]}
    (M.HERE / "results" / "eval2.json").write_text(json.dumps(res, indent=1))
    print(json.dumps(res, indent=1))


if __name__ == "__main__":
    main()
