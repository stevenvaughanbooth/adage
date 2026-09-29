"""Cost anatomy of held-out control sessions: where do the dollars go, and what is the oracle
ceiling of each family of levers from the literature?

A measurement instrument, not a learner. It reads the provider-reported `usage` of every API
request in each Claude Code transcript and reprices it (Haiku 4.5 list prices; Sonnet 5 at the
same ratios, so shares are price-independent). It then evaluates three *oracle* counterfactuals
under the cumulative-context cost model (every later request re-reads what an earlier one added):

  A. drop failed tool calls (is_error results) with their call and result tokens: the ceiling of
     "avoid the dead end" reflexes (what Adage does today);
  B. cap every tool result at K tokens: the ceiling of observation compression
     (AgentDiet, observation masking, SWE-Pruner, CoACT), ignoring any behaviour change;
  D. drop exact repeats of an earlier call with no edit in between (result-cache ceiling);
  C. drop every request before the first edit except the first N: the ceiling of
     localisation / "go straight to the right file" levers (repo maps, Agentless, retrieval).

Token counts of individual tool results are estimated as chars/4 (the usage fields give per-request
totals only). Hand-written parts: none in the learner sense; `is_error` is Claude Code's own flag,
and "edit" is the tool name Edit/Write/MultiEdit (instrument only).

    nice -n 10 python3 cost_anatomy.py  ->  cost_anatomy.json
"""

from __future__ import annotations

import json
import random
import re
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parents[1]))
from traces import BASES, RUN  # noqa: E402

REPOS = ["attrs", "click", "marshmallow", "urfave-cli", "vue-core", "click-sonnet5", "vue-core-sonnet5"]
# $/M tokens: input, 1h cache write, 5m cache write, cache read, output
PRICE = {"haiku": (1.0, 2.0, 1.25, 0.10, 5.0), "sonnet": (3.0, 6.0, 3.75, 0.30, 15.0)}
EDIT = {"Edit", "Write", "MultiEdit", "NotebookEdit"}


def price_of(model: str):
    return PRICE["sonnet"] if "sonnet" in (model or "") else PRICE["haiku"]


def parse(path: Path):
    """Requests in order: each with usage, model, and the tool calls it issued with the size
    (chars) of the call input and of its result, and whether the result was an error."""
    rows = [json.loads(l) for l in path.open() if l.strip()]
    results = {}
    for r in rows:
        if r.get("type") != "user":
            continue
        c = r.get("message", {}).get("content")
        if isinstance(c, list):
            for x in c:
                if isinstance(x, dict) and x.get("type") == "tool_result":
                    cc = x.get("content")
                    text = cc if isinstance(cc, str) else json.dumps(cc)
                    results[x.get("tool_use_id")] = (bool(x.get("is_error")), len(text))
    reqs, order = {}, []
    for r in rows:
        if r.get("type") != "assistant" or r.get("isSidechain"):
            continue
        m = r.get("message", {})
        mid = m.get("id")
        if mid not in reqs:
            reqs[mid] = {"usage": m.get("usage") or {}, "model": m.get("model", ""), "calls": []}
            order.append(mid)
        for x in m.get("content") or []:
            if isinstance(x, dict) and x.get("type") == "tool_use":
                err, n = results.get(x.get("id"), (False, 0))
                reqs[mid]["calls"].append({"tool": x.get("name"), "in_chars": len(json.dumps(x.get("input"))),
                                           "out_chars": n, "error": err,
                                           "sig": x.get("name", "") + json.dumps(x.get("input"), sort_keys=True)})
    return [reqs[i] for i in order]


def req_cost(u, p):
    cc = u.get("cache_creation") or {}
    w1 = cc.get("ephemeral_1h_input_tokens", 0)
    w5 = cc.get("ephemeral_5m_input_tokens", 0)
    if not cc:
        w5 = u.get("cache_creation_input_tokens", 0)
    parts = {"input": u.get("input_tokens", 0) * p[0], "cache_write": w1 * p[1] + w5 * p[2],
             "cache_read": u.get("cache_read_input_tokens", 0) * p[3], "output": u.get("output_tokens", 0) * p[4]}
    return {k: v / 1e6 for k, v in parts.items()}


def counterfactual(reqs, p, drop_call, cap=None, drop_req=None):
    """Saving ($) if some calls are removed (their tokens never enter the context) or their
    results capped at `cap` tokens, or whole requests removed. Tokens removed at request j are
    no longer written to cache (at j+1) nor read by any later request."""
    n = len(reqs)
    saved = 0.0
    for j, r in enumerate(reqs):
        later = n - j - 1  # requests that would re-read tokens added after request j
        if drop_req and drop_req(j, r):
            # the request itself (its context read + output) disappears, and its output and results
            # no longer propagate
            c = req_cost(r["usage"], p)
            saved += c["cache_read"] + c["input"] + c["output"]
            tok = r["usage"].get("output_tokens", 0) + sum(x["out_chars"] for x in r["calls"]) / 4
            saved += tok * (p[1] + p[3] * max(later - 1, 0)) / 1e6
            continue
        for x in r["calls"]:
            tok = 0.0
            if drop_call and drop_call(x):
                tok = (x["in_chars"] + x["out_chars"]) / 4
                saved += (x["in_chars"] / 4) * p[4] / 1e6  # the call's own output tokens
            elif cap is not None:
                tok = max(0.0, x["out_chars"] / 4 - cap)
            if tok and later >= 1:
                saved += tok * (p[1] + p[3] * (later - 1)) / 1e6
    return saved


