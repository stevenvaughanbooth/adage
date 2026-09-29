#!/usr/bin/env python3
"""Score a recurring-workflow sandbox (the live runner's `score` command), outside the agent's view.

  python3 rw_score.py <task-id> <dir> [--keep-env]

Writes the variant's hidden check as tests/test_rw_hidden.py, restores the family's pristine CI
files (Click's tests/test_imports.py), reruns the repository's `rescue` steps, and runs the whole
test suite (the project's CI: the agent's own tests, the invariants and the hidden check). Prints
JSON: passed (exit 0 and tests ran), hidden_failed (which hidden checks failed), suite_failed (other
failing tests), changed_files. Then prunes the sandbox's .venv (disk), as RepoBench's scorer does.
"""

from __future__ import annotations

import json
import re
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parents[2] / "repobench"))
import common  # noqa: E402
import score as rb_score  # noqa: E402
from sandbox import run_steps  # noqa: E402

import families  # noqa: E402


def score(tid: str, d: Path) -> dict:
    f, v = families.parse_task(tid)
    r = common.repo(f["repo"])
    changed = rb_score.changed_files(d, {f["hidden_path"]})
    for p in f["pristine"]:
        blob = subprocess.run(["git", "-C", str(common.mirror(r)), "show", f"{f['base']}:{p}"], capture_output=True,
                              check=True).stdout
        (d / p).write_bytes(blob)
    (d / f["hidden_path"]).write_text(f["hidden"](v))
    rescue_error = None
    try:
        run_steps(r.get("rescue", []), d)
    except Exception as e:  # noqa: BLE001
        rescue_error = str(e)[:300]
    t0 = time.time()
    try:
        out = subprocess.run(["bash", "-c", f["command"]], cwd=d, capture_output=True, text=True, timeout=240,
                             env=rb_score.common_env())
        code, text = out.returncode, out.stdout + "\n" + out.stderr
    except subprocess.TimeoutExpired:
        code, text = -9, "TIMEOUT"
    lang = f.get("lang", "python")
    n = rb_score.count_tests(lang, text)
    if lang == "go":  # `--- FAIL: TestX`; hidden tests are named TestRWHidden*; a build failure fails all
        failed = sorted(set(re.findall(r"--- FAIL: (\S+)", text)))
        failed = [(f["hidden_path"] + "::" + x) if x.startswith("TestRWHidden") else x for x in failed]
        if code != 0 and not failed:
            failed = ["(build or setup failure)"]
    else:
        failed = sorted(set(re.findall(r"^(?:FAILED|ERROR) (\S+)", text, re.M)))
    hidden = Path(f["hidden_path"]).name
    return {"passed": code == 0 and bool(n), "hidden_passed": code == 0 and bool(n), "ci_passed": code == 0 and bool(n),
            "exit_code": code, "n_tests": n, "elapsed_s": round(time.time() - t0, 2),
            "hidden_failed": [x.split("::", 1)[-1] for x in failed if hidden in x],
            "suite_failed": [x for x in failed if hidden not in x][:20],
            "changed_files": changed[:50], "rescue_error": rescue_error,
            "tail": "\n".join(text.strip().splitlines()[-15:])}


def main(argv=None) -> int:
    argv = sys.argv[1:] if argv is None else argv
    keep = "--keep-env" in argv
    argv = [a for a in argv if a != "--keep-env"]
    d = Path(argv[1])
    print(json.dumps(score(argv[0], d)))
    if not keep:
        rb_score.prune(d)
    return 0


if __name__ == "__main__":
    sys.exit(main())
