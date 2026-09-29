"""Sensitivity of the oracle to the provenance instrument, the early-stop ceiling on failed sessions,
and the pass-rate -> $ per resolved task translation."""

from __future__ import annotations

import json
import pickle
import sys
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent))
from ceilings import reprice, boot_ratio, boot_mean, lifetime  # noqa: E402

CACHE = Path("/tmp/adage-research/waste-anatomy")
HERE = Path(__file__).resolve().parent
RNG = np.random.default_rng(11)


def share(H, fn):
    b = np.array([reprice(s) for s in H])
    v = np.array([reprice(s, remove=frozenset(fn(s))) for s in H])
    return {"calls_saved": boot_mean(b[:, 0] - v[:, 0]), "usd_saved_share": boot_ratio(b[:, 2] - v[:, 2], b[:, 2])}


def main():
    S = pickle.load((CACHE / "calls.pkl").open("rb"))
    out = {}
    for g, H in {"haiku_v2": [s for s in S if s["arm"] == "control" and s["fam"] == "haiku" and s["base"] == "adage-rb2"],
                 "sonnet_v2": [s for s in S if s["arm"] == "control" and s["fam"] == "sonnet" and s["base"] == "adage-rb2"]}.items():
        o = {}
        # oracle (a) sensitivity: looser notions of "needed"
        o["oracle_keep_all_reads"] = share(H, lambda s: {c["idx"] for c in s["calls"] if not c["needed"] and c["name"] != "Read"})
        o["oracle_keep_all_referenced"] = share(H, lambda s: {c["idx"] for c in s["calls"] if not c["needed"] and not c["referenced"]})
        o["oracle_keep_all_explore"] = share(H, lambda s: {c["idx"] for c in s["calls"] if not c["needed"] and c["role"] != "explore"})
        # early stop: failed sessions truncated at the passing sessions' median call count (oracle
        # knows which will fail; pass rate unchanged by construction)
        med = int(np.median([len(s["calls"]) for s in H if s["passed"]]))
        o["early_stop_failed_at_pass_median"] = share(H, lambda s: {c["idx"] for c in s["calls"] if not s["passed"] and c["idx"] >= med})
        o["early_stop_failed_at_pass_median"]["median_pass_calls"] = med
        # $ per resolved task and what pass-rate points are worth
        cost = np.array([s["cost_actual"] for s in H])
        ok = np.array([bool(s["passed"]) for s in H], float)
        idx = RNG.integers(0, len(H), (2000, len(H)))
        cpr = cost[idx].sum(1) / np.maximum(ok[idx].sum(1), 1)
        o["usd_per_resolved"] = [float(cost.sum() / ok.sum()), float(np.percentile(cpr, 5)), float(np.percentile(cpr, 95))]
        o["pass_rate"] = float(ok.mean())
        o["usd_per_resolved_change_per_+5_points"] = float((cost.mean() / (ok.mean() + 0.05)) / (cost.mean() / ok.mean()) - 1)
        # carry multiplier illustration: 5k tokens appended at request 3
        v = []
        for s in H:
            n, P = s["n_req"], s["prices"]
            if n > 5:
                v.append(5000 * (P["pw"] + P["pr"] * max(n - 5, 0)) / 1e6)
        o["usd_of_5k_tokens_added_at_request_3"] = boot_mean(v)
        o["tokens_processed_per_token_added"] = boot_mean([sum(s["C"]) / max(sum(s["D"]), 1) for s in H])
        out[g] = o
    (HERE / "extras.json").write_text(json.dumps(out, indent=1))
    print(json.dumps(out, indent=1))


if __name__ == "__main__":
    main()
