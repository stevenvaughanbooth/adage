"""Intrinsic evaluation against the hand-labelled gold set (the measurement instrument).

Gold: 350 held-out control-arm Bash commands, 70 per repository, labelled by hand into 18
functional classes (README §Gold). Pairs: every pair among them (61,075), and a fixed 300-pair
stratified sample (150 equivalent, 75 hard non-equivalent sharing a head word, 75 random).

Protocol. Gold items are split into two halves by id parity: `dev` (tunes the similarity
threshold tau of the clustered methods) and `test` (reported).
  in-repo:  representations fitted on the non-control sessions of all five repositories.
  LORO:     fitted without the held-out repository's sessions; tau tuned on the other
            repositories' dev items; reported on the held-out repository's test items (pairs
            within it, and pairs between it and the other repositories' test items).
"""
from __future__ import annotations

import json
import random
import sys
import time
from collections import Counter
from itertools import combinations

import numpy as np

import common as C
import data
import methods as M

OUT = data.HERE / "results"
OUT.mkdir(exist_ok=True)


def gold():
    items = json.load(open(data.HERE / "gold_commands.json"))
    lab = dict(l.rstrip("\n").split("\t") for l in open(data.HERE / "gold_labels.tsv") if not l.startswith("id\t"))
    for x in items:
        x["gold"] = lab[x["id"]]
        x["half"] = "dev" if int(x["id"][2:]) % 2 == 0 else "test"
    return items


def pair_stats(items, pred, pairs=None):
    tp = fp = fn = tn = 0
    it = pairs if pairs is not None else combinations(range(len(items)), 2)
    for i, j in it:
        g = items[i]["gold"] == items[j]["gold"]
        p = pred[i] == pred[j]
        tp += g and p
        fp += p and not g
        fn += g and not p
        tn += not g and not p
    P = tp / (tp + fp) if tp + fp else 0.0
    R = tp / (tp + fn) if tp + fn else 0.0
    F = 2 * P * R / (P + R) if P + R else 0.0
    return {"P": P, "R": R, "F1": F, "acc": (tp + tn) / max(1, tp + fp + fn + tn), "n": tp + fp + fn + tn}


def purity(items, pred):
    n = len(items)
    by_p, by_g = {}, {}
    for x, p in zip(items, pred):
        by_p.setdefault(p, Counter())[x["gold"]] += 1
        by_g.setdefault(x["gold"], Counter())[p] += 1
    pur = sum(c.most_common(1)[0][1] for c in by_p.values()) / n
    inv = sum(c.most_common(1)[0][1] for c in by_g.values()) / n
    return {"purity": pur, "inv_purity": inv, "F_purity": 2 * pur * inv / (pur + inv), "clusters": len(by_p)}


def sample_pairs(items, seed=3):
    rng = random.Random(seed)
    idx = list(range(len(items)))
    allp = list(combinations(idx, 2))
    pos = [p for p in allp if items[p[0]]["gold"] == items[p[1]]["gold"]]
    neg = [p for p in allp if items[p[0]]["gold"] != items[p[1]]["gold"]]
    head = lambda i: M._head(items[i]["cmd"])
    hard = [p for p in neg if head(p[0]) == head(p[1])]
    out = rng.sample(pos, 150) + rng.sample(hard, 75) + rng.sample(neg, 75)
    with open(data.HERE / "gold_pairs.tsv", "w") as f:
        f.write("id_a\tid_b\tequivalent\tkind\n")
        for k, (i, j) in enumerate(out):
            kind = "pos" if k < 150 else ("hard-neg" if k < 225 else "neg")
            f.write(f"{items[i]['id']}\t{items[j]['id']}\t{int(items[i]['gold'] == items[j]['gold'])}\t{kind}\n")
    return out


def methods_list(S):
    tabs = C.baseline_tables(S)
    rules = dict(M.RULES)
    for k in ("taxonomy", "intent", "lgg-loose"):
        rules[k] = C.table_rule(k, tabs[k])
    return rules


def evaluate(items, pred, sel):
    """Metrics over the items in `sel` (indices)."""
    sub = [items[i] for i in sel]
    pp = [pred[i] for i in sel]
    return {**pair_stats(sub, pp), **purity(sub, pp)}


