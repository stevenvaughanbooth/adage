#!/usr/bin/env python3
"""Score the hindsight-shortcut learner with the sprint's common harness (`evals/lab/research/harness`).

  cd evals/lab/research && OMP_NUM_THREADS=1 nice -n 10 python3 hindsight-shortcuts/harness_eval.py

`fit(train_sessions)` (harness `Session`s) labels the training traces in hindsight, builds the library
of shortcut classes and the kNN over training decision points, tunes θ by leave-one-task-out on the
training traces (assumed misfire cost 2 calls, the harness default), and returns a policy. At
`SessionStart` and after every tool batch (`PostToolBatch`) the policy embeds the situation and
delivers the best-scoring class once per session:

- a Bash command class -> `Directive("run", command=...)` (Sat: a later successful call with its first
  two words, or the goal class for a test runner; W: failed calls / environment friction);
- a file class -> `Directive("read_files", files=...)` (Sat: a Read/Edit of one; W: searches and
  listings) — uncalibrated, so also reported at p_follow 0.5;
- a fact class -> `Directive("advice")` (costed, never credited).

The harness's Sat/W classes are its hand-written measurement instrument; the learner never sees them.
"""

from __future__ import annotations

import json
import sys
from collections import defaultdict
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent))

from harness import Directive, load as hload, score  # noqa: E402
from harness.drivers import curve, loro, split  # noqa: E402
from harness.drivers import _pooled  # noqa: E402
from harness.leakage import scan_memory  # noqa: E402
from harness.policies.u11 import U11  # noqa: E402

import load as L  # noqa: E402
from hindsight import Idf, label, learn_writers, toks  # noqa: E402
from matcher import Embedder, Situations, hist_text, sim  # noqa: E402
from experiment import Memory, tune  # noqa: E402

EMB = Embedder("potion-8M")
_CF = None
_BG = None


def BACKGROUND():
    """Training-phase sessions of every repository (never a held-out task)."""
    global _BG
    if _BG is None:
        _BG = [h for h in hload().train() if "haiku" in h.model]
    return _BG


def changed_files() -> dict:
    """sid -> the run's final diff files (state.json), for labelling training traces only."""
    global _CF
    if _CF is None:
        _CF = {s.sid: s.changed_files for s in L.load()}
    return _CF


def convert(h) -> L.Session:
    """A harness Session -> this package's Session (same transcript, the harness's loader)."""
    batch_of = {}
    for b, r in enumerate(h.responses):
        for k in r.calls:
            batch_of[k] = b
    calls = []
    for k, c in enumerate(h.calls):
        b = batch_of.get(k, max(batch_of.values(), default=0))
        r = h.responses[b] if h.responses else None
        gen = (r.output / max(len(r.calls), 1)) if r else 0.0
        out = L.strip_root(c.output or "")
        calls.append(L.Call(c.tool, c.input or {}, L.render_input(c.tool, c.input or {}), out[:L.OUT_CAP],
                            c.out_chars or len(out), bool(c.is_error), b, gen))
    usage = [(float(r.input), float(r.cache_write), float(r.cache_read), float(r.output)) for r in h.responses]
    return L.Session(h.sid, f"{h.source}/{h.slice}", h.repo, h.model, h.run, h.phase, h.arm, h.task, h.passed,
                     float(h.cost_usd or 0.0), changed_files().get(h.sid, []), L.strip_root(h.prompt or ""),
                     calls, len(h.responses), [c.batch + 1 for c in calls], usage)


class View:
    """A minimal stand-in for Situations over one live decision point."""

    def __init__(self, prompt, calls, i, writers, tools):
        s = L.Session("q", "", "", "", "", "", "", "", None, 0.0, [], prompt, calls)
        errs = sum(c.is_error for c in calls[:i])
        wr = any(c.tool in writers for c in calls[:i])
        last = calls[i - 1].tool if i > 0 else "<start>"
        self.P = EMB([" ".join(prompt.split())[:1000]])
        self.H = EMB([hist_text(s, i)])
        self.S = np.array([[np.log1p(i) / 4.0, errs / max(i, 1), 1.0 if wr else 0.0] +
                           [1.0 if last == t else 0.0 for t in tools]], np.float32)


class ShortcutPolicy:
    events = {"SessionStart", "PostToolBatch"}

    def __init__(self, mem: Memory, theta, writers, tools, k=20):
        self.mem, self.theta, self.writers, self.tools, self.k = mem, theta, writers, tools, k
        self.eligible = [c.cid for c in mem.lib.classes if len(c.sessions) >= 2]
        self.stored_texts = [mem.lib.classes[c].rep.text for c in self.eligible]

    def directive(self, cid):
        rep = self.mem.lib.classes[cid].rep
        route = f"hs-{rep.kind}-{cid}"
        if rep.fam == "cmd" and rep.tool == "Bash":
            sess = self.mem.by_sid[rep.sid]
            cmd = sess.calls[rep.content[0]].inp_text  # a procedure's first repair, else the command
            cmd = cmd.splitlines()[0][:200] if cmd else ""
            return Directive("run", command=cmd, text=rep.text[:300], route=route)
        paths = sorted(t for t in rep.key if "/" in t or "." in t)
        if rep.fam == "file" and paths:
            return Directive("read_files", files=tuple(paths[:3]), text=rep.text[:300], route=route)
        return Directive("advice", text=rep.text[:300], route=route)

    def __call__(self, sit):
        if self.theta is None or not self.eligible:
            return None
        st = sit.state
        if "calls" not in st:
            st["calls"], st["done"] = [], set()
        hist = sit.history
        calls = st["calls"]
        for c in hist[len(calls):]:
            out = L.strip_root(c.output or "")
            calls.append(L.Call(c.tool, c.input or {}, L.render_input(c.tool, c.input or {}), out[:L.OUT_CAP],
                                c.out_chars or len(out), bool(c.is_error), 0, 0.0))
        i = len(calls)
        q = View(L.strip_root(sit.session.prompt or ""), calls, i, self.writers, self.tools)
        m = self.mem.sits
        row = sim(q, [0], m)[0]
        kk = min(self.k, len(row))
        nn = np.argpartition(-row, kk - 1)[:kk]
        w = np.maximum(row[nn], 0) ** 2
        tot = w.sum()
        if tot <= 0:
            return None
        sc = defaultdict(float)
        for wi, r in zip(w, nn):
            for c in self.mem.labels[r]:
                sc[c] += wi / tot
        cands = [(v, c) for c, v in sc.items() if v >= self.theta and c not in st["done"]
                 and len(self.mem.lib.classes[c].sessions) >= 2]
        if not cands:
            return None
        v, c = max(cands)
        st["done"].add(c)
        return self.directive(c)


