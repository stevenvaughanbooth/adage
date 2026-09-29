"""Distil a repository card from past sessions, and a contextual (situation-keyed) index of the same facts.

Input: the discovery events (`facts.extract`) of a repository's memory sessions (the train split).

1. **Candidate facts.** Each event's needed atom set is a candidate: a locator fact (atoms some call
   discovered and later calls used) or a fix fact (a failure and what the recovery changed).
2. **Roughly the same fact.** Fix facts are clustered by potion-8M embedding of their grounded text
   ("<failing call> -> <fix>"), average linkage, cosine >= `tau_c`; a cluster's facts pool their
   support and it is rendered by its medoid. Locator facts are paths/identifiers, which are the
   same fact only when identical, so their atoms are counted exactly.
3. **Value.** A fact's value is the calls (and $) it would have saved in memory sessions of *other
   tasks* (leave-task-out support >= `m`): re-discovered knowledge, not the one session that
   found it. Facts are chosen greedily by marginal saved calls per token, until the marginal
   saving no longer pays for carrying its tokens (in calls, at the memory sessions' mean $/call and
   cache-read price) or the token budget is reached.
4. **Rendering.** Verbatim from the trace, no LLM: "`<discovering call>` → atoms" and "`<failing
   call>` failed; what worked: `<fix>`".
5. **Contextual index.** Every selected (or merely valuable) fact keeps the situations in which it
   was needed: the embedding of the call issued just before its first obviated call (or session
   start). At PreToolUse of a held-out call the index returns the facts whose situation cosine
   is >= theta; each is delivered once.
"""

from __future__ import annotations

from collections import defaultdict
from dataclasses import dataclass, field

import numpy as np

import facts
import kd_embed
import kd_load
from common import dollars

HEADER = "Repository notes learned from earlier sessions in this repo:\n"


@dataclass
class Item:
    kind: str
    need: frozenset
    text: str
    support: set = field(default_factory=set)  # (sid, event idx)
    tasks: set = field(default_factory=set)
    triggers: list = field(default_factory=list)  # situation texts ("" = session start)
    gain: float = 0.0

    @property
    def tokens(self) -> float:
        return len(self.text) / 4 + 2


def render(e: facts.Event, atoms_needed) -> str:
    if e.kind == "chain":
        return f"- `{e.fail_inp[:140]}` failed here; what worked: `{e.rec_inp[:200]}`"
    return f"- `{e.text[:100]}` shows: " + ", ".join(sorted(a for a in atoms_needed if not a.startswith("NOT:"))[:12])


def candidates(mem, E, tau_c: float = 0.85) -> list[Item]:
    items: dict = {}
    chains = []
    for s in mem:
        for i, e in enumerate(E[s.sid]):
            trig = s.calls[e.trigger - 1].inp if e.trigger > 0 else ""
            if e.kind == "chain":
                chains.append((s, i, e, trig))
                continue
            key = ("loc", e.need)
            it = items.setdefault(key, Item("locator", e.need, render(e, e.need)))
            it.support.add((s.sid, i))
            it.tasks.add(s.task)
            it.triggers.append(trig)
    if chains:
        V = kd_embed.enc([f"{e.fail_inp[:200]} -> {e.rec_inp[:200]}" for _, _, e, _ in chains])
        n = len(chains)
        # average-linkage agglomerative clustering on cosine (small n: plain O(n^3) is fine)
        cl = [[i] for i in range(n)]
        S = V @ V.T
        while True:
            best, bi, bj = -1, -1, -1
            for a in range(len(cl)):
                for b in range(a + 1, len(cl)):
                    m = float(S[np.ix_(cl[a], cl[b])].mean())
                    if m > best:
                        best, bi, bj = m, a, b
            if best < tau_c or bi < 0:
                break
            cl[bi] += cl.pop(bj)
        for c in cl:
            sub = S[np.ix_(c, c)].mean(1)
            med = c[int(np.argmax(sub))]
            s0, _, e0, _ = chains[med]
            it = Item("chain", e0.need, render(e0, e0.need))
            for i in c:
                s, j, e, trig = chains[i]
                it.support.add((s.sid, j))
                it.tasks.add(s.task)
                it.triggers.append(trig)
                # a cluster member's own need also counts as covered by this fact's atoms if
                # the medoid's text contains it (checked at scoring time via atoms of the text)
            items[("chain", med)] = it
    return list(items.values())


