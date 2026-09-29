"""Interim replay value metric (architecture-review §5.2, outcome-optimisation §5.1) over `adage eval
--json` output: per directive, useful / harmful / calls saved; Ŝ per session, with a task-clustered
bootstrap. Until the replay-metric (L8) implementation lands in adage-eval.

  python3 shat.py <dataset-dir> <eval.json> [--follow 0.8] [--cost 0.1]
"""
import json, re, sys, random, collections, math

EXEC = re.compile(r"(?m)collected [1-9]\d* items?|\b\d+ (passed|failed|errors?)\b|\bRan [1-9]\d* tests?\b|Test Files\s+\d|^\s*Tests:\s+\d|^(ok|FAIL)\s+\S+\s+[\d.]+s|^--- (PASS|FAIL)|test result: (ok|FAILED)")
NOTEXEC = re.compile(r"no tests ran|No module named|ModuleNotFoundError|command not found|Cannot find module")
PASSED = re.compile(r"\b\d+ (passed|failed)\b|Test Files\s+\d|test result: ")
ENV = re.compile(r"No module named|command not found|Cannot find module")
TEST = re.compile(r"(^|[\s;&|(])(\S*python3?(\.\d+)?\s+(-\S+\s+)*-m\s+(pytest|unittest)|\S*pytest|py\.test|vitest|jest|(npm|pnpm|yarn|bun)\s+(run\s+)?test|go\s+test|cargo\s+test|tox|nox)\b")
FRICTION = re.compile(r"pip3?\s+(install|list|show)|-m\s+pip\b|uv\s+(add|pip|sync)|npm\s+(install|ci)\b|\bwhich\b|\.venv|venv/|site-packages|pyvenv|--version|\bls\b.*(bin|venv|lib)|pyproject\.toml|setup\.cfg|tox\.ini|requirements")


def executed(out):
    return bool(EXEC.search(out)) and not (NOTEXEC.search(out) and not PASSED.search(out))


def runner(cmd):
    """(env, family) of a test command, or None."""
    m = TEST.search(cmd)
    if not m:
        return None
    t = m.group(0)
    fam = "pytest" if "pytest" in t or "py.test" in t else ("unittest" if "unittest" in t else re.sub(r"\s+", " ", t.strip().split("/")[-1]))
    if re.search(r"(source|\.)\s+\S*venv/bin/activate", cmd) or re.search(r"\S*venv/bin/(python|pytest)", t):
        env = "venv"
    elif re.search(r"\b(uv|poetry|pdm|hatch)\s+run\b", cmd):
        env = "managed"
    elif ".local/bin" in t:
        env = "user-site"
    else:
        env = "bare"
    return (env, fam)


def trace(path):
    """Calls in order: (turn, tool, command-or-path, output, is_error)."""
    turn, last_msg = 0, None
    calls, res = [], {}
    for line in open(path):
        try:
            j = json.loads(line)
        except Exception:
            continue
        m = j.get("message") or {}
        c = m.get("content")
        if not isinstance(c, list):
            continue
        for b in c:
            if b.get("type") == "tool_use":
                if m.get("id") != last_msg:
                    turn += 1
                    last_msg = m.get("id")
                i = b.get("input", {})
                text = i.get("command") or i.get("file_path") or " ".join(str(i.get(k, "")) for k in ("pattern", "path", "glob"))
                calls.append([turn, b["name"], text, b["id"]])
            if b.get("type") == "tool_result":
                cc = b.get("content")
                if isinstance(cc, list):
                    cc = " ".join(x.get("text", "") for x in cc if isinstance(x, dict))
                res[b["tool_use_id"]] = (str(cc), bool(b.get("is_error")))
    return [(t, n, cmd, *res.get(i, ("", False))) for t, n, cmd, i in calls]


def is_goal(call):
    _, n, cmd, out, _ = call
    return n == "Bash" and runner(cmd) is not None and executed(out)


DISCOVER = re.compile(r"__tests__|(^|[\s/'\"])tests?/|\.spec\.|\.test\.|test_\w*|_test\.go|conftest")


def friction(call, targets=False):
    """W(d): environment and invocation friction; with `targets` (a directive that names the test
    files), test discovery too: searching for which tests exist or cover something."""
    _, n, cmd, out, err = call
    if targets and n in ("Grep", "Glob") and DISCOVER.search(cmd):
        return True
    if n != "Bash":
        return False
    if targets and re.match(r"\s*(find|ls|grep|rg|tree)\b", cmd) and DISCOVER.search(cmd) and runner(cmd) is None:
        return True
    r = runner(cmd)
    if r is not None and not executed(out):
        return True
    return bool(FRICTION.search(cmd)) or bool(ENV.search(out) and not executed(out))


def directed(text):
    m = re.search(r"\n  (.+)", text)
    return m.group(1).strip() if m else ""


H = 15
RECALL = []


