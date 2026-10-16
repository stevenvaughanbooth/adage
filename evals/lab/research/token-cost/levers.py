"""Zero-harm $ levers and the $ value of a saved call, on held-out control sessions.

1. Cache TTL: Claude Code wrote every cache entry with the 1-hour TTL (2x input price). Sessions
   here have inter-call gaps p99.9 = 124 s (one gap > 300 s in 16.6k), so a 5-minute TTL (1.25x)
   would have served the same reads. Counterfactual: reprice writes at 1.25x; where a gap > 300 s,
   the cached prefix expires and is re-written (cache read -> 5m write).
2. Re-read dedupe (PreToolUse on Read): a Read whose lines were all already shown by an earlier Read
   of the same file with no Edit/Write since is answered with a 40-token stub; partial overlaps
   are trimmed to the new lines. No call is added or removed.
3. $ per saved call: the session's cost divided by its calls (average), and the marginal cost
   of a call (its own context re-read + output, plus carrying its output and result).
"""
from __future__ import annotations

import datetime
import json

import numpy as np

from common import PRICES, boot_cluster, call_cost, fam, harness_sets, session_cost
from describe import S, rereads


def ttl5m(s):
    p = PRICES[fam(s["model"])]
    ts = [datetime.datetime.fromisoformat(c["ts"].replace("Z", "+00:00")).timestamp() if c.get("ts") else None
          for c in s["calls"]]
    sv = 0.0
    for k, c in enumerate(s["calls"]):
        sv += c["cw1h"] * (p["cw1h"] - p["cw5m"]) / 1e6
        if k and ts[k] and ts[k - 1] and ts[k] - ts[k - 1] > 300:
            sv -= c["cr"] * (p["cw5m"] - p["cr"]) / 1e6
    return sv


def dedupe(s):
    return sum(max(r["tok"] * frac - 40, 0) * r["carry"] for r, frac in rereads(s))


def per_call(s):
    return session_cost(s) / len(s["calls"])


def main():
    out = {}
    for f in ("haiku", "sonnet"):
        H, _ = harness_sets(S, f)
        g = [s["task"] for s in H]
        boot = lambda v: boot_cluster(v, groups=g)  # noqa: E731
        boot_ratio = lambda a, b: boot_cluster(a, b, g)  # noqa: E731
        base = np.array([session_cost(s) for s in H])
        t = np.array([ttl5m(s) for s in H])
        d = np.array([dedupe(s) for s in H])
        pc = np.array([per_call(s) for s in H])
        out[f] = dict(
            n=len(H), base_cost=boot(base),
            ttl_5m_usd=boot(t), ttl_5m_pct=boot_ratio(t, base),
            reread_dedupe_usd=boot(d), reread_dedupe_pct=boot_ratio(d, base),
            usd_per_call_avg=boot(pc), pct_per_call=boot_ratio(pc, base),
            current_adage_posthoc_1p0_calls_usd=boot(pc * 1.0), current_adage_preemptive_2p11_calls_usd=boot(pc * 2.11),
            current_adage_2p11_pct=boot_ratio(pc * 2.11, base),
        )
        # sensitivity: price the Sonnet traces at Sonnet 4.6 list prices
    json.dump(out, open("levers.json", "w"), indent=1)
    for f, r in out.items():
        print("==", f)
        for k, v in r.items():
            print(" ", k, tuple(round(x, 4) for x in v) if isinstance(v, tuple) else v)


if __name__ == "__main__":
    main()
