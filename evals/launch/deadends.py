#!/usr/bin/env python3
"""Where the dead ends Adage's instincts target actually happened, and why they were (not) caught.

  python3 evals/launch/deadends.py <runner root>... [--json out.json]

For each held-out session of each root (stream.jsonl), finds Bash calls whose output carries one of
U11's failure signatures (the test runner run outside the project's environment: `No module named
pytest`, `pytest: command not found`, `No module named '<package>'`) and records whether the
harness saw the call fail (`is_error`, so PostToolUseFailure fired and a failure-triggered instinct
could match) or not (typically `… 2>&1 | head`: the pipeline's status is head's, 0). Also counts the
directives each session received, by route.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from collections import Counter
from pathlib import Path

SIGS = [r"No module named pytest", r"pytest: command not found", r"command not found: pytest",
        r"No module named '(click|attr|attrs|marshmallow)'", r"python: command not found"]
SIG = re.compile("|".join(SIGS))
TEST = re.compile(r"\b(pytest|py\.test|tox|nox)\b")


RAN = re.compile(r"test session starts|\d+ (passed|failed|error)")
PROJECT_ENV = re.compile(r"\.venv/bin/|source \.venv|\. \.venv|uv run|\btox\b|\bnox\b")


def session(stream: Path) -> dict:
    """U11 dead ends in one session, and the recovery after the first: `recovery_calls` is the
    number of tool calls (any tool) from the first dead end to the first test run that actually ran
    tests; `next_test_in_env` whether the next test command after it used the project's environment
    (what U11 directs), whatever arguments it carried."""
    uses, order = {}, []
    out = {"u11_hits": 0, "u11_hits_error": 0, "u11_hits_piped_ok": 0, "examples": [],
           "recovery_calls": None, "next_test_in_env": None}
    first, results = None, {}
    for line in stream.read_text(errors="replace").splitlines():
        try:
            m = json.loads(json.loads(line)["line"])
        except (ValueError, KeyError, TypeError):
            continue
        if m.get("type") == "assistant":
            for c in m.get("message", {}).get("content", []) or []:
                if c.get("type") == "tool_use":
                    order.append(c.get("id"))
                    if c.get("name") == "Bash":
                        uses[c.get("id")] = (c.get("input") or {}).get("command", "")
        elif m.get("type") == "user":
            content = m.get("message", {}).get("content", [])
            for c in content if isinstance(content, list) else []:
                if not (isinstance(c, dict) and c.get("type") == "tool_result" and c.get("tool_use_id") in uses):
                    continue
                cmd = uses[c["tool_use_id"]]
                text = c.get("content")
                text = text if isinstance(text, str) else json.dumps(text)
                results[c["tool_use_id"]] = (cmd, text)
                if TEST.search(cmd) and SIG.search(text):
                    out["u11_hits"] += 1
                    first = first or c["tool_use_id"]
                    if c.get("is_error"):
                        out["u11_hits_error"] += 1
                    else:
                        out["u11_hits_piped_ok"] += 1
                    if len(out["examples"]) < 2:
                        out["examples"].append({"cmd": cmd[:160], "is_error": bool(c.get("is_error")),
                                                "sig": SIG.search(text).group(0)})
    if first and first in order:
        i0 = order.index(first)
        for j, uid in enumerate(order[i0 + 1:], start=1):
            if uid not in results:
                continue
            cmd, text = results[uid]
            if TEST.search(cmd) and out["next_test_in_env"] is None:
                out["next_test_in_env"] = bool(PROJECT_ENV.search(cmd))
            if TEST.search(cmd) and not SIG.search(text) and RAN.search(text):
                out["recovery_calls"] = j
                break
    return out


def routes_of(run: dict) -> list[str]:
    ids = []
    for d in run.get("directives") or []:
        # A hook_response carries its text twice (output and stdout): count each route once.
        ids += sorted(set(re.findall(r'route=\\*"([A-Za-z0-9_.-]+)', d.get("raw", ""))))
    return ids


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("roots", nargs="+")
    ap.add_argument("--json")
    a = ap.parse_args(argv)
    rows = []
    for root in map(Path, a.roots):
        state = json.loads((root / "state.json").read_text())
        for rid, run in sorted(state["runs"].items()):
            if run.get("phase") == "train" or run.get("status") not in ("passed", "failed"):
                continue
            st = root / "runs" / rid / "stream.jsonl"
            if not st.exists():
                continue
            s = session(st)
            m = run.get("metrics") or {}
            spool = m.get("adage") or []
            would = [w for rec in spool for w in (rec.get("would_have") or [])] if isinstance(spool, list) else []
            rows.append({"root": f"{root.parent.parent.name}/{root.name}", "run": rid, "arm": run.get("arm"), "task": run.get("task"),
                         "passed": m.get("passed"), "turns": m.get("turns"), "cost_usd": m.get("cost_usd"),
                         "delivered_routes": routes_of(run), "follow": m.get("follow"), **s})
    by = Counter()
    for r in rows:
        k = (r["root"], r["arm"])
        by[k + ("sessions",)] += 1
        by[k + ("with_u11_dead_end",)] += r["u11_hits"] > 0
        by[k + ("u11_hits",)] += r["u11_hits"]
        by[k + ("u11_hits_error",)] += r["u11_hits_error"]
        by[k + ("u11_hits_piped_ok",)] += r["u11_hits_piped_ok"]
        by[k + ("sessions_with_directive",)] += bool(r["delivered_routes"])
        by[k + ("directives",)] += len(r["delivered_routes"])
        if r["u11_hits"]:
            by[k + ("next_test_in_env",)] += bool(r["next_test_in_env"])
            if r["recovery_calls"] is not None:
                by[k + ("recovered",)] += 1
                by[k + ("recovery_calls_sum",)] += r["recovery_calls"]
    for k in {k[:2] for k in by}:
        n = by[k + ("recovered",)]
        by[k + ("mean_recovery_calls",)] = round(by[k + ("recovery_calls_sum",)] / n, 2) if n else "–"
    keys = sorted({k[:2] for k in by})
    cols = ["sessions", "with_u11_dead_end", "u11_hits", "u11_hits_error", "u11_hits_piped_ok",
            "next_test_in_env", "recovered", "mean_recovery_calls", "sessions_with_directive", "directives"]
    print("| root | arm | " + " | ".join(cols) + " |")
    print("|" + " --- |" * (2 + len(cols)))
    for k in keys:
        print(f"| {k[0]} | {k[1]} | " + " | ".join(str(by[k + (c,)]) for c in cols) + " |")
    fired = Counter(rt for r in rows for rt in r["delivered_routes"])
    print("\nroutes delivered:", dict(fired))
    if a.json:
        Path(a.json).write_text("\n".join(json.dumps(r) for r in rows) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
