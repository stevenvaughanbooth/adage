"""Oracle ceilings per lever, re-priced under the cumulative-context model, with 90% bootstrap CIs.

Counterfactual pricing (`reprice`): every request k has a measured context C_k (input + cache
write + cache read tokens), output O_k and the tokens it appended, D_k = C_{k+1} - C_k, split over
its tool calls (result tokens by characters, the rest as per-request overhead). Removing a call
removes its tokens from every later request's context; removing every call of a request removes
the request itself (its prefix re-read and its output). A kept request re-reads what was already
cached at the cache-read price and writes what is new since the previous kept request at the
cache-write price. With nothing removed the model reproduces the measured cost (checked below).
Assumes the rest of the session proceeds unchanged: that is what makes these *ceilings*.
"""

from __future__ import annotations

import json
import pickle
import sys
from collections import Counter, defaultdict
from pathlib import Path

import numpy as np

CACHE = Path("/tmp/adage-research/waste-anatomy")
HERE = Path(__file__).resolve().parent
RNG = np.random.default_rng(20260929)
B = 2000


def reprice(s, remove=frozenset(), trim=None, prefix_add=0.0):
    """Return (calls, context tokens processed, $) after removing calls / trimming result tokens /
    adding tokens to the prefix (pre-loaded context)."""
    trim = trim or {}
    P = s["prices"]
    C, W, I, O = map(np.asarray, (s["C"], s["W"], s["I"], s["O"]))
    n = s["n_req"]
    x = np.zeros(n)
    live = np.ones(n, bool)
    per_req = defaultdict(list)
    for c in s["calls"]:
        per_req[c["k"]].append(c)
    for k, cs in per_req.items():
        rem = [c for c in cs if c["idx"] in remove]
        for c in rem:
            x[k] += c["res_tok"] + c["over_tok"]
        for c in cs:
            if c["idx"] not in remove and c["idx"] in trim:
                x[k] += min(trim[c["idx"]], c["res_tok"])
        if len(rem) == len(cs):
            live[k] = False
    cum = np.r_[0.0, np.cumsum(x)[:-1]]  # tokens removed before request k
    Cp = C - cum + prefix_add
    cost, tokens, prev = 0.0, 0.0, None
    for k in range(n):
        if not live[k]:
            continue
        w = W[k] + prefix_add if prev is None else max(Cp[k] - Cp[prev], 0.0)
        w = min(w, Cp[k] - I[k])
        cost += ((Cp[k] - w - I[k]) * P["pr"] + w * P["pw"] + I[k] * P["pin"] + O[k] * P["pout"]) / 1e6
        tokens += Cp[k]
        prev = k
    ncalls = sum(1 for c in s["calls"] if c["idx"] not in remove)
    return ncalls, tokens, cost


def chars_to_tok(c, keep_chars):
    return c["res_tok"] * (1 - keep_chars / c["res_chars"]) if c["res_chars"] else 0.0


# ------------------------------------------------------------------ levers (oracle)

def lever_sets(s):
    calls = s["calls"]
    ids = lambda f: frozenset(c["idx"] for c in calls if f(c))
    tests = [c for c in calls if c["role"] == "test"]
    last_test = tests[-1]["idx"] if tests else None
    out = {}
    # (a) oracle: everything not in the provenance slice of the final diff
    out["oracle_final_diff"] = dict(remove=ids(lambda c: not c["needed"]))
    out["oracle_final_diff+1_test"] = dict(remove=ids(lambda c: not c["needed"] and c["idx"] != last_test))
    out["oracle_graded_src_only"] = dict(remove=ids(lambda c: not c["needed_src"]))
    # (b) per lever class
    needed_explore = [c for c in calls if c["role"] == "explore" and c["needed"]]
    out["preload_right_files"] = dict(remove=ids(lambda c: c["role"] == "explore"),
                                      prefix_add=sum(c["res_tok"] for c in needed_explore))
    out["skip_env_friction"] = dict(remove=ids(lambda c: c["seg"] == "env_friction"))
    tr = {c["idx"]: chars_to_tok(c, c["ref_chars"]) for c in calls if c["role"] == "test"}
    out["scoped_tests"] = dict(remove=ids(lambda c: c["role"] == "test" and c["redundant"]), trim=tr)
    out["trim_outputs"] = dict(trim={c["idx"]: chars_to_tok(c, c["ref_chars"]) for c in calls})
    out["avoid_deadend_edits"] = dict(remove=ids(lambda c: c["seg"] in ("edit_deadend", "deadend_runs")))
    out["skip_redundant"] = dict(remove=ids(lambda c: c["seg"] == "redundant"))
    out["post_edit_tail"] = dict(remove=ids(lambda c: c["seg"] in ("verify", "vcs") and c["idx"] != last_test))
    keep = ids(lambda c: c["survive"] or c["idx"] == last_test)
    # the tool's read-before-edit rule: keep the read that licensed each kept edit
    for c in calls:
        if c["survive"] and c["name"] != "Write":
            for y in reversed(calls[:c["idx"]]):
                if y["name"] == "Read" and y["file"] == c["file"] and not y["err"]:
                    keep = keep | {y["idx"]}
                    break
    out["know_answer_from_memory"] = dict(remove=ids(lambda c: c["idx"] not in keep))
    return out


