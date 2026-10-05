"""Held-out evaluation: learned card vs contextual delivery vs the AGENTS.md baseline.

  OMP_NUM_THREADS=1 nice -n 10 python3 evaluate.py

For every repository: learn from its train split, score on its held-out control sessions (v1+v2,
Haiku 4.5). Scores are the information-sufficiency counterfactual of `facts.py` (a discovery is
obviated when the delivered text contains every atom it needed, delivered before it was needed),
in calls and $ (cumulative-context model), net of the cost of carrying the delivered text.
Hyperparameters (support m, token budget, retrieval threshold theta) are chosen
**leave-one-repo-out**: for each repository, the configuration with the best mean net $ saving on
the other four repositories' held-out sessions. Then the uptake calibration: the live agents-md
arm tells how many of the discoveries its AGENTS.md "obviates" on paper the agents still made.
"""

from __future__ import annotations

import itertools
import json
from collections import defaultdict
from pathlib import Path

import numpy as np

import facts
import kd_embed
import kd_load, os
RES = "results/strict" if os.environ.get("KD_STRICT") == "1" else "results"
import learner
from common import carry, dollars, events_for
from stats import boot

HERE = Path(__file__).resolve().parent
CTX = HERE.parents[2] / "repobench" / "context"
REPOS = ["attrs", "click", "marshmallow", "urfave-cli", "vue-core"]


def ucall(s):
    return s.cost / max(1, len(s.calls))


def score_static(s, ev, A, tokens):
    ks = set()
    for e in ev:
        if facts.covers(A, e.need):
            ks |= set(e.calls)
    saved = dollars(s, ks)
    c = carry(s, tokens, 0) if tokens else 0.0
    return dict(calls=len(ks), calls_net=len(ks) - c / ucall(s), usd=saved - c, carry_usd=c, frac_usd=(saved - c) / s.cost,
                obv=sorted(ks))


def score_ctx(s, ev, deliv, items, preemptive=False):
    item_atoms = [facts.atoms(it.text) for it in items]
    times = sorted(deliv)
    ks = set()
    for e in ev:
        A = set()
        for t in times:
            if t < e.trigger or (preemptive and t == e.trigger):
                for i in deliv[t]:
                    A |= item_atoms[i]
        if facts.covers(A, e.need):
            ks |= set(e.calls)
    c = 0.0
    tok = 0.0
    for t, ids in deliv.items():
        r = 0 if t < 0 else s.calls[t].req
        tk = sum(items[i].tokens for i in ids) + 10  # + hook wrapper
        tok += tk
        c += carry(s, tk, r)
    saved = dollars(s, ks)
    return dict(calls=len(ks), calls_net=len(ks) - c / ucall(s), usd=saved - c, carry_usd=c, frac_usd=(saved - c) / s.cost,
                tokens=tok, n_deliv=sum(len(v) for v in deliv.values()), obv=sorted(ks))


def filemap(repo):
    import subprocess, glob
    d = sorted(glob.glob(f"/tmp/adage-repobench/runs/{repo}/runs/eval-*-control/repo"))[0]
    files = subprocess.run(["git", "-C", d, "ls-files"], capture_output=True, text=True).stdout
    return "Files in this repository:\n" + files


FIXED_CARD = (0, 3000)  # the configuration LORO selects for every repository (see heldout.json "chosen")


def summarize(rows, key):
    xs = [r[key] for r in rows]
    m, ci = boot(xs, [r["task"] for r in rows])
    return dict(mean=round(m, 4), ci90=[round(ci[0], 4), round(ci[1], 4)], n=len(xs))


