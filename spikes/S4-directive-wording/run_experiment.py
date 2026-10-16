#!/usr/bin/env python3
"""
S4 directive-wording spike: run harness.

Runs `claude -p` headlessly against the sandbox repo, one arm x task x repeat at a time,
loading a per-run --settings file that wires up hooks/directive_hook.py (or no hooks, for
the no-directive baseline). Every run is logged to results/run_log.csv with a running count
against the approved budget, and the harness stops immediately (no retry loop) if a run
reports a usage-limit or auth error.

Usage:
    python3 run_experiment.py [--dry-run] [--budget 70] [--max-turns 12]

--dry-run prints the commands and settings it *would* run, resets the sandbox, and writes
a zero-row run_log.csv header, without ever invoking `claude`. Use it to sanity-check the
matrix and flag construction without spending any budget.
"""
import argparse
import csv
import json
import os
import subprocess
import sys
import time
import uuid

HERE = os.path.dirname(os.path.abspath(__file__))
SANDBOX = os.path.join(HERE, "sandbox")
HOOK_SCRIPT = os.path.join(HERE, "hooks", "directive_hook.py")
RESULTS = os.path.join(HERE, "results")
TRANSCRIPTS = os.path.join(RESULTS, "transcripts")
RUN_SETTINGS_DIR = os.path.join(RESULTS, "run-settings")
RUN_LOG = os.path.join(RESULTS, "run_log.csv")

ARMS = ["A", "B", "C"]  # imperative / factual / factual+nonce
TASKS = ["T1", "T2", "T3", "T4"]
MAIN_REPEATS = 5  # 3 arms x 4 tasks x 5 = 60
BASELINE_REPEATS = 2  # no-directive baseline, cheap: 4 tasks x 2 = 8

TASK_PROMPTS = {
    "T1": "Start work on W-4821.",
    "T2": "The auth test suite is red. Please fix it.",
    "T3": (
        "Add an `email` field (string) to the User message in schema/service.proto, "
        "and make sure everything stays consistent before you finish."
    ),
    # Wrong-directive task: unrelated to the work-item route the hook injects. A correct
    # agent ignores the injected directive and does what was actually asked.
    "T4": "This repo has no LICENSE file. Add an MIT license file at the repo root, dated 2026, copyright S4 Spike.",
}

# Scoped so the headless agent can only run the sandbox's harmless, hand-written scripts,
# plus read/edit files inside the sandbox (Claude Code already confines Edit/Write to the
# session's cwd and --add-dir list).
ALLOWED_TOOLS = [
    "Read",
    "Edit",
    "Write",
    "Glob",
    "Grep",
    "Bash(./scripts/wi-lookup:*)",
    "Bash(make gen)",
    "Bash(make test-auth)",
    "Bash(npm test)",
    "Bash(python3 tests/test_auth.py)",
    "Bash(git status)",
    "Bash(git diff:*)",
    "Bash(cat:*)",
    "Bash(ls:*)",
]

USAGE_LIMIT_MARKERS = [
    "usage limit reached",
    "exceeded your usage limit",
    "you've hit your usage limit",
    "you have exceeded your usage limit",
    "rate_limit_error",
    "quota exceeded",
    "please upgrade your plan",
]
AUTH_ERROR_MARKERS = [
    "failed to authenticate",
    "oauth session expired",
    "not logged in",
    "invalid api key",
    "please run /login",
]


def build_settings(arm, task):
    """Write a per-run settings file wiring hooks/directive_hook.py with ADAGE_ARM/ADAGE_TASK
    baked directly into the command string (not relied on via inherited env), and return its
    path. Baseline (arm is None) gets an empty hooks block: no directive at all."""
    os.makedirs(RUN_SETTINGS_DIR, exist_ok=True)
    if arm is None:
        settings = {"hooks": {}}
        path = os.path.join(RUN_SETTINGS_DIR, f"baseline-{task}.json")
    else:
        cmd = f"ADAGE_ARM={arm} ADAGE_TASK={task} python3 {HOOK_SCRIPT}"
        settings = {
            "hooks": {
                "SessionStart": [{"hooks": [{"type": "command", "command": cmd}]}],
                "UserPromptSubmit": [{"hooks": [{"type": "command", "command": cmd}]}],
                "PostToolUse": [
                    {"matcher": "*", "hooks": [{"type": "command", "command": cmd}]}
                ],
                "Stop": [{"hooks": [{"type": "command", "command": cmd}]}],
            }
        }
        path = os.path.join(RUN_SETTINGS_DIR, f"arm-{arm}-{task}.json")
    with open(path, "w") as f:
        json.dump(settings, f, indent=2)
    return path


