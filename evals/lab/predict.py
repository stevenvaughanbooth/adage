"""Predictors: the case-based (kNN over embedded situations) one, and n-gram baselines.

Every predictor answers two questions at a decision point:

- `next(sit)`: the single most likely next action, per level (key / template / full), used for
  top-1 accuracy (no abstention);
- `directive(sit)`: the best directive candidate `(directive_key, confidence, support)`. A candidate
  is only ever an action a training session actually took **and that succeeded** there, and it
  must be directive-eligible (a non-exploratory command, or a concrete file read/edit). Candidates
  come from what the neighbours did in their next `horizon` steps, discounted by distance.
"""

from __future__ import annotations

import collections
from dataclasses import dataclass

import numpy as np

from situations import Situation

LEVELS = ("key", "template", "full")


@dataclass
class Params:
    model: str = "potion-8M"
    w_prompt: float = 0.3
    w_hist: float = 0.4
    w_struct: float = 0.3
    k: int = 10
    power: float = 2.0  # similarity sharpening
    fail_weight: float = 0.5  # weight of a neighbour whose session failed its hidden tests
    horizon: int = 5
    decay: float = 0.7
    min_sessions: int = 2  # a directive needs support from this many distinct training sessions
    novel_only: bool = False  # never propose an action the session already took
    conf_sim: bool = False  # confidence = vote share x mean raw similarity of the supporters
    # "next": candidates are what neighbours did in their next `horizon` steps (decayed);
    # "useful": candidates are the actions that WOULD have been useful directives at the
    # neighbour's own decision point (taken later, successfully, after waste), so the confidence
    # estimates P(useful | situation) directly.
    target: str = "next"
    run_only: bool = False  # directives only for commands (not file reads/edits)


def _future(sit: Situation, horizon: int):
    """(distance, step) for the steps from the decision point on."""
    st = sit.session.steps
    return [(j, st[sit.i + j]) for j in range(min(horizon, len(st) - sit.i))]


_UF: dict = {}


def useful_future(sit: Situation) -> dict:
    """{directive_key: distance} for the actions that would have been useful directives at `sit`:
    eligible, first taken at t' >= i successfully, with a failed step or two exploratory steps in
    [i, t') (the lab's usefulness definition, applied to a training situation)."""
    key = (sit.session.sid, sit.i)
    if key in _UF:
        return _UF[key]
    steps = sit.session.steps
    out, seen = {}, set()
    fails = explore = 0
    for t in range(sit.i, len(steps)):
        st = steps[t]
        a = st.action
        if a.directive_eligible and a.directive_key not in seen:
            seen.add(a.directive_key)
            if not st.failed and (fails >= 1 or explore >= 2):
                out[a.directive_key] = t - sit.i
        fails += st.failed
        explore += a.explore
    _UF[key] = out
    return out


def _dense(x):
    return (x.toarray() if hasattr(x, "toarray") else np.asarray(x)).astype(np.float32)


class StructIndex:
    """Vectorised structural similarity (same last key, Jaccard of recent keys, flag agreement)."""

    def __init__(self, vocab: dict):
        self.vocab = vocab

    def feats(self, sits):
        V = len(self.vocab) + 1
        last = np.array([self.vocab.get(s.last_keys[-1], V - 1) if s.last_keys else -1 for s in sits])
        bag = np.zeros((len(sits), V), np.float32)
        for r, s in enumerate(sits):
            for key in set(s.last_keys):
                bag[r, self.vocab.get(key, V - 1)] = 1
        fl = np.array([[float(x) for x in s.flags] for s in sits], np.float32)
        return last, bag, fl

    def sim(self, A, B):
        la, ba, fa = A
        lb, bb, fb = B
        s = 0.4 * ((la[:, None] == lb[None, :]) & (la[:, None] >= 0))
        inter = ba @ bb.T
        union = ba.sum(1)[:, None] + bb.sum(1)[None, :] - inter
        jac = np.where(union > 0, inter / np.maximum(union, 1), 1.0)
        s = s + 0.3 * jac
        agree = np.zeros_like(inter)
        for c in range(fa.shape[1]):
            agree += (fa[:, c][:, None] == fb[:, c][None, :])
        return s + 0.3 * agree / fa.shape[1]


