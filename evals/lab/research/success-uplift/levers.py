"""What distinguishes sessions that pass the hidden tests from those that fail, on the same task?

Units: eval sessions of every arm (control, adage, agents-md: the arms made no measurable
difference to pass rate, `arms.py`), stratified by task x model x bench, so every contrast is
"passed vs failed attempts at the same task, same model, same turn cap".

Behaviours are features of learned action clusters (`clusters.py`; Drain + potion-8M + k-means,
fitted on the training split): for each cluster c, `any` (c happened), `tail` (c among the last
5 calls: "before finishing, do c"), `endok` (the last c exited 0), `again` (c again after its first
exit-0 run: "not stopping at the first green"), `late:c|d` (c after the last d; coarse clusters
only), `many` (more c than the stratum median). Exit status is Claude Code's is_error flag, not a
parsed message.

Protocol (held-out tasks): 5-fold GroupKFold over tasks (a task's haiku and sonnet strata stay
together), repeated over 4 fold seeds. On the training folds, rank features by the one-sided 90%
lower bound of the Mantel-Haenszel risk difference (MH RD) and keep the top 5 that vary in >= 4
training strata. On the held-out fold, record those features' rows. A feature's held-out effect is
the MH RD over every held-out fold in which it was selected, with a task-cluster bootstrap 90% CI.
Leave-one-repo-out: the same, selecting on four repositories (clusters refitted without the fifth's
training sessions) and testing on the fifth.

Also: the prespecified hypotheses from the brief, each mapped to learned clusters by their names
(that mapping is the one manual step, disclosed), estimated on all tasks with the same estimator;
and oracle-instrument features that use the task's upstream gold files (measurement only).
"""

from __future__ import annotations

import json
import math
import sys
from collections import Counter, defaultdict
from pathlib import Path

import numpy as np

import clusters as CL
from load import load
from stats_util import boot_mh, mh_rd, n_per_arm, n_per_arm_stratified

TASKS = Path(__file__).resolve().parents[3] / "repobench" / "tasks"
TAIL = 5


def gold():
    g = {}
    for f in TASKS.glob("*.json"):
        for t in json.loads(f.read_text())["tasks"]:
            g[t["id"]] = t
    return g


def features(s, lab, k, pairs=True) -> dict[str, int]:
    st = s["steps"]
    n = len(st)
    f = {}
    seq = list(lab)
    err = [x["err"] for x in st]
    last = {}
    first_ok = {}
    cnt = Counter(seq)
    for i, c in enumerate(seq):
        last[c] = i
        if not err[i] and c not in first_ok:
            first_ok[c] = i
    for c in range(k):
        p = f"k{k}:c{c}"
        f[f"any:{p}"] = int(c in last)
        f[f"tail:{p}"] = int(c in seq[-TAIL:])
        f[f"endok:{p}"] = int(c in last and not err[last[c]])
        f[f"again:{p}"] = int(c in first_ok and any(seq[j] == c for j in range(first_ok[c] + 1, n)))
        f[f"cnt:{p}"] = cnt.get(c, 0)
        f[f"pos:{p}"] = seq.index(c) if c in last else n  # first occurrence; binarised to `early`
    if pairs:
        for c in range(k):
            for d in range(k):
                if c != d:
                    f[f"late:k{k}:c{c}|c{d}"] = int(d in last and c in last and last[c] > last[d])
    return f


def binarise(S, F):
    """`cnt` features become `many` (above the stratum median); `pos` becomes `early` (first
    occurrence before the stratum median)."""
    by = defaultdict(list)
    for s in S:
        by[stratum(s)].append(s["sid"])
    for sids in by.values():
        keys = [x for x in F[sids[0]] if x.startswith("cnt:")]
        for key in keys:
            med = np.median([F[x][key] for x in sids])
            for x in sids:
                F[x]["many:" + key[4:]] = int(F[x][key] > med)
        for key in [x for x in F[sids[0]] if x.startswith("pos:")]:
            med = np.median([F[x][key] for x in sids])
            for x in sids:
                F[x]["early:" + key[4:]] = int(F[x][key] < med)
    for x in F:
        for key in [q for q in F[x] if q.startswith(("cnt:", "pos:"))]:
            del F[x][key]


def stratum(s):
    return (s["task"], s["model"], s["bench"])


def rd_lb(rows):
    """MH RD and an approximate one-sided 90% lower bound (Sato-style variance with +0.5 smoothing)."""
    g = defaultdict(lambda: [0, 0, 0, 0])
    for st, x, y in rows:
        a = g[st]
        if x:
            a[0] += 1; a[1] += y
        else:
            a[2] += 1; a[3] += y
    num = den = var = 0.0
    m = 0
    for n1, y1, n0, y0 in g.values():
        if n1 and n0:
            w = n1 * n0 / (n1 + n0)
            num += w * (y1 / n1 - y0 / n0)
            den += w
            q1 = (y1 + .5) / (n1 + 1); q0 = (y0 + .5) / (n0 + 1)
            var += w * w * (q1 * (1 - q1) / n1 + q0 * (1 - q0) / n0)
            m += 1
    if not den:
        return float("nan"), float("nan"), 0
    est = num / den
    return est, est - 1.2816 * math.sqrt(var) / den, m


