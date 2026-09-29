"""Score the learned-equivalence reflex learner with the sprint's common harness
(`evals/lab/research/harness`): $ and calls per task, task-clustered 90% CIs.

Policy = downstream.py's learner (outcome-optimising variant fixes under a representation R),
delivered predictively: at PreToolUse of the first class-X call when X's learned failure prior
is >= 0.5, and at PostToolUse of an X call that failed (the harness's `sit.failed`, i.e. is_error,
or a learned failure template). No hindsight. Directive: `run <Y's most common successful command>`.

R is fitted on non-control sessions (train / adage / agents-md) excluding, under LORO, the
held-out repository. The reflexes are learned from what the driver hands `fit`, restricted to
train + control arms (the adage arm was steered by U11, so its fixes are the instinct's doing).
"""
from __future__ import annotations

import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))

import common as C  # noqa: E402
import data  # noqa: E402
import downstream as D  # noqa: E402
from harness import Directive, load, score  # noqa: E402
from harness.drivers import leave_task_out, loro, split  # noqa: E402

REPOS = set(data.REPOS)


def recs_of(s):
    out = []
    for i, c in enumerate(s.calls):
        cmd = str(c.input.get("command", "")) if c.tool == "Bash" and isinstance(c.input, dict) else ""
        out.append(data.Rec(repo=s.repo, skey=s.sid, idx=i, tool=c.tool, cmd=data._strip(cmd, s.cwd),
                            out=data._strip(c.output[:1500], s.cwd), is_error=bool(c.is_error), turn=0))
    return out


class Wrapped:
    def __init__(self, s):
        self.key, self.repo, self.task = s.sid, s.repo, s.task
        self.recs = recs_of(s)


_REPS, _FM = {}, {}


def rep_for(name, exclude):
    k = (name, exclude)
    if k not in _REPS:
        S = C.sessions()
        taus = json.load(open(HERE / "results" / "taus.json"))
        _REPS[k] = D.representations(S, exclude_repo=exclude, taus={n: t for n, t in taus.items() if n == name})[name]
        _FM[exclude] = D.FailModel().fit([s for s in S if s.arm != "control" and s.repo != exclude])
    return _REPS[k], _FM[exclude]


class Policy:
    events = {"PreToolUse", "PostToolUse"}

    def __init__(self, name, train):
        repos = {s.repo for s in train}
        missing = REPOS - repos
        exclude = next(iter(missing)) if len(missing) == 1 else None
        (self.lab, coarse), self.fm = rep_for(name, exclude)
        learnset = [Wrapped(s) for s in train if s.arm in ("train", "control")]
        self.rf = D.learn(learnset, self.lab, coarse, self.fm)
        self.name = f"learned-equivalence[{name}]"
        self.cache = {}

    def label(self, cmd):
        if cmd not in self.cache:
            self.cache[cmd] = self.lab([cmd])[0]
        return self.cache[cmd]

    def __call__(self, sit):
        c = sit.call
        if c is None or c.tool != "Bash":
            return None
        cmd = data._strip(str(c.input.get("command", "")), sit.session.cwd)
        if not cmd.strip():
            return None
        x = self.label(cmd)
        seen = sit.state.setdefault("seen", set())
        fired = sit.state.setdefault("fired", set())
        rf = self.rf.get(x)
        if sit.event == "PreToolUse":
            first = x not in seen
            seen.add(x)
            ok = rf is not None and first and rf["fail_x"] >= 0.5
        else:
            r = data.Rec(repo="", skey="", idx=0, tool="Bash", cmd=cmd, out=c.output[:1500], is_error=bool(sit.failed), turn=0)
            ok = rf is not None and self.fm.failed(r)
        if not ok or x in fired:
            return None
        fired.add(x)
        return Directive("run", text=f"In this repository, run this instead:\n  {rf['cmd'][:300]}", command=rf["cmd"], route=f"ae:{x[:40]}")


def main(names=("taxonomy", "hybrid", "lgg-H", "tfidf")):
    corpus = load()
    test = corpus.held_out(model="haiku")
    out = {}
    for name in names:
        fit = lambda tr, n=name: Policy(n, tr)
        res = {}
        for label, fn in (("split", lambda: split(fit, corpus, model="haiku")),
                          ("loro", lambda: loro(fit, corpus, model="haiku")),
                          ("leave_task_out", lambda: leave_task_out(fit, corpus, model="haiku"))):
            rep = fn()
            print(f"--- {name} / {label}\n{rep.headline()}", flush=True)
            res[label] = rep.to_json()
            if label == "leave_task_out":
                strict = leave_task_out(fit, corpus, model="haiku", sat="strict")
                half = leave_task_out(fit, corpus, model="haiku", p_follow=0.5)
                print(f"    strict Sat: {strict.headline().splitlines()[2]}\n    p_follow=0.5: {half.headline().splitlines()[2]}", flush=True)
                res["leave_task_out_strict"] = strict.to_json()
                res["leave_task_out_pfollow05"] = half.to_json()
        out[name] = res
    out["test_set"] = {"filter": "held_out(model='haiku')", "sessions": len(test)}
    json.dump(out, open(HERE / "results" / "harness.json", "w"), indent=1, default=str)


if __name__ == "__main__":
    main(tuple(sys.argv[1].split(",")) if len(sys.argv) > 1 else ("taxonomy", "hybrid", "lgg-H", "tfidf"))
