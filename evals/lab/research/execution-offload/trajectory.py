"""Would hook-run tests after each edit change the trajectory? (earlier failure detection)

For every failing test run k of a held-out session (instrument: harness `features.Act.run`,
`executed`, `is_error`: MEASUREMENT ONLY), look back to the previous test run p. If the agent made
edits in more than one model turn between p and k, a hook that ran the tests after the *first*
edit turn could have exposed the failure earlier. The calls between that first edit turn and k are
the *detection lag*.

Three estimates of what earlier detection saves, from loose to conservative:
  U  (upper)   every lag call                      (the agent would have stopped and re-planned)
  D  (dead)    lag calls that are edits later revised (a later edit of the same file removes
               ≥ 50% of the earlier edit's new text: a dead-end edit, learned-free string test),
               plus non-test executions between them (runs of dead-end code)
  C  (conservative) D × P(the first edit turn already failed); unknown from traces, so bounded by
               the share of lag windows whose *first* edit is itself revised later
Priced with the harness cost model (removal of those calls, no injection: the offload's own
injection is already charged by verifier.py).
"""
from __future__ import annotations

import json
import re
import sys
from collections import defaultdict

from data import corpus, records
from learner import authoring
from offload import aggregate, fmt, session_value

# MEASUREMENT INSTRUMENT (hand-written): test-failure text, for runs whose exit status a pipe hid
FAIL_TEXT = re.compile(r"\b\d+ failed\b|\bFAILED\b|^FAIL\b|^--- FAIL|\b\d+ errors?\b|Test Files\s+\d+ failed", re.M)

sys.path.insert(0, "/home/user/adage/evals/lab/research")


def revised(R, i):
    """Is edit i later (partly) undone: a later edit of the same file whose old text contains
    ≥ 50% of edit i's new lines?"""
    r = R[i]
    f = r.args.get("file_path")
    new = r.args.get("new_string") or r.args.get("content") or ""
    lines = [x.strip() for x in str(new).splitlines() if len(x.strip()) >= 8]
    if not f or not lines:
        return False
    for j in range(i + 1, len(R)):
        q = R[j]
        if authoring(q) and q.args.get("file_path") == f:
            old = str(q.args.get("old_string") or "")
            if q.tool == "Write":
                cont = str(q.args.get("content") or "")
                gone = sum(1 for x in lines if x not in cont)
            else:
                gone = sum(1 for x in lines if x in old)
            if gone >= 0.5 * len(lines):
                return True
    return False


def analyse(s):
    R = records(s)
    A = s.acts
    tests = [k for k in range(len(R)) if A[k].run is not None and not A[k].env_sig]
    out = {"U": set(), "D": set(), "C": set(), "fail_runs": 0, "multi_turn": 0, "first_revised": 0}
    prev = -1
    for k in tests:
        fail = R[k].failed or bool(FAIL_TEXT.search(R[k].out))
        if fail:
            out["fail_runs"] += 1
            edit_turns = []
            for i in range(prev + 1, k):
                if authoring(R[i]) and (not edit_turns or R[i].resp != edit_turns[-1][-1][1]):
                    edit_turns.append([(i, R[i].resp)])
                elif authoring(R[i]):
                    edit_turns[-1].append((i, R[i].resp))
            if len(edit_turns) >= 2:
                out["multi_turn"] += 1
                first_end = edit_turns[0][-1][0]
                lag = list(range(first_end + 1, k))
                out["U"].update(lag)
                dead = [i for i in lag if authoring(R[i]) and revised(R, i)]
                runs = [i for i in lag if R[i].tool == "Bash" and not authoring(R[i]) and A[i].run is None
                        and dead and i > dead[0]]
                out["D"].update(dead + runs)
                if any(revised(R, i) for i, _ in edit_turns[0]):
                    out["first_revised"] += 1
                    out["C"].update(dead + runs)
        prev = k
    return out


def main():
    c = corpus()
    res = {}
    for model in ("haiku", "sonnet"):
        S = c.held_out(model=model)
        rows = defaultdict(list)
        cl = []
        stats = defaultdict(int)
        for s in S:
            a = analyse(s)
            for key in ("fail_runs", "multi_turn", "first_revised"):
                stats[key] += a[key]
            for key in ("U", "D", "C"):
                rows[key].append(session_value(s, [(i - 1, i, 0) for i in sorted(a[key])], [], model))
            cl.append(s.cluster)
        res[model] = {k: aggregate(rows[k], cl) for k in ("U", "D", "C")}
        res[model]["counts"] = dict(stats, sessions=len(S))
        print(model, dict(stats), len(S))
        for k in ("U", "D", "C"):
            print("  ", k, fmt(res[model][k]))
    json.dump(res, open("results/trajectory.json", "w"), indent=1)


if __name__ == "__main__":
    main()
