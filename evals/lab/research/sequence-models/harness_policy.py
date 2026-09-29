"""Score the skip-ahead sequence models with the common harness (evals/lab/research/harness).

fit(train_sessions) -> policy. The learner sees harness Sessions only through (tool name, argument
text, is_error, model turn) — the same raw view as load.py — builds the learned action space,
trains the model on the "eventually, after a related dead end" target, calibrates by cross-fitting,
and at SessionStart / PostToolBatch delivers the best not-yet-delivered cluster whose calibrated
p >= theta. The delivered directive is the cluster's most frequent training member in this repo:
a Bash member -> Directive("run", command=...), a file-tool member -> Directive("read_files", files=...).
The harness's Sat/W classes are the (hand-written) measurement; nothing from them is used to learn.

    python harness_policy.py            # split + loro, Haiku held-out, p_follow 1.0 and 0.5
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent))

from harness import Directive, load as hload, score  # noqa: E402
from harness.drivers import loro, split  # noqa: E402
from load import arg_text  # noqa: E402
from skip import Sess, crossfit_calibrator, discovery_weights, labels, make  # noqa: E402
from space import Space  # noqa: E402


def to_dict(s) -> dict:
    turn_of = {}
    for j, r in enumerate(s.responses):
        for i in r.calls:
            turn_of[i] = j
    steps, b, last = [], -1, None
    for i, c in enumerate(s.calls):
        t = turn_of.get(i, i)
        if t != last:
            b += 1
            last = t
        steps.append({"tool": c.tool, "arg": arg_text(c.tool, c.input or {}), "err": bool(c.is_error), "turn": t,
                      "batch": b, "usd": 1.0, "chars": 0})
    return {"sid": s.sid, "repo": s.repo, "task": s.task, "arm": s.arm, "passed": s.passed,
            "prompt": (s.prompt or "")[:1000], "model": s.model or "", "steps": steps,
            "usd": float(s.cost_usd or 1.0), "api_turns": max(len(s.responses), 1)}


class Partial:
    """A held-out session prefix, as the models' score() expects (one decision point at its end)."""

    def __init__(self, repo, prompt, texts, err, space):
        self.repo, self.prompt, self.texts, self.n = repo, prompt, texts, len(texts)
        self.c = space.assign(texts) if texts else np.zeros(0, int)
        self.err = np.array(err, bool)
        self.dp = [self.n]
        self._feat = self._seen = None


class SkipPolicy:
    events = {"SessionStart", "PostToolBatch"}

    def __init__(self, train, model="repo-prior", theta=0.5, K=128, max_per_session=3):
        self.name = f"skip-{model}-θ{theta}"
        raw = [to_dict(s) for s in train if s.calls]
        self.sp = Space(K=K).fit(raw)
        self.K = self.sp.K
        tr = [Sess(d, self.sp) for d in raw]
        W = discovery_weights(tr, self.K)
        trd = [(s, labels(s, W, self.K)) for s in tr]
        self.cal = crossfit_calibrator(model, self.K, trd, folds=3 if len(trd) > 100 else 5)
        self.m = make(model, self.K).fit(trd)
        self.theta, self.maxn = theta, max_per_session
        self.rep = {}
        for R in {s.repo for s, _ in trd} | {None}:
            texts = {t for s, _ in trd if R is None or s.repo == R for t in s.texts}
            self.rep[R] = [next((t for t, _ in self.sp.members[c].most_common() if t in texts), None)
                           for c in range(self.K)]

    def directive(self, text, c):
        tool, _, arg = text.partition(" ")
        if tool == "Bash" and arg and "<<more" not in arg:
            return Directive("run", command=arg, text=f"You will likely need: `{arg}`", route=f"skip-c{c}")
        if tool in ("Read", "Edit", "Write", "MultiEdit") and arg:
            return Directive("read_files", files=(arg.split()[0],), text=f"Relevant file: {arg.split()[0]}",
                             route=f"skip-c{c}")
        return None

    def __call__(self, sit):
        st = sit.state
        done = st.setdefault("done", set())
        if len(done) >= self.maxn:
            return None
        h = sit.history
        p = Partial(sit.session.repo, sit.session.prompt or "",
                    [f"{c.tool} {arg_text(c.tool, c.input or {})}" for c in h], [c.is_error for c in h], self.sp)
        sc = self.m.score(p)[0]
        pr = self.cal.predict(sc) + 1e-6 * sc / max(sc.max(), 1e-12)
        reps = self.rep.get(sit.session.repo) or self.rep[None]
        for c in np.argsort(-pr)[:8]:
            if pr[c] < self.theta:
                return None
            if int(c) in done:
                continue
            text = reps[c] or self.rep[None][c]
            d = self.directive(text, int(c)) if text else None
            if d is None:
                continue
            done.add(int(c))
            return d
        return None


def main():
    corpus = hload()
    out = {}
    for model, theta in [("repo-prior", 0.5), ("repo-prior", 0.15), ("ppm1-repo", 0.5), ("mlp", 0.5)]:
        fit = lambda tr, model=model, theta=theta: SkipPolicy(tr, model, theta)
        for drv, name in [(split, "split"), (loro, "loro")]:
            if name == "loro" and model == "mlp":
                continue
            for pf in (1.0, 0.5):
                rep = drv(fit, corpus, model="haiku", p_follow=pf)
                key = f"{model}@{theta}:{name}:pf{pf}"
                print(key, "\n", rep.headline(), flush=True)
                out[key] = rep.to_json()
    (HERE / "results").mkdir(exist_ok=True)
    (HERE / "results" / "harness.json").write_text(json.dumps(out, indent=1, default=str))


if __name__ == "__main__":
    main()
