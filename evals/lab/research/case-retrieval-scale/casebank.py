"""Outcome-weighted case-based retrieval (ExpRAG / ReasoningBank style) over grown memory.

Situation (a decision point: after the prompt and after each tool batch) = three potion-8M vectors:
  prompt   the task prompt (600 chars)
  history  the last 5 actions as the agent issued them (+ "[failed]" when the harness flagged it)
  output   the last tool result, Drain-style masked (digits / hex runs -> <*>), 300 chars
Similarity = mean of the three cosines (weights selectable).

Memory = a set of past sessions (any arm). From the memory alone the learner discovers:
  - action identity: the lab's `directive_key` (normalize.py; hand-written, see README);
  - failure: the harness's is_error flag (no error strings);
  - landmarks ("achieving actions"): action keys that passed sessions in memory take successfully
    in many sessions (outcome-weighted session coverage >= cov_min, success rate >= 0.5, >= 2
    sessions). Nothing names a test runner or a command.

Successor targets of a memory situation (session s, step i):
  next   the action taken at i, if it succeeded; weight x exp(-steps-to-next-landmark / 10): what the
         agent did next that led to progress fastest
  skip   the eventual achieving actions: every first-successful occurrence of a landmark at
         i <= t < i + horizon; value = the gap t - i (calls a skip-ahead directive would jump over)
  avoid  the commands that failed in [i, i+3); value = the memory's failure rate of that command
Neighbour weight = sim^2 x (1 if the session passed its hidden tests else fail_w).
Candidate = argmax weighted agreement over the k nearest situations (>= 2 distinct sessions).
"""

from __future__ import annotations

import hashlib
import math
import re
import sys
from dataclasses import dataclass, field
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent.parent))

from data import CACHE, load  # noqa: E402

EQUIV = 0.95
MASK = re.compile(r"0x[0-9a-fA-F]+|[0-9a-f]{7,}|\d+(\.\d+)?")


def mask(t: str) -> str:
    return MASK.sub("<*>", " ".join(t.split()))[:300]


# --------------------------------------------------------------------------------------------
# corpus: sessions, decision points, embeddings, per-step arrays


@dataclass
class Corpus:
    sessions: list
    keys: list  # kid -> directive_key
    kid: list  # per session: np.int32 per step (-1: not directive-eligible)
    succ: list  # per session: bool per step (not failed)
    bash: list  # per session: bool per step: eligible shell command (avoid candidates)
    sit_i: list  # per session: step index of each decision point
    sit_off: np.ndarray  # first situation row of each session
    P: np.ndarray  # per session prompt vectors
    H: np.ndarray  # per situation history vectors
    O: np.ndarray  # per situation output vectors
    sit_sess: np.ndarray = field(default=None)


def _texts(s):
    prompt = " ".join(s.prompt.split())[:600]
    starts = {}
    for i, st in enumerate(s.steps):
        starts.setdefault(st.batch, i)
    its, hs, os_ = [], [], []
    for b in sorted(starts):
        i = starts[b]
        last = s.steps[max(0, i - 5):i]
        hs.append(" | ".join(h.action.text + (" [failed]" if h.failed else "") for h in last) or "start")
        os_.append(("[failed] " if i and s.steps[i - 1].failed else "") + (mask(s.steps[i - 1].out) if i else "start"))
        its.append(i)
    return prompt, its, hs, os_


