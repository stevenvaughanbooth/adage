#!/usr/bin/env python3
"""Fixture-only hook. It never accepts executable commands from its payload."""
import fcntl
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import time


class Cancelled(Exception):
    pass


def monotonic_ns():
    # Python <3.10 on macOS rebases time.monotonic per process. Use the shared OS clock.
    return time.clock_gettime_ns(time.CLOCK_MONOTONIC)


def log(root, kind, **fields):
    record = dict(kind=kind, monotonic_ns=monotonic_ns(), pid=os.getpid(), **fields)
    with (root / "events.jsonl").open("a") as out:
        fcntl.flock(out, fcntl.LOCK_EX)
        out.write(json.dumps(record, sort_keys=True) + "\n")
        out.flush()


def run(root, payload, delay=0):
    root = root.resolve(strict=True)
    if not (root / ".reflex-fixture").is_file():
        raise ValueError("not a prepared fixture directory")
    event = payload.get("hook_event_name", "unknown")
    calls = sorted(c.get("tool_use_id", "") for c in payload.get("tool_calls", []))
    identity = {key: payload.get(key) for key in ("session_id", "turn_id", "tool_use_id")}
    identity["fixture_batch_key"] = hashlib.sha256(json.dumps(calls).encode()).hexdigest() if calls else None
    log(root, "hook_enter", event=event, **identity)
    if event not in ("UserPromptSubmit", "PostToolBatch"):
        log(root, "hook_exit", event=event, action="observe_only", **identity)
        return {}
    key = hashlib.sha256(json.dumps([event, identity, payload.get("prompt")], sort_keys=True).encode()).hexdigest()
    state_path = root / (key + ".receipt.json")
    with (root / (key + ".lock")).open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        if state_path.exists():
            receipt = json.loads(state_path.read_text())
            log(root, "duplicate_suppressed", event=event, receipt_key=key, previous_state=receipt["state"])
        else:
            receipt = dict(state="running", fixture=True, receipt_key=key, event=event, **identity)
            state_path.write_text(json.dumps(receipt))
            child = None
            def cancel(_signum, _frame):
                # InterruptedError may be swallowed/retried by subprocess/select internals.
                raise Cancelled("fixture hook cancelled")
            previous = signal.signal(signal.SIGTERM, cancel)
            try:
                child = subprocess.Popen([sys.executable, str(Path(__file__).resolve()), "--action", str(root), str(delay)],
                                         stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, start_new_session=True)
                log(root, "script_spawn", event=event, child_pid=child.pid, receipt_key=key)
                stdout, stderr = child.communicate(timeout=max(2, delay + 1))
                receipt.update(state="succeeded" if child.returncode == 0 else "failed", exit_code=child.returncode,
                               stdout=stdout, stderr=stderr, child_pid=child.pid)
            except (Cancelled, KeyboardInterrupt, subprocess.TimeoutExpired) as error:
                if child is not None:
                    try:
                        os.killpg(child.pid, signal.SIGKILL)
                    except ProcessLookupError:
                        pass
                    child.communicate()
                receipt.update(state="unknown", error=type(error).__name__)
            finally:
                signal.signal(signal.SIGTERM, previous)
                state_path.write_text(json.dumps(receipt, sort_keys=True))
                log(root, "receipt", **receipt)
        log(root, "hook_exit", event=event, receipt_key=key, state=receipt["state"])
        return {"hookSpecificOutput": {"hookEventName": event, "additionalContext":
                "Adage substrate fixture receipt (executed by the hook, not by you): " + json.dumps(receipt, sort_keys=True)}}


if __name__ == "__main__":
    if sys.argv[1] == "--action":
        root = Path(sys.argv[2]).resolve(strict=True)
        if not (root / ".reflex-fixture").is_file():
            raise SystemExit("not a fixture")
        log(root, "script_start")
        time.sleep(float(sys.argv[3]))
        with (root / "effects.txt").open("a") as out:
            out.write("fixture-effect\n")
        log(root, "script_end")
        print("REFLEX_FIXTURE_OK")
    else:
        print(json.dumps(run(Path(sys.argv[1]), json.load(sys.stdin), float(sys.argv[2]) if len(sys.argv) > 2 else 0)))
