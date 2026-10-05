"""Situations, their learned similarity, and the situation -> shortcut matcher.

A decision point comes before each batch of tool calls (one assistant message). Its situation:

- the task prompt, embedded (potion-base-8M, the model Adage ships);
- the recent history, embedded: the last 5 calls' inputs, whether the harness flagged them, and the
  first line of the last two outputs;
- structure: log step, share of flagged calls so far, whether a (learned) writer tool has run, and the
  last tool name (the trace's own schema; no taxonomy).

Similarity = W_P cos(prompt) + W_H cos(history) + W_S struct. The matcher is a similarity-weighted
kNN over *memory* decision points, each labelled in hindsight with the shortcut classes that would
have saved calls there (a class c is positive at point i of memory session m if an instance of c in m
has t* >= i and D ∩ [i, t*) non-empty). score(c | x) = Σ w·1[c positive] / Σ w over the k nearest
points from other tasks; a class is delivered once per session, at most one per decision point, when
score >= θ and it is supported by >= 2 memory sessions of other tasks.
"""

from __future__ import annotations

import hashlib
import sys
from collections import defaultdict
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))  # evals/lab
import embed  # noqa: E402

CACHE = Path("/tmp/adage-research/hindsight-shortcuts")
W_P, W_H, W_S = 0.3, 0.5, 0.2
K = 20
POWER = 2.0


def points(s) -> list[int]:
    out, seen = [], set()
    for i, c in enumerate(s.calls):
        if c.batch not in seen:
            seen.add(c.batch)
            out.append(i)
    return out


def hist_text(s, i: int) -> str:
    last = s.calls[max(0, i - 5):i]
    parts = []
    for j, c in enumerate(last):
        t = c.inp_text[:150].replace("\n", " ")
        if c.is_error:
            t += " [flagged]"
        if j >= len(last) - 2:
            first = next((l for l in c.out.splitlines() if l.strip()), "")[:120]
            t += " -> " + first
        parts.append(t)
    return " | ".join(parts) or "start of session"


class Embedder:
    def __init__(self, name: str = "potion-8M"):
        self.name = name
        self.model = embed.load(name)
        self.path = CACHE / f"emb-{name}.npz"
        self.mem = {}
        if self.path.exists():
            z = np.load(self.path)
            self.mem = {k: z[k] for k in z.files}
        self.dirty = False

    def __call__(self, texts):
        keys = [hashlib.sha1(t.encode()).hexdigest() for t in texts]
        miss = sorted({k: t for k, t in zip(keys, texts) if k not in self.mem}.items())
        if miss:
            V = self.model.encode([t for _, t in miss])
            for (k, _), v in zip(miss, V):
                self.mem[k] = v.astype(np.float16)
            self.dirty = True
        return np.stack([self.mem[k].astype(np.float32) for k in keys])

    def save(self):
        if self.dirty:
            tmp = self.path.with_suffix(".tmp.npz")
            np.savez(tmp, **self.mem)
            tmp.replace(self.path)  # atomic: a killed run never leaves a torn cache
            self.dirty = False


class Situations:
    """All decision points of a set of sessions, with their vectors."""

    def __init__(self, sessions, emb: Embedder, writers: set, tools: list):
        self.rows = []  # (sid, task, i)
        P, Hh, St = [], [], []
        ptext = {}
        for s in sessions:
            ptext[s.sid] = " ".join(s.prompt.split())[:1000]
            errs = np.cumsum([0] + [c.is_error for c in s.calls])
            wr = np.cumsum([0] + [c.tool in writers for c in s.calls])
            for i in points(s):
                self.rows.append((s.sid, s.task, i))
                P.append(ptext[s.sid])
                Hh.append(hist_text(s, i))
                last = s.calls[i - 1].tool if i > 0 else "<start>"
                oh = [1.0 if last == t else 0.0 for t in tools]
                St.append([np.log1p(i) / 4.0, errs[i] / max(i, 1), 1.0 if wr[i] else 0.0] + oh)
        self.P = emb(P) if P else np.zeros((0, 256), np.float32)
        self.H = emb(Hh) if Hh else np.zeros((0, 256), np.float32)
        self.S = np.array(St, np.float32)
        self.index = defaultdict(list)
        for r, (sid, _, _) in enumerate(self.rows):
            self.index[sid].append(r)


def sim(q: Situations, qr, m: Situations) -> np.ndarray:
    """Similarity of query rows `qr` of q to every row of m: shape (len(qr), len(m.rows))."""
    cp = q.P[qr] @ m.P.T
    ch = q.H[qr] @ m.H.T
    a, b = q.S[qr], m.S
    d_num = np.abs(a[:, None, :3] - b[None, :, :3])
    num = 1.0 - np.minimum(d_num, 1.0).mean(-1)
    same_tool = (a[:, None, 3:] * b[None, :, 3:]).sum(-1)
    st = 0.75 * num + 0.25 * same_tool
    return W_P * cp + W_H * ch + W_S * st


def scores(q: Situations, sid: str, m: Situations, mlabels: list, task: str, k: int = K) -> list[dict]:
    """For each decision point of session `sid` (in q), {class: score} from the k nearest memory
    points of other tasks. mlabels[r] = {cid: value} for memory row r."""
    qr = q.index[sid]
    if not qr or not len(m.rows):
        return [{} for _ in qr]
    Sm = sim(q, qr, m)
    bad = np.array([t == task for (_, t, _) in m.rows])
    Sm[:, bad] = -np.inf
    out = []
    kk = min(k, int((~bad).sum()))
    if kk <= 0:
        return [{} for _ in qr]
    for row in Sm:
        nn = np.argpartition(-row, kk - 1)[:kk]
        w = np.maximum(row[nn], 0) ** POWER
        tot = w.sum()
        sc = defaultdict(float)
        if tot > 0:
            for wi, r in zip(w, nn):
                for c in mlabels[r]:
                    sc[c] += wi / tot
        out.append(dict(sc))
    return out
