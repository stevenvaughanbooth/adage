"""Hand-label evaluation: held-out AUC/F1 (models trained on non-control sessions) and
leave-one-repo-out AUC (trained on no session of the held-out repository, any arm).

  python3 evaluate.py            -> results/eval.json, and caches per-fold scores for replay.py
"""

from __future__ import annotations

import json
import pickle
import sys
import time

import numpy as np
from sklearn.metrics import f1_score, roc_auc_score

import features as F
import models as M
import value as V

CACHE = F.load.CACHE
OUT = M.HERE / "results"
BASE_REPOS = ["attrs", "click", "marshmallow", "urfave-cli", "vue-core"]


def templates(a, T, fit, tag):
    p = CACHE / f"tpl_{tag}.pkl"
    if p.exists():
        return pickle.loads(p.read_bytes())
    t0 = time.time()
    _, tp = F.drain_fit(T, fit)
    p.write_bytes(pickle.dumps(tp))
    print(f"drain {tag}: {time.time() - t0:.0f}s", file=sys.stderr)
    return tp


def fold(a, T, fit, tag):
    tp = templates(a, T, fit, tag)
    H = F.hash_templates(tp)
    fo = M.Fold(a, H, fit)
    st = V.states(a, fit)
    Vt = V.td_lambda(a, st, fit)
    Vm = V.monte_carlo(a, st, fit)
    fo.s["td-advantage"] = -V.advantage(a, st, Vt)
    fo.s["mc-advantage"] = -V.advantage(a, st, Vm)
    fo.V, fo.st, fo.tp = Vt, st, tp
    return fo


