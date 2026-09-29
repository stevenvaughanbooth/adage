"""The learned-outcome reflex learner, as a harness learner: `fit(train_sessions) -> policy`.

No hand-written string list, tool taxonomy or regex anywhere in it:
  1. Outcome model: potion-8M embedding of the output and of the action, the tool name as an
     opaque categorical, output length → logistic regression taught by the harness's exit status
     (`is_error`) on the training sessions. At runtime it reads the output, so a failure the exit
     status hides (`… 2>&1 | tail`) is still seen. Optionally Drain line templates (`drain=True`).
  2. Situations: k-means over the training outputs' embeddings (the "template dictionary").
  3. Reflexes: for a training call the model scores as a failure, the agent's own way out: the
     first later call (≤ 15) with the same intent (action cosine ≥ 0.8) that the model scores as a
     success. Kept per output cluster when ≥ 2 training sessions support the same way out and it
     the agent got past ≥ 50% of that cluster's failures at all.
  4. Delivery:
       post-hoc    at PostToolUse of a call the model scores as a failure, in a reflex's cluster,
                   whose action resembles the reflex's failing actions (cos ≥ 0.6);
       predictive  at PreToolUse, from the action alone: cos ≥ 0.9 to an action that failed into
                   a reflex's cluster in training (no hindsight), and not already the way out.
     Each reflex fires at most once per session.
"""

from __future__ import annotations

import sys
from collections import defaultdict
from pathlib import Path

import re

import numpy as np
from sklearn.cluster import MiniBatchKMeans
from sklearn.linear_model import LogisticRegression

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parents[1]))  # evals/lab
sys.path.insert(0, str(HERE.parent))  # evals/lab/research
import embed  # noqa: E402
from harness import Directive  # noqa: E402

_M = None
PIPE = re.compile(r"(?<![|])[|](?![|])")


def model():
    global _M
    if _M is None:
        _M = embed.load("potion-8M")
    return _M


def arg_of(c) -> str:
    i = c.input or {}
    if c.tool == "Bash":
        return str(i.get("command", ""))
    if c.tool in ("Grep", "Glob"):
        return f"{i.get('pattern', '')} {i.get('path', '') or ''}"
    return str(i.get("file_path") or i.get("notebook_path") or "")


def out_text(o: str) -> str:
    o = (o or "").strip()
    return o if len(o) <= 1000 else o[:600] + "\n" + o[-400:]


def act_text(c) -> str:
    return f"{c.tool} {arg_of(c)[:300]}"


class OutcomeModel:
    def __init__(self):
        self.vocab = {}

    def X(self, eo, ea, tools, lens):
        T = np.zeros((len(tools), len(self.vocab) + 1), np.float32)
        for i, t in enumerate(tools):
            T[i, self.vocab.get(t, len(self.vocab))] = 1
        ln = np.c_[np.log1p(lens) / 10, np.asarray(lens) == 0].astype(np.float32)
        return np.c_[eo, ea, T, ln]

    def fit(self, eo, ea, tools, lens, y):
        self.vocab = {t: i for i, t in enumerate(sorted(set(tools)))}
        self.lr = LogisticRegression(C=1.0, max_iter=2000, class_weight="balanced")
        self.lr.fit(self.X(eo, ea, tools, lens), y)
        return self

    def p(self, eo, ea, tools, lens):
        return self.lr.predict_proba(self.X(eo, ea, tools, lens))[:, 1]


