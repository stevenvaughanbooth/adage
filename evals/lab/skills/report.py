#!/usr/bin/env python3
"""Summarise a skill-lab run root: per arm, outcome, cost, turns, tokens, directives, and how
closely the sessions followed the skill's procedure (the alignment, per step).

  python3 report.py <SKILL.md> <root> [--neither <rb2 root> --tasks t1,t2] [--json out.json]

`value captured` = (arm - neither) / (skill - neither) per metric: the share of what the skill in
context bought that the compiled reflexes bought too (1.0 = all of it; > 1: more).
"""

from __future__ import annotations

import json
import statistics
import sys
from collections import Counter, defaultdict
from pathlib import Path

from abstract import read_session
from align import analyse
from procedure import approx_tokens, parse

ARM_NAMES = {"off": "neither", "control": "neither", "agents-md": "skill", "adage": "compiled"}


def usage(run_dir: Path) -> dict:
    p = run_dir / "stream.jsonl"
    if not p.exists():
        return {}
    ids = set()
    for line in p.read_text().splitlines():
        try:
            m = json.loads(json.loads(line)["line"])
        except (ValueError, KeyError):
            continue
        if m.get("type") == "assistant":
            ids.add((m.get("message") or {}).get("id"))
        if m.get("type") == "result":
            u = m.get("usage") or {}
            return {"input_total": (u.get("input_tokens") or 0) + (u.get("cache_read_input_tokens") or 0)
                    + (u.get("cache_creation_input_tokens") or 0), "output": u.get("output_tokens") or 0,
                    "model_calls": len(ids - {None})}
    return {}


def directive_text(r: dict) -> tuple[int, Counter]:
    chars, routes = 0, Counter()
    for d in r.get("directives") or []:
        try:
            j = json.loads(d["raw"])
            ctx = json.loads(j["output"])["hookSpecificOutput"].get("additionalContext", "")
        except (ValueError, KeyError, TypeError):
            continue
        chars += len(ctx)
        import re
        routes.update(re.findall(r'route="([a-z0-9-]+)"', ctx))
    return chars, routes


def load_runs(root: Path, tasks=None, arms_rename=None, phase="eval"):
    s = json.loads((root / "state.json").read_text())
    out = []
    for r in s["runs"].values():
        if r.get("phase") != phase or not r.get("metrics") or (tasks and r["task"] not in tasks):
            continue
        if (r["metrics"].get("status") or "ok") in ("infra_error", "scorer_error"):
            continue
        arm = (arms_rename or ARM_NAMES).get(r["arm"], r["arm"])
        u = usage(root / "runs" / r["id"])
        chars, routes = directive_text(r)
        tr = list((root / "private" / "transcripts").glob(f"*/{r['metrics'].get('session_id')}.jsonl"))
        out.append({"arm": arm, "task": r["task"], "passed": bool(r["metrics"].get("passed")),
                    "cost": r["metrics"].get("cost_usd") or 0.0, "turns": r["metrics"].get("turns") or 0,
                    "dead_ends": r["metrics"].get("dead_end_hits") or 0, **u, "directive_tokens": round(chars / 4),
                    "routes": routes, "transcript": str(tr[0]) if tr else None})
    return out


def agg(rows: list[dict], skill_tokens: int) -> dict:
    if not rows:
        return {}
    med = lambda k: statistics.median([r[k] for r in rows if r.get(k) is not None]) if rows else None
    mean = lambda k: round(statistics.mean([r[k] for r in rows if r.get(k) is not None]), 4) \
        if any(r.get(k) is not None for r in rows) else None
    by_task = defaultdict(list)
    for r in rows:
        by_task[r["task"]].append(r["passed"])
    out = {"n": len(rows), "pass_rate": round(sum(r["passed"] for r in rows) / len(rows), 3),
           "task_pass_rate": round(statistics.mean(statistics.mean(v) for v in by_task.values()), 3),
           "mean_cost_usd": mean("cost"), "median_turns": med("turns"), "mean_turns": mean("turns"),
           "mean_dead_ends": mean("dead_ends"), "mean_input_tokens": mean("input_total"),
           "mean_output_tokens": mean("output"), "mean_model_calls": mean("model_calls"),
           "mean_directive_tokens": mean("directive_tokens"),
           "routes_delivered": dict(sum((r["routes"] for r in rows), Counter()))}
    if rows[0]["arm"] == "skill" and out.get("mean_model_calls") is not None:
        # The skill text rides in every model call's context (cached, but still read).
        out["skill_tokens_read"] = round(skill_tokens * out["mean_model_calls"])
    return out


def captured(a: dict, skill: dict, neither: dict) -> dict:
    res = {}
    for k in ("pass_rate", "mean_cost_usd", "mean_turns", "mean_input_tokens"):
        if None in (a.get(k), skill.get(k), neither.get(k)) or skill[k] == neither[k]:
            res[k] = None
        else:
            res[k] = round((a[k] - neither[k]) / (skill[k] - neither[k]), 2)
    return res


def adherence(graph: dict, rows: list[dict]) -> dict:
    """Per arm, the alignment of its sessions to the procedure: per step, sessions that did it."""
    sessions = [read_session(r["transcript"]) for r in rows if r.get("transcript")]
    if not sessions:
        return {}
    a = analyse(graph, sessions)
    return {s["id"]: {"did": s["aligned"], "applicable": s["applicable"], "exact": s["exact"],
                      "timely": s["timely"], "forbidden_used": s["avoided_hits"]} for s in a["steps"]}


def main(argv):
    skill, root = argv[0], Path(argv[1])
    opt = {argv[i]: argv[i + 1] for i in range(2, len(argv) - 1) if argv[i].startswith("--")}
    graph = parse(skill).to_json()
    stoks = approx_tokens(Path(skill).read_text())
    rows = load_runs(root)
    tasks = sorted({r["task"] for r in rows})
    if "--neither" in opt:
        extra = load_runs(Path(opt["--neither"]), tasks=set(tasks), arms_rename={"control": "neither"})
        rows += [r for r in extra if r["arm"] == "neither"]
    arms = {}
    for arm in ("neither", "skill", "compiled"):
        rr = [r for r in rows if r["arm"] == arm]
        if rr:
            arms[arm] = agg(rr, stoks)
            arms[arm]["procedure"] = adherence(graph, rr)
    if all(k in arms for k in ("neither", "skill", "compiled")):
        arms["compiled"]["value_captured"] = captured(arms["compiled"], arms["skill"], arms["neither"])
    per_task = defaultdict(dict)
    for r in rows:
        per_task[r["task"]].setdefault(r["arm"], []).append(
            {"passed": r["passed"], "cost": round(r["cost"], 4), "turns": r["turns"]})
    out = {"skill": graph["skill"]["name"], "skill_tokens": stoks, "tasks": tasks, "arms": arms,
           "per_task": per_task, "total_cost_usd": round(sum(r["cost"] for r in rows if r["arm"] != "neither"
                                                           or "--neither" not in opt), 4)}
    text = json.dumps(out, indent=1, default=str)
    if "--json" in opt:
        Path(opt["--json"]).write_text(text + "\n")
    print(text)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