def reset_sandbox():
    if not os.path.isdir(os.path.join(SANDBOX, ".git")):
        raise SystemExit(f"Sandbox missing at {SANDBOX} — run ./generate_sandbox.sh first.")
    subprocess.run(["git", "checkout", "--", "."], cwd=SANDBOX, check=True,
                    stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    subprocess.run(["git", "clean", "-fdx", "-q"], cwd=SANDBOX, check=True)


def last_result_event(stdout_text):
    """The stream-json `result` event carries the authoritative outcome (subtype, is_error,
    errors[]) — parse it instead of grepping raw text, which false-positives on things like a
    UUID that happens to contain "429"."""
    result = None
    for line in (stdout_text or "").splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            obj = json.loads(line)
        except json.JSONDecodeError:
            continue
        if obj.get("type") == "result":
            result = obj
    return result


def classify_status(returncode, stdout_text, stderr_text):
    """Returns one of: ok, max_turns_reached, usage_limit, auth_error, error_<subtype>,
    no_result_event. Only usage_limit / auth_error should ever stop the harness loop —
    everything else (including max_turns_reached) is a legitimate, countable data point."""
    result = last_result_event(stdout_text)
    combined_low = ((stdout_text or "") + "\n" + (stderr_text or "")).lower()

    if result is not None:
        subtype = result.get("subtype") or ""
        result_text = (result.get("result") or "").lower()
        errors_text = " ".join(result.get("errors") or []).lower()
        blob = " ".join([subtype.lower(), result_text, errors_text, combined_low])

        if "authenticate" in result_text or "oauth" in result_text or any(
            m in blob for m in AUTH_ERROR_MARKERS
        ):
            return "auth_error"
        if any(m in blob for m in USAGE_LIMIT_MARKERS):
            return "usage_limit"
        if subtype == "error_max_turns":
            return "max_turns_reached"
        if result.get("is_error"):
            return f"error_{subtype or 'unknown'}"
        return "ok"

    # No result event at all — the CLI itself failed before/without producing one (bad flags,
    # crash, or an auth failure so early no result event was emitted).
    for m in AUTH_ERROR_MARKERS:
        if m in combined_low:
            return "auth_error"
    for m in USAGE_LIMIT_MARKERS:
        if m in combined_low:
            return "usage_limit"
    return "no_result_event"


def ensure_log_header():
    os.makedirs(RESULTS, exist_ok=True)
    if not os.path.exists(RUN_LOG):
        with open(RUN_LOG, "w", newline="") as f:
            w = csv.writer(f)
            w.writerow([
                "run_id", "seq", "arm", "task", "repeat", "started_at", "ended_at",
                "status", "exit_code", "model", "max_turns", "transcript_path", "notes",
            ])


def count_prior_runs():
    if not os.path.exists(RUN_LOG):
        return 0
    with open(RUN_LOG) as f:
        return max(0, sum(1 for _ in f) - 1)


def count_budget_spent():
    """Rows that actually invoked `claude` (i.e. not dry-run rows) — this is what counts
    against the ~70-run budget, regardless of outcome (a usage_limit or errored attempt
    still spent a call)."""
    if not os.path.exists(RUN_LOG):
        return 0
    n = 0
    with open(RUN_LOG) as f:
        for row in csv.DictReader(f):
            if row.get("status") != "dry_run":
                n += 1
    return n


RETRIABLE_STATUSES = {"usage_limit", "auth_error", "dry_run"}


def completed_combos():
    """(arm, task, repeat) triples that already produced a usable result — including a
    max-turns cutoff or a model-side error, which are legitimate data points, not harness
    failures — so a resumed invocation doesn't re-spend budget on work already done. Only
    usage_limit/auth_error (blocked before any usable result) and dry_run are retried."""
    done = set()
    if not os.path.exists(RUN_LOG):
        return done
    with open(RUN_LOG) as f:
        for row in csv.DictReader(f):
            if row.get("status") not in RETRIABLE_STATUSES:
                arm = row["arm"] if row["arm"] != "" else None
                done.add((arm, row["task"], int(row["repeat"])))
    return done


def append_log(row):
    with open(RUN_LOG, "a", newline="") as f:
        csv.writer(f).writerow(row)


def run_one(arm, task, repeat, model, max_turns, dry_run, seq):
    run_id = f"{seq:03d}-{arm or 'BASE'}-{task}-r{repeat}"
    settings_path = build_settings(arm, task)
    prompt = TASK_PROMPTS[task]
    transcript_path = os.path.join(TRANSCRIPTS, f"{run_id}.stream.jsonl")
    stderr_path = os.path.join(TRANSCRIPTS, f"{run_id}.stderr.log")
    os.makedirs(TRANSCRIPTS, exist_ok=True)

    cmd = [
        "claude", "-p", prompt,
        "--model", model,
        "--settings", settings_path,
        "--allowedTools", *ALLOWED_TOOLS,
        "--max-turns", str(max_turns),
        "--output-format", "stream-json",
        "--verbose",
        "--permission-mode", "default",
        "--setting-sources", "",
        "--no-session-persistence",
    ]

    print(f"[{seq:03d}] {run_id}  arm={arm} task={task} repeat={repeat}")
    if dry_run:
        print("  (dry-run) settings:", settings_path)
        print("  (dry-run) cmd:", " ".join(repr(c) for c in cmd))
        append_log([run_id, seq, arm, task, repeat, "", "", "dry_run", "", model,
                    max_turns, transcript_path, "dry-run only, not executed"])
        return "dry_run"

    reset_sandbox()
    started = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
    try:
        proc = subprocess.run(
            cmd, cwd=SANDBOX, capture_output=True, text=True, timeout=600,
        )
    except subprocess.TimeoutExpired as e:
        ended = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
        with open(stderr_path, "w") as f:
            f.write(f"TIMEOUT after 600s\nstdout so far:\n{e.stdout or ''}\nstderr so far:\n{e.stderr or ''}")
        append_log([run_id, seq, arm, task, repeat, started, ended, "timeout", "", model,
                    max_turns, transcript_path, "600s timeout"])
        return "timeout"

    ended = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
    with open(transcript_path, "w") as f:
        f.write(proc.stdout)
    with open(stderr_path, "w") as f:
        f.write(proc.stderr)

    status = classify_status(proc.returncode, proc.stdout, proc.stderr)

    append_log([run_id, seq, arm, task, repeat, started, ended, status, proc.returncode,
                model, max_turns, transcript_path, ""])

    if status in ("usage_limit", "auth_error"):
        print(f"  !! {status} detected — stopping the harness per the safety rule "
              f"(no retry loop). See {stderr_path}")
    elif status == "max_turns_reached":
        print(f"  (agent hit --max-turns {max_turns} without finishing — recorded as data, not an error)")
    return status


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--dry-run", action="store_true")
    ap.add_argument("--budget", type=int, default=70)
    ap.add_argument("--max-turns", type=int, default=12)
    ap.add_argument("--model", default="claude-sonnet-5")
    args = ap.parse_args()

    ensure_log_header()

    # Canonical, stable matrix — index (seq) is the combo's fixed position, not a count of
    # log rows, so a resumed invocation numbers runs the same way every time.
    matrix = []
    for arm in ARMS:
        for task in TASKS:
            for r in range(1, MAIN_REPEATS + 1):
                matrix.append((arm, task, r))
    for task in TASKS:
        for r in range(1, BASELINE_REPEATS + 1):
            matrix.append((None, task, r))

    print(f"Canonical matrix: {len(matrix)} runs "
          f"({len(ARMS)} arms x {len(TASKS)} tasks x {MAIN_REPEATS} repeats "
          f"+ {len(TASKS)} tasks x {BASELINE_REPEATS} baseline repeats)")
    if len(matrix) > args.budget:
        raise SystemExit(f"Matrix ({len(matrix)}) exceeds budget ({args.budget}); trim repeats.")

    done = completed_combos()
    spent = count_budget_spent()
    if not args.dry_run and done:
        print(f"Resuming: {len(done)} combo(s) already completed, {spent} run(s) already "
              f"counted against the budget.")

    for seq, (arm, task, r) in enumerate(matrix, start=1):
        if (arm, task, r) in done:
            continue
        if not args.dry_run and spent >= args.budget:
            print(f"Budget of {args.budget} runs reached; stopping.")
            break
        status = run_one(arm, task, r, args.model, args.max_turns, args.dry_run, seq)
        if status != "dry_run":
            spent += 1
        if status in ("usage_limit", "auth_error"):
            print(f"Stopping after {spent} run(s) attempted due to '{status}'.")
            break

    print(f"Done. Total rows in {RUN_LOG}: {count_prior_runs()}. "
          f"Runs counted against budget so far: {count_budget_spent()}.")


if __name__ == "__main__":
    main()
