"""The click micro live slice: per session, directives delivered (from the hook's own record in the
transcript: hook attachments), whether the agent's next test run was in the directed class
(abstract follow), environment dead ends hit, and calls from the first test attempt to the first
test run that ran tests."""
import os
import json, glob, re, sys, collections
sys.path.insert(0, os.environ.get("LD", "/tmp/adage-learner") + "")
from shat import trace, runner, executed, ENV

R = os.environ.get("LD", "/tmp/adage-learner") + "/slice/click-slice"
st = json.load(open(f"{R}/state.json"))["runs"]
rows = []
for rid, run in sorted(st.items()):
    if run.get("phase") != "eval":
        continue
    m = run.get("metrics") or {}
    sid = m.get("session_id")
    f = glob.glob(f"{R}/private/transcripts/*-{rid}-repo/{sid}.jsonl")
    if not f:
        continue
    cs = trace(f[0])
    text = open(f[0]).read()
    directives = m.get("adage") or []
    tests = [(i, c) for i, c in enumerate(cs) if c[1] == "Bash" and runner(c[2])]
    dead = sum(1 for _, c in tests if not executed(c[3]) and ENV.search(c[3]))
    first = tests[0][0] if tests else None
    goal = next((i for i, c in tests if executed(c[3])), None)
    to_goal = (goal - first) if first is not None and goal is not None else None
    venv_runs = sum(1 for _, c in tests if runner(c[2])[0] == "venv")
    rows.append({"run": rid, "arm": run["arm"], "task": run["task"], "passed": m.get("passed"), "cost": m.get("cost_usd"),
                 "directives": len(directives), "routes": [d.get("route") for d in directives],
                 "follow": m.get("follow"), "test_runs": len(tests), "venv_runs": venv_runs, "dead_ends": dead,
                 "calls_first_test_to_goal": to_goal, "turns": m.get("turns")})
for r in rows:
    print(json.dumps(r))
by = collections.defaultdict(list)
for r in rows:
    by[r["arm"]].append(r)
for arm, rs in by.items():
    tg = [r["calls_first_test_to_goal"] for r in rs if r["calls_first_test_to_goal"] is not None]
    print(arm, "sessions", len(rs), "cost", round(sum(r["cost"] or 0 for r in rs), 2), "directives", sum(r["directives"] for r in rs),
          "dead ends", sum(r["dead_ends"] for r in rs), "venv test runs", sum(r["venv_runs"] for r in rs), "/", sum(r["test_runs"] for r in rs),
          "mean calls first test->goal", round(sum(tg) / len(tg), 2) if tg else None, tg)