class KNN:
    name = "knn"

    def __init__(self, train_sits: list[Situation], p: Params, embedder):
        self.p = p
        self.train = [s for s in train_sits if s.i < len(s.session.steps)]
        vocab = {}
        for s in self.train:
            for key in s.last_keys:
                vocab.setdefault(key, len(vocab))
        self.si = StructIndex(vocab)
        self.emb = embedder
        if hasattr(embedder, "model") and hasattr(embedder.model, "fit") and not embedder.model.fitted:
            embedder.model.fit([s.prompt_text for s in self.train] + [s.hist_text for s in self.train])
        full = embedder is not None and (p.w_prompt or p.w_hist or getattr(embedder, "always", False))
        self.P = embedder.encode([s.prompt_text for s in self.train]) if full else None
        self.H = embedder.encode([s.hist_text for s in self.train]) if full else None
        self.S = self.si.feats(self.train)
        self.wout = np.array([1.0 if s.session.passed else p.fail_weight for s in self.train], np.float32)

    def components(self, sits: list[Situation]) -> dict:
        """The three similarity matrices (test x memory), computed once and re-weighted per config."""
        out = {}
        if self.P is not None:
            out["P"] = _dense(self.emb.encode([s.prompt_text for s in sits]) @ self.P.T)
        if self.H is not None:
            out["H"] = _dense(self.emb.encode([s.hist_text for s in sits]) @ self.H.T)
        out["S"] = self.si.sim(self.si.feats(sits), self.S).astype(np.float32)
        return out

    def sims(self, sits: list[Situation], comp: dict | None = None) -> np.ndarray:
        p = self.p
        comp = comp or self.components(sits)
        M = np.zeros((len(sits), len(self.train)), np.float32)
        if p.w_prompt:
            M += p.w_prompt * comp["P"]
        if p.w_hist:
            M += p.w_hist * comp["H"]
        if p.w_struct:
            M += p.w_struct * comp["S"]
        return M

    def run(self, sits: list[Situation], comp: dict | None = None):
        """For each situation: ({level: predicted}, (directive_key, conf, support) | None).
        `comp`: precomputed `components(sits)` (the lab's parameter sweep reuses them)."""
        if not sits:
            return []
        M = self.sims(sits, comp)
        p = self.p
        out = []
        k = min(p.k, M.shape[1])
        top = np.argpartition(-M, k - 1, axis=1)[:, :k]
        for r, sit in enumerate(sits):
            idx = top[r]
            w = np.maximum(M[r, idx], 0) ** p.power * self.wout[idx]
            votes = {lv: collections.Counter() for lv in LEVELS}
            dvotes, dsess = collections.Counter(), collections.defaultdict(set)
            dsup = collections.defaultdict(list)
            for j, wt in zip(idx, w):
                nb = self.train[j]
                a = nb.session.steps[nb.i].action
                for lv in LEVELS:
                    votes[lv][getattr(a, lv)] += wt
                if p.target == "useful":
                    fut = useful_future(nb).items()
                else:
                    fut, seen = [], set()
                    for d, st in _future(nb, p.horizon):
                        a = st.action
                        if st.failed or not a.directive_eligible or a.directive_key in seen:
                            continue
                        seen.add(a.directive_key)
                        fut.append((a.directive_key, d))
                for dk, d in fut:
                    dvotes[dk] += wt * (p.decay ** d if p.target == "next" else 1.0)
                    dsess[dk].add(nb.session.sid)
                    dsup[dk].append(j)
            pred = {lv: (votes[lv].most_common(1)[0][0] if votes[lv] else None) for lv in LEVELS}
            tot = float(w.sum())
            cand = None
            done = {st.action.directive_key for st in sit.session.steps[:sit.i]} if p.novel_only else ()
            for key, v in dvotes.most_common():
                if p.run_only and not key.startswith("Bash:"):
                    continue
                if len(dsess[key]) >= p.min_sessions and key not in done:
                    conf = v / tot if tot > 0 else 0.0
                    if p.conf_sim:
                        conf *= float(np.mean([M[r, j] for j in dsup[key]]))
                    cand = (key, conf, len(dsess[key]))
                    break
            out.append((pred, cand))
        return out


class NGram:
    """Order-n Markov over templates (the situation is only the last n action templates), with
    back-off to lower orders. Directive candidates: what followed the same context within the
    horizon in training, confidence = share of the context's occurrences."""

    def __init__(self, train_sits: list[Situation], order: int, horizon: int = 5, decay: float = 0.7,
                 min_sessions: int = 2, target: str = "next", run_only: bool = False):
        self.name = f"order-{order}" + ("-useful" if target == "useful" else "")
        self.run_only = run_only
        self.order = order
        self.nxt = {lv: collections.defaultdict(collections.Counter) for lv in LEVELS}
        self.dir = collections.defaultdict(collections.Counter)
        self.dsess = collections.defaultdict(lambda: collections.defaultdict(set))
        self.count = collections.Counter()
        self.min_sessions = min_sessions
        for s in train_sits:
            steps = s.session.steps
            if s.i >= len(steps):
                continue
            for n in range(order + 1):
                ctx = self.ctx(s, n)
                if ctx is None:
                    continue
                a = steps[s.i].action
                for lv in LEVELS:
                    self.nxt[lv][ctx][getattr(a, lv)] += 1
                self.count[ctx] += 1
                if target == "useful":
                    fut = [(k, 1.0) for k in useful_future(s)]
                else:
                    fut, seen = [], set()
                    for d, st in _future(s, horizon):
                        a = st.action
                        if st.failed or not a.directive_eligible or a.directive_key in seen:
                            continue
                        seen.add(a.directive_key)
                        fut.append((a.directive_key, decay ** d))
                for k, wt in fut:
                    self.dir[ctx][k] += wt
                    self.dsess[ctx][k].add(s.session.sid)

    @staticmethod
    def ctx(s: Situation, n: int):
        steps = s.session.steps[:s.i]
        if len(steps) < n:
            return None
        return (n,) + tuple(st.action.template for st in steps[len(steps) - n:])

    def run(self, sits):
        out = []
        for s in sits:
            pred, cand = {}, None
            for lv in LEVELS:
                for n in range(self.order, -1, -1):
                    c = self.ctx(s, n)
                    if c is not None and self.nxt[lv].get(c):
                        pred[lv] = self.nxt[lv][c].most_common(1)[0][0]
                        break
                else:
                    pred[lv] = None
            for n in range(self.order, -1, -1):
                c = self.ctx(s, n)
                if c is None or not self.dir.get(c):
                    continue
                for key, v in self.dir[c].most_common():
                    if self.run_only and not key.startswith("Bash:"):
                        continue
                    if len(self.dsess[c][key]) >= self.min_sessions:
                        cand = (key, v / self.count[c], len(self.dsess[c][key]))
                        break
                if cand:
                    break
            out.append((pred, cand))
        return out
