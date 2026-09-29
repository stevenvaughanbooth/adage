"""Ceilings for what action equivalence could unlock, on held-out control sessions.

1. Recovery ceiling (instrument, downstream.oracle): every failed call pre-empted by exactly the
   fix the agent later found.
2. Redundancy ceiling: calls that repeat an earlier call of the session with nothing edited in
   between, so their output could not have changed (a "you already ran this" / memoise lever).
   Exact: the same normalised command. Rough: the same fine class under a representation AND the
   same set of non-flag operands (so a different test file is not a repeat).
Both are reported as calls/session and share of calls, with 90% bootstrap CIs over sessions.
"""
import json

import common as C
import data
import downstream as D
import feats as FE

EDIT = ("Edit", "Write", "MultiEdit", "NotebookEdit")


def operands(cmd):
    return tuple(sorted(t for a, p in FE.lex(cmd) if not p for t in a[1:] if not t.startswith("-")))


def redundancy(sess, lab=None):
    recs = sess.recs
    idx = [i for i, r in enumerate(recs) if r.tool == "Bash" and r.cmd.strip()]
    L = dict(zip(idx, lab([recs[i].cmd for i in idx]))) if (lab and idx) else {}
    seen, n = set(), 0
    for i, r in enumerate(recs):
        if r.tool in EDIT:
            seen = set()
            continue
        if r.tool == "Read":
            k = ("R", r.cmd)
        elif r.tool == "Bash" and r.cmd.strip():
            if any(t in r.cmd for t in (">", "sed -i", "rm ", "mv ", "cp ", "git commit", "git add", "pip install", "npm install")):
                seen = set()  # a shell write: be conservative (MEASUREMENT heuristic)
                continue
            k = ("B", L[i], operands(r.cmd)) if lab else ("B", FE.text(r.cmd))
        else:
            continue
        n += k in seen
        seen.add(k)
    return n


def run():
    S = C.sessions()
    held = [s for s in S if s.arm == "control"]
    taus = json.load(open(data.HERE / "results" / "taus.json"))
    reps = D.representations(S, taus=taus)
    out = {}
    calls = [len(s.recs) for s in held]
    for name in ("exact", "taxonomy", "lgg-L", "a2v", "rrr-first", "hybrid-first"):
        lab = None if name == "exact" else reps[name][0]
        r = [redundancy(s, lab) for s in held]
        out[name] = {"calls": C.boot(r), "share": C.boot([x / c for x, c in zip(r, calls)])}
        print(name, out[name])
    json.dump(out, open(data.HERE / "results" / "ceiling.json", "w"), indent=1)


if __name__ == "__main__":
    run()
