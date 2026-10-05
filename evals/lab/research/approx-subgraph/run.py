"""The experiment: mine, select by MDL, and evaluate on held-out sessions.

Settings
- `chrono`: the RepoBench split. Train = each repository's 7-10 `train` sessions; test = its
  held-out control sessions (Haiku 4.5; Sonnet 5 control sessions of click/vue-core reported
  separately).
- `grown`: leave-task-out with more memory. 5 task-grouped folds per repository over every
  unsteered Haiku session (train, control, agents-md arms); test = the control sessions of the
  held-out fold's tasks.
- `loro`: leave-one-repository-out. Train = every unsteered Haiku session of the other four
  repositories (the node vocabulary must recur in >= 2 repositories); test = the target
  repository's held-out control sessions.

Usage: python3 run.py <setting> [alphabet ...]   -> results/<setting>.json
"""

from __future__ import annotations

import json
import math
import sys
import time
from pathlib import Path

import data
import evaluate as ev
import mdl

HERE = Path(__file__).resolve().parent
OUT = HERE / "results"
GAP = 8
ALPHABETS = ["literal", "full", "key", "emb@0.9", "emb@0.8", "emb@0.7", "emb@0.6"]
UNSTEERED = {"train", "control", "agents-md"}


def smin_for(n: int) -> int:
    return max(3, math.ceil(0.1 * n))


def run_one(train, test, kind, group="task", min_groups=2, max_cands=3000):
    t0 = time.time()
    lab = ev.make(kind, train, group=group, min_groups=min_groups)
    X = [lab.labels(s) for s in train]
    Y = [lab.labels(s) for s in test]
    smin = smin_for(len(X))
    P = mdl.closed(mdl.mine(X, smin, GAP))
    ct = mdl.CodeTable(X, GAP).fit(P, max_cands=max_cands)
    H = ct.heldout_bits(Y)
    table = ct.table
    # the frequent (non-MDL) alternative: the 20 closed patterns with the highest support x length
    freq = sorted(P, key=lambda p: (-len(P[p]) * (len(p) - 1), p))[:20]
    use_ct = ev.usefulness(table, Y, GAP)
    use_fr = ev.usefulness(freq, Y, GAP)
    dirs_ct = [(pre, suf) for p in table for pre, suf in ev.splits(p) if not suf[0].endswith("!")]
    dirs_fr = [(pre, suf) for p in freq for pre, suf in ev.splits(p) if not suf[0].endswith("!")]
    rep_ct = ev.replay(dirs_ct, test, Y, GAP)
    rep_fr = ev.replay(dirs_fr, test, Y, GAP)
    rep_null = ev.replay(dirs_ct, test, Y, GAP, at_start=True)
    comp = [ev.compile_suffix(lab, train, suf) for _, suf in dirs_ct]
    unk = sum(a.startswith("?") for y in Y for a in y) / max(1, sum(len(y) for y in Y))
    pats = []
    for p in table:
        pats.append({"labels": list(p), "readable": [lab.describe(a, 2) for a in p],
                     "train_usage": ct.use.get(p, 0), "train_support": len(P[p]),
                     "heldout_sessions": sum(any(q == p for q, _ in h[2]) for h in H)})
    freq_read = [{"labels": list(p), "readable": [lab.describe(a, 1) for a in p], "train_support": len(P[p])}
                 for p in freq[:10]]
    return {
        "alphabet": kind, "labels": lab.k, "heldout_unknown": unk, "n_train": len(X), "n_test": len(Y),
        "smin": smin, "mined_closed": len(P), "table": len(table),
        "train_bits": [ct.base_bits, ct.bits],
        "heldout": [{"sid": s.sid, "task": s.task, "bits": h[0], "bits0": h[1], "used": len(h[2])}
                    for s, h in zip(test, H)],
        "patterns": pats, "frequent_top": freq_read,
        "use_ct": [{**{k: v for k, v in r.items() if k not in ("pattern", "prefix", "suffix")},
                    "prefix": list(r["prefix"]), "suffix": list(r["suffix"])} for r in use_ct],
        "use_freq": [{**{k: v for k, v in r.items() if k not in ("pattern", "prefix", "suffix")},
                      "prefix": list(r["prefix"]), "suffix": list(r["suffix"])} for r in use_fr],
        "replay_ct": rep_ct, "replay_freq": rep_fr, "replay_null": rep_null,
        "compile": [[list(x) for x in c] for c in comp],
        "secs": time.time() - t0,
    }


