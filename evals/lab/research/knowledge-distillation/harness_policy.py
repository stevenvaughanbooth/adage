"""The distilled facts as a harness policy: contextual delivery at PreToolUse (and SessionStart).

  cd evals/lab/research && OMP_NUM_THREADS=1 nice -n 10 python3 knowledge-distillation/harness_policy.py

fit(train_sessions) learns the repository's facts exactly as `learner.py` does (strict events: repo
paths and failure fixes; m = 0, 1000-token selection, the LORO-chosen card configuration), from
harness sessions converted to this lab's shape. The policy then delivers one fact per event:

- SessionStart: the facts first needed at session start (their situation is "no call yet"),
  bundled as one `read_files` directive of their paths;
- PreToolUse of call t: the not-yet-delivered fact whose training situations are most similar
  (potion-8M cosine of the proposed call's input) if >= theta. A fix fact is a `run` of the command
  that worked (a test-runner run is scored in the calibrated Runner class); a locator fact is a
  `read_files` of the paths it names (UNCALIBRATED class; reported at p_follow 0.5 too).

The harness's Sat/W classes are its hand-written instrument; the learner never imports them.
"""

from __future__ import annotations

import json
import os
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent))
os.environ.setdefault("KD_STRICT", "1")

import numpy as np  # noqa: E402

import common  # noqa: E402
import facts  # noqa: E402
import kd_embed  # noqa: E402
import kd_load  # noqa: E402
import learner  # noqa: E402
from harness import Directive, load, score  # noqa: E402

THETAS = (0.6, 0.7, 0.8)


class Adapted:
    """A harness Session in this lab's shape (only what facts.extract needs)."""

    def __init__(self, s):
        self.sid, self.repo, self.task, self.arm, self.run = s.sid, s.repo, s.task, s.arm, s.run
        root = s.cwd
        self.prompt = kd_load.strip(s.prompt or "", [root])
        self.calls = []
        for c in s.calls:
            inp = kd_load.strip(kd_load.render_input(c.tool, c.input if isinstance(c.input, dict) else {}), [root])[:4000]
            self.calls.append(kd_load.Call(c.tool, inp, kd_load.strip(c.output or "", [root]), bool(c.is_error), 0, 0.0,
                                           c.out_chars or len(c.output or "")))
        self.n_requests = len(s.responses)
        self.cost = s.cost_usd or 0.0


def events_of(adapted, prior):
    M = kd_embed.enc([c.inp for c in adapted.calls]) if adapted.calls else np.zeros((0, 256), np.float32)
    ev = facts.extract(adapted, prior, lambda i, j: float(M[i] @ M[j]))
    P = common.repo_paths(adapted.repo)
    return [e for e in ev if e.kind == "chain" or e.need <= P]


class KDPolicy:
    events = {"SessionStart", "PreToolUse"}

    def __init__(self, train, theta=0.7, budget=1000, p_follow=None, cap=99):
        self.cap = cap
        mem = [Adapted(s) for s in train]
        self.repo = mem[0].repo if mem else None
        self.theta, self.p_follow = theta, p_follow
        if not mem:
            self.items = []
            return
        # the prior is learned from the *other* repositories' training traces (kd_load corpus)
        prior = common.prior(kd_load.load(), self.repo)
        E = {m.sid: events_of(m, prior) for m in mem}
        cand = learner.candidates(mem, E)
        self.items = learner.select(mem, E, cand, m=0, budget=budget)
        self.ctx = learner.Contextual(self.items, theta)
        self.P = common.repo_paths(self.repo)
        kd_embed.save()

    def memory_texts(self):
        return [it.text for it in self.items]

    def _directive(self, it, route):
        if it.kind == "chain":
            cmd = it.text.split("what worked: `", 1)[-1].rstrip("`")
            cmd = cmd.split(" ; ")[-1].removeprefix("Bash: ")
            return Directive("run", text=it.text, command=cmd, route=route, p_follow=self.p_follow)
        files = tuple(sorted(a for a in it.need if a in self.P and "." in a.rsplit("/", 1)[-1]))
        return Directive("read_files", text=it.text, files=files, route=route, p_follow=self.p_follow)

    def __call__(self, sit):
        if not self.items:
            return None
        st = sit.state
        if st.get("n", 0) >= self.cap:
            return None
        d = self._decide(sit, st)
        if d is not None:
            st["n"] = st.get("n", 0) + 1
        return d

    def _decide(self, sit, st):
        done = st.setdefault("done", set())
        if sit.event == "SessionStart":
            ids = [i for i in self.ctx.start if self.items[i].kind == "locator"]
            if not ids:
                return None
            done.update(ids)
            files = tuple(sorted({a for i in ids for a in self.items[i].need if a in self.P and "." in a.rsplit("/", 1)[-1]}))
            text = learner.HEADER + "\n".join(self.items[i].text for i in ids)
            return Directive("read_files", text=text, files=files, route="kd-start", p_follow=self.p_follow)
        if sit.event != "PreToolUse" or len(self.ctx.V) == 0:
            return None
        root = sit.session.cwd
        inp = kd_load.strip(kd_load.render_input(sit.call.tool, sit.call.input if isinstance(sit.call.input, dict) else {}), [root])
        v = kd_embed.enc([inp])[0]
        sims = self.ctx.V @ v
        best, bi = -1.0, -1
        for j in np.argsort(-sims):
            if sims[j] < self.theta:
                break
            i = int(self.ctx.owner[j])
            if i not in done:
                best, bi = float(sims[j]), i
                break
        if bi < 0:
            return None
        done.add(bi)
        return self._directive(self.items[bi], f"kd-{bi}")