def make_fit(lam: float = 2.0, log=None):
    def fit(train):
        mine = [convert(h) for h in train]
        repos = {s.repo for s in mine}
        # token weights need other repositories as background (a single repository's own hot file
        # would otherwise weigh nothing); only training-phase sessions of *other* repositories
        bg = [convert(h) for h in BACKGROUND() if h.repo not in repos] if len(repos) == 1 else []
        idf = Idf(mine + bg)
        writers = learn_writers(mine, min_calls=5)
        tools = sorted({c.tool for s in mine for c in s.calls}) + ["<start>"]
        labs = {s.sid: label(s, idf, writers) for s in mine}
        mem = Memory(mine, labs, idf, EMB, writers, tools)
        th = tune(mem).get(f"knn|{lam}")
        if log is not None:
            log.append({"n_train": len(mine), "repos": sorted({s.repo for s in mine}), "theta": th,
                        "classes": len(mem.lib.classes),
                        "eligible": sum(1 for c in mem.lib.classes if len(c.sessions) >= 2)})
        return ShortcutPolicy(mem, th, writers, tools)
    return fit


def headline_json(rep):
    j = rep.to_json()
    return {k: j[k] for k in j if k in ("name", "test_set_id", "summary", "assumptions")} if isinstance(j, dict) else j


def main():
    corpus = hload()
    test = corpus.held_out(model="haiku")
    test_v2 = corpus.held_out(model="haiku", source="v2")
    out = {}
    fitlog = []
    fit = make_fit(2.0, fitlog)

    # the headline: train split -> held-out Haiku control (v1+v2), and v2 alone (the Rust baseline's 82)
    for name, f, kw in [("split", fit, {}), ("split_pf0.5", fit, {"p_follow": 0.5}),
                        ("split_lam0", make_fit(0.0), {}), ("split_lam0_misfire0", make_fit(0.0), {"misfire_calls": 0.0})]:
        rep = split(f, corpus, model="haiku", name=name, **kw)
        print(rep.headline(), flush=True)
        out[name] = rep.to_json()
    # v2-only to compare with U11 on the Rust table's 82 sessions
    reps = []
    for sl in sorted({s.slice for s in test_v2}):
        t = [s for s in test_v2 if s.slice == sl]
        pol = fit(corpus.train(repo=t[0].repo))
        reps.append((sl, pol, t))
    v2reps = [score(p, t, name=f"v2:{sl}") for sl, p, t in reps]
    rv2 = _pooled("split-v2", v2reps, v2reps[0].assumptions, 0.90)
    print(rv2.headline(), flush=True)
    out["split_v2"] = rv2.to_json()
    u = score(U11("post-hoc"), test_v2, name="U11 post-hoc (v2)")
    print(u.headline(), flush=True)
    out["u11_posthoc_v2"] = u.to_json()
    # U11 + shortcuts together (does the lever add to today's system?)
    both = []
    for sl, p, t in reps:
        u11 = U11("post-hoc")

        def combo(sit, p=p, u11=u11):
            d = u11(sit)
            return d if d is not None else (p(sit) if sit.event in p.events else None)
        both.append(score(combo, t, name=f"u11+hs:{sl}"))
    rb = _pooled("U11 + shortcuts (v2)", both, both[0].assumptions, 0.90)
    print(rb.headline(), flush=True)
    out["u11_plus_shortcuts_v2"] = rb.to_json()

    rep = loro(fit, corpus, model="haiku", pool="all", name="loro")
    print(rep.headline(), flush=True)
    out["loro"] = rep.to_json()
    cv = curve(fit, corpus, sizes=(2, 5, 10), model="haiku")
    out["curve"] = {str(n): r.to_json() for n, r in cv.items()}
    for n, r in cv.items():
        print(r.headline(), flush=True)
    # grown memory in the same repository (train + every session of the other held-out tasks)
    from harness.drivers import leave_task_out
    for lam in (0.0, 2.0):
        r = leave_task_out(make_fit(lam), corpus, model="haiku", name=f"leave-task-out λ={lam}")
        print(r.headline(), flush=True)
        out[f"leave_task_out_lam{lam}"] = r.to_json()
    # leakage: scan what the learner stores (the delivered texts)
    texts = []
    for sl, p, t in reps:
        texts += p.stored_texts
    try:
        out["scan_memory"] = scan_memory(texts, test, corpus.train())
    except Exception as e:  # pragma: no cover
        out["scan_memory"] = f"error: {e}"
    out["fits"] = fitlog
    EMB.save()
    (HERE / "harness_results.json").write_text(json.dumps(out, indent=1, default=str))
    print("wrote", HERE / "harness_results.json")


if __name__ == "__main__":
    main()