def main():
    a = F.table()
    T = F.outputs()
    calls = M.all_calls()
    li, lab = M.labels(a)
    hc_fail, hc_succ = M.hand_coded(a, [calls[i] for i in li])
    y_fail = np.isin(lab, ["E", "T"]).astype(int)
    y_dead = (lab == "E").astype(int)
    y_succ = (lab == "S").astype(int)
    groups = a["sid"][li]
    pool = a["arm"] != "control"
    res = {"n_labels": int(len(li)), "label_counts": {c: int((lab == c).sum()) for c in "SIET"},
           "n_label_sessions": int(len(set(groups))), "pool_sessions": int(len(set(a["sid"][pool]))),
           "pool_calls": int(pool.sum())}

    main_fold = fold(a, T, pool, "main")
    scores = dict(main_fold.s)
    scores["B0 is_error"] = a["is_error"].astype(float)
    hc = np.zeros(len(a["sid"]))
    hc[li] = hc_fail
    scores["B1 hand-coded"] = hc

    def table(ys, name):
        out = {}
        for k, s in scores.items():
            auc, lo, hi = M.boot_auc(ys, s[li], groups)
            out[k] = {"auc": round(auc, 3), "ci90": [round(lo, 3), round(hi, 3)]}
        return out

    res["failure_auc"] = table(y_fail, "failure")
    res["deadend_auc"] = table(y_dead, "dead-end (E only)")
    # dead end vs everything *but* task failures: the question the reflex trigger asks
    keep = lab != "T"
    res["deadend_vs_ok_auc"] = {k: round(roc_auc_score(y_dead[keep], s[li][keep]), 3) for k, s in scores.items()}
    # piped / masked failures: failures the harness did not flag
    masked = a["is_error"][li] == 0
    res["failure_auc_is_error_false"] = {
        k: round(roc_auc_score(y_fail[masked], s[li][masked]), 3) for k, s in scores.items()
        if len(set(y_fail[masked])) == 2}
    res["n_failures_not_flagged"] = int((y_fail[masked]).sum())
    # success
    ssc = {"B1 hand-coded (executed>ok>failed)": hc_succ, "learned recover×(1−fail)": main_fold.success[li],
           "1 − learned fail": 1 - main_fold.p_fail[li], "B0 1 − is_error": 1 - a["is_error"][li]}
    res["success_auc"] = {}
    for k, s in ssc.items():
        auc, lo, hi = M.boot_auc(y_succ, s, groups)
        res["success_auc"][k] = {"auc": round(auc, 3), "ci90": [round(lo, 3), round(hi, 3)]}
    # F1 at the teacher's own 0.5 (no label tuning) and 3-class macro F1
    pf = main_fold.p_fail[li]
    res["f1_failure"] = {
        "B0 is_error": round(f1_score(y_fail, a["is_error"][li]), 3),
        "B1 hand-coded": round(f1_score(y_fail, hc_fail), 3),
        "learned drain+potion→exit (p>0.5)": round(f1_score(y_fail, main_fold.s["drain+potion→exit"][li] > 0.5), 3),
        "learned | is_error (p>0.5)": round(f1_score(y_fail, pf > 0.5), 3)}
    succ_thr = np.quantile(main_fold.success[pool], 0.75)  # pool-derived, no labels
    pred3 = np.where(pf > 0.5, "F", np.where(main_fold.success[li] > succ_thr, "S", "I"))
    hc3 = np.where(hc_fail > 0, "F", np.where(hc_succ == 2, "S", "I"))
    lab3 = np.where(np.isin(lab, ["E", "T"]), "F", lab)
    res["macro_f1_3class"] = {"B1 hand-coded": round(f1_score(lab3, hc3, average="macro"), 3),
                              "learned": round(f1_score(lab3, pred3, average="macro"), 3)}

    # leave-one-repo-out: no session of the held-out repository (any arm, any model) in training
    loro = {}
    pooled = {k: np.zeros(len(li)) for k in ["drain→exit", "potion→exit", "drain+potion→exit", "retry-excess",
                                            "combined", "td-advantage", "drain+potion→exit | is_error",
                                            "combined | is_error"]}
    for r in BASE_REPOS:
        fit = a["base_repo"] != r
        fo = fold(a, T, fit, f"loro-{r}")
        sel = a["base_repo"][li] == r
        for k in pooled:
            pooled[k][sel] = fo.s[k][li][sel]
        loro[r] = {k: round(roc_auc_score(y_fail[sel], fo.s[k][li][sel]), 3) for k in pooled}
        loro[r]["B1 hand-coded"] = round(roc_auc_score(y_fail[sel], hc_fail[sel]), 3)
        loro[r]["B0 is_error"] = round(roc_auc_score(y_fail[sel], a["is_error"][li][sel]), 3)
        np.save(CACHE / f"pfail_loro-{r}.npy", fo.p_fail)
        np.save(CACHE / f"cluster_loro-{r}.npy", fo.cluster)
    res["loro_by_repo"] = loro
    res["loro_pooled_failure_auc"] = {}
    for k, s in pooled.items():
        auc, lo, hi = M.boot_auc(y_fail, s, groups)
        res["loro_pooled_failure_auc"][k] = {"auc": round(auc, 3), "ci90": [round(lo, 3), round(hi, 3)]}
    for k, s in [("B0 is_error", a["is_error"][li].astype(float)), ("B1 hand-coded", hc_fail)]:
        auc, lo, hi = M.boot_auc(y_fail, s, groups)
        res["loro_pooled_failure_auc"][k] = {"auc": round(auc, 3), "ci90": [round(lo, 3), round(hi, 3)]}

    np.save(CACHE / "pfail_main.npy", main_fold.p_fail)
    np.save(CACHE / "psucc_main.npy", main_fold.success)
    np.save(CACHE / "cluster_main.npy", main_fold.cluster)
    np.save(CACHE / "adv_main.npy", -main_fold.s["td-advantage"])
    OUT.mkdir(exist_ok=True)
    (OUT / "eval.json").write_text(json.dumps(res, indent=1))
    print(json.dumps(res, indent=1))


if __name__ == "__main__":
    main()