def boot_ratio(num, den, b=B):
    num, den = np.asarray(num, float), np.asarray(den, float)
    n = len(num)
    idx = RNG.integers(0, n, (b, n))
    r = num[idx].sum(1) / den[idx].sum(1)
    return float(num.sum() / den.sum()), float(np.percentile(r, 5)), float(np.percentile(r, 95))


def boot_mean(v, b=B):
    v = np.asarray(v, float)
    idx = RNG.integers(0, len(v), (b, len(v)))
    m = v[idx].mean(1)
    return float(v.mean()), float(np.percentile(m, 5)), float(np.percentile(m, 95))


def ceilings(H, label):
    base = [reprice(s) for s in H]
    rows = {}
    for s_i, s in enumerate(H):
        for name, kw in lever_sets(s).items():
            rows.setdefault(name, []).append(reprice(s, **kw))
    res = {}
    b = np.array(base)
    for name, v in rows.items():
        v = np.array(v)
        d = b - v
        res[name] = {
            "calls_saved_per_session": boot_mean(d[:, 0]),
            "calls_saved_share": boot_ratio(d[:, 0], b[:, 0]),
            "context_tokens_saved_share": boot_ratio(d[:, 1], b[:, 1]),
            "usd_saved_per_session": boot_mean(d[:, 2]),
            "usd_saved_share": boot_ratio(d[:, 2], b[:, 2]),
        }
    res["_baseline"] = {"sessions": len(H), "calls": boot_mean(b[:, 0]), "context_tokens": boot_mean(b[:, 1]),
                        "usd": boot_mean(b[:, 2])}
    return res


# ------------------------------------------------------------------ anatomy tables

SEGS = ["orient", "env_friction", "test", "repro_exec", "edit_final", "edit_deadend", "deadend_runs",
        "redundant", "verify", "vcs", "other"]


def lifetime(c):
    return c["own_usd"] + c["carry_usd"]


def anatomy(H):
    out = {}
    tot_usd = np.array([s["cost_actual"] for s in H])
    for seg in SEGS:
        calls = np.array([sum(1 for c in s["calls"] if c["seg"] == seg) for s in H], float)
        usd = np.array([sum(lifetime(c) for c in s["calls"] if c["seg"] == seg) for s in H])
        tok = np.array([sum(c["res_tok"] for c in s["calls"] if c["seg"] == seg) for s in H])
        out[seg] = {"calls_per_session": boot_mean(calls), "usd_share": boot_ratio(usd, tot_usd),
                    "tokens_added_per_session": boot_mean(tok)}
    out["_toolless_final_answers"] = {"usd_share": boot_ratio([s["toolless_usd"] for s in H], tot_usd)}
    return out


def cost_structure(H):
    """Where the $ physically goes, and the carry multiplier."""
    pr_fixed, pr_carry, writes, outs, tot = [], [], [], [], []
    mult_num, mult_den, later_w = [], [], []
    for s in H:
        P = s["prices"]
        C, W, I, O = map(np.asarray, (s["C"], s["W"], s["I"], s["O"]))
        n = s["n_req"]
        S0 = C[0]
        fixed = n * S0 * P["pr"] / 1e6
        reads = ((C - W - I) * P["pr"]).sum() / 1e6
        pr_fixed.append(fixed)
        pr_carry.append(reads - fixed)
        writes.append((W * P["pw"]).sum() / 1e6)
        outs.append((O * P["pout"]).sum() / 1e6)
        tot.append(s["cost_actual"])
        # token-weighted number of later re-reads of an appended token
        D = np.asarray(s["D"])
        L = np.array([max(n - k - 2, 0) for k in range(n)], float)
        later_w.append((D * L).sum() / max(D.sum(), 1))
        mult_num.append((D * (P["pw"] + P["pr"] * L)).sum())
        mult_den.append((D * P["pw"]).sum())
    tot = np.array(tot)
    return {
        "read_fixed_prefix_share": boot_ratio(pr_fixed, tot),
        "read_carried_content_share": boot_ratio(pr_carry, tot),
        "cache_write_share": boot_ratio(writes, tot),
        "output_share": boot_ratio(outs, tot),
        "token_weighted_later_rereads": boot_mean(later_w),
        "lifetime_cost_over_write_cost": boot_ratio(mult_num, mult_den),
        "requests_per_session": boot_mean([s["n_req"] for s in H]),
        "prefix_tokens_first_request": boot_mean([s["C"][0] for s in H]),
        "final_context_tokens": boot_mean([s["C"][-1] for s in H]),
    }


def early_token_price(H):
    """$ lifetime of 1k tokens appended at call index k (median over sessions), per family."""
    out = {}
    for k in (1, 5, 10, 20, 30):
        v = []
        for s in H:
            n, P = s["n_req"], s["prices"]
            if k < n:
                v.append(1000 * (P["pw"] + P["pr"] * max(n - k - 2, 0)) / 1e6)
        if v:
            out[f"at_request_{k}"] = float(np.median(v))
    return out


