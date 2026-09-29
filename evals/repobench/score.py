#!/usr/bin/env python3
"""Score a finished RepoBench-Local sandbox against the task's hidden tests.

  python3 evals/repobench/score.py <repo-id> <task-id> <dir>

Runs outside the agent's view (the live runner calls it after the session). It writes the fix
commit's version of every test file the fix touched into the sandbox (overwriting whatever the
agent left there), reruns the repository's `rescue` steps (so an environment the agent broke does
not decide the verdict), runs the repository's `hidden` command on those files, and prints JSON:

  passed         the hidden command exited 0 and ran at least one test
  hidden_passed  the same (the runner reads both)
  ci_passed      the same: there is no separate CI step
  n_tests        tests the hidden command reported, when it says
  elapsed_s      how long the hidden run took
  touched_gold   the agent changed at least one file the real fix changed
  changed_files  files the agent changed, added or removed (git status), hidden tests excluded
  tail           the last lines of the hidden run's output
"""

from __future__ import annotations

import json
import re
import subprocess
import sys
import time
from pathlib import Path

import common
from sandbox import run_steps


def hidden_command(r: dict, t: dict) -> str:
    tests = [p for p in t["test_files"] if runnable(r["lang"], p)]
    pkgs = sorted({("./" + str(Path(p).parent)) if str(Path(p).parent) != "." else "./" for p in t["test_files"]
                   if p.endswith(".go")})
    names = " ".join(f"--test {Path(p).stem}" for p in tests if p.endswith(".rs"))
    return r["hidden"].format(tests=" ".join(tests), pkgs=" ".join(pkgs) or "./", testnames=names)


def runnable(lang: str, path: str) -> bool:
    """Is this hidden file a test module the runner takes as an argument (not a helper, fixture or
    snapshot)?"""
    name = path.rsplit("/", 1)[-1]
    if lang == "python":
        return name.endswith(".py") and (name.startswith("test_") or name.endswith("_test.py"))
    if lang in ("typescript", "javascript"):
        return bool(re.search(r"\.(spec|test)\.[cm]?[jt]sx?$", name))
    if lang == "rust":  # integration test targets (tests/<name>.rs); the hidden command takes {testnames}
        return name.endswith(".rs") and path.count("/") == 1 and path.startswith("tests/")
    if lang == "c":  # janet-style script suites run by the built binary
        return name.endswith(".janet") and name.startswith("suite")
    if lang == "make":  # the hidden command is a fixed `make`-driven suite; any hidden file feeds it
        return True
    return name.endswith("_test.go")


def count_tests(lang: str, out: str) -> int | None:
    """How many tests the runner reported (passed + failed), or None if it does not say."""
    if lang == "python":
        n = sum(int(m.group(1)) for m in re.finditer(r"(\d+) (passed|failed|error|errors)\b", out))
        return n
    if lang in ("typescript", "javascript"):
        m = re.search(r"Tests\s+(.*)", out)
        if m:
            return sum(int(x) for x in re.findall(r"(\d+) (?:passed|failed)", m.group(1)))
        m = re.search(r"# tests (\d+)", out)
        return int(m.group(1)) if m else None
    if lang == "go":
        return len(re.findall(r"^(ok|FAIL|---)\s", out, re.M)) or None
    if lang == "rust":
        return sum(int(a) + int(b) for a, b in re.findall(r"test result: \w+\. (\d+) passed; (\d+) failed", out))
    if lang in ("c", "make"):
        found = re.findall(r"(\d+) of (\d+) tests passed", out)
        return sum(int(b) for _, b in found) if found else None
    return None


def write_hidden(r: dict, t: dict, d: Path) -> None:
    m = common.mirror(r)
    for p in t["test_files"]:
        blob = subprocess.run(["git", "-C", str(m), "show", f"{t['sha']}:{p}"], capture_output=True)
        dest = d / p
        if blob.returncode != 0:  # deleted by the fix
            if dest.exists():
                dest.unlink()
            continue
        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.write_bytes(blob.stdout)


