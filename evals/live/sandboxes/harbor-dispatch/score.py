#!/usr/bin/env python3
"""Score one incident in a finished harbor-dispatch sandbox.

  python3 score.py <sandbox_dir> <INC-id>

Prints JSON:
  passed            the task succeeded: all four below hold
  ci_passed         the repository's own CI (`npm run check`) passed with the hidden test in place
  ci_ran_hidden     the CI output shows the hidden suite ran (a CI that silently skipped it fails)
  hidden_passed     the hidden test, run explicitly with the scorer's own resolver, ran at least as
                    many tests as it declares and all of them passed
  stale_generated   src/generated/registry.ts differs from what the pristine generator makes from
                    the agent's registry/*.csv (a skipped `npm run codegen`, or a hand edit)
plus hidden_tests_run / hidden_tests_expected / hidden_failures and the tails of both outputs.

The hidden test lives outside the sandbox (hidden/<ID>.test.ts), is copied in only while scoring,
and is removed again afterwards. Staleness is checked with the template's generator, not the
sandbox's, so an edited generator cannot hide stale output.
"""

import json
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
HOOKS = HERE / "score_hooks.mjs"
PRISTINE_CODEGEN = HERE / "template" / "tools" / "codegen.mjs"
STALE_MARK = "is out of date"


def env():
    e = dict(os.environ, NO_COLOR="1", FORCE_COLOR="0", npm_config_update_notifier="false",
             npm_config_fund="false", npm_config_audit="false", npm_config_loglevel="warn")
    e.pop("NODE_OPTIONS", None)
    e.pop("NODE_TEST_CONTEXT", None)
    return e


def run(argv, cwd, timeout):
    try:
        p = subprocess.run(argv, cwd=cwd, capture_output=True, text=True, env=env(), timeout=timeout)
        return p.returncode, p.stdout + p.stderr
    except subprocess.TimeoutExpired as e:
        out = (e.stdout or b"").decode() if isinstance(e.stdout, bytes) else (e.stdout or "")
        return 124, out + f"\n[score.py: timed out after {timeout}s]"
    except OSError as e:
        return 127, f"[score.py: {e}]"


def tap_count(out, key):
    m = re.findall(rf"^# {key} (\d+)$", out, re.M)
    return int(m[-1]) if m else 0


def score(sandbox_dir, item_id):
    sandbox = Path(sandbox_dir).resolve()
    hidden = HERE / "hidden" / f"{item_id}.test.ts"
    if not hidden.exists():
        return {"id": item_id, "passed": False, "error": f"hidden test not found: {hidden}"}
    node = shutil.which("node")
    if not node:
        return {"id": item_id, "passed": False, "error": "`node` is not on PATH"}
    expected = len(re.findall(r"^\s*test\(", hidden.read_text(), re.M))
    suite = f"hidden {item_id}"
    dest = sandbox / "test" / f"hidden-{item_id}.test.ts"
    dest.parent.mkdir(exist_ok=True)
    shutil.copy(hidden, dest)
    try:
        ci_rc, ci_out = run(["npm", "run", "check"], sandbox, 300)
        hid_rc, hid_out = run([node, "--import", str(HOOKS), "--test", "--test-reporter=tap", str(dest)], sandbox, 120)
    finally:
        dest.unlink(missing_ok=True)
    gen_rc, gen_out = run([node, str(PRISTINE_CODEGEN), "--verify", "--root", str(sandbox)], sandbox, 60)

    tests_run, passes, fails = tap_count(hid_out, "tests"), tap_count(hid_out, "pass"), tap_count(hid_out, "fail")
    hidden_passed = hid_rc == 0 and expected > 0 and tests_run >= expected and passes == tests_run and fails == 0
    ci_passed = ci_rc == 0
    ci_ran_hidden = suite in ci_out
    stale = gen_rc != 0 and STALE_MARK in gen_out
    return {
        "id": item_id,
        "passed": ci_passed and ci_ran_hidden and hidden_passed and not stale,
        "ci_passed": ci_passed,
        "ci_ran_hidden": ci_ran_hidden,
        "hidden_passed": hidden_passed,
        "hidden_tests_run": tests_run,
        "hidden_tests_expected": expected,
        "hidden_failures": fails,
        "stale_generated": stale,
        "registry_error": gen_rc != 0 and not stale,
        "ci_tail": ci_out[-1500:],
        "hidden_tail": hid_out[-1500:],
        "codegen_check": gen_out.strip()[-500:],
    }


if __name__ == "__main__":
    if len(sys.argv) != 3:
        sys.exit("usage: score.py <sandbox_dir> <INC-id>")
    print(json.dumps(score(sys.argv[1], sys.argv[2]), indent=2))
