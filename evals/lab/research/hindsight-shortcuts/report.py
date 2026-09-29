#!/usr/bin/env python3
"""Print the README's tables from results.json, breakdown.json and harness_results.json."""

from __future__ import annotations

import json
from pathlib import Path

HERE = Path(__file__).resolve().parent


def f(v, pct=False):
    if not v or v.get("mean") is None:
        return "–"
    m = 100 if pct else 1
    u = "%" if pct else ""
    return f"{m * v['mean']:+.2f}{u} [{m * v['lo']:+.2f}, {m * v['hi']:+.2f}]" if not pct else \
        f"{m * v['mean']:+.1f}% [{m * v['lo']:+.1f}, {m * v['hi']:+.1f}]"


def prec(v):
    p = v.get("precision") or {}
    return f"{p['p']:.2f} [{p['lo']:.2f}, {p['hi']:.2f}] (n={p['n']})" if p else "–"


def own_table(rows: dict, pols, names=None):
    out = ["| policy | calls saved/session (λ=0) | net calls if a misfire costs 2 | $ saved, share of session $ | "
           "precision [Wilson 90%] | shortcuts/session |", "| --- | --- | --- | --- | --- | ---: |"]
    for p in pols:
        v = rows.get(p)
        if not v:
            continue
        out.append(f"| {(names or {}).get(p, p)} | {f(v.get('net_calls') or v.get('calls_saved'))} | "
                   f"{f(v.get('net_calls_l2'))} | {f(v.get('pct_cost'), True)} | {prec(v)} | "
                   f"{v.get('per_session', '–')} |")
    return "\n".join(out)


NAMES = {"knn|0.0": "learned kNN matcher, θ tuned for λ=0", "knn|2.0": "learned kNN matcher, θ tuned for λ=2",
         "prior|0.0": "frequency prior (at session start), λ=0", "prior|2.0": "frequency prior, λ=2",
         "knn@0.3": "kNN, fixed θ=0.3", "knn@0.5": "kNN, fixed θ=0.5", "knn@0.7": "kNN, fixed θ=0.7",
         "ceiling": "**ceiling: perfect situation matcher** (library, support ≥ 2)",
         "oracle": "**absolute hindsight oracle** (every shortcut, library or not)"}


def harness_line(name, j):
    s = j.get("summary", {})
    g = lambda k: s.get(k) or {}
    def ci(k, pct=False):
        v = g(k)
        if not v:
            return "–"
        if isinstance(v, dict) and "mean" in v:
            return f(v, pct)
        return str(v)
    return f"| {name} | {s.get('sessions', '–')} / {s.get('tasks', '–')} | {ci('usd')} | {ci('usd_share', True)} | {ci('calls')} | {s.get('directives', '–')} |"


def main():
    r = json.loads((HERE / "results.json").read_text())
    pols = ["knn|0.0", "knn|2.0", "prior|0.0", "prior|2.0", "knn@0.3", "knn@0.5", "knn@0.7", "ceiling", "oracle"]
    for g in ("haiku_all", "haiku_v2", "adage_arm_v2", "sonnet"):
        print(f"\n#### {g}\n")
        print(own_table(r["main"][g], pols, NAMES))
    print("\n#### per repository (Haiku, v1+v2)\n")
    print("| repo | learned kNN λ=0 | learned kNN λ=2 | prior λ=0 | ceiling | oracle | ceiling share of $ |")
    print("| --- | --- | --- | --- | --- | --- | --- |")
    for rp in ("attrs", "click", "marshmallow", "urfave-cli", "vue-core"):
        v = r["main"][f"repo:{rp}"]
        print(f"| {rp} | {f(v['knn|0.0']['net_calls'])} | {f(v['knn|2.0']['net_calls'])} | "
              f"{f(v['prior|0.0']['net_calls'])} | {f(v['ceiling']['calls_saved'])} | {f(v['oracle']['calls_saved'])} | "
              f"{f(v['ceiling']['pct_cost'], True)} |")
    print("\n#### leave-one-repo-out\n")
    print("| held-out repo | learned kNN λ=0 | … precision | learned kNN λ=2 | ceiling (other repos' library) | ceiling share of $ |")
    print("| --- | --- | --- | --- | --- | --- |")
    for rp, v in r["loro"].items():
        print(f"| {rp} | {f(v['knn|0.0']['net_calls'])} | {prec(v['knn|0.0'])} | {f(v['knn|2.0']['net_calls'])} | "
              f"{f(v['ceiling']['calls_saved'])} | {f(v['ceiling']['pct_cost'], True)} |")
    print("\n#### learning curve (Haiku, 158 sessions; n=2, 5: 5 random draws pooled)\n")
    print("| memory | learned kNN λ=0 | share of $ | precision | learned kNN λ=2 | prior λ=0 | ceiling | ceiling share of $ |")
    print("| --- | --- | --- | --- | --- | --- | --- | --- |")
    for n in ("2", "5", "10", "all"):
        v = r["curve"][n]
        print(f"| {n} | {f(v['knn|0.0']['net_calls'])} | {f(v['knn|0.0']['pct_cost'], True)} | {prec(v['knn|0.0'])} | "
              f"{f(v['knn|2.0']['net_calls'])} | {f(v['prior|0.0']['net_calls'])} | {f(v['ceiling']['calls_saved'])} | "
              f"{f(v['ceiling']['pct_cost'], True)} |")
    b = HERE / "breakdown.json"
    if b.exists():
        d = json.loads(b.read_text())
        print("\n#### where the hindsight waste is (Haiku, 158 held-out control sessions)\n")
        print("| | calls/session | share of calls | share of $ |")
        print("| --- | --- | --- | --- |")
        print(f"| every non-achieving call (hindsight waste) | {f(d['waste'])} | {f(d['waste_pct_calls'], True)} | {f(d['waste_usd_pct_cost'], True)} |")
        for k, nm in (("cmd", "command / procedure shortcuts"), ("file", "file shortcuts"), ("fact", "fact shortcuts"),
                      ("all", "all shortcut kinds (the lever's absolute oracle)")):
            print(f"| {nm} | {f(d['oracle_' + k])} | {f(d['oracle_' + k + '_pct_calls'], True)} | "
                  f"{f(d['oracle_' + k + '_usd_pct_cost'], True)} |")
        print("\n| library (perfect matcher) | command | file | fact | all | share of $ |")
        print("| --- | --- | --- | --- | --- | --- |")
        for nm, o in d["library"].items():
            print(f"| {nm} | {f(o['cmd'])} | {f(o['file'])} | {f(o['fact'])} | {f(o['all'])} | {f(o['all_pct_cost'], True)} |")
    h = HERE / "harness_results.json"
    if h.exists():
        j = json.loads(h.read_text())
        print("\n#### common harness\n")
        for k, v in j.items():
            if isinstance(v, dict) and "summary" in v:
                print(k, json.dumps(v["summary"])[:600])


if __name__ == "__main__":
    main()
