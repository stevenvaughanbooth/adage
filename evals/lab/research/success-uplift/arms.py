"""Live arm effects on pass rate (hidden tests): adage vs control and agents-md vs control, per
bench x repo x model, with Wilson intervals per arm, Newcombe intervals for the difference, and a
pooled task-stratified Mantel-Haenszel difference with a task-cluster bootstrap (90%).

RepoBench v2 has no agents-md arm; agents-md vs control is therefore from v1 (30-turn cap,
Claude Code of the same generation), reported as such.
"""

from __future__ import annotations

import json
from collections import defaultdict

from load import load
from stats_util import boot_mh, newcombe, wilson


def main():
    S = [s for s in load() if s["phase"] == "eval"]
    out = {"per_slice": [], "pooled": {}}
    cells = defaultdict(lambda: [0, 0])
    for s in S:
        c = cells[(s["bench"], s["repo"], s["model"], s["arm"])]
        c[0] += s["passed"]; c[1] += 1
    slices = sorted({k[:3] for k in cells})
    print(f"{'bench':4} {'repo':12} {'model':8} {'arm':9} {'pass':>9}  {'wilson90':>13}  diff vs control [newcombe90]")
    for sl in slices:
        k0, n0 = cells.get(sl + ("control",), (0, 0))
        for arm in ("control", "adage", "agents-md"):
            if sl + (arm,) not in cells:
                continue
            k, n = cells[sl + (arm,)]
            lo, hi = wilson(k, n)
            row = dict(bench=sl[0], repo=sl[1], model=sl[2], arm=arm, k=k, n=n, rate=round(k / n, 3),
                       wilson90=[round(lo, 3), round(hi, 3)])
            msg = ""
            if arm != "control" and n0:
                d, dl, dh = newcombe(k, n, k0, n0)
                row["diff"] = [round(d, 3), round(dl, 3), round(dh, 3)]
                msg = f"{d:+.3f} [{dl:+.3f}, {dh:+.3f}]"
            out["per_slice"].append(row)
            print(f"{sl[0]:4} {sl[1]:12} {sl[2]:8} {arm:9} {k:3}/{n:<3} {k/n:.2f}  [{lo:.2f}, {hi:.2f}]  {msg}")
    # pooled, stratified by task x model x bench (the arms share tasks and repeats)
    for bench in ("v1", "v2", "all"):
        for model in ("haiku45", "sonnet5", "all"):
            for arm in ("adage", "agents-md"):
                rows = [((s["task"], s["model"], s["bench"]), int(s["arm"] == arm), int(s["passed"])) for s in S
                        if s["arm"] in ("control", arm) and bench in (s["bench"], "all") and model in (s["model"], "all")]
                if not any(r[1] for r in rows):
                    continue
                e, lo, hi, w = boot_mh(rows, cluster_of=lambda r: r[0][0])
                n1 = sum(r[1] for r in rows); n0 = len(rows) - n1
                out["pooled"][f"{bench}/{model}/{arm}"] = dict(rd=round(e, 3), ci90=[round(lo, 3), round(hi, 3)],
                                                             n_arm=n1, n_control=n0)
                print(f"pooled {bench}/{model} {arm} - control: MH RD {e:+.3f} [{lo:+.3f}, {hi:+.3f}]  n={n1} vs {n0}")
    json.dump(out, open("arms.json", "w"), indent=1)


if __name__ == "__main__":
    main()
