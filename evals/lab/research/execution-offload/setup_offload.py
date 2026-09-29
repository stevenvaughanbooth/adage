"""The hook sets the environment up itself at SessionStart (E2-style: staged, verified, applied),
so the agent's first test run works and its environment friction never happens.

Learning (generic): in pool sessions, a *repair* is a non-authoring shell command the agent ran
between a failed call of template T and the next successful call of the same template T. Repairs
whose exact key recurs in ≥ 2 pool sessions of the repository (split), or in ≥ 2 other repositories
(leave-one-repo-out), are the learned setup. The hook runs them in order at SessionStart; one that
fails is not applied and delivers nothing.

Scoring (harness cost model; the harness `features.Act.env_friction` instrument is used for
MEASUREMENT ONLY, to say which calls a working environment would have made unnecessary):
  removed   (a) the agent's own calls with a learned setup key;
            (b) conservative: friction calls before the agent's own first setup call, in sessions
                where the agent's next test run after that setup call executed tests (evidence
                the setup is what fixed it);
            (b') upper: every friction call of the session.
  injected  the agent's own result for setup calls it would have made; otherwise a 30-token note
            per applied setup command at SessionStart ("ran X: exit 0").
  A repair must also be *specific* to repair windows: it sits in one in ≥ half the pool sessions
  that run it at all (so habitual `ls`/`git status` are not learned as setup).
"""
from __future__ import annotations

import json
import sys
from collections import Counter, defaultdict

from data import corpus, records
from learner import authoring, inline_script, template
from offload import WRAPPER, aggregate, fmt, hit_tokens, session_value

sys.path.insert(0, "/home/user/adage/evals/lab/research")
from harness.leakage import assert_clean  # noqa: E402


def repairs(s):
    R = records(s)
    out = set()
    for i, r in enumerate(R):
        if not r.failed or r.tool != "Bash":
            continue
        tp = template(r)
        for j in range(i + 1, len(R)):
            q = R[j]
            if authoring(q):
                break
            if template(q) == tp and not q.failed:
                for x in R[i + 1:j]:
                    if x.tool == "Bash" and template(x) != tp and not inline_script(x) and not x.failed:
                        out.add(x.key)
                break
    return out


LIFT = 0.5  # a repair must sit in a failure→success window in ≥ half the sessions that run it


def learn(pool, repo, loro):
    per_repo = defaultdict(Counter)
    anywhere = defaultdict(Counter)
    for s in pool:
        for k in {r.key for r in records(s)}:
            anywhere[s.repo][k] += 1
        for k in repairs(s):
            per_repo[s.repo][k] += 1
    for r in per_repo:
        for k in list(per_repo[r]):
            if per_repo[r][k] < LIFT * anywhere[r][k]:
                del per_repo[r][k]
    if not loro:
        return sorted(k for k, n in per_repo[repo].items() if n >= 2)
    reps = defaultdict(set)
    for r, cn in per_repo.items():
        for k in cn:
            reps[k].add(r)
    return sorted(k for k, rs in reps.items() if len(rs) >= 2)


def score_session(s, setup, model, upper=False):
    R = records(s)
    A = s.acts
    if not setup:
        return session_value(s, [], [], model)
    removed = {k for k, r in enumerate(R) if r.key in setup}
    first = min(removed) if removed else None
    if upper:
        removed |= {k for k in range(len(R)) if A[k].env_friction}
    elif first is not None:
        nxt = next((k for k in range(first + 1, len(R)) if A[k].run is not None), None)
        if nxt is not None and A[nxt].executed:
            removed |= {k for k in range(first) if A[k].env_friction}
    own = {k for k in removed if R[k].key in setup}
    tok = sum(30 + WRAPPER for key in setup if not any(R[k].key == key for k in own))
    hits = [(-1, k, hit_tokens(s, R[k]) if k in own else 0) for k in sorted(removed)]
    val = session_value(s, hits, [(-1, tok)] if tok else [], model)
    val["misses"] = 0
    return val


def main():
    c = corpus()
    out = {}
    for model in ("haiku", "sonnet"):
        test = c.held_out(model=model)
        repos = sorted({s.repo for s in test})
        out[model] = {}
        for proto in ("split", "loro"):
            rows, cl, learned = defaultdict(list), [], {}
            for r in repos:
                te = [s for s in test if s.repo == r]
                pool = c.train() if proto == "split" else [s for s in c.sessions if s.repo != r]
                assert_clean(pool, te)
                setup = learn(pool, r, proto == "loro")
                learned[r] = [k.split("|", 1)[1][:80] for k in setup]
                for s in te:
                    rows["conservative"].append(score_session(s, set(setup), model))
                    rows["upper (all friction)"].append(score_session(s, set(setup), model, upper=True))
                    cl.append(s.cluster)
            res = {k: aggregate(v, cl) for k, v in rows.items()}
            res["learned_setup"] = learned
            out[model][proto] = res
            print(f"== {model} {proto}")
            for k in rows:
                print("  ", k, "|", fmt(res[k]))
            for r, ks in learned.items():
                print("     ", r, ks[:6])
    json.dump(out, open("results/setup.json", "w"), indent=1)


if __name__ == "__main__":
    main()
