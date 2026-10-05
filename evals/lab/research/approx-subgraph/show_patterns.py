"""Print MDL-selected patterns in readable form (each item: its cluster's top abstracted members),
with their held-out follow rate. -> results/patterns.txt"""

from __future__ import annotations

import math
from pathlib import Path

import data
import evaluate as ev
import mdl
import nodes
from run import GAP, UNSTEERED

HERE = Path(__file__).resolve().parent


def item(lab, a):
    if a.startswith("?"):
        return "<unseen>"
    c = int(a.strip("c!"))
    top = [t for t, _ in lab.members[c].most_common(2)]
    return "{" + " ~ ".join(t[:48] for t in top) + (f" +{len(lab.members[c]) - 2}" if len(lab.members[c]) > 2 else "") + "}" + (" FAILED" if a.endswith("!") else "")


def show(title, train, test, tau, group):
    lab = nodes.Labeler(train, tau, group=group)
    X = [lab.labels(s) for s in train]
    Y = [lab.labels(s) for s in test]
    P = mdl.closed(mdl.mine(X, max(3, math.ceil(0.1 * len(X))), GAP))
    ct = mdl.CodeTable(X, GAP).fit(P, max_cands=800)
    out = [f"\n## {title}: {len(train)} training sessions, {len(test)} held-out, {lab.k} clusters, {len(ct.table)} patterns\n"]
    rows = []
    for p in ct.table:
        u = ev.usefulness([p], Y, GAP)[0]
        rows.append((u["n_prefix"], p, u))
    for n, p, u in sorted(rows, key=lambda r: -r[0])[:12]:
        out.append(f"- train use {ct.use.get(p, 0)}, held-out: prefix in {u['n_prefix']} sessions, "
                   f"last step followed within 8 in {u['n_follow']} (base rate {u['base']:.2f})")
        out.append("    " + "\n    -> ".join(item(lab, a) for a in p))
    return "\n".join(out)


def main():
    S = data.load()
    txt = []
    for r in ("click", "attrs", "vue-core"):
        pool = [s for s in S if s.repo == r and s.model == "haiku" and s.arm in UNSTEERED]
        hold = set(sorted({s.task for s in pool})[0::5])  # run.py grown, fold 0
        tr = [s for s in pool if s.task not in hold]
        test = [s for s in pool if s.task in hold and s.arm == "control"]
        txt.append(show(f"{r}, same repository (grown memory, fold 0), tau 0.8", tr, test, 0.8, "task"))
    for r in ("click", "urfave-cli"):
        test = [s for s in S if s.repo == r and s.role == "test" and s.model == "haiku"]
        tr = [s for s in S if s.repo != r and s.model == "haiku" and s.arm in UNSTEERED]
        txt.append(show(f"{r}, leave-one-repo-out, tau 0.7", tr, test, 0.7, "repo"))
    (HERE / "results" / "patterns.txt").write_text("\n".join(txt))
    print("\n".join(txt))


if __name__ == "__main__":
    main()
