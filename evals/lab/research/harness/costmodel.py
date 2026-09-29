"""The cumulative-context cost model: what a session cost, and what removing some of its tool calls
(or adding a directive's text) would change, in tokens and API-list dollars.

Every API response j reads the whole conversation so far. Claude Code caches it, so response j is
billed  cache_read_j·r_read + cache_write_j·r_write + input_j·r_in + output_j·r_out  (its own usage
record in the transcript; priced with `crates/adage-core/src/pricing.toml`). The context grows
between responses by g_j = ctx_{j+1} − ctx_j (ctx = cache_read + cache_write + input): response j's
output plus its tool calls' results (and any hook text). Tokens that enter the context at j are
written once (by response j+1, at the TTL rate the transcript records: Claude Code writes the
1-hour cache, 2x input) and then read by every later response. So a tool call's price
depends on *when* it happens: an early wasted call is carried for the rest of the session.

Removing call k (issued by response j) saves
    Δ_k · (r_write + r_read · L_j)                         L_j = responses after j+1
    Δ_k = g_j · w_k / Σ_{i∈j} w_i                          w = characters of the call's input + output
and, if every call response j issued is removed, the response itself:
    cache_read_j·r_read + input_j·r_in + output_j·r_out
(its cache write is g_{j−1}, which the next response still has to write). Adding a directive of
τ tokens after call t costs τ · (r_write + r_read · (responses after t − 1)).

First-order: it ignores that the counterfactual agent's later responses would be different, cache
TTL expiry (5 min; sessions here are fast) and compaction (none in RepoBench; g_j < 0 is clipped).
Checked against the harness's own total_cost_usd in `harness.calibrate.cost_model_check()`.
"""

from __future__ import annotations

import math
import tomllib
from functools import lru_cache

from .traces import ADAGE, Session

PRICING = ADAGE / "crates" / "adage-core" / "src" / "pricing.toml"


@lru_cache(maxsize=None)
def _table():
    return tomllib.loads(PRICING.read_text())["model"]


@lru_cache(maxsize=None)
def rates(model: str):
    """($/Mtok) input, output, cache_write, cache_read for a model id (longest matching prefix)."""
    m = (model or "").strip().lower()
    m = m.removeprefix("anthropic.")
    best = max((e for e in _table() if m.startswith(e["prefix"])), key=lambda e: len(e["prefix"]), default=None)
    if best is None:
        best = next(e for e in _table() if e["prefix"] == "claude-haiku-4-5")  # unpriced: flagged by `priced()`
    return (best["input"], best["output"], best["cache_write"], best["cache_read"])


def priced(model: str) -> bool:
    m = (model or "").strip().lower()
    return any(m.startswith(e["prefix"]) for e in _table())


class SessionCost:
    """Precomputed per-session cost structure (cheap; built once per session and memoised)."""

    def __init__(self, s: Session):
        self.s = s
        R = s.responses
        self.n = len(R)
        self.rate = [tuple(x / 1e6 for x in rates(r.model or s.model)) for r in R]
        # The cache-write rate each response actually paid: Claude Code writes the 1-hour TTL here
        # (2x input), which pricing.toml's 5-minute rate (1.25x) under-prices by ~12% a session.
        self.wrate = [(((r.cache_write - r.cache_write_1h) * rt[2] + r.cache_write_1h * 2 * rt[0]) / r.cache_write)
                      if r.cache_write else rt[2] for r, rt in zip(R, self.rate)]
        self.cost = [r.cache_read * rt[3] + r.cache_write * w + r.input * rt[0] + r.output * rt[1]
                     for r, rt, w in zip(R, self.rate, self.wrate)]
        self.total = sum(self.cost)
        ctx = [r.cache_read + r.cache_write + r.input for r in R]
        self.ctx_tokens = sum(ctx)  # context tokens processed over the session
        self.growth = [max(0, (ctx[j + 1] - ctx[j]) if j + 1 < self.n else R[j].output) for j in range(self.n)]
        # Which response issued each call, and the call's weight in that response's growth.
        self.resp_of = {}
        self.weight = {}
        for j, r in enumerate(R):
            ws = []
            for k in r.calls:
                c = s.calls[k]
                ws.append(len(str(c.input)) + max(c.out_chars, len(c.output)) + 1)
            tot = sum(ws) or 1
            for k, w in zip(r.calls, ws):
                self.resp_of[k] = j
                self.weight[k] = w / tot
        seen = 0
        order = []
        for j, r in enumerate(R):
            order.append(seen)  # calls issued before response j
            seen += len(r.calls)
        self.calls_before = order

    def responses_after_call(self, t: int) -> int:
        """Responses that read a text injected after call t (t = −1: before any call)."""
        return sum(1 for b in self.calls_before if b >= t + 1)

    def removal(self, removed) -> tuple[float, float]:
        """($ saved, context tokens saved) if these call indices never happened."""
        removed = set(removed)
        if not removed or not self.n:
            return 0.0, 0.0
        by_resp = {}
        for k in removed:
            j = self.resp_of.get(k)
            if j is not None:
                by_resp.setdefault(j, []).append(k)
        usd = tok = 0.0
        R = self.s.responses
        for j, ks in by_resp.items():
            rt = self.rate[j]
            later = max(0, self.n - j - 2)
            share = sum(self.weight[k] for k in ks)
            whole = len(ks) == len(R[j].calls)
            d = self.growth[j] * (1.0 if whole else share)
            usd += d * (self.wrate[min(j + 1, self.n - 1)] + rt[3] * later)
            tok += d * (1 + later)
            if whole:
                usd += R[j].cache_read * rt[3] + R[j].input * rt[0] + R[j].output * rt[1]
                tok += R[j].cache_read + R[j].cache_write + R[j].input
        return usd, tok

    def injection(self, tokens: int, t: int) -> tuple[float, float]:
        """($, context tokens) of carrying `tokens` of injected text from after call t to the end."""
        m = self.responses_after_call(t)
        if m == 0:
            return 0.0, 0.0
        j0 = self.n - m
        rt = self.rate[min(j0, self.n - 1)]
        return tokens * (self.wrate[min(j0, self.n - 1)] + rt[3] * (m - 1)), float(tokens * m)

    def usd_per_call(self) -> float:
        n = self.s.n_calls
        return self.total / n if n else 0.0


_MEMO: dict = {}


def of(s: Session) -> SessionCost:
    c = _MEMO.get(id(s))
    if c is None or c.s is not s:
        c = _MEMO[id(s)] = SessionCost(s)
    return c


def tokens_of(text: str) -> int:
    """Rust `tokens_of`: 4 characters a token, rounded up."""
    return math.ceil(len(text) / 4)
