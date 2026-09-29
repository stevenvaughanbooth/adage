"""Oracle ceilings of offloading: a perfect hook runs every call of a class the moment before the
agent would, delivers its result, and the agent's turn for it disappears.

The classes are a HAND-WRITTEN MEASUREMENT INSTRUMENT (harness `features.Act` plus a few command
heads): they size the pool; no learner uses them. The last block ("reachable") is structural and
learning-free: calls whose exact key a hook *could* have proposed at that moment (a repeat of an
earlier call in the session, a key seen in ≥2 training sessions of the repo, or a Read of a path
printed by the previous call).
"""
from __future__ import annotations

import json
import re
from collections import Counter, defaultdict

from data import corpus, model_of, records
from offload import aggregate, fmt, hit_tokens, result_tokens, session_value


def compact_tokens(s, r, m):
    return min(hit_tokens(s, r), result_tokens(r, m, compact=True)) if not r.failed else hit_tokens(s, r)

GIT = re.compile(r"^git\b")
LINT = re.compile(r"\b(ruff|black|gofmt|prettier|eslint|isort|flake8|mypy|tsc|go vet|go build)\b")
AUTHOR = ("Edit", "Write", "MultiEdit", "NotebookEdit")
PATHY = re.compile(r"[\w./-]+\.[A-Za-z]{1,5}\b")


def classes(repo_keys):
    def reachable(s, recs, k, seen):
        r = recs[k]
        if r.key in seen or repo_keys[s.repo][r.key] >= 2:
            return True
        if r.tool == "Read" and k > 0:
            p = r.args.get("file_path", "")
            return bool(p) and p in recs[k - 1].out and set(r.args) <= {"file_path"}
        return False

    return {
        "every call that authors nothing (not Edit/Write, not an inline script)":
            lambda s, recs, k, seen: recs[k].tool not in AUTHOR and "<<" not in recs[k].text and " -c " not in recs[k].text,
        "test runs (runner invocations)": lambda s, recs, k, seen: s.acts[k].run is not None,
        "test runs that executed tests": lambda s, recs, k, seen: s.acts[k].run is not None and s.acts[k].executed,
        "git commands": lambda s, recs, k, seen: recs[k].tool == "Bash" and bool(GIT.match(recs[k].text)),
        "env probes / installs (U11 instrument)": lambda s, recs, k, seen: s.acts[k].env_probe,
        "lint / format / type-check": lambda s, recs, k, seen: recs[k].tool == "Bash" and bool(LINT.search(recs[k].text)),
        "searches and listings": lambda s, recs, k, seen: s.acts[k].tool in ("Grep", "Glob") or (
            recs[k].tool == "Bash" and recs[k].text.split()[:1] in (["find"], ["grep"], ["ls"], ["rg"])),
        "Reads": lambda s, recs, k, seen: recs[k].tool == "Read",
        "REACHABLE: exact repeat of an earlier call in the session":
            lambda s, recs, k, seen: recs[k].key in seen,
        "REACHABLE: key seen in >=2 training sessions of the repo":
            lambda s, recs, k, seen: repo_keys[s.repo][recs[k].key] >= 2 and recs[k].key not in seen,
        "REACHABLE: any of repeat / repo key / path printed by the previous call": reachable,
    }


def run():
    c = corpus()
    repo_keys = defaultdict(Counter)
    for s in c.train():
        for k in {r.key for r in records(s)}:
            repo_keys[s.repo][k] += 1
    out = {}
    for m in ("haiku", "sonnet"):
        S = c.held_out(model=m)
        R = {s.sid: records(s) for s in S}
        out[m] = {}
        for name, pred in classes(repo_keys).items():
            for compact in ((False, True) if name.startswith("test runs") else (False,)):
                rows, cl = [], []
                for s in S:
                    recs, seen, hits = R[s.sid], set(), []
                    for k, r in enumerate(recs):
                        if pred(s, recs, k, seen):
                            hits.append((k - 1, k, compact_tokens(s, r, m) if compact else hit_tokens(s, r)))
                        seen.add(r.key)
                    rows.append(session_value(s, hits, [], m))
                    cl.append(s.cluster)
                a = aggregate(rows, cl)
                tag = name + (" [compact passing output]" if compact else "")
                out[m][tag] = a
                print(m, "|", tag, "|", fmt(a), flush=True)
    json.dump(out, open("results/ceilings.json", "w"), indent=1)


if __name__ == "__main__":
    run()
