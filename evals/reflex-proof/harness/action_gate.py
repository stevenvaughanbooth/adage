#!/usr/bin/env python3
"""Learned action-elimination gate: prepare offline, human grant, then bounded live pairs.

Uses the production learner/hook/executor and shared paid-run ledger. The small hook
wrapper only records delivery and restricts the live agent's shell tool to the exact
read-only lookup. It does not predict, execute, or fabricate reflex results itself.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import random
import re
import shlex
import shutil
import subprocess
import sys
import tempfile
import time

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[2]
sys.path.insert(0, str(REPO / "evals/live"))
from budget import Budget, execution_lock, known_cost  # noqa: E402
from runner import spool_records  # noqa: E402
from probe import execute as execute_probe  # noqa: E402

FIELDS = {"result": {"type": "text", "stdout": True, "max": 2000}}
SCORE_FIELDS = ("id", "title", "description", "acceptance", "files")


def write(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    temp = path.with_suffix(path.suffix + ".tmp")
    temp.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n")
    temp.replace(path)


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def run(argv, **kw):
    p = subprocess.run([str(a) for a in argv], text=True, capture_output=True, **kw)
    if p.returncode:
        raise RuntimeError(f"{argv}: {p.stderr}")
    return p.stdout


def env_for(home):
    # Preserve agent protection markers. No approvals are written by this program.
    return dict(os.environ, ADAGE_HOME=str(home), ADAGE_AUTO_LEARN="0", ADAGE_ARM="treated")


def source_hashes():
    paths = [p for p in (REPO / "crates").rglob("*") if p.is_file()
             and (p.suffix == ".rs" or p.name == "Cargo.toml")]
    paths += [REPO / "Cargo.toml", REPO / "Cargo.lock", Path(__file__)]
    return {str(p.relative_to(REPO)): digest(p) for p in sorted(paths)}


def prepare(root, binary_dir):
    root.mkdir(parents=True, exist_ok=True)
    if any(root.iterdir()):
        raise ValueError("prepare requires an empty directory")
    root = root.resolve()
    binary_dir = binary_dir.resolve()
    binaries = {name: str(binary_dir / name) for name in ("adage", "adage-exec")}
    for path in binaries.values():
        if not Path(path).is_file():
            raise ValueError(f"build both production binaries first: {path}")
    (root / "private/bin").mkdir(parents=True)
    for name, path in binaries.items():
        frozen = root / "private/bin" / name
        shutil.copy2(path, frozen)
        binaries[name] = str(frozen)
    data = REPO / "evals/datasets/live-20260928-py-clinic-s12"
    source = json.loads((data / "manifest.json").read_text())
    training = [s for s in source["sessions"] if s["phase"] == "train"]
    train_ids = {s["task"] for s in training}
    tasks = sorted({s["task"] for s in source["sessions"] if s["phase"] == "eval"} - train_ids)
    if len(tasks) < 3:
        raise ValueError("need at least three held-out task instances")
    tasks = tasks[:3]
    spec = next(s["spec"] for s in json.loads((REPO / "evals/gen/specs/smoke.json").read_text())
                ["sandboxes"] if s["seed"] == 12)
    checkout = root / "wardline-clinic"
    run([sys.executable, "-B", REPO / "evals/gen/gen.py", "--spec-json", json.dumps(spec),
         "--seed", "12", "--repo-only", checkout])
    run(["git", "init", "-q", checkout])
    history = root / "private/history"
    history.mkdir(parents=True)
    for session in training:
        text = (data / session["file"]).read_text()
        old = json.loads(text.splitlines()[0])["cwd"]
        (history / Path(session["file"]).name).write_text(text.replace(old, str(checkout)))
    home = root / "private/adage-home"
    command = [binaries["adage"], "condition", "--json", "--repo-root", str(checkout),
               "--transcripts", *[str(p) for p in sorted(history.glob("*.jsonl"))]]
    learned = run(command, cwd=checkout, env=env_for(home))
    (root / "private/learn.json").write_text(learned)
    index = json.loads((home / "index.json").read_text())
    lookups = [r for r in index["routes"] if r["route"]["id"] == "lookup-work-item"
               and r["track"].endswith("/learned")]
    if len(lookups) != 1:
        raise ValueError("default production learner did not produce one lookup reflex")
    candidate = lookups[0]
    template = candidate["route"]["step"]["command"]
    if template != "python3 tools/case {item}":
        raise ValueError(f"review a changed learned command before running: {template}")
    expected, raw = {}, {}
    for task in tasks:
        value = json.loads(run(["python3", "tools/case", task, "--json"], cwd=checkout))
        expected[task] = {k: value[k] for k in SCORE_FIELDS}
        raw[task] = run(["python3", "tools/case", task], cwd=checkout)
    write(root / "private/expected.json", expected)
    write(root / "private/expected-stdout.json", raw)
    # This policy is inert until a person grants the exact reflex. It is private to this eval.
    (home / "policy.toml").write_text('[exec]\nallow_raw_output = true\n')
    grant = [binaries["adage"], "review", "delegate", candidate["track"], candidate["route"]["id"],
             "--level", "e1", "--fields", json.dumps(FIELDS, separators=(",", ":")),
             "--repo", str(checkout), "--by", "launch-gate-human"]
    approve = root / "approve.sh"
    approve.write_text("#!/bin/sh\nset -eu\n" + f"export ADAGE_HOME={shlex.quote(str(home))}\n"
                       + shlex.join(grant) + "\n")
    approve.chmod(0o700)
    rng = random.Random(20260930)
    matrix = []
    for task in tasks:
        arms = ["suggest", "automatic"]
        rng.shuffle(arms)
        matrix.extend({"task": task, "arm": arm} for arm in arms)
    manifest = dict(schema=1, root=str(root), checkout=str(checkout), home=str(home),
        binaries=binaries, binary_sha256={k: digest(Path(v)) for k, v in binaries.items()},
        source_revision=run(["git", "rev-parse", "HEAD"], cwd=REPO).strip(),
        source_files=source_hashes(), index_sha256=digest(home / "index.json"),
        script_sha256=digest(checkout / "tools/case"), approve_sha256=digest(approve),
        frozen_files={str(p.relative_to(root)): digest(p) for p in
            [*sorted(p for p in checkout.rglob("*") if p.is_file() and ".git" not in p.parts),
             root / "private/expected.json", root / "private/expected-stdout.json", home / "policy.toml",
             *sorted(p for p in (home / "tracks").rglob("*") if p.is_file())]},
        harness_files={str(p): digest(p) for p in [Path(__file__), HERE / "probe.py", REPO / "evals/live/budget.py"]},
        training=[dict(file=s["file"], task=s["task"], sha256=digest(data / s["file"])) for s in training],
        default_thresholds=True, supplied_outcomes=False, edited_learned_route=False,
        candidate=candidate, fields=FIELDS, matrix=matrix, seed=20260930,
        expected_claude_version="2.1.282", model="claude-haiku-4-5-20251001",
        total_cap_usd=3.0, session_cap_usd=0.50,
        claim="Live lookup/triage mechanism pilot, not general coding productivity or GPU energy measurement",
        gates={"quality": "all five exact fields match, ignoring whitespace only, in every run",
               "elimination": "each baseline uses the learned lookup; each automatic run makes zero tool calls",
               "delivery": "complete production E1 result before the first observed model API dispatch",
               "accounting": "reported terminal cost for every paid attempt; unknown stops dispatch"})
    write(root / "manifest.json", manifest)
    return {"root": str(root), "approval_command": shlex.join([str(approve)]),
            "matrix": matrix, "total_cap_usd": 3, "grants_written": False, "paid_calls": 0}


def verify(manifest):
    root, home = Path(manifest["root"]), Path(manifest["home"])
    for name, path in manifest["binaries"].items():
        if digest(Path(path)) != manifest["binary_sha256"][name]:
            raise ValueError("production binary changed; prepare a new frozen experiment")
    if digest(home / "index.json") != manifest["index_sha256"]:
        raise ValueError("learned index changed")
    if digest(Path(manifest["checkout"]) / "tools/case") != manifest["script_sha256"]:
        raise ValueError("lookup script changed")
    if digest(root / "approve.sh") != manifest["approve_sha256"]:
        raise ValueError("approval recipe changed")
    for relative, expected in manifest["frozen_files"].items():
        if digest(root / relative) != expected:
            raise ValueError(f"frozen input changed: {relative}")
    for path, expected in manifest["harness_files"].items():
        if digest(Path(path)) != expected:
            raise ValueError(f"evaluation harness changed: {path}")


def call_hook(manifest, event, payload, automatic=True):
    name = "adage-exec" if automatic else "adage"
    return run([manifest["binaries"][name], "hook", "claude", event],
               cwd=manifest["checkout"], env=env_for(manifest["home"]), input=json.dumps(payload))


def context(output):
    if not output.strip():
        return ""
    return json.loads(output).get("hookSpecificOutput", {}).get("additionalContext", "")


def data_payload(output):
    ctx = context(output)
    blocks = re.findall(r"<adage-data\b[^>]*>\n.*?\n([^\n]+)\n</adage-data>", ctx, re.S)
    if len(blocks) != 1:
        return None
    return json.loads(blocks[0]).get("result")


def check(root):
    manifest = json.loads((root / "manifest.json").read_text())
    verify(manifest)
    if not (Path(manifest["home"]) / "approvals.json").is_file():
        raise ValueError(f"human approval is missing; run {root / 'approve.sh'} in your terminal")
    results = []
    for task in sorted({m["task"] for m in manifest["matrix"]}):
        payload = dict(session_id=f"gate-offline-{task}-{time.time_ns()}", cwd=manifest["checkout"],
                       hook_event_name="UserPromptSubmit", prompt=f"Please triage {task}.")
        automatic = call_hook(manifest, "UserPromptSubmit", payload)
        expected = run(["python3", "tools/case", task], cwd=manifest["checkout"])
        delivered = data_payload(automatic)
        if delivered is None or delivered.strip() != expected.strip():
            raise ValueError(f"complete production result missing for {task}: {automatic}")
        results.append(dict(task=task, complete=True, bytes=len(delivered.encode())))
    write(root / "offline.json", {"production_complete_delivery": results, "paid_calls": 0})
    return results


def hook_wrapper(root, run_dir, arm, task):
    manifest = json.loads((root / "manifest.json").read_text())
    payload = json.load(sys.stdin)
    event = payload.get("hook_event_name", "")
    if event == "PreToolUse":
        allowed = allowed_call(payload, task)
        out = {} if allowed else {"hookSpecificOutput": {"hookEventName": "PreToolUse",
            "permissionDecision": "deny", "permissionDecisionReason": "This read-only trial permits only the exact work-item lookup."}}
        print(json.dumps(out))
        return
    verify(manifest)
    start = time.monotonic_ns()
    output = call_hook(manifest, event, payload, automatic=arm == "automatic")
    record = dict(event=event, start_monotonic_ns=start, end_monotonic_ns=time.monotonic_ns(),
                  output=output, payload_session=payload.get("session_id"))
    # Each hook writes one line; hook ordering is synchronous for this single prompt trial.
    with (run_dir / "production-hooks.jsonl").open("a") as handle:
        handle.write(json.dumps(record) + "\n")
    print(output, end="")


def allowed_call(payload, task):
    inputs = payload.get("tool_input", {})
    return (payload.get("tool_name") == "Bash" and isinstance(inputs, dict)
            and inputs.get("command") == f"python3 tools/case {task}"
            and not inputs.get("dangerouslyDisableSandbox") and not inputs.get("run_in_background"))


def grade(text, expected):
    try:
        found = json.loads(text.strip().removeprefix("```json").removesuffix("```").strip())
    except (ValueError, TypeError):
        return False
    def normal(value):
        if isinstance(value, str):
            return " ".join(value.split())
        if isinstance(value, list):
            return [normal(v) for v in value]
        return value
    return isinstance(found, dict) and all(normal(found.get(k)) == normal(expected[k]) for k in SCORE_FIELDS)


def live(root, limit):
    manifest = json.loads((root / "manifest.json").read_text())
    verify(manifest)
    check(root)  # no paid call until the actual production E1 path delivers complete results
    with execution_lock(root):
        path = root / "state.json"
        state = json.loads(path.read_text()) if path.exists() else {"runs": {}}
        budget = Budget(state, manifest["total_cap_usd"], manifest["session_cap_usd"],
                        persist=lambda: write(path, state))
        completed = 0
        for entry in manifest["matrix"]:
            key = f"{entry['task']}-{entry['arm']}"
            if key in state["runs"]:
                continue  # no silent retries or exclusions
            if completed >= limit:
                break
            run_dir = root / "runs" / key
            run_dir.mkdir(parents=True, exist_ok=False)
            (run_dir / ".reflex-fixture").write_text("production learned lookup gate\n")
            settings = {"autoMemoryEnabled": False, "hooks": {}}
            for event in ("SessionStart", "UserPromptSubmit", "PreToolUse", "PostToolUse", "Stop", "SessionEnd"):
                argv = [sys.executable, "-B", str(Path(__file__)), "hook", str(root), str(run_dir), entry["arm"], entry["task"]]
                settings["hooks"][event] = [{"hooks": [{"type": "command", "command": shlex.join(argv), "timeout": 10}]}]
            write(run_dir / "settings.json", settings)
            task = entry["task"]
            prompt = (f"Triage {task}. Return its id, exact title, complete description, complete acceptance criteria, "
                      "and file paths as a JSON object with keys id,title,description,acceptance,files. "
                      "Copy the work item's wording, without paraphrasing. Do not modify files. Return only JSON.")
            argv = ["claude", "--print", "--restricted", "--model", manifest["model"],
                    "--max-budget-usd", str(manifest["session_cap_usd"]), "--max-turns", "4",
                    "--output-format", "stream-json", "--verbose", "--include-hook-events",
                    "--debug-file", str(run_dir / "claude-debug.log"), "--no-session-persistence", "--no-chrome",
                    "--setting-sources", "", "--settings", str(run_dir / "settings.json"),
                    "--strict-mcp-config", "--mcp-config", '{"mcpServers":{}}', "--disable-slash-commands",
                    "--tools", "Bash", "--allowedTools", f"Bash(python3 tools/case {task})",
                    "--permission-mode", "dontAsk", "--permission-prompts", "none", prompt]
            # probe.execute supplies the proven stream/wall-time/cancellation capture. Its cwd
            # is the manifest root, so use the generated checkout and keep logs in the run dir
            # through a separate execution_root supported by the shared probe.
            run_manifest = dict(root=str(run_dir), execution_root=manifest["checkout"], argv=argv,
                expected_claude_version=manifest["expected_claude_version"], budget_usd=manifest["session_cap_usd"])
            write(run_dir / "manifest.json", run_manifest)
            attempt, reason = budget.reserve(key)
            if attempt is None:
                raise ValueError(reason)
            result = None
            try:
                if not budget.dispatch(attempt):
                    raise ValueError("accounting blocked dispatch")
                result = execute_probe(run_dir / "manifest.json", manifest["session_cap_usd"], 120)
                state["runs"][key] = {**entry, "summary": result}
            finally:
                budget.settle(attempt, result)
                write(path, state)
            completed += 1
        return report(root)


def report(root):
    manifest = json.loads((root / "manifest.json").read_text())
    verify(manifest)
    expected = json.loads((root / "private/expected.json").read_text())
    raw = json.loads((root / "private/expected-stdout.json").read_text())
    state = json.loads((root / "state.json").read_text())
    rows = []
    for key, run_record in state["runs"].items():
        run_dir = root / "runs" / key
        stream = [json.loads(s) for s in (run_dir / "stdout.jsonl").read_text().splitlines() if s.strip()]
        # De-duplicate assistant message content if a harness emits the same message twice.
        tools, successful_results = {}, set()
        for item in stream:
            if item.get("type") == "assistant":
                for block in item.get("message", {}).get("content", []):
                    if block.get("type") == "tool_use":
                        tools[block["id"]] = {"name": block.get("name"), "input": block.get("input")}
            if item.get("type") == "user":
                for block in item.get("message", {}).get("content", []):
                    if block.get("type") == "tool_result" and not block.get("is_error", False):
                        successful_results.add(block.get("tool_use_id"))
        s = run_record["summary"]
        hooks = [json.loads(line) for line in (run_dir / "production-hooks.jsonl").read_text().splitlines()]
        deliveries = [h for h in hooks if h["event"] == "UserPromptSubmit" and data_payload(h["output"]) is not None]
        complete = len(deliveries) == 1 and data_payload(deliveries[0]["output"]).strip() == raw[run_record["task"]].strip()
        receipts = []
        for sid in {h["payload_session"] for h in hooks if h.get("payload_session")}:
            if not re.fullmatch(r"[\w-]+", sid):
                raise ValueError("unsafe session id in hook evidence")
            spool = Path(manifest["home"]) / "sessions" / f"{sid}.spool"
            if spool.is_file():
                receipts.extend(e for record in spool_records(spool) for e in record.get("exec", []))
        label = manifest["candidate"]["track"] + "/" + manifest["candidate"]["route"]["id"]
        execution_ok = (len(receipts) == 1 and receipts[0].get("route") == label
                        and receipts[0].get("level") == "e1" and receipts[0].get("exit") == 0
                        and not receipts[0].get("truncated") and "withheld" not in receipts[0])
        process = json.loads((run_dir / "run.json").read_text())
        process_ok = process.get("exit_code") == 0 and not process.get("cancelled") and not s.get("result_is_error")
        baseline_ok = len(tools) == 1 and all(i in successful_results and allowed_call(
            {"tool_name": t["name"], "tool_input": t["input"]}, run_record["task"]) for i, t in tools.items())
        debug = (run_dir / "claude-debug.log").read_text()
        dispatches = re.findall(r"^(\S+) \[DEBUG\] \[API REQUEST\] /v1/messages x-client-request-id=([\w-]+) source=(\w+)", debug, re.M)
        contexts = re.findall(r"^(\S+) \[DEBUG\] Hook UserPromptSubmit .* provided additionalContext \((\d+) chars\)", debug, re.M)
        barrier = bool(deliveries and contexts and dispatches and contexts[0][0] < dispatches[0][0])
        rows.append(dict(key=key, task=run_record["task"], arm=run_record["arm"],
            quality=grade(s.get("result_text", ""), expected[run_record["task"]]),
            process_ok=process_ok, exact_successful_baseline_lookup=baseline_ok,
            complete_delivery=complete, execution_receipts=receipts, successful_production_execution=execution_ok,
            agent_tool_calls=list(tools.values()), agent_tool_count=len(tools),
            production_deliveries=len(deliveries), observed_api_dispatches=len(dispatches),
            delivery_before_first_dispatch=barrier, cost_usd=s.get("total_cost_usd"),
            usage=s.get("usage"), duration_api_ms=s.get("duration_api_ms"),
            status=s.get("result_status"), diagnostics={"dispatches": dispatches, "contexts": contexts}))
    gate = passed_gate(manifest, rows, state["accounting"])
    result = dict(mechanism_gate_passed=gate, planned_runs=len(manifest["matrix"]), rows=rows,
                  accounting=state["accounting"], claim=manifest["claim"])
    write(root / "report.json", result)
    return result


def passed_gate(manifest, rows, accounting):
    keys = {f"{m['task']}-{m['arm']}" for m in manifest["matrix"]}
    if len(rows) != len(keys) or {r["key"] for r in rows} != keys or accounting.get("blocked_reason"):
        return False
    attempts = accounting.get("attempts", [])
    if len(attempts) != len(keys) or {a["run"] for a in attempts} != keys:
        return False
    if any(a.get("status") != "settled" or known_cost(a.get("cost_usd")) is None
           or a["cost_usd"] > manifest["session_cap_usd"] for a in attempts):
        return False
    if sum(a["cost_usd"] for a in attempts) > manifest["total_cap_usd"]:
        return False
    costs = {a["run"]: a["cost_usd"] for a in attempts}
    if any(known_cost(r.get("cost_usd")) is None or r["cost_usd"] != costs[r["key"]] for r in rows):
        return False
    return all(r["quality"] and r["process_ok"] and r["status"] == "success" and
        (r["agent_tool_count"] == 0 and r["complete_delivery"] and r["successful_production_execution"]
         and r["delivery_before_first_dispatch"] if r["arm"] == "automatic" else
         r["exact_successful_baseline_lookup"] and r["production_deliveries"] == 0
         and not r["execution_receipts"]) for r in rows)


def main():
    p = argparse.ArgumentParser(description=__doc__)
    subs = p.add_subparsers(dest="command", required=True)
    prep = subs.add_parser("prepare")
    prep.add_argument("--root", type=Path)
    prep.add_argument("--binary-dir", type=Path, default=REPO / "target/debug")
    for name in ("check", "live", "report"):
        cmd = subs.add_parser(name)
        cmd.add_argument("root", type=Path)
        if name == "live":
            cmd.add_argument("--limit", type=int, default=2, help="dispatch at most this many new sessions")
    h = subs.add_parser("hook")
    for name in ("root", "run_dir"):
        h.add_argument(name, type=Path)
    h.add_argument("arm", choices=["suggest", "automatic"])
    h.add_argument("task")
    args = p.parse_args()
    if args.command == "prepare":
        value = prepare(args.root or Path(tempfile.mkdtemp(prefix="adage-action-gate-")), args.binary_dir)
    elif args.command == "hook":
        hook_wrapper(args.root, args.run_dir, args.arm, args.task)
        return
    elif args.command == "live":
        value = live(args.root.resolve(), args.limit)
    else:
        value = {"check": check, "report": report}[args.command](args.root.resolve())
    print(json.dumps(value, indent=2))


if __name__ == "__main__":
    main()
