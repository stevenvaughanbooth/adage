"""Cumulative-context cost model: what removing a set of tool calls from a session would have saved.

Each API call (assistant message) is priced from its own usage: input, cache write, cache read and
output tokens at the model's list price (`load.PRICES`, from crates/adage-core/src/pricing.toml).
Removing tool calls R:

- an API call all of whose tool calls are in R disappears with its whole cost (it re-read the full
  context and generated the calls);
- every removed call's input and result (chars / 4) and every removed message's output leave the
  context: they are no longer cache-written at the next API call nor cache-read at every later one.

Counterfactual cost = actual cost minus both. The agent's other behaviour is held fixed (the usual
replay assumption). Tokens saved are reported the same way ("processed tokens": every token the API
would no longer have read or written).
"""

from __future__ import annotations

from load import price


def msg_cost(u, p) -> float:
    pin, pout, pcw, pcr = p
    i, cw, cr, o = u
    return (i * pin + cw * pcw + cr * pcr + o * pout) / 1e6


def session_cost(s) -> float:
    p = price(s.model)
    return sum(msg_cost(u, p) for u in s.usage)


def saved(s, R) -> dict:
    """$ and tokens saved by removing calls R (indices into s.calls)."""
    R = set(R)
    if not R:
        return {"usd": 0.0, "tokens": 0.0, "calls": 0}
    p = price(s.model)
    pin, pout, pcw, pcr = p
    by_msg = {}
    for k, c in enumerate(s.calls):
        by_msg.setdefault(c.batch, []).append(k)
    gone_msgs = {b for b, ks in by_msg.items() if all(k in R for k in ks)}
    usd, tok = 0.0, 0.0
    # removed context items: (entry message index, tokens)
    items = []
    for k in R:
        c = s.calls[k]
        items.append((c.batch + 1, (len(c.inp_text) + c.out_chars) / 4.0))
    for b in gone_msgs:
        u = s.usage[b]
        usd += msg_cost(u, p)
        tok += sum(u)
        items.append((b + 1, u[3]))
    for b, u in enumerate(s.usage):
        if b in gone_msgs:
            continue
        cw_red = sum(t for e, t in items if e == b)
        cr_red = sum(t for e, t in items if e < b)
        cw_red = min(cw_red, u[1])
        cr_red = min(cr_red, u[2])
        usd += (cw_red * pcw + cr_red * pcr) / 1e6
        tok += cw_red + cr_red
    return {"usd": usd, "tokens": tok, "calls": len(R)}