def score(ds, ev, follow=0.8, cost=0.1):
    man = json.load(open(f"{ds}/manifest.json"))
    by_id = {s["id"]: s for s in man["sessions"]}
    traces = {s["id"]: trace(f"{ds}/{by_id[s['id']]['file']}") for s in ev["sessions"]}
    # Runner classes that ran tests somewhere in the held-out traces: a replay of d would succeed.
    verified = set()
    for cs in traces.values():
        for c in cs:
            if is_goal(c):
                verified.add(runner(c[2]))
    rows = []
    for s in ev["sessions"]:
        cs = traces[s["id"]]
        task = by_id[s["id"]].get("task") or s["id"]
        credited = set()  # a call is saved at most once per session, by the first directive before it
        for d in s["directives"]:
            cmd = directed(d["text"])
            want = runner(cmd)
            k0 = max((i for i, c in enumerate(cs) if c[0] <= d["turn"]), default=-1)
            after = cs[k0 + 1:]
            tstar = next((i for i, c in enumerate(after) if is_goal(c) and runner(c[2]) == want), None)
            goal = next((i for i, c in enumerate(after) if is_goal(c)), None)
            misfire = any(c[1] == "Bash" and runner(c[2]) == want and not executed(c[3]) and ENV.search(c[3]) for c in after[:H])
            useful = tstar is not None or (want in verified)
            harmful = (misfire and tstar is None) or not useful
            end = min(goal if goal is not None else H, H)
            named = bool(re.search(r"\S+\.(py|ts|js|go|tsx)\b", cmd))
            w = 0
            for k, c in enumerate(after[:end]):
                if friction(c, named) and (k0 + 1 + k) not in credited:
                    w += 1
                    if useful:
                        credited.add(k0 + 1 + k)
            saved = w if useful and not harmful else 0
            harm_after = any(re.search(r"pip3?\s+install|-m\s+pip\s+install", c[2]) and ".venv" not in c[2] for c in after[:end + 1] if c[1] == "Bash")
            win = tstar is not None and after[tstar][0] - d["turn"] <= 3
            rows.append({"session": s["id"], "task": task, "route": d["route"], "command": cmd, "useful": useful, "window": win,
                         "harmful": harmful, "saved": saved, "sat_now": tstar == 0 or (tstar is not None and tstar < 3),
                         "net": follow * saved - cost, "pip_after": harm_after})
    keys = covered = 0
    for s in ev["sessions"]:
        cs = traces[s["id"]]
        first = min((d["turn"] for d in s["directives"]), default=None)
        for c in cs:
            if c[1] == "Bash" and runner(c[2]) is not None and not executed(c[3]) and ENV.search(c[3]):
                keys += 1
                covered += int(first is not None and first < c[0])
    RECALL.append((covered, keys))
    return rows, len(ev["sessions"])


def wilson_lb(k, n, z=1.2816):
    if n == 0:
        return 0.0
    p = k / n
    return (p + z * z / (2 * n) - z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n))) / (1 + z * z / n)


def summary(rows, n_sessions, seed=7, B=2000):
    by_s = collections.defaultdict(float)
    task_of = {}
    for r in rows:
        by_s[r["session"]] += r["net"]
        task_of[r["session"]] = r["task"]
    affected = list(by_s)
    per_task = collections.defaultdict(list)
    for sid in affected:
        per_task[task_of[sid]].append(by_s[sid])
    tasks = list(per_task)
    mean_aff = sum(by_s.values()) / len(affected) if affected else 0.0
    rnd = random.Random(seed)
    boots = []
    for _ in range(B if tasks else 0):
        smp = [rnd.choice(tasks) for _ in tasks]
        vals = [v for t in smp for v in per_task[t]]
        boots.append(sum(vals) / len(vals))
    boots.sort()
    lo = boots[int(0.05 * len(boots))] if boots else 0.0
    n = len(rows)
    useful = sum(r["useful"] for r in rows)
    harmful = sum(r["harmful"] for r in rows)
    return {"directives": n, "affected_sessions": len(affected), "sessions": n_sessions,
            "S_per_affected": round(mean_aff, 3), "S_per_affected_lb90": round(lo, 3),
            "S_per_session": round(sum(by_s.values()) / n_sessions, 3) if n_sessions else 0.0,
            "useful": useful, "useful_lb90": round(wilson_lb(useful, n), 3), "harmful": harmful,
            "followed_at_window": sum(r["window"] for r in rows),
            "dead_end_recall": "%d/%d" % (sum(c for c, _ in RECALL), sum(k for _, k in RECALL)),
            "harmful_share": round(harmful / n, 3) if n else 0.0,
            "pip_after_directive": sum(r["pip_after"] for r in rows)}


def main(argv):
    follow = float(argv[argv.index("--follow") + 1]) if "--follow" in argv else 0.8
    cost = float(argv[argv.index("--cost") + 1]) if "--cost" in argv else 0.1
    routes = re.compile(argv[argv.index("--routes") + 1]) if "--routes" in argv else None
    pairs = [(argv[i + 1], argv[i + 2]) for i, a in enumerate(argv) if a == "--pair"]
    if not pairs:
        pairs = [(argv[1], argv[2])]
    all_rows, n_all = [], 0
    for ds, evp in pairs:
        ev = json.load(open(evp))
        if "--v2" in argv:
            runs = {x["id"]: x["run"] for x in json.load(open(f"{ds}/manifest.json"))["sessions"]}
            ev["sessions"] = [x for x in ev["sessions"] if runs.get(x["id"], "").startswith("v2-")]
        if routes:
            for x in ev["sessions"]:
                x["directives"] = [d for d in x["directives"] if routes.search(d["route"])]
        rows, n = score(ds, ev, follow, cost)
        for r in rows:
            r["task"] = f"{ds}:{r['task']}"
        all_rows += rows
        n_all += n
    rows, n = all_rows, n_all
    if rows:
        print("  per directive: saved %.2f (net %.2f), useful %d/%d" % (
            sum(r["saved"] for r in rows) / len(rows), sum(r["net"] for r in rows) / len(rows),
            sum(r["useful"] for r in rows), len(rows)))
    by_route = collections.defaultdict(list)
    for r in rows:
        by_route[(r["route"], r["command"])].append(r)
    if "--quiet" not in argv:
        for (rid, cmd), rs in sorted(by_route.items()):
            s = summary(rs, n)
            print(f"  {rid:18s} {cmd[:48]:48s} n={s['directives']:3d} useful={s['useful']} harmful={s['harmful']} S/aff={s['S_per_affected']}")
    print(json.dumps(summary(rows, n)))


if __name__ == "__main__":
    main(sys.argv)