GRID = [(th, cap) for th in (0.8, 0.9, 0.95) for cap in (1, 3, 99)]


def main():
    from harness.drivers import split
    corpus = load()
    test = corpus.held_out(model="haiku")
    out = {"grid": {}}
    by_repo = {}
    for th, cap in GRID:
        rep = split(lambda tr, th=th, cap=cap: KDPolicy(tr, theta=th, cap=cap), corpus, model="haiku", name=f"kd theta={th} cap={cap}")
        s = rep.summary()
        by_repo[(th, cap)] = {k: v["usd_saved_share"] for k, v in rep.by(lambda r: r.repo).items()}
        out["grid"][f"theta={th} cap={cap}"] = {k: s[k] for k in ("usd_saved_share", "usd_saved_share_ci", "calls_saved_per_task",
                                                                 "calls_saved_ci", "s_hat_rust", "directives", "useful_rate")}
        print(f"theta={th} cap={cap}: $ {100 * s['usd_saved_share']:+.1f}% calls {s['calls_saved_per_task']:+.2f} "
              f"S^ {s['s_hat_rust']:+.2f} d={s['directives']} useful {s['useful_rate']:.2f}", flush=True)
    repos = sorted(by_repo[GRID[0]])
    chosen = {r: max(GRID, key=lambda g: np.mean([by_repo[g][o] for o in repos if o != r])) for r in repos}
    out["chosen_leave_one_repo_out"] = {r: list(g) for r, g in chosen.items()}
    print("chosen leave-one-repo-out:", chosen)

    def fit(tr, pf=None):
        th, cap = chosen.get(tr[0].repo, (0.9, 1)) if tr else (0.9, 1)
        return KDPolicy(tr, theta=th, cap=cap, p_follow=pf)
    final = split(fit, corpus, model="haiku", name="kd contextual (params chosen leave-one-repo-out)")
    print(final.headline())
    out["final"] = final.to_json()
    half = split(lambda tr: fit(tr, 0.5), corpus, model="haiku", name="kd contextual, p_follow=0.5")
    print(half.headline())
    out["final_p_follow_0.5"] = half.to_json()
    out["loro_note"] = ("facts are per-repository: harness loro() gives the learner no memory of the held-out "
                        "repository, so it would deliver nothing. What must transfer is the method: the delivery "
                        "parameters above are chosen on the other four repositories, and the prior is learned from "
                        "other repositories only.")
    from harness.leakage import scan_memory
    texts = []
    for r in repos:
        texts += KDPolicy(corpus.train(repo=r)).memory_texts()
    out["scan_memory"] = str(scan_memory(texts, test, corpus.train()))
    print("scan_memory:", out["scan_memory"][:300])
    json.dump(out, open(HERE / "results" / "harness.json", "w"), indent=1, default=str)


if __name__ == "__main__":
    main()