def sessions():
    seen = set()
    for base in BASES:
        for repo in REPOS:
            tdir = base / repo / "transcripts"
            if not tdir.is_dir():
                continue
            for f in sorted(tdir.glob("*/*.jsonl")):
                if f.stem in seen:
                    continue
                rid = re.sub(r"-repo$", "", re.sub(r"^.*-runs-(?=(train|eval)-)", "", f.parent.name))
                m = RUN.match(rid)
                if not m:
                    continue
                phase, _, h, _, arm = m.groups()
                if phase != "eval" or arm != "control":
                    continue
                seen.add(f.stem)
                yield repo, f"{repo}-{h}", f


def boot(vals, reps=2000, seed=0):
    rnd = random.Random(seed)
    n = len(vals)
    ms = sorted(sum(rnd.choice(vals) for _ in range(n)) / n for _ in range(reps))
    return [ms[int(0.05 * reps)], ms[int(0.95 * reps)]]


def main():
    rows = []
    for repo, task, f in sessions():
        reqs = parse(f)
        if not reqs:
            continue
        p = price_of(reqs[0]["model"])
        parts = {"input": 0, "cache_write": 0, "cache_read": 0, "output": 0}
        for r in reqs:
            for k, v in req_cost(r["usage"], p).items():
                parts[k] += v
        total = sum(parts.values())
        calls = [x for r in reqs for x in r["calls"]]
        res_tok = sum(x["out_chars"] for x in calls) / 4
        first_edit = next((j for j, r in enumerate(reqs) if any(x["tool"] in EDIT for x in r["calls"])), len(reqs))
        row = {"repo": repo, "task": task, "model": reqs[0]["model"], "requests": len(reqs), "calls": len(calls),
               "failed_calls": sum(x["error"] for x in calls), "cost": total,
               **{f"share_{k}": v / total for k, v in parts.items()},
               "result_tokens": res_tok,
               "max_context": max(r["usage"].get("cache_read_input_tokens", 0) + r["usage"].get("cache_creation_input_tokens", 0) + r["usage"].get("input_tokens", 0) for r in reqs),
               "requests_before_first_edit": first_edit,
               "A_drop_failed": counterfactual(reqs, p, lambda x: x["error"]) / total}
        # D: exact repeats of an earlier call (same tool, same input) with no edit in between: the
        # ceiling of a "you already have this" / result-cache lever.
        seen_sigs = set()
        for r in reqs:
            for x in r["calls"]:
                if x["tool"] in EDIT:
                    seen_sigs.clear()
                    x["repeat"] = False
                    continue
                x["repeat"] = x["sig"] in seen_sigs
                seen_sigs.add(x["sig"])
        row["repeat_calls"] = sum(x["repeat"] for x in calls)
        row["D_drop_repeats"] = counterfactual(reqs, p, lambda x: x["repeat"]) / total
        for cap in (250, 500, 1000, 2000):
            row[f"B_cap_{cap}"] = counterfactual(reqs, p, None, cap=cap) / total
        # C: keep the first 3 requests before the first edit (reading the prompt, one search, one
        # read), drop the rest of the pre-edit exploration: an upper bound on perfect localisation.
        row["C_perfect_localisation"] = counterfactual(reqs, p, None, drop_req=lambda j, r: 3 <= j < first_edit) / total
        rows.append(row)

    def summ(sel, key):
        v = [r[key] for r in sel]
        return {"mean": sum(v) / len(v), "ci90": boot(v), "n": len(v)}

    groups = {"haiku": [r for r in rows if "haiku" in r["model"]], "sonnet": [r for r in rows if "sonnet" in r["model"]]}
    keys = ["cost", "requests", "calls", "failed_calls", "max_context", "requests_before_first_edit", "share_input",
            "share_cache_write", "share_cache_read", "share_output", "A_drop_failed", "B_cap_250", "B_cap_500",
            "B_cap_1000", "B_cap_2000", "C_perfect_localisation", "repeat_calls", "D_drop_repeats"]
    out = {g: {k: summ(sel, k) for k in keys} for g, sel in groups.items() if sel}
    out["per_repo"] = {repo: {k: summ([r for r in rows if r["repo"] == repo], k) for k in
                              ["cost", "calls", "A_drop_failed", "B_cap_1000", "C_perfect_localisation"]}
                       for repo in sorted({r["repo"] for r in rows})}
    (HERE / "cost_anatomy.json").write_text(json.dumps(out, indent=1))
    for g in groups:
        if g in out:
            print(g, {k: (round(v["mean"], 3), [round(c, 3) for c in v["ci90"]]) for k, v in out[g].items()}, out[g]["cost"]["n"])
    for repo, d in out["per_repo"].items():
        print(repo, {k: round(v["mean"], 3) for k, v in d.items()}, d["cost"]["n"])


if __name__ == "__main__":
    main()
