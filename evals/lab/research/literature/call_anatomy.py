"""Where do the calls of a held-out control session go? A measurement instrument (it uses the lab's
hand-written normaliser `normalize.py` to name actions, so it is a baseline view, not a learner).

For each Haiku/Sonnet held-out control session: calls by normalised key, by phase (before the first
edit / between first and last edit / after the last edit), the share of tool-result characters by
key, and the cost share of the requests after the last edit (the verification tail).

    nice -n 10 python3 call_anatomy.py -> call_anatomy.json
"""
import json, sys
from collections import Counter, defaultdict
from pathlib import Path
import numpy as np
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parents[1])); sys.path.insert(0, str(HERE))
from normalize import normalize_tool
from cost_anatomy import sessions, parse, price_of, req_cost, counterfactual, boot, EDIT

def main():
    agg = {}
    for grp in ("haiku", "sonnet"):
        agg[grp] = {"calls": Counter(), "chars": Counter(), "phase": Counter(), "n": 0, "tail_share": [],
                    "pre_share": [], "tail_calls": [], "pre_calls": [], "loop_calls": [], "test_like": []}
    for repo, task, f in sessions():
        rows = [json.loads(l) for l in f.open() if l.strip()]
        res = {}
        for r in rows:
            c = r.get("message", {}).get("content") if r.get("type") == "user" else None
            if isinstance(c, list):
                for x in c:
                    if isinstance(x, dict) and x.get("type") == "tool_result":
                        cc = x.get("content"); res[x.get("tool_use_id")] = len(cc if isinstance(cc, str) else json.dumps(cc))
        reqs = parse(f)
        if not reqs:
            continue
        g = "sonnet" if "sonnet" in reqs[0]["model"] else "haiku"
        A = agg[g]; A["n"] += 1
        keys = []
        for r in rows:
            if r.get("type") != "assistant" or r.get("isSidechain"):
                continue
            for x in r.get("message", {}).get("content") or []:
                if isinstance(x, dict) and x.get("type") == "tool_use":
                    a = normalize_tool(x.get("name", ""), x.get("input") or {})
                    k = a.key if x.get("name") == "Bash" else x.get("name")
                    keys.append((k, res.get(x.get("id"), 0), x.get("name") in EDIT))
        edits = [i for i, (_, _, e) in enumerate(keys) if e]
        fe, le = (edits[0], edits[-1]) if edits else (len(keys), len(keys))
        for i, (k, n, e) in enumerate(keys):
            A["calls"][k] += 1; A["chars"][k] += n
            A["phase"]["pre" if i < fe else "tail" if i > le else "loop"] += 1
        A["pre_calls"].append(fe); A["tail_calls"].append(len(keys) - le - 1 if edits else 0); A["loop_calls"].append(le - fe + 1 if edits else 0)
        A["test_like"].append(sum(1 for k, _, _ in keys if "test" in k))
        p = price_of(reqs[0]["model"]); total = sum(sum(req_cost(r["usage"], p).values()) for r in reqs)
        req_edit = [j for j, r in enumerate(reqs) if any(x["tool"] in EDIT for x in r["calls"])]
        if req_edit:
            A["tail_share"].append(counterfactual(reqs, p, None, drop_req=lambda j, r: j > req_edit[-1] + 1) / total)
    out = {}
    for g, A in agg.items():
        if not A["n"]:
            continue
        tc = sum(A["calls"].values()); tch = sum(A["chars"].values())
        out[g] = {"n": A["n"], "calls_per_session": tc / A["n"],
                  "top_keys": [(k, round(v / A["n"], 2), round(A["chars"][k] / tch, 3)) for k, v in A["calls"].most_common(15)],
                  "phase_calls_per_session": {k: v / A["n"] for k, v in A["phase"].items()},
                  "pre_first_edit_calls": float(np.mean(A["pre_calls"])),
                  "loop_calls": float(np.mean(A["loop_calls"])), "tail_calls": float(np.mean(A["tail_calls"])),
                  "test_named_calls": float(np.mean(A["test_like"])),
                  "tail_cost_share_after_last_edit_plus_one": {"mean": float(np.mean(A["tail_share"])), "ci90": boot(A["tail_share"])}}
    (HERE / "call_anatomy.json").write_text(json.dumps(out, indent=1))
    print(json.dumps(out, indent=1))
main()
