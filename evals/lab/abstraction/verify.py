"""Execute compiled directives in rebuilt sandboxes (offline) to check they are real commands.

For each distinct (task, compiled directive) in results/compile_points.jsonl, the task's base
commit is exported from the local mirror, its environment is rebuilt from the local caches with
the repository's own setup steps (evals/repobench/repos.json; `--offline`, nothing downloaded), and
the directive is run in a verification form that selects without judging the code:

    python  <directive> --collect-only -q     ok = exit 0 and "N tests collected"
    node    <directive> --passWithNoTests      ok = vitest reported "Test Files" (failures are
                                               expected: the bug is not fixed at base)
    go      go test -count=1 -run '^$' <pkgs>  ok = the packages build and the runner starts

    python3 verify.py [--per-repo N] [--repos click,attrs]
"""
from __future__ import annotations

import argparse
import collections
import json
import os
import re
import shutil
import subprocess
import sys

import facts as F
import traces as T

HERE = os.path.dirname(os.path.abspath(__file__))
WORK = os.environ.get("VERIFY_WORK", "/tmp/claude-0/-home-user-adage/d6ae021f-492b-5125-b4ff-8e8a0676a66b/scratchpad/abstraction-verify")
CACHE = "/tmp/adage-repobench"


def sh(cmd, cwd, timeout=600, env=None):
    e = dict(os.environ)
    e.update(env or {})
    try:
        p = subprocess.run(cmd, cwd=cwd, shell=True, capture_output=True, text=True, timeout=timeout, env=e)
        return p.returncode, p.stdout + p.stderr
    except subprocess.TimeoutExpired as x:
        return 124, str(x)


def setup_steps(repo):
    for r in json.load(open(F.REPOS_JSON))["repos"]:
        if r["id"] == repo:
            return [s.replace("{cache}", CACHE) for s in r["setup"]]
    return []


def prepare(repo, base):
    d = os.path.join(WORK, repo, base[:12])
    ok_flag = os.path.join(d, ".adage-verify-ok")
    if os.path.exists(ok_flag):
        return d, True, ""
    shutil.rmtree(d, ignore_errors=True)
    os.makedirs(d)
    g = F.MIRRORS / F.MIRROR[repo]
    rc, out = sh(f"git -C {g} archive {base} | tar -x -C {d}", d)
    if rc:
        return d, False, out
    for step in setup_steps(repo):
        step = step.replace("uv venv", "uv venv --offline").replace("uv pip install", "uv pip install --offline")
        step = step.replace("--prefer-offline", "--offline")
        env = {"GOFLAGS": "-mod=mod", "GOPROXY": "off"} if repo == "urfave-cli" else {}
        rc, out = sh(step, d, env=env)
        if rc:
            return d, False, f"{step}: {out[-800:]}"
    open(ok_flag, "w").close()
    return d, True, ""


def check(lang, argv, d):
    if lang == "python":
        rc, out = sh(argv + " --collect-only -q -p no:cacheprovider", d, timeout=180)
        n = re.search(r"(\d+) tests? collected|collected (\d+) item", out)
        return rc == 0 and bool(n), out[-300:]
    if lang == "node":
        rc, out = sh(argv + " --passWithNoTests --reporter=dot", d, timeout=300)
        return "Test Files" in out, out[-300:]
    if lang == "go":
        pk = argv.split()[2:]
        rc, out = sh("go test -count=1 -run '^$' " + " ".join(pk), d, timeout=300, env={"GOFLAGS": "-mod=mod", "GOPROXY": "off"})
        return rc == 0, out[-300:]
    return False, "?"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--per-repo", type=int, default=40)
    ap.add_argument("--repos", default="click,attrs,marshmallow,urfave-cli,vue-core")
    a = ap.parse_args()
    rows = [json.loads(l) for l in open(os.path.join(HERE, "results", "compile_points.jsonl"))]
    tasks = {}
    for r in rows:
        if not r.get("compiled"):
            continue
        run = r["sid"].split("/")[-1]
        m = re.match(r"(?:train|eval)-(.+?-[0-9a-f]{8})-r\d+", run)
        tasks.setdefault((r["repo"], m.group(1), r["compiled"]), 0)
        tasks[(r["repo"], m.group(1), r["compiled"])] += 1
    by_repo = collections.defaultdict(list)
    for (repo, task, comp), n in sorted(tasks.items()):
        by_repo[repo].append((task, comp, n))
    res = {}
    for repo in a.repos.split(","):
        items = by_repo.get(repo, [])
        # deterministic sample: spread over tasks, most frequent directives first
        items.sort(key=lambda x: (-x[2], x[0], x[1]))
        per_task = collections.defaultdict(list)
        for it in items:
            per_task[it[0]].append(it)
        sample = []
        while len(sample) < a.per_repo and any(per_task.values()):
            for t in sorted(per_task):
                if per_task[t] and len(sample) < a.per_repo:
                    sample.append(per_task[t].pop(0))
        tmeta = T.tasks(repo)
        ok = total = 0
        fails = []
        for task, comp, n in sample:
            base = tmeta[task]["base"]
            d, good, err = prepare(repo, base)
            total += 1
            if not good:
                fails.append((task, comp, "setup: " + err[-300:]))
                continue
            lang = F.scan(repo, d, base).lang
            passed, tail = check(lang, comp, d)
            ok += passed
            if not passed:
                fails.append((task, comp, tail))
            print(repo, task, "OK " if passed else "BAD", comp, file=sys.stderr)
        res[repo] = {"checked": total, "ok": ok, "rate": ok / max(1, total), "fails": fails[:5]}
    with open(os.path.join(HERE, "results", "verify.json"), "w") as f:
        json.dump(res, f, indent=1)
    print(json.dumps({k: {kk: vv for kk, vv in v.items() if kk != "fails"} for k, v in res.items()}, indent=1))


if __name__ == "__main__":
    main()