def main():
    S = kd_load.load()
    E = events_for(S)
    V = kd_embed.session_vecs(S)
    ctrl = {r: [s for s in S if s.repo == r and s.arm == "control"] for r in REPOS}
    mem = {r: [s for s in S if s.repo == r and s.arm == "train"] for r in REPOS}
    agents = {r: (CTX / f"{r}.md").read_text() for r in REPOS}

    grid_m = [0, 1, 2]
    grid_budget = [150, 400, 1000, 3000, 1e9]
    grid_theta = [0.6, 0.7, 0.8, 0.9]
    per = {}  # (repo, method, params) -> rows
    cards = {}

    def res_card_params(r):
        return FIXED_CARD

    def cards_for_combo(r, pc):
        return cards[(r,) + pc][0]
    for r in REPOS:
        cand = learner.candidates(mem[r], E)
        for m in grid_m:
            for b in grid_budget:
                items = [learner.Item(**{k: getattr(it, k) for k in ("kind", "need", "text", "support", "tasks", "triggers")})
                         for it in cand]
                sel = learner.select(mem[r], E, items, m=m, budget=b)
                txt = learner.card_text(sel)
                cards[(r, m, b)] = (txt, sel)
                A = facts.atoms(txt)
                rows = []
                for s in ctrl[r]:
                    d = score_static(s, E[s.sid], A, len(txt) / 4)
                    d["task"] = s.task
                    rows.append(d)
                per[(r, "card", (m, b))] = rows
            # contextual: every candidate with leave-task-out support >= m (no budget: it is keyed)
            pool = [it for it in cand if len(it.tasks) >= m + 1]
            for th in grid_theta:
                cx = learner.Contextual(pool, th)
                for pre in (False, True):
                    rows = []
                    for s in ctrl[r]:
                        d = score_ctx(s, E[s.sid], cx.deliveries(s, V[s.sid]), pool, pre)
                        d["task"] = s.task
                        rows.append(d)
                    per[(r, "ctx-pre" if pre else "ctx", (m, th))] = rows
                # the selected card's facts, delivered by situation instead of at session start
                sel = cards[(r, m, FIXED_CARD[1])][1]
                cs = learner.Contextual(sel, th)
                rows = []
                for s in ctrl[r]:
                    d = score_ctx(s, E[s.sid], cs.deliveries(s, V[s.sid]), sel, True)
                    d["task"] = s.task
                    rows.append(d)
                per[(r, "ctx-sel-pre", (m, th))] = rows
        # baselines
        A = facts.atoms(agents[r])
        per[(r, "agents-md", ())] = [dict(score_static(s, E[s.sid], A, len(agents[r]) / 4), task=s.task) for s in ctrl[r]]
        # no-learning repo map: the tracked files (`git ls-files` of a held-out checkout), Aider-style
        fm = filemap(r)
        per[(r, "filemap", ())] = [dict(score_static(s, E[s.sid], facts.atoms(fm), len(fm) / 4), task=s.task) for s in ctrl[r]]
        am = agents[r] + "\n" + fm
        per[(r, "agents-md+filemap", ())] = [dict(score_static(s, E[s.sid], facts.atoms(am), len(am) / 4), task=s.task) for s in ctrl[r]]
        pc = res_card_params(r)
        ct = cards_for_combo(r, pc) + "\n" + fm
        per[(r, "card+filemap", ())] = [dict(score_static(s, E[s.sid], facts.atoms(ct), len(ct) / 4), task=s.task) for s in ctrl[r]]
        # oracle ceilings
        trainA = set()
        for s in mem[r]:
            for e in E[s.sid]:
                trainA |= e.need
        per[(r, "oracle-train", ())] = [dict(score_static(s, E[s.sid], trainA, 0), task=s.task) for s in ctrl[r]]
        per[(r, "oracle-all", ())] = [dict(score_static(s, E[s.sid], set().union(*[e.need for e in E[s.sid]] or [set()]), 0),
                                           task=s.task) for s in ctrl[r]]
        print(r, "done", flush=True)

    # leave-one-repo-out selection of hyperparameters per method
    def mean_usd(rows):
        return float(np.mean([x["usd"] for x in rows]))

    res = {"per_repo": {}, "pooled": {}, "chosen": {}}
    methods = ["card", "ctx", "ctx-pre", "ctx-sel-pre"]
    pooled = defaultdict(list)
    for r in REPOS:
        res["per_repo"][r] = {}
        for meth in methods:
            params = sorted({p for (rr, mm, p) in per if mm == meth})
            best = max(params, key=lambda p: np.mean([mean_usd(per[(o, meth, p)]) for o in REPOS if o != r]))
            res["chosen"].setdefault(meth, {})[r] = best
            rows = per[(r, meth, best)]
            pooled[meth] += rows
            res["per_repo"][r][meth] = {k: summarize(rows, k) for k in ("calls", "calls_net", "usd", "frac_usd")}
            if meth == "card":
                res["per_repo"][r][meth]["card_tokens"] = len(cards[(r,) + best][0]) / 4
                res["per_repo"][r][meth]["n_facts"] = len(cards[(r,) + best][1])
        for meth in ("agents-md", "filemap", "agents-md+filemap", "card+filemap", "oracle-train", "oracle-all"):
            rows = per[(r, meth, ())]
            pooled[meth] += rows
            res["per_repo"][r][meth] = {k: summarize(rows, k) for k in ("calls", "calls_net", "usd", "frac_usd")}
        # the learned card at each repo's own best params too (in-repo tuned, optimistic)
    for meth, rows in pooled.items():
        res["pooled"][meth] = {k: summarize(rows, k) for k in ("calls", "calls_net", "usd", "frac_usd")}
        res["pooled"][meth]["usd_share"] = round(sum(x["usd"] for x in rows) /
                                                 sum(s.cost for r in REPOS for s in ctrl[r]), 4)
    # fixed-parameter pooled table for every configuration (not LORO), for transparency
    res["grid"] = {f"{meth}{p}": round(float(np.mean([x["usd"] for r in REPOS for x in per[(r, meth, p)]])), 5)
                   for (_, meth, p) in per if meth in methods}
    # cards as delivered (LORO-chosen params)
    Path(Path(RES) / "cards").mkdir(parents=True, exist_ok=True)
    for r in REPOS:
        p = res["chosen"]["card"][r]
        (Path(RES) / "cards" / f"{r}.md").write_text(cards[(r,) + p][0] or "(empty)\n")
    json.dump(res, open(Path(RES) / "heldout.json", "w"), indent=1, default=str)
    # keep per-session rows for the calibration step
    import pickle
    pickle.dump({"per": {f"{k[0]}|{k[1]}|{k[2]}": v for k, v in per.items()}, "chosen": res["chosen"]},
                open(kd_load.CACHE / ("per-strict.pkl" if os.environ.get("KD_STRICT") == "1" else "per.pkl"), "wb"))
    print(json.dumps(res["pooled"], indent=1))
    print(json.dumps(res["chosen"], indent=1, default=str))


if __name__ == "__main__":
    main()