def build_corpus(model: str = "potion-8M") -> Corpus:
    import os
    os.environ["ADAGE_LAB_CACHE"] = str(CACHE)
    import embed
    ss = load()
    keys, kmap = [], {}
    kid, succ, bash, sit_i = [], [], [], []
    ptxt, htxt, otxt, off = [], [], [], [0]
    for s in ss:
        ks = []
        for st in s.steps:
            a = st.action
            if a.directive_eligible:
                k = a.directive_key
                if k not in kmap:
                    kmap[k] = len(keys)
                    keys.append(k)
                ks.append(kmap[k])
            else:
                ks.append(-1)
        kid.append(np.array(ks, np.int32))
        succ.append(np.array([not st.failed for st in s.steps]))
        bash.append(np.array([st.action.tool == "Bash" and st.action.directive_eligible for st in s.steps]))
        p, its, hs, os_ = _texts(s)
        ptxt.append(p)
        htxt += hs
        otxt += os_
        sit_i.append(its)
        off.append(off[-1] + len(its))
    enc = embed.Cached(model, embed.load(model))
    # learned action equivalence: complete-linkage clusters of the keys' own potion vectors at
    # cosine >= EQUIV, within one tool (the trace's tool name), e.g. `python -m pytest {test_file}`
    # = `python3 -m pytest {test_file}`, `python3 <<heredoc` = `python <<heredoc`.
    from scipy.cluster.hierarchy import fcluster, linkage
    K = enc.encode(keys)
    cls = np.zeros(len(keys), int)
    nxt = 0
    tools = sorted({k.split(" ")[0].split(":")[0] for k in keys})
    freq = np.zeros(len(keys))
    for kk_ in kid:
        np.add.at(freq, kk_[kk_ >= 0], 1)
    for tl in tools:
        ids = [i for i, k in enumerate(keys) if k.split(" ")[0].split(":")[0] == tl]
        if len(ids) == 1:
            lab = np.array([1])
        else:
            D = np.clip(1 - K[ids] @ K[ids].T, 0, 2)
            iu = np.triu_indices(len(ids), 1)
            lab = fcluster(linkage(D[iu], "complete"), 1 - EQUIV, "distance")
        for i, l in zip(ids, lab):
            cls[i] = nxt + l - 1
        nxt += lab.max()
    names = {}
    for c in range(nxt):
        mem = [i for i in range(len(keys)) if cls[i] == c]
        names[c] = keys[max(mem, key=lambda i: freq[i])]
    members = {c: [keys[i] for i in range(len(keys)) if cls[i] == c] for c in range(nxt)}
    kid = [np.where(kk_ >= 0, cls[np.maximum(kk_, 0)], -1).astype(np.int32) for kk_ in kid]
    raw_keys = keys
    keys = [names[c] for c in range(nxt)]
    P = enc.encode(ptxt)
    H = enc.encode(htxt)
    O = enc.encode(otxt)
    enc.save()
    c = Corpus(ss, keys, kid, succ, bash, sit_i, np.array(off), P.astype(np.float32), H.astype(np.float32),
               O.astype(np.float32))
    c.members = members
    c.raw_keys = raw_keys
    c.sit_sess = np.repeat(np.arange(len(ss)), np.diff(c.sit_off))
    return c


# --------------------------------------------------------------------------------------------
# memory


@dataclass
class Params:
    k: int = 20
    w: tuple = (1 / 3, 1 / 3, 1 / 3)  # prompt, history, output
    fail_w: float = 0.5
    cov_min: float = 0.2
    speed_tau: float = 10.0
    outcome: bool = True  # outcome weighting of neighbours
    run_only: bool = False  # skip/next candidates: shell commands only
    horizon: int = 20  # skip: achieving actions within this many calls ahead


