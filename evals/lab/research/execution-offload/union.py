"""The addressable ceiling of offloading: the union of every turn a *perfect* offload hook could
remove without doing the model's reasoning, priced once (no double counting).

  A  exact-reachable: calls whose exact key the hook could have proposed (repeat, repo habit,
     path printed by the previous call), perfectly predicted, no misses
  B  after-edit test runs: every test run that follows an edit with no test run in between
     (a perfect verifier, perfect coverage), result compacted on pass
  C  environment friction (a perfect setup at SessionStart; the U11 oracle class)
Instrument classes (B, C) are HAND-WRITTEN MEASUREMENT (harness features.Act). → results/union.json
"""
from __future__ import annotations

import json
from collections import Counter, defaultdict

from ceilings import compact_tokens
from data import corpus, records
from learner import authoring
from offload import aggregate, fmt, hit_tokens, session_value


def main():
    c = corpus()
    repo_keys = defaultdict(Counter)
    for s in c.train():
        for k in {r.key for r in records(s)}:
            repo_keys[s.repo][k] += 1
    out = {}
    for m in ("haiku", "sonnet"):
        S = c.held_out(model=m)
        parts = {"A exact-reachable": [], "B after-edit test runs": [], "C env friction": [], "A+B+C": [],
                 "A+B (no friction; U11 already owns C)": []}
        cl = []
        for s in S:
            R, A = records(s), s.acts
            seen, sets = set(), {"A": set(), "B": set(), "C": set()}
            last_edit, last_test = -1, -1
            for k, r in enumerate(R):
                ok = r.key in seen or repo_keys[s.repo][r.key] >= 2 or (
                    r.tool == "Read" and k > 0 and r.args.get("file_path", "") in R[k - 1].out
                    and set(r.args) <= {"file_path"} and r.args.get("file_path"))
                if ok and not authoring(r):
                    sets["A"].add(k)
                if A[k].run is not None:
                    if last_edit > last_test:
                        sets["B"].add(k)
                    last_test = k
                if authoring(r):
                    last_edit = k
                if A[k].env_friction:
                    sets["C"].add(k)
                seen.add(r.key)

            def val(ks):
                return session_value(s, [(k - 1, k, compact_tokens(s, R[k], m) if k in sets["B"] else hit_tokens(s, R[k]))
                                         for k in sorted(ks)], [], m)
            parts["A exact-reachable"].append(val(sets["A"]))
            parts["B after-edit test runs"].append(val(sets["B"]))
            parts["C env friction"].append(val(sets["C"]))
            parts["A+B+C"].append(val(sets["A"] | sets["B"] | sets["C"]))
            parts["A+B (no friction; U11 already owns C)"].append(val(sets["A"] | sets["B"]))
            cl.append(s.cluster)
        out[m] = {k: aggregate(v, cl) for k, v in parts.items()}
        for k in parts:
            print(m, "|", k, "|", fmt(out[m][k]))
    json.dump(out, open("results/union.json", "w"), indent=1)


if __name__ == "__main__":
    main()