def changed_files(d: Path, exclude) -> list[str]:
    """Files the agent changed since the sandbox's base commit: committed (agents do commit),
    staged, unstaged or untracked."""
    q = ["git", "-C", str(d), "--no-optional-locks"]
    roots = subprocess.run(q + ["rev-list", "--max-parents=0", "HEAD"], capture_output=True, text=True).stdout.split()
    files = []
    if roots:
        files += subprocess.run(q + ["diff", "--name-only", roots[-1]], capture_output=True, text=True).stdout.split("\n")
    out = subprocess.run(q + ["status", "--porcelain", "-uall"], capture_output=True, text=True).stdout
    files += [line[3:].split(" -> ")[-1].strip().strip('"') for line in out.splitlines()]
    seen = []
    for p in files:
        p = p.strip()
        if (p and p not in exclude and p not in seen and not p.startswith((".venv/", "node_modules/"))
                and "__pycache__/" not in p and not p.endswith(".pyc")):
            seen.append(p)
    return seen


def run_hidden(r: dict, t: dict, d: Path) -> dict:
    cmd = hidden_command(r, t)
    t0 = time.time()
    try:
        out = subprocess.run(["bash", "-c", cmd], cwd=d, capture_output=True, text=True,
                             timeout=r.get("hidden_timeout", 90), env=common_env())
        code, text = out.returncode, (out.stdout + "\n" + out.stderr)
    except subprocess.TimeoutExpired as e:
        code, text = -9, f"TIMEOUT after {e.timeout}s"
    n = count_tests(r["lang"], text)
    ok = code == 0 and (n is None or n > 0)
    return {"passed": ok, "exit_code": code, "n_tests": n, "elapsed_s": round(time.time() - t0, 2),
            "command": cmd, "tail": "\n".join(text.strip().splitlines()[-25:])}


def common_env() -> dict:
    import os
    env = dict(os.environ)
    env.pop("CLAUDECODE", None)
    env["CI"] = "1"
    env.setdefault("GOFLAGS", "-mod=mod")
    return env


def score(rid: str, tid: str, d: Path) -> dict:
    r = common.repo(rid)
    t = common.task(rid, tid)
    d = Path(d)
    changed = changed_files(d, set(t["test_files"]))
    write_hidden(r, t, d)
    try:
        run_steps(r.get("rescue", []), d)
    except Exception as e:  # noqa: BLE001 - a failed rescue is recorded, the tests still run
        rescue_error = str(e)[:300]
    else:
        rescue_error = None
    res = run_hidden(r, t, d)
    gold = set(t.get("gold_files") or t["src_files"])
    res.update(hidden_passed=res["passed"], ci_passed=res["passed"], changed_files=changed[:50],
               touched_gold=bool(gold & set(changed)), rescue_error=rescue_error)
    return res


PRUNE_HOME = ("go", ".cache", ".npm", "tmp", ".local/share/pnpm", ".local/lib", ".cargo/registry", ".cargo/git")


def prune(d: Path, extra=()) -> None:
    """After scoring, drop what can be rebuilt and is large: the sandbox's environment (.venv,
    node_modules) and, for a runner run, the session home's package caches (a Go module cache is
    ~100 MB a session). The transcript (home/.claude), the stream and the source stay."""
    import shutil
    d = Path(d)
    shutil.rmtree(d / ".venv", ignore_errors=True)
    for rel in extra:  # the repository's build output (repos.json `prune`: a Cargo target/, a make build/)
        shutil.rmtree(d / rel, ignore_errors=True)
    for nm in sorted(d.glob("**/node_modules"), key=lambda p: len(p.parts)):
        if nm.exists():
            shutil.rmtree(nm, ignore_errors=True)
    home = d.parent / "home"
    if (home / ".claude").is_dir():
        for sub in PRUNE_HOME:
            p = home / sub
            if p.exists():
                subprocess.run(["chmod", "-R", "u+w", str(p)], capture_output=True)  # Go's cache is read-only
                shutil.rmtree(p, ignore_errors=True)


def main(argv=None) -> int:
    argv = sys.argv[1:] if argv is None else argv
    keep = "--keep-env" in argv
    argv = [a for a in argv if a != "--keep-env"]
    if len(argv) != 3:
        raise SystemExit("usage: score.py <repo-id> <task-id> <dir> [--keep-env]")
    print(json.dumps(score(*argv[:2], Path(argv[2]))))
    if not keep:
        prune(Path(argv[2]), common.repo(argv[0]).get("prune", ()))
    return 0


if __name__ == "__main__":
    sys.exit(main())
