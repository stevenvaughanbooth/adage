"""Scoring an *offload*: the hook runs a call itself and hands the agent its result, so the model
turn that would have issued that call never happens.

Extension of the harness semantics (documented in README §Scoring):

  hit   the hook ran X after call t (at PostToolBatch of the turn ending at t); the agent's next
        occurrence of X (same key; or the covering variant, below) comes at k ∈ (t, t+W], before
        any file edit in between. Counterfactual: call k is not issued; its response (model turn)
        disappears; X's result enters the context after t instead.
          $ = removal({k})                  harness costmodel: the response's own read + output
                                            + the call's tokens carried to the end
            − injection(result tokens, t)   harness costmodel: the result re-enters at t, carried
                                            one response longer than the agent's own copy
  miss  the hook ran X and the agent never took X in the window:
          $ = − injection(result tokens, t) − harm_calls · $/call
  p_skip is the probability that, handed the result, the agent does not issue X anyway (the E1
  analogue of p_follow). A hit is worth p_skip·removal − injection (the injection is paid either way).

Tokens of a delivered result: output characters × the model's fitted tokens/char (token-cost:
0.324 Haiku, 0.387 Sonnet) + the command line + a 40-token wrapper.
"""
from __future__ import annotations

import sys

sys.path.insert(0, "/home/user/adage/evals/lab/research")
from harness import costmodel as CM  # noqa: E402
from harness import stats as ST  # noqa: E402

TOK_PER_CHAR = {"haiku": 0.324, "sonnet": 0.387}
WRAPPER = 40


def result_tokens(rec, model: str, compact: bool = False) -> int:
    chars = rec.out_chars
    if compact and not rec.failed:
        chars = min(chars, 200)  # "exit 0" + the last line(s) of a passing run
    return int(chars * TOK_PER_CHAR[model] + len(rec.text) / 4 + WRAPPER)


def hit_tokens(s, rec) -> int:
    """Tokens a hit re-injects: exactly what the agent's own result added to the context (its
    response's context growth minus the response's own output), plus the command and a wrapper."""
    cm = CM.of(s)
    j = rec.resp
    if j < 0:
        return WRAPPER
    g = cm.growth[j] * (cm.weight.get(rec.i, 1.0) if not rec.solo else 1.0)
    res = max(0.0, g - s.responses[j].output) if rec.solo else g
    return int(res + len(rec.text) / 4 + WRAPPER)


def session_value(s, hits, misses, model, p_skip=1.0, harm_calls=0.0):
    """hits: [(t, k, tokens)], misses: [(t, tokens)] → dict of $ / calls / tokens for one session."""
    cm = CM.of(s)
    removed = sorted({k for _, k, _ in hits})
    usd_rm, tok_rm = cm.removal(removed) if removed else (0.0, 0.0)
    inj_usd = inj_tok = 0.0
    for t, _, tok in hits:
        u, tk = cm.injection(tok, t)
        inj_usd += u
        inj_tok += tk
    miss_usd = 0.0
    for t, tok in misses:
        u, tk = cm.injection(tok, t)
        miss_usd += u
        inj_tok += tk
    upc = cm.usd_per_call()
    net_usd = p_skip * usd_rm - inj_usd - miss_usd - harm_calls * upc * len(misses)
    net_calls = p_skip * len(removed) - harm_calls * len(misses)
    return {"usd": net_usd, "calls": net_calls, "gross_usd": usd_rm, "inj_usd": inj_usd + miss_usd,
            "base_usd": cm.total, "base_calls": s.n_calls, "hits": len(removed), "misses": len(misses),
            "tok": p_skip * tok_rm - inj_tok}


def aggregate(rows, clusters, level=0.90):
    """rows: per-session dicts from session_value; clusters: task ids. Task-clustered 90% CIs."""
    usd = [r["usd"] for r in rows]
    calls = [r["calls"] for r in rows]
    base = [r["base_usd"] for r in rows]
    bcalls = [r["base_calls"] for r in rows]
    m_u, ci_u = ST.clustered(usd, clusters, level=level)
    m_c, ci_c = ST.clustered(calls, clusters, level=level)
    sh, sh_ci = ST.clustered_ratio(usd, base, clusters, level=level)
    csh, csh_ci = ST.clustered_ratio(calls, bcalls, clusters, level=level)
    n = len(rows)
    return {"sessions": n, "tasks": len(set(clusters)),
            "baseline_usd": sum(base) / max(n, 1), "baseline_calls": sum(bcalls) / max(n, 1),
            "usd_saved": m_u, "usd_ci": ci_u, "usd_share": sh, "usd_share_ci": sh_ci,
            "calls_saved": m_c, "calls_ci": ci_c, "calls_share": csh, "calls_share_ci": csh_ci,
            "hits_per_session": sum(r["hits"] for r in rows) / max(n, 1),
            "misses_per_session": sum(r["misses"] for r in rows) / max(n, 1),
            "gross_usd_share": sum(r["gross_usd"] for r in rows) / max(sum(base), 1e-9),
            "inj_usd_share": sum(r["inj_usd"] for r in rows) / max(sum(base), 1e-9)}


def fmt(a) -> str:
    return (f"{a['sessions']} sess/{a['tasks']} tasks: $ {a['usd_saved']:+.4f} [{a['usd_ci'][0]:+.4f}, {a['usd_ci'][1]:+.4f}]"
            f" = {100*a['usd_share']:+.1f}% [{100*a['usd_share_ci'][0]:+.1f}, {100*a['usd_share_ci'][1]:+.1f}];"
            f" calls {a['calls_saved']:+.2f} [{a['calls_ci'][0]:+.2f}, {a['calls_ci'][1]:+.2f}]"
            f" ({100*a['calls_share']:+.1f}%); hits {a['hits_per_session']:.2f}, misses {a['misses_per_session']:.2f}/sess;"
            f" gross {100*a['gross_usd_share']:.1f}%, injected {100*a['inj_usd_share']:.1f}%")
