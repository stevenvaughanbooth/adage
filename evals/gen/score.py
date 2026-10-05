#!/usr/bin/env python3
"""Score one task in a finished generated sandbox. Python 3 standard library only.

  python3 score.py <sandbox_dir> <task>                        (inside a bundle: uses the bundle)
  python3 score.py --bundle DIR <sandbox_dir> <task>
  python3 score.py --spec-json '{...}' --seed N <sandbox_dir> <task>
                                     (evals/gen/score.py: regenerates the bundle in a temporary
                                      directory first; what the live runner's `score` runs)

Prints JSON:
  passed            the task succeeded: all four below hold
  ci_passed         the repository's own CI command passed with the hidden test in place
  ci_ran_hidden     the CI output shows the hidden test ran (a CI that silently skipped it fails)
  hidden_passed     the hidden test, run explicitly with the scorer's own setup, ran at least as many
                    tests as it declares, and all passed
  stale_generated   generated code differs from what the scorer's pristine generator makes from
                    the sandbox's inputs (a skipped regenerate step, or a hand edit)
plus hidden_tests_run / hidden_tests_expected and the tails of each output.

The hidden test lives outside the sandbox, is copied in only while scoring, and removed afterwards.
How to run the CI, the hidden test and the staleness check comes from the bundle's scoring.json.
"""

import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent


def env(scrub=()):
    e = dict(os.environ, NO_COLOR="1", FORCE_COLOR="0", PYTHONDONTWRITEBYTECODE="1",
             npm_config_update_notifier="false", npm_config_fund="false", npm_config_audit="false",
             npm_config_loglevel="warn")
    for k in ("NODE_OPTIONS", "NODE_TEST_CONTEXT", *scrub):
        e.pop(k, None)
    return e


def run(argv, cwd, extra_env, timeout, scrub=()):
    try:
        p = subprocess.run(argv, cwd=cwd, capture_output=True, text=True, env={**env(scrub), **extra_env},
                           timeout=timeout)
        return p.returncode, p.stdout + p.stderr
    except subprocess.TimeoutExpired as e:
        out = e.stdout.decode() if isinstance(e.stdout, bytes) else (e.stdout or "")
        return 124, out + f"\n[score.py: timed out after {timeout}s]"
    except OSError as e:
        return 127, f"[score.py: {e}]"


def counts(fmt, out):
    """(tests run, failures) from a unittest or TAP report."""
    if fmt == "unittest":
        m = re.findall(r"^Ran (\d+) tests?", out, re.M)
        ran = int(m[-1]) if m else 0
        f = re.search(r"^FAILED \((.*)\)$", out, re.M)
        fails = sum(int(x) for x in re.findall(r"\d+", f.group(1))) if f else 0
        return ran, fails
    last = lambda key: int((re.findall(rf"^# {key} (\d+)$", out, re.M) or ["0"])[-1])  # noqa: E731
    return last("tests"), last("fail")


def score(bundle, sandbox, task):
    bundle, sandbox = Path(bundle).resolve(), Path(sandbox).resolve()
    sc = json.loads((bundle / "scoring.json").read_text())
    tasks = {t["id"]: t for t in json.loads((bundle / "tasks.json").read_text())["tasks"]}
    if task not in tasks:
        return {"id": task, "passed": False, "error": f"unknown task {task!r}"}
    t = tasks[task]
    hidden = bundle / t["hidden"]
    slug = task.lower().replace("-", "_")
    kv = {"bundle": str(bundle), "sandbox": str(sandbox), "python": sys.executable,
          "node": shutil.which("node") or "node", "task": task, "slug": slug}
    dest = sandbox / sc["hidden_dest"].format(**kv)
    kv["dest"] = str(dest)
    fill = lambda argv: [a.format(**kv) for a in argv]  # noqa: E731
    scrub = sc.get("scrub_env", [])
    for d in sandbox.rglob("__pycache__"):
        shutil.rmtree(d, ignore_errors=True)
    dest.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy(hidden, dest)
    try:
        ci_rc, ci_out = run(fill(sc["ci"]), sandbox, {}, 300, scrub)
        hid_rc, hid_out = run(fill(sc["explicit"]), sandbox, sc.get("env", {}), 120, scrub)
    finally:
        dest.unlink(missing_ok=True)
    stale, stale_out = False, ""
    if sc.get("stale"):
        rc, stale_out = run(fill(sc["stale"]), sandbox, {}, 60, scrub)
        stale = rc != 0 and sc["stale_marker"] in stale_out
    expected = t.get("hidden_tests", 1)
    ran, fails = counts(sc["format"], hid_out)
    hidden_passed = hid_rc == 0 and ran >= expected > 0 and fails == 0
    ci_passed = ci_rc == 0
    ci_ran_hidden = sc["ran_marker"].format(**kv) in ci_out
    return {
        "id": task,
        "passed": ci_passed and ci_ran_hidden and hidden_passed and not stale,
        "ci_passed": ci_passed,
        "ci_ran_hidden": ci_ran_hidden,
        "hidden_passed": hidden_passed,
        "hidden_tests_run": ran,
        "hidden_tests_expected": expected,
        "hidden_failures": fails,
        "stale_generated": stale,
        "ci_tail": ci_out[-1500:],
        "hidden_tail": hid_out[-1500:],
        "stale_check": stale_out.strip()[-500:],
    }


def main(argv):
    args = list(argv)

    def take(name):
        if name not in args:
            return None
        i = args.index(name)
        v = args[i + 1]
        del args[i:i + 2]
        return v

    bundle, spec_json, seed = take("--bundle"), take("--spec-json"), take("--seed")
    if len(args) != 2:
        print(__doc__, file=sys.stderr)
        return 2
    sandbox, task = args
    if spec_json:
        if seed is None:
            print("score.py: --spec-json needs --seed", file=sys.stderr)
            return 2
        sys.path.insert(0, str(HERE))
        import gen  # noqa: E402  (only in evals/gen, never inside a bundle)
        with tempfile.TemporaryDirectory(prefix="adage-gen-score-") as tmp:
            b = gen.build(json.loads(spec_json), int(seed), Path(tmp) / "bundle", repo=False)
            result = score(b, sandbox, task)
    else:
        result = score(Path(bundle) if bundle else HERE, sandbox, task)
    print(json.dumps(result, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