def pass_fail(H):
    """Segment calls/$ in failed minus passed sessions (v2 only: uncensored), and within-task."""
    out = {}
    P_ = [s for s in H if s["passed"]]
    F_ = [s for s in H if s["passed"] is False]
    tasks = defaultdict(list)
    for s in H:
        tasks[s["task"]].append(s)
    mixed = {t: v for t, v in tasks.items() if len({x["passed"] for x in v}) == 2}
    for seg in SEGS[:-1] + ["_total"]:
        f = (lambda s: len(s["calls"])) if seg == "_total" else (
            lambda s, seg=seg: sum(1 for c in s["calls"] if c["seg"] == seg))
        g = (lambda s: s["cost_actual"]) if seg == "_total" else (
            lambda s, seg=seg: sum(lifetime(c) for c in s["calls"] if c["seg"] == seg))
        a, b = np.array([f(s) for s in F_], float), np.array([f(s) for s in P_], float)
        ga, gb = np.array([g(s) for s in F_]), np.array([g(s) for s in P_])
        ia = RNG.integers(0, len(a), (B, len(a)))
        ib = RNG.integers(0, len(b), (B, len(b)))
        dd = a[ia].mean(1) - b[ib].mean(1)
        du = ga[ia].mean(1) - gb[ib].mean(1)
        # within task: mean over mixed tasks of (fail mean - pass mean)
        wt = [np.mean([f(x) for x in v if not x["passed"]]) - np.mean([f(x) for x in v if x["passed"]])
              for v in mixed.values()]
        wt = np.array(wt)
        iw = RNG.integers(0, len(wt), (B, len(wt)))
        out[seg] = {
            "fail_minus_pass_calls": [float(a.mean() - b.mean()), float(np.percentile(dd, 5)), float(np.percentile(dd, 95))],
            "fail_minus_pass_usd": [float(ga.mean() - gb.mean()), float(np.percentile(du, 5)), float(np.percentile(du, 95))],
            "within_task_fail_minus_pass_calls": [float(wt.mean()), float(np.percentile(wt[iw].mean(1), 5)),
                                                  float(np.percentile(wt[iw].mean(1), 95))],
            "fail_mean_calls": float(a.mean()), "pass_mean_calls": float(b.mean()),
        }
    # share of $ spent in failed sessions
    out["_usd_share_in_failed"] = boot_ratio([s["cost_actual"] if not s["passed"] else 0 for s in H],
                                             [s["cost_actual"] for s in H])
    out["_n"] = {"pass": len(P_), "fail": len(F_), "mixed_tasks": len(mixed)}
    # hindsight: did the failed session's final diff even touch source?
    out["_failed_without_surviving_src_edit"] = sum(1 for s in F_ if not any(c["src_edit"] for c in s["calls"]))
    return out


def main():
    S = pickle.load((CACHE / "calls.pkl").open("rb"))
    ctrl = [s for s in S if s["arm"] == "control"]
    # model check
    chk = np.array([reprice(s)[2] / s["cost_actual"] for s in ctrl])
    groups = {
        "haiku_v2": [s for s in ctrl if s["fam"] == "haiku" and s["base"] == "adage-rb2"],
        "sonnet_v2": [s for s in ctrl if s["fam"] == "sonnet" and s["base"] == "adage-rb2"],
        "haiku_v1_censored30": [s for s in ctrl if s["fam"] == "haiku" and s["base"] == "adage-repobench"],
        "sonnet_v1_censored30": [s for s in ctrl if s["fam"] == "sonnet" and s["base"] == "adage-repobench"],
    }
    groups["haiku_v2_same_repos_as_sonnet"] = [s for s in groups["haiku_v2"] if s["repo"] in ("click", "vue-core")]
    groups["all_control"] = ctrl
    res = {"reprice_check_model_over_measured": [float(np.median(chk)), float(chk.min()), float(chk.max())]}
    for g, H in groups.items():
        res[g] = {"n": len(H), "cost_structure": cost_structure(H), "early_token_price_usd_per_1k": early_token_price(H),
                  "anatomy": anatomy(H), "ceilings": ceilings(H, g)}
    res["pass_fail_haiku_v2"] = pass_fail(groups["haiku_v2"])
    res["pass_fail_sonnet_v2"] = pass_fail(groups["sonnet_v2"]) if len(groups["sonnet_v2"]) > 4 else None
    # per repo (Haiku v2 + v1 urfave), the headline ceilings
    per_repo = {}
    for repo in ("attrs", "click", "marshmallow", "vue-core", "urfave-cli"):
        H = [s for s in ctrl if s["fam"] == "haiku" and s["repo"] == repo and
             (s["base"] == "adage-rb2" or repo == "urfave-cli")]
        c = ceilings(H, repo)
        per_repo[repo] = {"n": len(H), **{k: c[k]["usd_saved_share"] for k in c if not k.startswith("_")}}
    res["per_repo_usd_saved_share"] = per_repo
    (HERE / "ceilings.json").write_text(json.dumps(res, indent=1))
    print("ok", file=sys.stderr)


if __name__ == "__main__":
    main()
