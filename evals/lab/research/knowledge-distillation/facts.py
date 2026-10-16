"""Discovery events: what an agent had to find out in a session, and what it cost.

Nothing here knows a tool name, a command or an error string. Two generic, learned structures:

1. **Information flow (provenance).** A call's input and output are split into atoms (maximal runs
   of identifier/path characters, with path prefixes and basenames). An atom is *discovered* by
   call k when it first appears in k's output and a later call's input uses it: call k's output
   was consumed. C(k) is the set of atoms k discovered. Atoms the model routinely writes with no
   provenance at all (`python`, `grep`, `self`, `-m`...) are **prior knowledge**: estimated from the
   training traces as P(first input use has no provenance), they are removed from C(k).
2. **Failure → recovery.** A call the harness flagged as an error (is_error) is followed, within a
   window, by a call that is *roughly the same action* (embedding cosine ≥ τ) and did not fail:
   the recovery. The chain is the failed call, any further similar failures, and the calls whose
   discovered atoms flowed into the recovery; its **fix** F is what the recovery added
   (atoms(recovery) − atoms(failure)), or, when nothing was added, what it removed. Pure: every call
   between the failure and the recovery must be in the chain (otherwise the failure was fixed by
   some other work, e.g. an edit, and is not a knowledge gap).

A card (any text) *obviates* a discovery call when every atom it discovered is in the card, and a
failure chain when its fix is in the card. This scorer is the same for the learned card and for
the AGENTS.md baseline; it is an information-sufficiency counterfactual (100% uptake), calibrated
against the live agents-md arm in `evaluate.py`.
"""

from __future__ import annotations

import re
from collections import Counter, defaultdict
from dataclasses import dataclass, field

import numpy as np

TOK = re.compile(r"[A-Za-z0-9_.\-/@]+")


def atoms(text: str) -> set[str]:
    out = set()
    for t in TOK.findall(text):
        t = t.rstrip(".-/")
        while t.startswith("./"):
            t = t[2:]
        if len(t) < 3 or not re.search(r"[A-Za-z]", t):
            continue
        out.add(t)
        if "/" in t:
            ps = [p for p in t.split("/") if p]
            for i in range(1, len(ps)):
                pre = "/".join(ps[:i])
                if len(pre) >= 3:
                    out.add(pre)
            if len(ps[-1]) >= 3:
                out.add(ps[-1])
    return out


@dataclass
class Event:
    kind: str  # "locator" | "chain"
    calls: list[int]  # the calls it would obviate
    need: frozenset  # atoms the card must contain
    trigger: int  # the call before which the knowledge was needed (first obviated call)
    text: str  # a grounded rendering (verbatim from the trace)
    recovery: int = -1
    fail_inp: str = ""
    rec_inp: str = ""


@dataclass
class SessFacts:
    sess: object
    in_atoms: list[set]
    events: list[Event] = field(default_factory=list)
    head_uses: Counter = field(default_factory=Counter)  # atom -> first input use without provenance
    prov_uses: Counter = field(default_factory=Counter)  # atom -> first input use with provenance


def flow(sess) -> tuple[list[set], dict[int, set], dict[int, set], Counter, Counter]:
    """Per call: expanded input atoms; C(k) (atoms first seen in k's output and used later);
    consumers: call j -> atoms it used whose source is an earlier output."""
    known = atoms(sess.prompt)
    src: dict[str, int] = {}
    C: dict[int, set] = defaultdict(set)
    fed: dict[int, set] = defaultdict(set)  # j -> source calls whose atoms j used
    head, prov = Counter(), Counter()
    ins = []
    for k, c in enumerate(sess.calls):
        ia = atoms(c.inp.split(": ", 1)[-1])
        ins.append(ia)
        for a in ia:
            if a in known:
                continue
            if a in src:
                C[src[a]].add(a)
                fed[k].add(src[a])
                prov[a] += 1
            else:
                head[a] += 1
        known |= ia
        for a in atoms(c.out):
            if a not in known and a not in src:
                src[a] = k
    return ins, C, fed, head, prov