def select(S, F, keys, top=5, min_strata=4):
    scored = []
    for key in keys:
        rows = [(stratum(s), F[s["sid"]][key], int(s["passed"])) for s in S]
        est, lb, m = rd_lb(rows)
        if m >= min_strata and not math.isnan(lb):
            scored.append((lb, est, key))
    scored.sort(reverse=True)
    return scored[:top]


def cv(S, F, keys, groups, seeds=(0, 1, 2, 3), folds=5):
    held = defaultdict(list)  # feature -> rows on held-out tasks where it was selected
    top1 = []
    picks = Counter()
    ug = sorted(set(groups.values()))
    for seed in seeds:
        rng = np.random.default_rng(seed)
        perm = rng.permutation(ug)
        for f in range(folds):
            test = set(perm[f::folds])
            tr = [s for s in S if groups[s["sid"]] not in test]
            te = [s for s in S if groups[s["sid"]] in test]
            sel = select(tr, F, keys)
            for r, (_, _, key) in enumerate(sel):
                picks[key] += 1
                rows = [((seed, stratum(s)), F[s["sid"]][key], int(s["passed"])) for s in te]
                held[key] += rows
                if r == 0:
                    top1 += rows
    return held, top1, picks


def summarise(rows):
    if not rows:
        return None
    e, lo, hi, w = boot_mh(rows, cluster_of=lambda r: r[0][1][0], B=1000)
    return dict(rd=round(e, 3), ci90=[round(lo, 3), round(hi, 3)], weight=round(w, 1),
                strata=len({r[0] for r in rows}))


def main(pop="all"):
    S = [s for s in load() if s["phase"] == "eval" and (pop == "all" or not s["censored"])]
    G = gold()
    # the informative sample: strata with >= 2 sessions
    cnt = Counter(stratum(s) for s in S)
    S = [s for s in S if cnt[stratum(s)] >= 2]
    y = np.array([s["passed"] for s in S], float)
    out = {"n_sessions": len(S), "n_strata": len(cnt)}

    # variance decomposition: how much of pass/fail is the task?
    bys = defaultdict(list)
    for s in S:
        bys[stratum(s)].append(s["passed"])
    within = sum(np.var(v) * len(v) for v in bys.values()) / len(y)
    mixed = [k for k, v in bys.items() if 0 < sum(v) < len(v)]
    oracle = np.mean([any(bys[stratum(s)]) for s in S])
    out["variance"] = dict(total=round(float(y.var()), 4), within_stratum=round(float(within), 4),
                           share_within=round(float(within / y.var()), 3), pass_rate=round(float(y.mean()), 3),
                           mixed_strata=len(mixed), sessions_in_mixed=sum(len(bys[k]) for k in mixed),
                           oracle_pass_if_every_attempt_matched_best=round(float(oracle), 3))
    print(json.dumps(out["variance"]))

    groups = {s["sid"]: s["task"] for s in S}
    res = {}
    for label, exclude in [("all", None)] + [(f"loro:{r}", r) for r in sorted({s["repo"] for s in S})]:
        F = {}
        for k in (12, 40):
            C = CL.fit(load(), k, exclude_repo=exclude)
            lab = CL.assign(load(), C, exclude_repo=exclude)
            if label == "all":
                out.setdefault("cluster_names", {})[k] = CL.names(load(), lab, k)
            for s in S:
                F.setdefault(s["sid"], {}).update(features(s, lab[s["sid"]], k, pairs=(k == 12)))
        binarise(S, F)
        keys = sorted(F[S[0]["sid"]])
        if label == "all":
            held, top1, picks = cv(S, F, keys, groups)
            res["cv"] = dict(
                top1=summarise(top1),
                picked={key: dict(times=n, held_out=summarise(held[key])) for key, n in picks.most_common(15)},
            )
            # full-data ranking for description (not a held-out number)
            res["full_data_top"] = [dict(key=k_, rd=round(e, 3), lb90=round(lb, 3))
                                    for lb, e, k_ in select(S, F, keys, top=25)]
            Fall = F
        else:
            r = label.split(":")[1]
            tr = [s for s in S if s["repo"] != r]
            te = [s for s in S if s["repo"] == r]
            sel = select(tr, F, keys)
            rows = [(("loro", stratum(s)), F[s["sid"]][sel[0][2]], int(s["passed"])) for s in te] if sel else []
            res[label] = dict(selected=[x[2] for x in sel], top1_held_out=summarise(rows))
        print(label, json.dumps(res.get(label, res.get("cv", {}).get("top1")))[:400])
    out["results"] = res

    # prespecified hypotheses + oracle instruments
    names12 = out["cluster_names"][12]; names40 = out["cluster_names"][40]
    hyp = prespecified(S, Fall, names12, names40, G)
    out["prespecified"] = hyp
    json.dump(out, open(f"levers_{pop}.json", "w"), indent=1, default=str)


