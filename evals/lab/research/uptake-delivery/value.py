"""Local value of a decision unit: calls and dollars from the trigger to the agent's goal.

Goal (learned, no signature list): the first call after the trigger that (a) has the same intent as
the failed trigger call (learned token containment of the trigger's recurrent tokens >= TAU_INTENT;
a `.venv/bin/python -m pytest x` rerun contains `python -m pytest x`), (b) did not error, and (c)
whose output is not a near-copy of the trigger's failure output (potion-8M cosine < TAU_SAME_OUT; a
piped `python -m pytest | head` "succeeds" with the same `No module named pytest` text).
Censored at H calls (cost H + 1).

Dead-end hits without a logged trigger (piped exit-0 failures, launch bug 2) are found the same
learned way: a call whose output is within TAU_SAME_OUT of any *logged* trigger output from another
repository (the logged would_have / delivered triggers are the labelled examples).

Dollar cost of a window: the assistant turns that issued its calls (usage x list price) plus the
carry: each call's tokens stay in context and are re-read (cache read) by every later turn.
"""
from __future__ import annotations

import numpy as np

import data
import judge
from units import action_text

H = 15
TAU_INTENT = 0.6
TAU_SAME_OUT = 0.9

_pot = None


def pot():
    global _pot
    if _pot is None:
        _pot = judge.Sim("potion-8M")
    return _pot


def out_text(c) -> str:
    return (c.out or "").strip()[:600] or "<empty>"


def goal_index(s: data.Sess, trig: int, con: judge.Containment, start: int | None = None):
    """Index of the first goal call after `trig` (searching from `start`), or None within H."""
    t = s.calls[trig]
    ttext = action_text(t, s.cwd)
    tout = pot().vec([out_text(t)])[0]
    start = trig + 1 if start is None else start
    for i in range(start, min(len(s.calls), trig + 1 + H)):
        c = s.calls[i]
        if c.name != "Bash" or c.err:
            continue
        if con.one(ttext, action_text(c, s.cwd)) < TAU_INTENT:
            continue
        if float(pot().vec([out_text(c)])[0] @ tout) >= TAU_SAME_OUT:
            continue
        return i
    return None


def price(s: data.Sess):
    return data.PRICES[s.model][3]


def window_cost(s: data.Sess, a: int, b: int) -> tuple[float, float]:
    """(calls, $) for calls a..b inclusive; $ = issuing turns + cache-read carry to session end."""
    calls = s.calls[a:b + 1]
    n_after = len(s.calls) - (b + 1)
    turn = sum(c.turn_usd for c in calls)
    carry = sum(c.res_tokens for c in calls) * n_after * price(s) / 1e6
    return float(len(calls)), float(turn + carry)


def local(s: data.Sess, trig: int, con, first: int | None = None):
    """Calls/$ spent after the trigger call up to and including the goal, censored at H (cost H+1).
    With `first` (the first dead-end hit, <= trig), the window starts there instead: the cost a
    pre-emptive directive could have avoided (the failed attempts themselves included)."""
    g = goal_index(s, trig, con)
    reached = g is not None
    g = g if reached else min(len(s.calls) - 1, trig + H)
    a = trig + 1 if first is None else first
    n, usd = window_cost(s, a, g)
    if not reached:
        n += 1.0
    return n, usd, reached
