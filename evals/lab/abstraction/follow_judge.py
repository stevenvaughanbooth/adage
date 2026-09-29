"""The follow judge as an abstract-action match (launch bug 1), replayed on RepoBench v2.

U11 directs `.venv/bin/python -m pytest` after `python -m pytest` fails with `No module named
pytest`. `adage_core::pattern::meets` credits a later command only when it is the directed one
plus options; agents add a test path, so every follow was judged "ignored". Judged instead as an
abstract action (role test-run, env class project, any scope and arguments), the same calls are
followed. Sessions: v2 `adage`-arm sessions whose record in /tmp/adage-rb2/deadends-v2.jsonl
names a delivered U11 route; the delivery point is the first test run that failed visibly with
the dead end (the route fires after a failure).
"""
from __future__ import annotations

import json
import os

import corpus

DEADENDS = "/tmp/adage-rb2/deadends-v2.jsonl"
DIRECTED = [".venv/bin/python", "-m", "pytest"]


def meets_literal(argv: list[str]) -> bool:
    """pattern::meets as the launch report describes it: the directed argv plus options only."""
    return argv[:3] == DIRECTED and all(t.startswith("-") for t in argv[3:])


def main():
    import shell
    recs = {}
    if os.path.exists(DEADENDS):
        for line in open(DEADENDS):
            r = json.loads(line)
            if r["arm"] == "adage" and any(x.startswith("dead-end") for x in r["delivered_routes"]):
                recs[r["run"]] = r
    L = [l for l in corpus.load() if l.sess.root == "v2" and l.sess.run in recs]
    n = lit = gram = 0
    for l in L:
        acts = l.acts
        calls = l.sess.calls
        d = next((i for i, (c, a) in enumerate(zip(calls, acts)) if a.role == "test-run" and a.outcome == "env-fail" and c.err), None)
        if d is None:
            continue
        nxt = next((i for i in range(d + 1, len(calls)) if acts[i].role == "test-run"), None)
        if nxt is None:
            continue
        n += 1
        segs = shell.segments(calls[nxt].command, l.sess.cwd)
        argv = next((s.argv for s in segs if s.argv and "pytest" in " ".join(s.argv)), [])
        lit += meets_literal(argv)
        gram += acts[nxt].envclass == "project"
    out = {"sessions_with_delivery": len(L), "judged": n, "followed_literal_meets": lit, "followed_abstract": gram}
    print(json.dumps(out))
    here = os.path.dirname(os.path.abspath(__file__))
    with open(os.path.join(here, "results", "follow_judge.json"), "w") as f:
        json.dump(out, f, indent=1)


if __name__ == "__main__":
    main()
