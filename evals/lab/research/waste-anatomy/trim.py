"""Output trimming: the realistic fraction of the 25% (Haiku) hindsight ceiling.

Policy: a PostToolUse rewrite that keeps a result's first `head` share and last `1-head` share of
a token budget K and replaces the middle with a one-line elision marker. It is tuned on the TRAIN
split (K and head, per tool; the tool name is the harness's own label, used as a context key) and
evaluated on held-out control sessions.

Harm model: when a trimmed-away line held a token the agent used later (hindsight, `line_ref`),
the agent must fetch it again: one extra call, priced at that session's mean lifetime $ per call,
plus the lost tokens carried from that point. Net = savings - harm.
"""

from __future__ import annotations

import json
import pickle
import sys
from collections import defaultdict
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent))
from ceilings import reprice, boot_ratio, boot_mean  # noqa: E402

CACHE = Path("/tmp/adage-research/waste-anatomy")
HERE = Path(__file__).resolve().parent
TOOLS = ("Read", "Bash", "Grep", "Glob")
KS = [250, 500, 1000, 2000, 4000, 8000, None]
HEADS = [0.5, 0.8, 1.0]


def trim_one(c, K, head):
    """Return (tokens removed, harm: lost referenced line count>0)."""
    if K is None or c["res_tok"] <= K or c["res_chars"] == 0:
        return 0.0, False
    ll = c["line_len"]
    tpc = c["res_tok"] / c["res_chars"]
    budget_h, budget_t = K * head / tpc, K * (1 - head) / tpc  # in chars
    cs = np.cumsum(ll)
    keep = (cs <= budget_h)
    rc = np.cumsum(ll[::-1])[::-1]
    keep |= (rc <= budget_t)
    removed = ll[~keep].sum() * tpc
    lost = bool(len(c["line_ref"]) and (~keep[c["line_ref"]]).any())
    return float(removed), lost


def evaluate(H, policy):
    base, new, harm = [], [], []
    for s in H:
        tr, lost = {}, 0
        for c in s["calls"]:
            K, head = policy.get(c["name"], (None, 1.0))
            r, l = trim_one(c, K, head)
            if r:
                tr[c["idx"]] = r
            lost += l
        b = reprice(s)
        n = reprice(s, trim=tr)
        per_call = s["cost_actual"] / max(len(s["calls"]), 1)
        base.append(b)
        new.append(n)
        harm.append(lost * per_call)
    base, new, harm = np.array(base), np.array(new), np.array(harm)
    gross = base[:, 2] - new[:, 2]
    return gross, harm, base[:, 2], np.array([0])


def tune(T):
    policy = {}
    for tool in TOOLS:
        best = (0.0, (None, 1.0))
        for K in KS:
            for h in HEADS:
                g, hm, b, _ = evaluate(T, {tool: (K, h)})
                net = (g - hm).sum() / b.sum()
                if net > best[0]:
                    best = (net, (K, h))
        policy[tool] = best[1]
    return policy


def main():
    S = pickle.load((CACHE / "calls.pkl").open("rb"))
    T = [s for s in S if s["arm"] == "train"]
    out = {}
    pol = tune(T)
    out["policy_tuned_on_train"] = {k: list(v) for k, v in pol.items()}
    for g, H in {
        "haiku_v2": [s for s in S if s["arm"] == "control" and s["fam"] == "haiku" and s["base"] == "adage-rb2"],
        "sonnet_v2": [s for s in S if s["arm"] == "control" and s["fam"] == "sonnet" and s["base"] == "adage-rb2"],
        "haiku_v1_urfave": [s for s in S if s["arm"] == "control" and s["repo"] == "urfave-cli"],
    }.items():
        gross, harm, base, _ = evaluate(H, pol)
        out[g] = {"n": len(H), "gross_usd_saved_share": boot_ratio(gross, base),
                  "harm_usd_share": boot_ratio(harm, base), "net_usd_saved_share": boot_ratio(gross - harm, base),
                  "extra_calls_per_session": boot_mean(harm / np.maximum(base, 1e-9) * 0 + np.array(
                      [sum(trim_one(c, *pol.get(c["name"], (None, 1.0)))[1] for c in s["calls"]) for s in H]))}
        # leave-one-repo-out tuning for the headline group
    lor = {}
    ctrl = [s for s in S if s["arm"] == "control" and s["fam"] == "haiku" and
            (s["base"] == "adage-rb2" or s["repo"] == "urfave-cli")]
    for repo in sorted({s["repo"] for s in ctrl}):
        Tr = [s for s in S if s["arm"] == "train" and s["repo"] != repo]
        p = tune(Tr)
        H = [s for s in ctrl if s["repo"] == repo]
        gross, harm, base, _ = evaluate(H, p)
        lor[repo] = {"n": len(H), "net_usd_saved_share": boot_ratio(gross - harm, base), "policy": {k: list(v) for k, v in p.items()}}
    out["leave_one_repo_out_haiku"] = lor
    (HERE / "trim.json").write_text(json.dumps(out, indent=1))
    print(json.dumps(out, indent=1))


if __name__ == "__main__":
    main()
