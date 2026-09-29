"""What the miner would see: support for "after a source edit → <next test run>" at each abstraction.

Per repository, over the 7–10 *training* sessions a learner gets (RepoBench's `train` phase): for
every session with a successful edit of a source file, the label of the first test run within
G = 8 events after an edit. The best continuation per method is the label most sessions share;
`support` is that session count, `tasks` its distinct tasks, `rate` = support / sessions with a
source edit, `wilson` its one-sided 90% Wilson lower bound. Adage's gates: support ≥ 5
(`min_support`), rate ≥ 0.5 (`prime_rate`), ≥ 2 tasks.
"""
from __future__ import annotations

import collections
import json
import math
import os

import corpus
import evaluate as E

G_GAP = 8


def wilson_lb(k, n, z=1.2816):
    if n == 0:
        return 0.0
    p = k / n
    d = 1 + z * z / n
    return (p + z * z / (2 * n) - z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n))) / d


def main():
    L = corpus.load()
    sessions = E.build_items(L)
    methods = ["literal", "argv", "headsub", "lgg-strict", "lgg-loose", "grammar-L1", "grammar-L2", "grammar-L3", "grammar-L3e", "combo", "intent"]
    out = {}
    for repo in sorted({s["repo"] for s in sessions}):
        tr = [s for s in sessions if s["repo"] == repo and s["phase"] == "train"]
        out[repo] = {}
        for m in methods:
            sup, tasks, n_edit = collections.Counter(), collections.defaultdict(set), 0
            for s in tr:
                recs = s["recs"]
                labs = set()
                had = False
                for i, r in enumerate(recs):
                    if not r["bash"] and r["shared"] == "edit:src" and r["outcome"] != "fail":
                        had = True
                        for r2 in recs[i + 1:i + 1 + G_GAP]:
                            if r2["bash"] and r2["act"].role == "test-run":
                                labs.add(r2[m])
                                break
                n_edit += had
                for lb in labs:
                    sup[lb] += 1
                    tasks[lb].add(s["task"])
            if sup:
                lb, k = max(sup.items(), key=lambda x: (x[1], len(tasks[x[0]]), x[0]))
                out[repo][m] = {"label": lb[:90], "support": k, "tasks": len(tasks[lb]), "sessions_with_edit": n_edit,
                                "rate": k / max(1, n_edit), "wilson": wilson_lb(k, n_edit),
                                "passes_gates": k >= 5 and k / max(1, n_edit) >= 0.5 and len(tasks[lb]) >= 2}
            else:
                out[repo][m] = {"label": None, "support": 0, "tasks": 0, "sessions_with_edit": n_edit, "rate": 0.0, "wilson": 0.0, "passes_gates": False}
    here = os.path.dirname(os.path.abspath(__file__))
    with open(os.path.join(here, "results", "miner_support.json"), "w") as f:
        json.dump(out, f, indent=1)
    print("| Method | " + " | ".join(out) + " |")
    print("|---|" + "---|" * len(out))
    for m in methods:
        cells = []
        for repo in out:
            x = out[repo][m]
            cells.append(f"{x['support']}/{x['sessions_with_edit']} ({x['tasks']} tasks){' ✓' if x['passes_gates'] else ''}")
        print(f"| {m} | " + " | ".join(cells) + " |")
    print()
    for repo in out:
        print(repo, "grammar-L3:", out[repo]["grammar-L3"]["label"], "| lgg-loose:", out[repo]["lgg-loose"]["label"])


if __name__ == "__main__":
    main()
