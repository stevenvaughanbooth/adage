"""Prototype of the outcome model on RepoBench traces: env dead-end triggers (a test run whose output
says `No module named ...`), the next test attempt's interpreter class, calls to the first test
run that executed tests (G_test)."""
import os
import json, re, sys, collections
sys.path.insert(0, os.environ.get("LD", "/tmp/adage-learner") + "")
from tr import calls

D = os.environ.get("LD", "/tmp/adage-learner") + ""
EXEC = re.compile(r"collected [1-9]\d* items?|\b\d+ (passed|failed|error)|Test Files |\bTests: |^ok\s|test result:|Ran \d+ tests?", re.M)
NOT = re.compile(r"no tests ran|ModuleNotFoundError|No module named|command not found")
TESTCMD = re.compile(r"pytest|vitest|npm (run )?test|go test|jest|tox|nox|unittest")
ENVFAIL = re.compile(r"No module named '?(\w+)|command not found|Cannot find module")


def cls(cmd):
    c = cmd
    if re.search(r"(source|\.) \S*venv/bin/activate", c) or re.search(r"\S*venv/bin/(python|pytest)", c):
        return "venv"
    if re.search(r"uv run", c):
        return "uv"
    if re.search(r"\.local/bin/pytest", c):
        return "user-site"
    if re.search(r"pip install", c):
        return "pip"
    return "bare"


def analyse(repo, split):
    m = json.load(open(f"{D}/ds/rb-{repo}/manifest.json"))
    ids = set(m["splits"]["heldout"][split])
    rows = []
    for s in m["sessions"]:
        if s["id"] not in ids:
            continue
        cs = calls(f"{D}/ds/rb-{repo}/{s['file']}")
        trig = None
        for i, (n, c, (err, out)) in enumerate(cs):
            if n != "Bash" or not TESTCMD.search(c):
                continue
            if ENVFAIL.search(out) and not EXEC.search(out):
                trig = i
                break
        if trig is None:
            continue
        # next test attempt after the trigger, and the goal
        nxt, goal = None, None
        for j in range(trig + 1, len(cs)):
            n, c, (err, out) = cs[j]
            if n != "Bash":
                continue
            if nxt is None and (TESTCMD.search(c) or "pip install" in c):
                nxt = cls(c)
            if TESTCMD.search(c) and EXEC.search(out) and not re.search(r"no tests ran", out):
                goal = j
                break
        rows.append((s["run"], cls(cs[trig][1]), nxt, (goal - trig) if goal else None))
    return rows


for repo in sys.argv[1:]:
    for split in ("train", "test"):
        rows = analyse(repo, split)
        by = collections.defaultdict(list)
        for r in rows:
            by[r[2]].append(r[3])
        print(repo, split, "triggers", len(rows), {k: (len(v), [x for x in v]) for k, v in by.items()})