def sessions():
    return data.load()


def chrono(alphas):
    S = sessions()
    res = {}
    for r in data.BASE_REPOS:
        tr = [s for s in S if s.repo == r and s.role == "train"]
        for model in ("haiku", "sonnet"):
            te = [s for s in S if s.repo == r and s.role == "test" and s.model == model]
            if not te:
                continue
            for a in alphas:
                res[f"{r}|{model}|{a}"] = run_one(tr, te, a)
                print(r, model, a, _brief(res[f"{r}|{model}|{a}"]), flush=True)
    return res


def grown(alphas, folds=5):
    S = sessions()
    res = {}
    for r in data.BASE_REPOS:
        pool = [s for s in S if s.repo == r and s.model == "haiku" and s.arm in UNSTEERED]
        tasks = sorted({s.task for s in pool})
        for a in alphas:
            parts = []
            for f in range(folds):
                hold = set(tasks[f::folds])
                tr = [s for s in pool if s.task not in hold]
                te = [s for s in pool if s.task in hold and s.arm == "control"]
                if te:
                    parts.append(run_one(tr, te, a, max_cands=800))
            res[f"{r}|haiku|{a}"] = merge(parts)
            print(r, a, _brief(res[f"{r}|haiku|{a}"]), flush=True)
    return res


def loro(alphas):
    S = sessions()
    res = {}
    for r in data.BASE_REPOS:
        tr = [s for s in S if s.repo != r and s.model == "haiku" and s.arm in UNSTEERED]
        te = [s for s in S if s.repo == r and s.role == "test" and s.model == "haiku"]
        for a in alphas:
            res[f"{r}|haiku|{a}"] = run_one(tr, te, a, group="repo", min_groups=2, max_cands=800)
            print(r, a, _brief(res[f"{r}|haiku|{a}"]), flush=True)
    return res


def merge(parts):
    out = dict(parts[0])
    for k in ("heldout", "replay_ct", "replay_freq", "replay_null", "use_ct", "use_freq", "patterns", "compile"):
        out[k] = [x for p in parts for x in p[k]]
    for k in ("labels", "table", "mined_closed", "n_train", "n_test", "secs"):
        out[k] = [p[k] for p in parts]
    out["heldout_unknown"] = sum(p["heldout_unknown"] for p in parts) / len(parts)
    return out


def _brief(r):
    b = sum(h["bits0"] - h["bits"] for h in r["heldout"])
    b0 = sum(h["bits0"] for h in r["heldout"])
    cov = sum(h["used"] > 0 for h in r["heldout"]) / max(1, len(r["heldout"]))
    net = sum(x["net"]["all"] for x in r["replay_ct"]) / max(1, len(r["replay_ct"]))
    return (f"k={r['labels']} unk={r['heldout_unknown']:.2f} table={r['table']} saved={b / max(1, len(r['heldout'])):.1f}b/s "
            f"({100 * b / max(1, b0):.1f}%) cov={cov:.2f} net_all={net:.2f} t={r['secs'] if isinstance(r['secs'], float) else sum(r['secs']):.0f}s")


if __name__ == "__main__":
    setting = sys.argv[1]
    alphas = sys.argv[2:] or ALPHABETS
    OUT.mkdir(exist_ok=True)
    res = {"chrono": chrono, "grown": grown, "loro": loro}[setting](alphas)
    p = OUT / f"{setting}.json"
    old = json.loads(p.read_text()) if p.exists() else {}
    old.update(res)
    p.write_text(json.dumps(old, default=str))