class LearnedOutcomePolicy:
    events = {"PreToolUse", "PostToolUse"}

    def __init__(self, sessions, k=64, theta=0.5, H=15, tau_int=0.8, tau_trig=0.6, tau_pre=0.9,
                 min_sessions=2, min_rate=0.5, mode="post-hoc", seed=0):
        self.theta, self.tau_trig, self.tau_pre, self.mode = theta, tau_trig, tau_pre, mode
        rows = [(si, j, c) for si, s in enumerate(sessions) for j, c in enumerate(s.calls)]
        self.reflexes = []
        if len(rows) < 20:
            return
        m = model()
        eo = m.encode([out_text(c.output) for _, _, c in rows])
        ea = m.encode([act_text(c) for _, _, c in rows])
        tools = [c.tool for _, _, c in rows]
        lens = [c.out_chars or len(c.output) for _, _, c in rows]
        y = np.array([int(c.is_error) for _, _, c in rows])
        if y.sum() < 3:
            return
        # a pipeline's exit status is its last stage's: a piped call's is_error does not label its
        # output, so the teacher skips those calls (one shell-grammar token; see eval2.py)
        clean = np.array([not (c.tool == "Bash" and PIPE.search(arg_of(c))) for _, _, c in rows])
        self.om = OutcomeModel().fit(eo[clean], ea[clean], [t for t, k in zip(tools, clean) if k],
                                     [l for l, k in zip(lens, clean) if k], y[clean])
        pf = np.maximum(self.om.p(eo, ea, tools, lens), y)
        kk = min(k, max(2, len(rows) // 20))
        self.km = MiniBatchKMeans(kk, random_state=seed, n_init=3, batch_size=2048).fit(eo)
        cl = self.km.predict(eo)
        sid = np.array([si for si, _, _ in rows])
        ev = defaultdict(list)
        for i in range(len(rows)):
            if pf[i] <= theta:
                continue
            resp = None
            for j in range(i + 1, min(len(rows), i + 1 + H)):
                if sid[j] != sid[i]:
                    break
                if rows[j][2].tool == "Bash" and float(ea[i] @ ea[j]) >= tau_int and pf[j] < 0.5:
                    resp = j
                    break
            if rows[i][2].tool == "Bash":
                ev[cl[i]].append((sid[i], i, resp))
        for c, lst in ev.items():
            wr = [x for x in lst if x[2] is not None]
            if not wr:
                continue
            best, sup = None, 0
            for _, _, j in wr:
                s_ = {s for s, _, jj in wr if float(ea[j] @ ea[jj]) >= tau_int}
                if len(s_) > sup:
                    best, sup = j, len(s_)
            # recoverable rate: the share of this cluster's failures the agent got past at all
            rate = np.mean([x[2] is not None for x in lst])
            group = [jj for _, _, jj in wr if float(ea[best] @ ea[jj]) >= tau_int]
            best = min(group, key=lambda jj: len(arg_of(rows[jj][2])))  # the most general spelling
            if sup >= min_sessions and rate >= min_rate:
                trig = np.stack([ea[i] for _, i, _ in lst])
                cen = trig.mean(0)
                cen /= np.linalg.norm(cen) + 1e-9
                self.reflexes.append({"cluster": int(c), "centroid": cen, "triggers": trig,
                                      "response": arg_of(rows[best][2]), "response_vec": ea[best],
                                      "support": sup, "rate": float(rate)})

    def memory_texts(self):
        return [r["response"] for r in self.reflexes]

    def directive(self, r):
        return Directive("run", command=r["response"], route=f"lo-{r['cluster']}",
                         text=f"That did not work here. In this repository the way past it was:\n  {r['response']}")

    def __call__(self, sit):
        if not self.reflexes or sit.call is None or sit.call.tool != "Bash":
            return None
        fired = sit.state.setdefault("fired", set())
        m = model()
        ea = m.encode([act_text(sit.call)])[0]
        if sit.event == "PreToolUse":
            if self.mode != "predictive":
                return None
            for r in self.reflexes:
                if r["cluster"] in fired or float(ea @ r["response_vec"]) >= 0.8:
                    continue
                if float((r["triggers"] @ ea).max()) >= self.tau_pre:
                    fired.add(r["cluster"])
                    return self.directive(r)
            return None
        # PostToolUse
        eo = m.encode([out_text(sit.call.output)])[0]
        p = self.om.p(eo[None], ea[None], [sit.call.tool], [sit.call.out_chars or len(sit.call.output)])[0]
        if max(p, float(sit.failed)) <= self.theta:
            return None
        c = int(self.km.predict(eo[None].astype(np.float32))[0])
        for r in self.reflexes:
            if r["cluster"] == c and c not in fired and float(ea @ r["centroid"]) >= self.tau_trig:
                fired.add(c)
                return self.directive(r)
        return None


def fitter(mode="post-hoc", **kw):
    return lambda sessions: LearnedOutcomePolicy(sessions, mode=mode, **kw)
