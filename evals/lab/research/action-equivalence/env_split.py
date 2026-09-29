"""The question's own example: does a representation put `pytest …` in the project's environment
with `.venv/bin/python -m pytest …`, and apart from `python -m pytest` under the system Python?

On the gold set (all 350 items; the classes are hand labels): among Python test runs,
  same-env recall   share of (proj, proj) and (sys, sys) pairs predicted equivalent
  cross-env split   share of (proj, sys) pairs predicted NOT equivalent
plus the same two numbers for Python repro runs (run:py:proj / run:py:sys).
"""
import json
from itertools import combinations

import common as C
import data
import eval_gold as E
import methods as M


def stats(items, pred, a, b):
    ia = [i for i, x in enumerate(items) if x["gold"] == a]
    ib = [i for i, x in enumerate(items) if x["gold"] == b]
    same = [(i, j) for grp in (ia, ib) for i, j in combinations(grp, 2)]
    cross = [(i, j) for i in ia for j in ib]
    return {"same_env_recall": sum(pred[i] == pred[j] for i, j in same) / len(same),
            "cross_env_split": sum(pred[i] != pred[j] for i, j in cross) / len(cross),
            "pairs": [len(same), len(cross)]}


def main():
    S = C.sessions()
    items = E.gold()
    cmds = [x["cmd"] for x in items]
    taus = json.load(open(data.HERE / "results" / "taus.json"))
    labs = {k: r.labeller() for k, r in E.methods_list(S).items()}
    calls = C.fit_calls(S)
    for lv in ("L", "H"):
        m = M.LearnedLGG(lv).fit(calls)
        labs[m.name] = m.labeller()
    for n, t in taus.items():
        labs[n] = C.clustered(n, S).labeller(t)
    out = {}
    for n, lab in labs.items():
        p = lab(cmds)
        out[n] = {"test": stats(items, p, "test:py:proj", "test:py:sys"), "repro": stats(items, p, "run:py:proj", "run:py:sys")}
        print(f"{n:10s} test same {out[n]['test']['same_env_recall']:.2f} split {out[n]['test']['cross_env_split']:.2f} | "
              f"repro same {out[n]['repro']['same_env_recall']:.2f} split {out[n]['repro']['cross_env_split']:.2f}")
    json.dump(out, open(data.HERE / "results" / "env_split.json", "w"), indent=1)


if __name__ == "__main__":
    main()
