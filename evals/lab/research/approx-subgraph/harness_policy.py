"""The final policy, scored by the common harness: MDL-selected, gap-tolerant patterns over learned
node clusters, compiled into delivered steps.

Learner (`fit(train_sessions) -> policy`), all from traces:
1. node labels: `nodes.Labeler` at τ = 0.8 (vocabulary recurring across tasks, or across
   repositories when the training pool spans several), plus the harness's is_error bit;
2. closed gapped patterns (G = 8, support >= max(3, 10% of sessions)), SQS/Krimp MDL selection;
3. each selected pattern split into (prefix -> suffix) as in `evaluate.splits`, suffix's first item
   a successful call; compiled by Track G's rule: the suffix item's dominant literal (>= 80% of its
   training occurrences) becomes a `run` directive; otherwise nothing is delivered (an `advice`
   text is never credited by replay, only costed; `ADVICE=1` delivers it to show that cost).

Runtime: at every PostToolUse the call is labelled (nearest centroid) and appended to the session's
label sequence; when a prefix completes at this call (gaps <= G) the directive is given, once per
session per directive, at most one per event (highest training usage first).

The call text is `evals/lab/normalize.py`'s rendering (`run <command>`, `read <path>`, with the
sandbox root stripped); its hand-written key/slot levels are not used.

`CUSTOM=1` scores the same deliveries as `custom` directives with Sat = a later successful call in the
suffix's learned cluster and W = failed calls (flagged: a learned, not calibrated, Sat).
"""

from __future__ import annotations

import json
import math
import os
import sys
from collections import Counter, defaultdict
from dataclasses import dataclass, field
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent))
sys.path.insert(0, str(HERE.parents[1]))

import evaluate as ev  # noqa: E402
import mdl  # noqa: E402
import nodes  # noqa: E402
from normalize import normalize_tool  # noqa: E402

from harness import Directive, load, score  # noqa: E402
from harness.drivers import leave_task_out, loro, split  # noqa: E402

GAP = 8
TAU = float(os.environ.get("TAU", "0.8"))
ADVICE = os.environ.get("ADVICE") == "1"
CUSTOM = os.environ.get("CUSTOM") == "1"


@dataclass
class E:
    text: str
    fail: bool


@dataclass
class S:
    sid: str
    task: str
    repo: str
    events: list = field(default_factory=list)


def text_of(call) -> str:
    return normalize_tool(call.tool, call.input or {}).text


def adapt(s) -> S:
    return S(s.sid, s.task, s.repo, [E(text_of(c), bool(c.is_error)) for c in s.calls])


class Policy:
    events = {"PostToolUse"}

    def __init__(self, lab, dirs):
        self.lab, self.dirs = lab, dirs

    def label(self, call) -> str:
        c = self.lab.cluster_of([text_of(call)])[0]
        return ("?" if c < 0 else f"c{c}") + ("!" if call.is_error else "")

    def __call__(self, sit):
        st = sit.state
        seq = st.setdefault("seq", [])
        seq.append(self.label(sit.call))
        done = st.setdefault("done", set())
        for i, (pre, suf, d) in enumerate(self.dirs):
            if i in done:
                continue
            if seq[-1] != pre[-1]:
                continue
            e = ev.first_completion(seq, pre, GAP)
            if e == len(seq) - 1:
                done.add(i)
                return d


def _custom(lab, label):
    def sat(act, call):
        return (not call.is_error) and lab.cluster_of([text_of(call)])[0] == int(label.strip("c!"))

    def waste(act, call):
        return bool(call.is_error)
    return sat, waste


def fit(train_sessions):
    tr = [adapt(s) for s in train_sessions if s.calls]
    multi = len({s.repo for s in tr}) > 1
    lab = nodes.Labeler(tr, TAU, group="repo" if multi else "task", min_groups=2)
    X = [lab.labels(s) for s in tr]
    P = mdl.closed(mdl.mine(X, max(3, math.ceil(0.1 * len(X))), GAP))
    ct = mdl.CodeTable(X, GAP).fit(P, max_cands=800 if len(X) > 30 else 3000)
    raw = defaultdict(Counter)
    for s, x in zip(tr, X):
        for l, e in zip(x, s.events):
            raw[l][e.text.split("\n")[0]] += 1
    dirs = []
    for p in sorted(ct.table, key=lambda p: -ct.use.get(p, 0)):
        for pre, suf in ev.splits(p):
            if suf[0].endswith("!") or suf[0].startswith("?"):
                continue
            c = raw[suf[0]]
            v, n = c.most_common(1)[0]
            route = f"approx-{'-'.join(pre)}-{suf[0]}"
            cmd = v.removeprefix("run ") if v.startswith("run ") else ""
            if CUSTOM:
                sat, waste = _custom(lab, suf[0])
                d = Directive("custom", text=f"Next: {v[:200]}", route=route, sat=sat, waste=waste)
            elif n / sum(c.values()) >= 0.8 and cmd and "\n" not in cmd:
                d = Directive("run", command=cmd, route=route)
            elif ADVICE:
                d = Directive("advice", text=f"Agents here usually do next: {v[:200]}", route=route)
            else:
                continue
            dirs.append((pre, suf, d))
    pol = Policy(lab, dirs)
    pol.n_dirs = len(dirs)
    return pol


def main():
    corpus = load()
    test = corpus.held_out(model="haiku")
    out = {}
    tag = f"tau{TAU}" + ("-advice" if ADVICE else "") + ("-custom" if CUSTOM else "")
    for name, drv in (("split", split), ("leave_task_out", leave_task_out), ("loro", loro)):
        rep = drv(fit, corpus, model="haiku")
        print(f"== {name} ({tag})")
        print(rep.headline(), flush=True)
        out[name] = rep.to_json()
        if name == "leave_task_out":
            rep5 = drv(fit, corpus, model="haiku", p_follow=0.5)
            print(f"== {name} p_follow=0.5\n" + rep5.headline(), flush=True)
            out[name + "_p0.5"] = rep5.to_json()
    (HERE / "results").mkdir(exist_ok=True)
    (HERE / "results" / f"harness-{tag}.json").write_text(json.dumps(out, default=str))
    print("held-out sessions", len(test))


if __name__ == "__main__":
    main()
