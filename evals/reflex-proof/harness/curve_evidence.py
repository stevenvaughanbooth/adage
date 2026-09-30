"""Publish compact observed learning-curve evidence; never export broad harness debug logs."""
from collections import Counter
from datetime import datetime, timezone
import argparse
import json
from pathlib import Path
import re
import shlex

import real_repo_pairs as pairs
import curve_heldout


def pytest_invocation(command):
    """Descriptive invocation audit: never count installs, comments, or pipe exit codes as tests."""
    if any(c in command for c in "\n\r`$;|<>"):
        return False
    try:
        argv = shlex.split(command)
    except ValueError:
        return False
    if argv[:3] == ["source", ".venv/bin/activate", "&&"]:
        argv = argv[3:]
    if not argv or any("&" in a for a in argv):
        return False
    program = Path(argv[0]).name
    return program == "pytest" or (bool(re.fullmatch(r"python(?:[0-9]+(?:\.[0-9]+)*)?", program))
                                   and argv[1:3] == ["-m", "pytest"])


def training_row(root, task, ledger):
    session = root / "sessions" / task
    key = str(session.relative_to(Path(pairs.read(root / "manifest.json")["budget_authority"])))
    attempt = ledger.get("runs", {}).get(key, {})
    row = dict(task=task, status=attempt.get("status", "not-dispatched"),
               cost_usd=attempt.get("result", {}).get("total_cost_usd"),
               terminal_error=attempt.get("result", {}).get("result_is_error"),
               independent_quality="not-scored", successful_pytest_calls=0, pytest_calls=[],
               source_sha256={})
    native = session / "native-transcript.jsonl"
    if not native.is_file():
        row["successful_pytest_calls"] = None
        return row
    calls, results = {}, {}
    for line in native.read_text().splitlines():
        item = json.loads(line)
        message = item.get("message", {}) if isinstance(item, dict) else {}
        content = message.get("content", []) if isinstance(message, dict) else []
        for block in content if isinstance(content, list) else []:
            if block.get("type") == "tool_use":
                calls[block["id"]] = block
            elif block.get("type") == "tool_result":
                results[block["tool_use_id"]] = block
    row["tool_calls"] = len(calls)
    row["tools"] = dict(Counter(c["name"] for c in calls.values()))
    for identity, call in calls.items():
        command = call.get("input", {}).get("command", "")
        if call.get("name") != "Bash" or not pytest_invocation(command):
            continue
        result = results.get(identity)
        # This is a recognized invocation, not semantic verification or an execution grant.
        success = result is not None and result.get("is_error") is False
        row["pytest_calls"].append(dict(tool_id=identity, command=command, tool_reported_success=success,
            result_present=result is not None))
        row["successful_pytest_calls"] += success
    pattern = re.compile(r"^\S+ \[DEBUG\] \[API REQUEST\] /v1/messages x-client-request-id=([\w-]+) source=(\w+)$")
    debug = session / "claude-debug.log"
    requests = [dict(request_id=m[1], source=m[2]) for line in debug.read_text().splitlines()
                if (m := pattern.fullmatch(line))] if debug.is_file() else []
    row["observed_model_requests"] = len(requests) if requests else None
    row["request_diagnostics"] = requests
    for name in ("manifest.json", "native-transcript.jsonl", "transcript-provenance.json", "run.json", "summary.json"):
        path = session / name
        if path.is_file():
            row["source_sha256"][name] = pairs.sha(path)
    return row


def report(root):
    m = pairs.read(root / "manifest.json")
    ledger = pairs.read(Path(m["budget_authority"]) / "paid-state.json")
    rows = [training_row(root, t, ledger) for t in m["families"][0]["train"]]
    checkpoints = []
    for count in m["checkpoints"]:
        folder = root / "checkpoints" / str(count)
        path = folder / "attempt.json"
        row = dict(sessions=count, status="not-run")
        if path.is_file():
            attempt = pairs.read(path)
            row.update(status=attempt["status"], route_count=attempt.get("route_count"),
                       execution_candidates=attempt.get("execution_candidates", []),
                       condition_elapsed_ms=attempt.get("elapsed_ns", 0) / 1e6,
                       binary_sha256=attempt["binary_sha256"], attempt_sha256=pairs.sha(path))
            index = folder / "adage-home/index.json"
            if index.is_file():
                row["routes_by_track"] = dict(Counter(r["track"] for r in pairs.read(index)["routes"]))
            output = folder / "condition.stdout.json"
            if output.is_file() and attempt["status"] == "completed":
                learned = pairs.read(output)
                row["sessions_read"] = learned.get("sessions_read")
                row["sessions_steered"] = learned.get("sessions_steered")
                row["engine"] = {k: v.get("engine") for k, v in learned.get("repos", {}).items()}
        checkpoints.append(row)
    heldout = curve_heldout.report(root)
    final = next((r for r in checkpoints if r["sessions"] == 15), {})
    status = ("training-complete-no-execution-candidate" if final.get("status") == "completed"
              and not final.get("execution_candidates") else "live-pilot-in-progress")
    quality_path = root / "training-quality/report.json"
    quality = pairs.read(quality_path) if quality_path.exists() else None
    return dict(schema=1, recorded_at=datetime.now(timezone.utc).isoformat(), protocol=m["protocol"],
        root_manifest_sha256=pairs.sha(root / "manifest.json"), status=status,
        independent_training_quality=quality,
        training_assignments=rows, checkpoints=checkpoints, heldout=heldout,
        known_training_cost_usd=sum(r["cost_usd"] for r in rows if pairs.known_cost(r["cost_usd"]) is not None),
        training_with_successful_pytest_observation=sum(bool(r["successful_pytest_calls"]) for r in rows),
        claims=dict(real_repository=True, authored_tasks=True, coding_quality_established=False,
                    cpu_action_elimination_established=heldout["passed"], gpu_utilization_measured=False),
        limitations=["Training CLI completion is not independent task correctness.",
            "The pytest invocation audit is descriptive; tool success alone does not establish semantic equivalence or test coverage. Pipelines and arbitrary compounds are excluded.",
            "All failed and unattempted assignments remain visible. Unknown costs are never zero.",
            "No broad debug logs, credentials or local execution grants are exported."])


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    if args.output.exists():
        parser.error("evidence snapshots are immutable")
    result = report(args.root.resolve(strict=True))
    pairs.write(args.output, result)
    print(json.dumps(dict(output=str(args.output), training_cost_usd=result["known_training_cost_usd"],
                         cpu_gate_passed=result["heldout"]["passed"])))