class Memory:
    def __init__(self, C: Corpus, idx: list[int], prm: Params):
        self.C, self.prm = C, prm
        self.idx = np.array(sorted(idx))
        nK = len(C.keys)
        wsum = 0.0
        cov = np.zeros(nK)
        nsess = np.zeros(nK)
        occ = np.zeros(nK)
        ok = np.zeros(nK)
        for s in self.idx:
            w = 1.0 if C.sessions[s].passed else prm.fail_w
            wsum += w
            kk, su = C.kid[s], C.succ[s]
            m = kk >= 0
            np.add.at(occ, kk[m], 1)
            np.add.at(ok, kk[m], su[m])
            u = np.unique(kk[m & su])
            cov[u] += w
            nsess[u] += 1
        self.fail_rate = np.where(occ > 0, 1 - ok / np.maximum(occ, 1), 0.0)
        self.lm = (cov / max(wsum, 1e-9) >= prm.cov_min) & (ok / np.maximum(occ, 1) >= 0.5) & (nsess >= 2)
        if prm.run_only:
            self.lm &= np.array([k.startswith("Bash:") for k in C.keys])
        self.n_landmarks = int(self.lm.sum())
        # situation rows
        rows = np.concatenate([np.arange(C.sit_off[s], C.sit_off[s + 1]) for s in self.idx])
        self.rows = rows
        self.sess = C.sit_sess[rows]
        self.Pm = C.P[self.sess]
        self.Hm = C.H[rows]
        self.Om = C.O[rows]
        self.wout = np.array([1.0 if (C.sessions[s].passed or not prm.outcome) else prm.fail_w for s in self.sess])
        # targets per situation
        nk, nw, sk, sg, av = [], [], [], [], []
        for s in self.idx:
            kk, su, ba = C.kid[s], C.succ[s], C.bash[s]
            n = len(kk)
            first = {}
            for t in range(n):
                if kk[t] >= 0 and su[t] and kk[t] not in first:
                    first[kk[t]] = t
            is_ach = np.zeros(n + 1, bool)
            for k_, t in first.items():
                if self.lm[k_]:
                    is_ach[t] = True
            nxt = np.full(n + 1, -1)
            for t in range(n - 1, -1, -1):
                nxt[t] = t if is_ach[t] else nxt[t + 1]
            ach = np.where(is_ach[:n])[0]
            for i in C.sit_i[s]:
                t = nxt[i] if i < n else -1
                gap = (t - i) if t >= 0 else None
                ahead = ach[(ach >= i) & (ach < i + prm.horizon)]
                sk.append(tuple(int(kk[a]) for a in ahead))
                sg.append(tuple(int(a - i) for a in ahead))
                if i < n and kk[i] >= 0 and su[i] and (not prm.run_only or ba[i]):
                    nk.append(kk[i])
                    nw.append(math.exp(-(gap if gap is not None else 30) / prm.speed_tau))
                else:
                    nk.append(-1)
                    nw.append(0.0)
                av.append(tuple({int(kk[j]) for j in range(i, min(n, i + 3)) if ba[j] and not su[j]}))
        self.next_k, self.next_w = np.array(nk), np.array(nw)
        self.skip_k, self.skip_gap = sk, sg
        self.avoid = av
        self.first_rows = np.array([C.sit_off[s] for s in self.idx])

    def nbytes(self) -> int:
        """Stored bytes if shipped: two i16 potion-8M vectors per situation (the prompt vector once
        per session) plus ~16 bytes of targets per situation."""
        d = self.Hm.shape[1]
        return len(self.rows) * (2 * d * 2 + 16) + len(self.idx) * d * 2

    # ----------------------------------------------------------------------------------------
    def query(self, s: int, targets=("next", "skip", "avoid")):
        """Best candidate per target at each decision point of session s:
        {target: [(kid, agreement, value, support) | None per situation]}."""
        C, prm = self.C, self.prm
        r0, r1 = C.sit_off[s], C.sit_off[s + 1]
        wp, wh, wo = prm.w
        sim = (wp * (self.Pm @ C.P[s])[:, None] + wh * (self.Hm @ C.H[r0:r1].T)
               + wo * (self.Om @ C.O[r0:r1].T))
        k = min(prm.k, sim.shape[0])
        top = np.argpartition(-sim, k - 1, axis=0)[:k]
        kk, su = C.kid[s], C.succ[s]
        out = {t: [] for t in targets}
        done = set()
        prev = 0
        for j, i in enumerate(C.sit_i[s]):
            for t in range(prev, i):
                if kk[t] >= 0 and su[t]:
                    done.add(int(kk[t]))
            prev = i
            nb = top[:, j]
            w = np.maximum(sim[nb, j], 0) ** 2 * self.wout[nb]
            tot = w.sum() or 1e-9
            ses = self.sess[nb]
            for tg in targets:
                agg, sup, val = {}, {}, {}
                for n_, wn, sn in zip(nb, w, ses):
                    if tg == "next":
                        c = self.next_k[n_]
                        if c < 0 or c in done:
                            continue
                        cands = ((c, wn * self.next_w[n_], 0.0),)
                    elif tg == "skip":
                        cands = tuple((c, wn, g) for c, g in zip(self.skip_k[n_], self.skip_gap[n_]) if c not in done)
                    else:
                        cands = tuple((c, wn, self.fail_rate[c]) for c in self.avoid[n_])
                    for c, ww, v in cands:
                        agg[c] = agg.get(c, 0) + ww
                        val[c] = val.get(c, 0) + ww * v
                        sup.setdefault(c, set()).add(sn)
                best = None
                for c, a in agg.items():
                    if len(sup[c]) < 2:
                        continue
                    if best is None or a > best[1]:
                        best = (int(c), a / tot, val[c] / max(a, 1e-12), len(sup[c]))
                if best and tg == "avoid":
                    best = (best[0], best[1], float(self.fail_rate[best[0]]), best[3])
                out[tg].append(best)
        return out

    def prior(self, targets=("skip",)):
        """No similarity: every memory session's first decision point votes (outcome-weighted).
        The frequency baseline the lab found as good as kNN at 10 sessions."""
        out = {}
        pos = np.searchsorted(self.rows, self.first_rows)
        w = self.wout[pos]
        tot = w.sum()
        for tg in targets:
            agg, val, sup = {}, {}, {}
            for p_, wn in zip(pos, w):
                if tg == "skip":
                    cv = list(zip(self.skip_k[p_], self.skip_gap[p_]))
                else:
                    cv = [(self.next_k[p_], 0)] if self.next_k[p_] >= 0 else []
                for c, v in cv:
                    agg[c] = agg.get(c, 0) + wn
                    val[c] = val.get(c, 0) + wn * v
                    sup.setdefault(c, set()).add(self.sess[p_])
            best = None
            for c, a in agg.items():
                if len(sup[c]) >= 2 and (best is None or a > best[1]):
                    best = (int(c), a / tot, val[c] / a, len(sup[c]))
            out[tg] = best
        return out

    def summary(self, s: int, m: int = 5, lines: int = 3):
        """Whole-session retrieval: the m memory sessions with the most similar prompts vote
        (sim^2 x outcome) for the achieving actions they took (first successful landmark
        occurrences); the top `lines` with >= 2 supporting sessions, with their agreement."""
        C = self.C
        sims = C.P[self.idx] @ C.P[s]
        order = np.argsort(-sims)[:m]
        agg, sup, pos = {}, {}, {}
        tot = 0.0
        for o in order:
            sidx = self.idx[o]
            w = max(sims[o], 0) ** 2 * (1.0 if C.sessions[sidx].passed or not self.prm.outcome else self.prm.fail_w)
            tot += w
            kk, su = C.kid[sidx], C.succ[sidx]
            seen = set()
            for t in range(len(kk)):
                if kk[t] >= 0 and su[t] and self.lm[kk[t]] and kk[t] not in seen:
                    seen.add(kk[t])
                    agg[kk[t]] = agg.get(kk[t], 0) + w
                    sup.setdefault(kk[t], set()).add(sidx)
                    pos.setdefault(kk[t], []).append(t / max(len(kk), 1))
        cands = [(int(c), a / max(tot, 1e-9), float(np.median(pos[c])), len(sup[c]))
                 for c, a in agg.items() if len(sup[c]) >= 2]
        cands.sort(key=lambda x: -x[1])
        return cands[:lines]
