#!/usr/bin/env python3
"""Score a work item: `make ci` must pass AND the hidden test must run and pass.

Usage: score.py <sandbox_dir> <item_id>
"""

import json
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path


def score(sandbox_dir, item_id):
    sandbox = Path(sandbox_dir)
    hidden = Path(__file__).resolve().parent / "hidden" / f"test_{item_id}.py"
    if not hidden.exists():
        return {"id": item_id, "passed": False, "error": f"hidden test not found: {hidden}"}

    # unittest discovery silently skips files that are not valid module names ("test_W-2042"),
    # so the hidden test is copied under an importable name.
    module = f"test_hidden_{item_id.replace('-', '_')}"
    dest = sandbox / "tests" / f"{module}.py"
    shutil.copy(hidden, dest)
    # No bytecode cache: a same-size edit within one second of a previous run would reuse stale .pyc.
    env = dict(os.environ, ACME_ENV="test", PYTHONDONTWRITEBYTECODE="1")
    for pyc in sandbox.rglob("__pycache__"):
        shutil.rmtree(pyc, ignore_errors=True)
    try:
        ci = subprocess.run(["make", "ci"], cwd=sandbox, capture_output=True, text=True, env=env)
        explicit = subprocess.run([sys.executable, "-m", "unittest", "-v", f"tests.{module}"],
                                  cwd=sandbox, capture_output=True, text=True, env=env)
    finally:
        dest.unlink(missing_ok=True)

    ci_out = ci.stdout + ci.stderr
    hid_out = explicit.stdout + explicit.stderr
    ran = re.search(r"Ran (\d+) tests?", hid_out)
    hidden_ran = int(ran.group(1)) if ran else 0
    return {
        "id": item_id,
        "passed": ci.returncode == 0 and explicit.returncode == 0 and hidden_ran > 0,
        "ci_passed": ci.returncode == 0,
        "hidden_passed": explicit.returncode == 0 and hidden_ran > 0,
        "hidden_tests_run": hidden_ran,
        "stale_generated": "generated models are stale" in ci_out,
        "ci_tail": ci_out[-1500:],
        "hidden_tail": hid_out[-1500:],
    }


if __name__ == "__main__":
    if len(sys.argv) != 3:
        sys.exit("usage: score.py <sandbox_dir> <item_id>")
    print(json.dumps(score(sys.argv[1], sys.argv[2]), indent=2))