def best_tau(labeller_at, items, sel):
    best = None
    for tau in M.TAUS:
        pred = labeller_at(tau)([x["cmd"] for x in items])
        f = pair_stats([items[i] for i in sel], [pred[i] for i in sel])["F1"]
        if best is None or f > best[1]:
            best = (tau, f)
    return best[0]


def run():
    S = C.sessions()
    items = gold()
    pairs300 = sample_pairs(items)
    test = [i for i, x in enumerate(items) if x["half"] == "test"]
    dev = [i for i, x in enumerate(items) if x["half"] == "dev"]
    rules = methods_list(S)
    cmds = [x["cmd"] for x in items]
    res = {"in_repo": {}, "loro": {}, "pairs300": {}, "fit_seconds": {}, "tau": {}}

    # ---- in-repo -------------------------------------------------------------------------
    calls = C.fit_calls(S)
    fitted = {}
    for name, r in rules.items():
        pred = r.labeller()(cmds)
        res["in_repo"][name] = evaluate(items, pred, test)
        res["pairs300"][name] = pair_stats(items, pred, pairs300)
    for name, cls in M.EMBEDDERS.items():
        t0 = time.time()
        m = C.clustered(name, S)
        res["fit_seconds"][name] = time.time() - t0
        fitted[name] = m
        tau = best_tau(m.labeller, items, dev)
        res["tau"][name] = tau
        pred = m.labeller(tau)(cmds)
        res["in_repo"][name] = evaluate(items, pred, test)
        res["pairs300"][name] = pair_stats(items, pred, pairs300)
        print(name, tau, round(res["in_repo"][name]["F1"], 3), f"{res['fit_seconds'][name]:.0f}s", flush=True)
    json.dump(res["tau"], open(OUT / "taus.json", "w"))  # read by downstream.py and ceiling.py
    for lv in ("L", "H"):
        m = M.LearnedLGG(lv).fit(calls)
        pred = m.labeller()(cmds)
        res["in_repo"][m.name] = evaluate(items, pred, test)
        res["pairs300"][m.name] = pair_stats(items, pred, pairs300)

    # ---- leave-one-repo-out --------------------------------------------------------------
    for repo in data.REPOS:
        calls = C.fit_calls(S, exclude_repo=repo)
        rin = [i for i in test if items[i]["repo"] == repo]
        rout = [i for i in test if items[i]["repo"] != repo]
        dev_o = [i for i in dev if items[i]["repo"] != repo]
        cross = [(i, j) for i in rin for j in rout]
        within = list(combinations(rin, 2))

        def rec(name, pred):
            d = res["loro"].setdefault(name, {})
            d[repo] = {"within": pair_stats(items, pred, within), "cross": pair_stats(items, pred, cross),
                       **purity([items[i] for i in rin], [pred[i] for i in rin])}

        for name, r in rules.items():
            rec(name, r.labeller()(cmds))
        for lv in ("L", "H"):
            m = M.LearnedLGG(lv).fit(calls)
            rec(m.name, m.labeller()(cmds))
        for name, cls in M.EMBEDDERS.items():
            m = C.clustered(name, S, repo)
            tau = best_tau(m.labeller, items, dev_o)
            rec(name, m.labeller(tau)(cmds))
        print("loro", repo, flush=True)
    json.dump(res, open(OUT / "gold_eval.json", "w"), indent=1)
    return res


def table(res):
    rows = ["| method | pair P | pair R | **pair F1** | purity | inv. purity | clusters | 300-pair acc | LORO within F1 | LORO cross F1 |", "|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|"]
    order = sorted(res["in_repo"], key=lambda k: -res["in_repo"][k]["F1"])
    for k in order:
        a = res["in_repo"][k]
        lo = res["loro"].get(k, {})
        # LORO pooled over repositories: micro-average of pair counts is not stored; report mean F1
        w = np.mean([v["within"]["F1"] for v in lo.values()]) if lo else float("nan")
        c = np.mean([v["cross"]["F1"] for v in lo.values()]) if lo else float("nan")
        rows.append(f"| {k} | {a['P']:.2f} | {a['R']:.2f} | **{a['F1']:.2f}** | {a['purity']:.2f} | {a['inv_purity']:.2f} | {a['clusters']} | {res['pairs300'][k]['acc']:.2f} | {w:.2f} | {c:.2f} |")
    return "\n".join(rows)


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "table":
        print(table(json.load(open(OUT / "gold_eval.json"))))
    else:
        r = run()
        print(table(r))
