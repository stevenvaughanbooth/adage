"""90% bootstrap CIs for pair F1 on the gold test half (resampling gold commands; in-repo fit)."""
import json

import numpy as np

import common as C
import data
import eval_gold as E
import methods as M


def pair_f1(gold, pred, ids=None):
    g = gold[:, None] == gold[None, :]
    p = pred[:, None] == pred[None, :]
    iu = np.triu_indices(len(gold), 1)
    g, p = g[iu], p[iu]
    if ids is not None:  # a resampled item paired with its own copy is not a pair
        keep = (ids[:, None] != ids[None, :])[iu]
        g, p = g[keep], p[keep]
    tp = (g & p).sum()
    P = tp / max(1, p.sum())
    R = tp / max(1, g.sum())
    return 2 * P * R / max(1e-9, P + R)


def main(B=400):
    S = C.sessions()
    items = E.gold()
    test = [i for i, x in enumerate(items) if x["half"] == "test"]
    cmds = [items[i]["cmd"] for i in test]
    gl = {c: k for k, c in enumerate(sorted({x["gold"] for x in items}))}
    gold = np.array([gl[items[i]["gold"]] for i in test])
    taus = json.load(open(data.HERE / "results" / "taus.json"))
    labs = {k: r.labeller() for k, r in E.methods_list(S).items() if k in ("taxonomy", "intent", "head", "headsub", "lgg-loose")}
    calls = C.fit_calls(S)
    labs["lgg-H"] = M.LearnedLGG("H").fit(calls).labeller()
    for n in ("a2v", "hybrid", "hybrid-first", "rrr-first", "tfidf"):
        labs[n] = C.clustered(n, S).labeller(taus[n])
    rng = np.random.default_rng(11)
    out = {}
    for n, lab in labs.items():
        p = lab(cmds)
        pm = {v: k for k, v in enumerate(sorted(set(p)))}
        pred = np.array([pm[v] for v in p])
        f = pair_f1(gold, pred)
        bs = []
        for _ in range(B):
            ii = rng.integers(0, len(test), len(test))
            bs.append(pair_f1(gold[ii], pred[ii], ii))
        out[n] = [float(f), float(np.percentile(bs, 5)), float(np.percentile(bs, 95))]
        print(n, [round(x, 3) for x in out[n]])
    json.dump(out, open(data.HERE / "results" / "gold_ci.json", "w"), indent=1)


if __name__ == "__main__":
    main()