def per_call_usd(sessions) -> float:
    return float(np.mean([s.cost / max(1, len(s.calls)) for s in sessions]))


def mean_requests(sessions) -> float:
    return float(np.mean([s.n_requests for s in sessions]))


def select(mem, E, items: list[Item], m: int = 1, budget: float = 1e9, value_usd: bool = False):
    """Greedy budgeted max-coverage over memory sessions with leave-task-out support."""
    _, _, cw, cr = kd_load.RATES["haiku"]
    ucall = per_call_usd(mem)
    nreq = mean_requests(mem)
    carry_calls_per_tok = (cw + cr * nreq) / 1e6 / ucall  # carrying 1 token for a session, in calls
    item_atoms = [facts.atoms(it.text) for it in items]
    # atom -> tasks it is supported by (through the items that contain it)
    chosen, A_by_item = [], []
    covered = {s.sid: set() for s in mem}  # obviated calls so far
    task_of = {s.sid: s.task for s in mem}
    used = 0.0

    def allowed_atoms(sid, sel):
        out = set()
        for j in sel:
            if len(items[j].tasks - {task_of[sid]}) >= m:
                out |= item_atoms[j]
        return out

    def obv(sel):
        tot = {}
        for s in mem:
            A = allowed_atoms(s.sid, sel)
            ks = set()
            for e in E[s.sid]:
                if facts.covers(A, e.need):
                    ks |= set(e.calls)
            tot[s.sid] = ks
        return tot

    base = obv([])
    cur = sum(len(v) for v in base.values())
    remaining = [j for j, it in enumerate(items) if len(it.tasks) >= max(1, m + 1 if m else 1)]
    while remaining:
        best, bj, bval = 0.0, -1, 0.0
        for j in remaining:
            if used + items[j].tokens > budget:
                continue
            o = obv(chosen + [j])
            val = sum(len(v) for v in o.values()) - cur
            net = val / len(mem) - items[j].tokens * carry_calls_per_tok
            if net > 0 and net / items[j].tokens > best:
                best, bj, bval = net / items[j].tokens, j, val
        if bj < 0:
            break
        chosen.append(bj)
        items[bj].gain = bval / len(mem)
        used += items[bj].tokens
        cur += bval
        remaining.remove(bj)
    return [items[j] for j in chosen]


def card_text(sel: list[Item]) -> str:
    if not sel:
        return ""
    return HEADER + "\n".join(it.text for it in sel) + "\n"


class Contextual:
    """Situation-keyed retrieval of facts at PreToolUse (cosine of the proposed call's input)."""

    def __init__(self, items: list[Item], theta: float):
        self.items = items
        self.theta = theta
        self.start = [i for i, it in enumerate(items) if any(t == "" for t in it.triggers)]
        texts, owner = [], []
        for i, it in enumerate(items):
            for t in it.triggers:
                if t:
                    texts.append(t)
                    owner.append(i)
        self.V = kd_embed.enc(texts) if texts else np.zeros((0, 256), np.float32)
        self.owner = np.array(owner)

    def deliveries(self, sess, M) -> dict[int, list[int]]:
        """{call index t: [item ids delivered at t's PreToolUse]}; -1 = session start."""
        out = {}
        seen = set()
        if self.start:
            out[-1] = list(self.start)
            seen |= set(self.start)
        if len(self.V) == 0:
            return out
        S = M @ self.V.T  # calls x triggers
        for t in range(len(sess.calls)):
            hit = set(self.owner[S[t] >= self.theta].tolist()) - seen
            if hit:
                out[t] = sorted(hit)
                seen |= hit
        return out
