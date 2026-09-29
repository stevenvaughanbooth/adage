#!/usr/bin/env python3
"""Markdown tables from `results/lab.json` and `results/engine.json` (pasted into README.md).

  python3 evals/lab/report.py [--lab results/lab.json] [--engine results/engine.json]
"""

from __future__ import annotations

import argparse
import collections
import json
import sys
from pathlib import Path

import traces

LAB = traces.LAB
REPOS = traces.REPOS


def f(x, nd=2):
    if x is None:
        return "–"
    if isinstance(x, float):
        return f"{x:.{nd}f}"
    return str(x)


def table(head, rows):
    out = ["| " + " | ".join(head) + " |", "|" + "|".join(" --- " if i == 0 else " ---: " for i in range(len(head))) + "|"]
    out += ["| " + " | ".join(str(c) for c in r) + " |" for r in rows]
    return "\n".join(out)


def main(argv=None) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--lab", default=str(LAB / "results" / "lab.json"))
    ap.add_argument("--engine", default=str(LAB / "results" / "engine.json"))
    a = ap.parse_args(argv)
    L = json.loads(Path(a.lab).read_text())
    E = json.loads(Path(a.engine).read_text()) if Path(a.engine).exists() else {}

    print("### Data\n")
    print(table(["repo", "train sessions", "train situations", "held-out sessions (control)", "held-out decision points"],
                [[r, v["train_sessions"], v["train_situations"], v["test_sessions"], v["test_situations"]]
                 for r, v in L["split"].items()]))

    print("\n### Models\n")
    rows = []
    for m, v in L["latency"].items():
        if m.startswith("knn-"):
            continue
        rows.append([m, v["licence"], f(v.get("size_mb"), 1), v.get("dim") or "sparse", f(v["median_ms"], 3),
                     f(v["p95_ms"], 3)])
    print(table(["model", "licence", "size MB", "dim", "embed 1 text median ms", "p95 ms"], rows))
    rows = [[k, f(v["median_ms"], 3), f(v["p95_ms"], 3)] for k, v in L["latency"].items() if k.startswith("knn-")]
    print()
    print(table(["brute-force kNN (numpy, 1 query, top-10)", "median ms", "p95 ms"], rows))

    print("\n### (a) Next-action top-1 accuracy (held-out control sessions; key / template / full)\n")
    names = [n for n in L["per_repo"][REPOS[0]] if n.startswith(("order-", "knn-acc"))]
    rows = []
    for n in names:
        row = [n]
        for r in REPOS:
            acc = L["per_repo"][r][n]["acc"]
            row.append(f"{acc['key']:.2f} / {acc['template']:.2f} / {acc['full']:.2f}")
        rows.append(row)
    print(table(["method"] + REPOS, rows))
    if E:
        print("\nCurrent Rust engine (`adage eval mine`, same split, its own event alphabet over exec/read/edit"
              " positions): engine / its order-1 / its order-2\n")
        rows = [[r, f(E[r]["mine"]["top1_engine"]), f(E[r]["mine"]["top1_baseline"]), f(E[r]["mine"]["top1_order2"]),
                 E[r]["mine"]["positions"]] for r in REPOS]
        print(table(["repo", "engine top-1", "order-1", "order-2", "positions"], rows))
        rows = []
        for n in names:
            row = [n]
            for r in REPOS:
                acc = L["per_repo"][r][n]["acc_exec"]
                row.append(f"{acc['template']:.2f} (n={acc['n']})")
            rows.append(row)
        print("\nLab methods restricted to command (non-exploratory Bash) positions, template level:\n")
        print(table(["method"] + REPOS, rows))

    print("\n### (b) Useful directives at the threshold tuned on attrs+click+marshmallow\n")
    print("Tuned operating points (pooled over the 3 tuning repos):\n")
    rows = []
    for n, sel in L["selected"].items():
        if not sel.get("theta"):
            continue
        rows.append([n, f(sel["theta"][0]), sel["theta"][1]])
    print(table(["method", "theta", "precision target reached"], rows))
    for r in REPOS:
        print(f"\n**{r}**" + (" (held out from tuning)" if r in ("urfave-cli", "vue-core") else " (tuning repo)") + "\n")
        rows = []
        for n, v in L["per_repo"][r].items():
            c = v.get("at_tuned_theta")
            if not c:
                continue
            rows.append([n, f(v["theta"]), f(c["issued_per_session"]), f(c["precision"]), f(c["precision_strict"]),
                         f(c["correct_rate"]), f(c["recall"], 3), f(c["saved_fail_per_session"]),
                         f(c["saved_discovery_per_session"]), f(c["saved_per_session"]), f(c["steps_per_session"], 1)])
        if E:
            rows.append(["Rust engine replay (default and demo thresholds)", "–", "0.00", "–", "–", "–", "0.000",
                         "0.00", "0.00", "0.00", "–"])
        print(table(["method", "θ", "directives/session", "useful precision", "strict precision (dead end in window)",
                     "correct (useful+redundant)", "recall", "failed cmds avoided/session",
                     "discovery turns saved/session", "window upper bound/session", "steps/session"], rows))

    print("\n### What the directives were (held-out sessions, at the tuned θ)\n")
    rows = []
    for r in REPOS:
        for n in [k for k in L["per_repo"][r] if k.startswith(("baseline-dir", "knn-dir:struct", "knn-dir:potion-8M"))]:
            ex = L["per_repo"][r][n].get("examples") or []
            c = collections.Counter(e["directive"] for e in ex)
            u = collections.Counter(e["directive"] for e in ex if e["kind"] == "useful")
            rows.append([r, n, "; ".join(f"`{k.removeprefix('Bash:')}` {u[k]}/{v}" for k, v in c.most_common(3))])
    print(table(["repo", "method", "top directives (useful/issued)"], rows))

    print("\n### Grown memory (training + other tasks' held-out sessions, leave-task-out)\n")
    rows = []
    for r, g in L["grown"].items():
        k, b = g["knn"]["at_tuned_theta"] or {}, g["baseline"]["at_tuned_theta"] or {}
        rows.append([r, g["memory_sessions"], f(g["knn"]["acc"]["template"]), f(k.get("issued_per_session")),
                     f(k.get("precision")), f(k.get("saved_discovery_per_session")), f(b.get("issued_per_session")),
                     f(b.get("precision")), f(b.get("saved_discovery_per_session"))])
    print(table(["repo", "sessions in repo", "kNN acc (template)", "kNN directives/session", "kNN precision",
                 "kNN discovery saved/session", "baseline directives/session", "baseline precision",
                 "baseline discovery saved/session"], rows))

    print("\n### Cross-repository memory (held-out repos, memory = the other 4 repos' training traces)\n")
    rows = []
    for r, v in L["cross_repo"].items():
        c = v["at_tuned_theta"] or {}
        rows.append([r, f(v["acc"]["template"]), f(c.get("issued_per_session")), f(c.get("precision")),
                     f(c.get("saved_discovery_per_session"))])
    print(table(["repo", "acc (template)", "directives/session", "precision", "discovery saved/session"], rows))

    print("\n### Chosen configuration\n")
    print("```json\n" + json.dumps(L["chosen"], indent=1) + "\n```")
    return 0


if __name__ == "__main__":
    sys.exit(main())