def _clusters_named(names, pat):
    import re
    return [i for i, n in enumerate(names) if re.search(pat, n.split(" | ")[0])]


def hyp_features(S, G, n12=None, n40=None):
    """The brief's candidate behaviours, mapped onto learned clusters by the clusters' names (the
    one manual step: disclosed), plus gold-file oracle instruments. Returns ({sid: {name: 0/1}}, mapping)."""
    if n12 is None:
        L = load()
        n12 = CL.names(L, CL.assign(L, CL.fit(L, 12)), 12)
        n40 = CL.names(L, CL.assign(L, CL.fit(L, 40)), 40)
    test40 = _clusters_named(n40, r"pytest|go test|npm test|vitest")
    edit12 = _clusters_named(n12, r"^Edit|^Write")
    adhoc40 = _clusters_named(n40, r"python3? -c|<<heredoc|python \{test_file\}")
    out = {"mapping": {"test_run_clusters_k40": test40, "edit_clusters_k12": edit12, "adhoc_script_clusters_k40": adhoc40}}
    lab = {}
    C40 = CL.fit(load(), 40); L40 = CL.assign(load(), C40)
    C12 = CL.fit(load(), 12); L12 = CL.assign(load(), C12)
    H = {}
    for s in S:
        l40, l12, st = L40[s["sid"]], L12[s["sid"]], s["steps"]
        n = len(st)
        is_test = [c in test40 for c in l40]
        is_edit = [c in edit12 for c in l12]
        last_edit = max([i for i in range(n) if is_edit[i]], default=-1)
        tests_after = [i for i in range(last_edit + 1, n) if is_test[i]]
        g = G.get(s["task"], {})
        src = set(g.get("src_files", [])); tst = set(g.get("test_files", []))
        tst_names = {t.rsplit("/", 1)[-1] for t in tst}
        tests_idx = [i for i in range(n) if is_test[i]]
        targeted = [i for i in tests_idx if any(t in st[i]["raw"] for t in tst_names)]
        broad = [i for i in tests_idx if not any(ch in st[i]["raw"] for ch in (".py", ".ts", ".go", "::", " -run", "-k "))]
        first_green = next((i for i in tests_idx if not st[i]["err"]), None)
        reads = {x["path"] for x in st if x["tool"] in ("Read",)} | {
            p for x in st if x["tool"] == "Bash" for p in src | tst if p in x["raw"]}
        changed = set(s["changed"])
        H[s["sid"]] = {
            "H1 test run after the last edit (before finishing)": int(bool(tests_after)),
            "H1b last test run after the last edit exited 0": int(bool(tests_after) and not st[tests_after[-1]]["err"]),
            "H2 ran the task's existing test module (targeted) [gold instrument]": int(bool(targeted)),
            "H2b targeted test run after the last edit [gold instrument]": int(any(i > last_edit for i in targeted)),
            "H3 ad-hoc reproduction script (python -c / heredoc / script) ": int(any(c in adhoc40 for c in l40)),
            "H3b ad-hoc script after the last edit": int(any(l40[i] in adhoc40 for i in range(last_edit + 1, n))),
            "H4 kept going after the first green test run": int(first_green is not None and any(
                is_edit[j] or is_test[j] for j in range(first_green + 1, n))),
            "H5 ran a broad/full test suite (no file or selector) [hand instrument]": int(bool(broad)),
            "H5b broad suite after the last edit [hand instrument]": int(any(i > last_edit for i in broad)),
            "R1 read a gold source file [gold instrument]": int(bool(src & reads)),
            "R2 edited a gold source file [gold instrument]": int(bool(src & changed)),
            "R3 first edit was to a gold source file [gold instrument]": int(next(
                (x["path"] for x in st if x["tool"] in ("Edit", "Write", "MultiEdit")), None) in src),
            "R4 edited a file outside the gold source + test files": int(bool(changed - src - tst - {"CHANGES.rst", "CHANGES.md"})),
            "C1 session censored (hit max_turns)": int(s["censored"]),
        }
    return H, out


def prespecified(S, F, n12, n40, G):
    H, out = hyp_features(S, G, n12, n40)
    res = {}
    for key in next(iter(H.values())):
        rows = [(("p", stratum(s)), H[s["sid"]][key], int(s["passed"])) for s in S]
        nc = [s for s in S if not s["censored"]]
        rows_nc = [(("p", stratum(s)), H[s["sid"]][key], int(s["passed"])) for s in nc]
        px = np.mean([r[1] for r in rows])
        e = summarise(rows)
        res[key] = dict(prevalence=round(float(px), 3), all=e, uncensored=summarise(rows_nc),
                        by_repo={r: summarise([row for row, s in zip(rows, S) if s["repo"] == r])
                                 for r in sorted({s["repo"] for s in S})})
        print(f"{key[:70]:70} P(X)={px:.2f} RD={e['rd'] if e else None} {e['ci90'] if e else ''}  "
              f"uncens={res[key]['uncensored']['rd'] if res[key]['uncensored'] else None}")
    return res


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "all")
