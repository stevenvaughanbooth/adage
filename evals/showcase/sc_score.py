#!/usr/bin/env python3
"""Score a showcase sandbox (the live runner's `score` command), outside the agent's view. No tokens.

  python3 sc_score.py <task-id> <dir> [--keep-env]

The task is done when the project's CI would accept it and the deliverables the prompt names exist:

  1. the family's source-level checks, on what the agent left (`checks(v, dir, changed)`; http-header:
     a documented `standard_headers!` entry + an own test; chi-middleware: Go doc comment, README row
     and link, an own test; the Python and urfave families check docs, changelog and own tests inside
     their hidden test instead);
  2. the family's pristine CI files restored (Click's light-imports test), the hidden test written,
     the repository's `rescue` steps rerun, and the family's CI command run: the whole suite with the
     hidden test (`tests` holds when it exits 0 and tests ran).

`passed` is all checks. Prints JSON (passed, checks, hidden_failed, suite_failed, n_tests,
changed_files, tail), then prunes the sandbox's environment and build output (disk), as RepoBench's
scorer does.
"""

from __future__ import annotations

import json
import os
import re
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
os.environ.setdefault("REPOBENCH_CACHE", "/tmp/adage-showcase/cache")
os.environ.setdefault("GOCACHE", "/tmp/adage-showcase/cache/go-build")
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent / "repobench"))
import common  # noqa: E402
import score as rb_score  # noqa: E402
from sandbox import run_steps  # noqa: E402

import families  # noqa: E402

TIMEOUT = {"rust": 900, "go": 300, "python": 300}


def failed_tests(lang: str, text: str, hidden: str) -> tuple[list[str], list[str]]:
    if lang == "go":
        failed = sorted(set(re.findall(r"--- FAIL: (\S+)", text)))
        hid = [x for x in failed if x.startswith(("TestRWHidden", "TestSCHidden"))]
    elif lang == "rust":
        failed = sorted(set(re.findall(r"^\s*(?:test )?(\S+) \.\.\. FAILED", text, re.M))
                        | set(re.findall(r"^---- (\S+) stdout ----", text, re.M)))
        hid = [x for x in failed if "showcase_hidden" in x]
    else:
        failed = sorted(set(re.findall(r"^(?:FAILED|ERROR) (\S+)", text, re.M)))
        hid = [x.split("::", 1)[-1] for x in failed if Path(hidden).name in x]
    return hid, [x for x in failed if x not in hid and Path(hidden).name not in x][:20]


def score(tid: str, d: Path) -> dict:
    f, v = families.parse_task(tid)
    r = common.repo(f["repo"])
    d = Path(d)
    changed = rb_score.changed_files(d, {f["hidden_path"]})
    checks = dict(f["checks"](v, d, changed)) if f.get("checks") else {}
    for p in f.get("pristine") or []:
        blob = subprocess.run(["git", "-C", str(common.mirror(r)), "show", f"{f['base']}:{p}"], capture_output=True,
                              check=True).stdout
        (d / p).write_bytes(blob)
    (d / f["hidden_path"]).parent.mkdir(parents=True, exist_ok=True)
    (d / f["hidden_path"]).write_text(f["hidden"](v))
    rescue_error = None
    try:
        run_steps(r.get("rescue", []), d)
    except Exception as e:  # noqa: BLE001 - a failed rescue is recorded; the tests still run
        rescue_error = str(e)[:300]
    t0 = time.time()
    lang = f["lang"]
    try:
        out = subprocess.run(["bash", "-c", f["command"]], cwd=d, capture_output=True, text=True,
                             timeout=TIMEOUT[lang], env=rb_score.common_env())
        code, text = out.returncode, out.stdout + "\n" + out.stderr
    except subprocess.TimeoutExpired:
        code, text = -9, "TIMEOUT"
    n = rb_score.count_tests(lang, text)
    checks["tests"] = code == 0 and bool(n)
    hid, suite = failed_tests(lang, text, f["hidden_path"])
    if code != 0 and not hid and not suite:
        suite = ["(build, collection or setup failure)"]
    return {"passed": all(checks.values()), "hidden_passed": checks["tests"] and not hid,
            "ci_passed": checks["tests"], "checks": checks, "exit_code": code, "n_tests": n,
            "elapsed_s": round(time.time() - t0, 2), "hidden_failed": hid, "suite_failed": suite,
            "changed_files": changed[:50], "rescue_error": rescue_error,
            "tail": "\n".join(text.strip().splitlines()[-15:])}


def main(argv=None) -> int:
    argv = sys.argv[1:] if argv is None else argv
    keep = "--keep-env" in argv
    argv = [a for a in argv if a != "--keep-env"]
    if len(argv) != 2:
        raise SystemExit("usage: sc_score.py <task-id> <dir> [--keep-env]")
    d = Path(argv[1])
    f, _ = families.parse_task(argv[0])
    print(json.dumps(score(argv[0], d)))
    if not keep:
        rb_score.prune(d, common.repo(f["repo"]).get("prune", ()))
    trim_go_cache()
    return 0


def trim_go_cache(limit_mb: int = 250) -> None:
    """The scorer's shared Go build cache grows with every scored checkout; keep the showcase's
    footprint bounded (it only costs a rebuild of the standard library's test deps)."""
    gc = Path(os.environ["GOCACHE"])
    if gc.is_dir() and sum(p.stat().st_size for p in gc.rglob("*") if p.is_file()) > limit_mb * 2**20:
        subprocess.run(["go", "clean", "-cache"], capture_output=True, env=dict(os.environ))


if __name__ == "__main__":
    sys.exit(main())