class Prior:
    """Is an atom prior knowledge rather than something this repository taught? Learned from
    traces of *other* repositories only (so it transfers and is leave-one-repo-out safe):

    - generic: it occurs (in any call's input or output) in the traces of >= `min_repos` other
      repositories (words, stdlib names, common tool vocabulary, `tests`, `src`...);
    - from the head: its first input uses in those traces mostly had no provenance.
    """

    def __init__(self, sessions, exclude_repo: str | None = None, min_repos: int = 2, min_n: int = 2,
                 thresh: float = 0.5):
        h, p = Counter(), Counter()
        per_repo = defaultdict(set)
        for s in sessions:
            if s.repo == exclude_repo:
                continue
            _, _, _, hh, pp = flow(s)
            h.update(hh.keys())
            p.update(pp.keys())
            A = per_repo[s.repo]
            A |= atoms(s.prompt)
            for c in s.calls:
                A |= atoms(c.inp)
                A |= atoms(c.out)
        self.nrep = Counter(a for A in per_repo.values() for a in A)
        self.h, self.p, self.min_n, self.thresh, self.min_repos = h, p, min_n, thresh, min_repos

    def known(self, a: str) -> bool:
        if self.nrep[a] >= self.min_repos:
            return True
        n = self.h[a] + self.p[a]
        return n >= self.min_n and self.h[a] / n >= self.thresh


def extract(sess, prior: Prior, sim, tau: float = 0.8, window: int = 8, failed=None, max_actions: int = 2) -> list[Event]:
    """Discovery events of one session. `sim(i, j)` is the cosine of calls i and j's inputs;
    `failed[k]` the failure flag (default: the harness's is_error)."""
    calls = sess.calls
    n = len(calls)
    failed = failed if failed is not None else [c.error for c in calls]
    ins, C, fed, _, _ = flow(sess)
    ev = []
    for k in sorted(C):
        need = {a for a in C[k] if not prior.known(a)}
        if need:
            ev.append(Event("locator", [k], frozenset(need), k, calls[k].inp[:160]))
    used = set()
    for k in range(n):
        if not failed[k] or k in used:
            continue
        chain = [k]
        rec = -1
        for j in range(k + 1, min(n, k + 1 + window)):
            if sim(k, j) >= tau:
                if failed[j]:
                    chain.append(j)
                    continue
                rec = j
                break
        if rec < 0:
            continue
        # provenance contributors to the recovery, transitively, inside (k, rec): probes, obviated
        contrib, stack = set(), [rec]
        while stack:
            x = stack.pop()
            for s_ in fed.get(x, ()):
                if k < s_ < rec and s_ not in contrib:
                    contrib.add(s_)
                    stack.append(s_)
        # the other calls in between are the fix actions (e.g. an install): kept, but part of the fix
        acts = [x for x in range(k + 1, rec) if x not in contrib and x not in chain]
        if len(acts) > max_actions:
            continue  # other work happened in between: the failure was not a knowledge gap
        need = set(ins[rec] - ins[chain[-1]])
        for x in acts:
            need |= ins[x]
        if not need:
            need = {"NOT:" + a for a in ins[chain[-1]] - ins[rec]}
        if not need:
            continue
        cs = sorted(set(chain) | contrib)
        used |= set(chain)
        fix = " ; ".join([calls[x].inp[:120] for x in acts] + [calls[rec].inp[:160]])
        ev.append(Event("chain", cs, frozenset(need), k, f"{calls[chain[-1]].inp[:160]} -> {fix}",
                        rec, calls[chain[-1]].inp, fix))
    return ev


def card_atoms(text: str) -> set[str]:
    return atoms(text)


def covers(A: set, need: frozenset) -> bool:
    for a in need:
        if a.startswith("NOT:"):
            if a[4:] not in A:
                return False
        elif a not in A:
            return False
    return True


def obviated(sess, events: list[Event], A_at, preemptive: bool = False) -> set[int]:
    """Calls obviated when the knowledge available before call t is A_at(t) (a set of atoms).
    With `preemptive`, knowledge delivered at call t's PreToolUse also counts for t itself
    (a rewrite/deny of a would-fail call); otherwise only from t+1."""
    out = set()
    for e in events:
        t = e.trigger
        if covers(A_at(t, preemptive), e.need):
            out |= set(e.calls)
    return out
