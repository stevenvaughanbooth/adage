"""Ceiling of a "you are done" lever: the cost share of everything after the first green test run
that follows the session's last edit (held-out control sessions).

INSTRUMENT ONLY (hand-written): a "test run" is a Bash call whose command mentions a test runner
seen in these traces (pytest, go test, npm test, vitest, jest), and "green" is: not is_error and
the output has a pass signature and no failure signature. A shipped lever would have to learn the
goal event (outcome-optimisation.md G_test) from the traces instead.

Also reports: does the session's hidden-test outcome differ between sessions whose tail is long
and short (is the tail doing anything for pass rate?).
"""
import json, re, sys
from pathlib import Path
import numpy as np
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parents[1])); sys.path.insert(0, str(HERE))
from cost_anatomy import sessions, parse, price_of, req_cost, counterfactual, boot, EDIT
from traces import BASES

RUNNER = re.compile(r"pytest|go test|npm (run )?test|vitest|jest")
PASS = re.compile(r"\b\d+ passed\b|^ok\s|\bPASS\b|Tests?:?\s+\d+ passed|✓", re.M)
FAIL = re.compile(r"\b\d+ failed\b|^FAIL|^--- FAIL|\berror\b|Error:", re.M)

def outcome(repo, f):
    rid = re.sub(r"-repo$", "", re.sub(r"^.*-runs-(?=(train|eval)-)", "", f.parent.name))
    for b in BASES:
        try:
            st = json.loads((b / repo / "state.json").read_text()).get("runs", {}).get(rid)
        except Exception:
            st = None
        if st:
            m = st.get("metrics") or {}
            v = m.get("hidden_passed", m.get("passed"))
            if v is None and st.get("status") in ("passed", "failed"):
                v = st["status"] == "passed"
            return v
    return None

def main():
    out = {}
    rows = []
    for repo, task, f in sessions():
        lines = [json.loads(l) for l in f.open() if l.strip()]
        res = {}
        for r in lines:
            c = r.get("message", {}).get("content") if r.get("type") == "user" else None
            if isinstance(c, list):
                for x in c:
                    if isinstance(x, dict) and x.get("type") == "tool_result":
                        cc = x.get("content"); res[x.get("tool_use_id")] = (bool(x.get("is_error")), cc if isinstance(cc, str) else json.dumps(cc))
        order, calls = [], {}
        for r in lines:
            if r.get("type") != "assistant" or r.get("isSidechain"):
                continue
            mid = r["message"].get("id")
            if mid not in calls:
                calls[mid] = []; order.append(mid)
            for x in r["message"].get("content") or []:
                if isinstance(x, dict) and x.get("type") == "tool_use":
                    calls[mid].append(x)
        reqs = parse(f)
        if not reqs:
            continue
        p = price_of(reqs[0]["model"]); total = sum(sum(req_cost(r["usage"], p).values()) for r in reqs)
        last_edit = max([j for j, m in enumerate(order) if any(x["name"] in EDIT for x in calls[m])], default=None)
        green = None
        if last_edit is not None:
            for j in range(last_edit + 1, len(order)):
                for x in calls[order[j]]:
                    cmd = (x.get("input") or {}).get("command", "") if x["name"] == "Bash" else ""
                    err, text = res.get(x.get("id"), (True, ""))
                    if cmd and RUNNER.search(cmd) and not err and PASS.search(text) and not FAIL.search(text):
                        green = j; break
                if green is not None:
                    break
        # requests after the green run's follow-up request (keep one request to write the answer)
        share = counterfactual(reqs, p, None, drop_req=lambda j, r: green is not None and j > green + 1) / total
        after = (len(reqs) - green - 2) if green is not None else 0
        rows.append({"model": reqs[0]["model"], "repo": repo, "green": green is not None, "share": share,
                     "after": max(after, 0), "passed": outcome(repo, f)})
    for g in ("haiku", "sonnet"):
        sel = [r for r in rows if g in r["model"]]
        gr = [r for r in sel if r["green"]]
        o = {"n": len(sel), "sessions_with_green_after_last_edit": len(gr),
             "requests_after_green": float(np.mean([r["after"] for r in gr])) if gr else 0,
             "ceiling_share_all_sessions": {"mean": float(np.mean([r["share"] for r in sel])), "ci90": boot([r["share"] for r in sel])},
             "pass_rate_green": float(np.mean([bool(r["passed"]) for r in gr])) if gr else None,
             "pass_rate_no_green": float(np.mean([bool(r["passed"]) for r in sel if not r["green"]])) if len(gr) < len(sel) else None}
        if gr:
            med = np.median([r["after"] for r in gr])
            o["pass_rate_green_long_tail"] = float(np.mean([bool(r["passed"]) for r in gr if r["after"] > med]))
            o["pass_rate_green_short_tail"] = float(np.mean([bool(r["passed"]) for r in gr if r["after"] <= med]))
        o["per_repo_ceiling"] = {r: round(float(np.mean([x["share"] for x in sel if x["repo"] == r])), 3)
                                 for r in sorted({x["repo"] for x in sel})}
        out[g] = o
    (HERE / "tail_after_green.json").write_text(json.dumps(out, indent=1))
    print(json.dumps(out, indent=1))

main()
