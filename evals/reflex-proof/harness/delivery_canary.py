#!/usr/bin/env python3
"""Read-only native delivery diagnostic; not a production reflex or savings eval."""
import argparse
import copy
import json
from pathlib import Path
import secrets
import shutil
import subprocess
import sys

import probe
import real_repo_gate as gate

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "live"))
from budget import Budget, execution_lock

ARMS = ("context", "replacement", "malformed", "control")
CAP = .25


def output(arm, original, marker):
    text = "DIAGNOSTIC HOOK APPENDIX (not file contents): receipt marker " + marker
    specific = {"hookEventName": "PostToolUse"}
    if arm == "context":
        specific["additionalContext"] = text
    elif arm == "replacement":
        # Read's native text shape; preserve every original field and original content.
        if original.get("type") != "text" or not isinstance(original.get("file", {}).get("content"), str):
            raise ValueError("unexpected native Read shape; do not fabricate a replacement")
        replacement = copy.deepcopy(original)
        replacement["file"]["content"] += "\n\n" + text
        specific["updatedToolOutput"] = replacement
    elif arm == "malformed":
        specific["updatedToolOutput"] = {"invalid_read_shape": text}
    elif arm != "control":
        raise ValueError("unknown arm")
    return {"hookSpecificOutput": specific}


def hook(session):
    payload = json.load(sys.stdin)
    config = json.loads((session / "hook-config.json").read_text())
    if payload.get("hook_event_name") != "PostToolUse" or payload.get("tool_name") != "Read":
        return {}
    gate.append(session / "hook-inputs.jsonl", payload)
    result = output(config["arm"], payload["tool_response"], config["marker"])
    gate.append(session / "hook-outputs.jsonl", result)
    return result


def prepare(authority, root):
    authority = authority.resolve(strict=True)
    root = root.resolve()
    if authority not in root.parents:
        raise ValueError("diagnostic must use original budget authority")
    root.mkdir(parents=True, exist_ok=False)
    protocol = dict(kind="native read-only delivery diagnostic", arms=list(ARMS),
        per_assignment_cap_usd=CAP, maximum_new_reservations_usd=CAP * len(ARMS),
        repetitions=1, savings_evidence=False, production_execution=False,
        authority=str(authority), no_outcome_retries=True,
        expected="original file marker in all arms; hook marker only in context/replacement",
        sources={str(p):gate.sha(p) for p in (Path(__file__), Path(probe.__file__), Path(gate.__file__),
            Path(__file__).resolve().parents[2] / "live/budget.py")})
    gate.write(root / "protocol.json", protocol)
    for arm in ARMS:
        session = root / arm
        checkout = session / "workspace"
        checkout.mkdir(parents=True)
        subprocess.run(["git", "init", "-q", str(checkout)], check=True)
        original = "FILE_" + secrets.token_hex(10)
        marker = "HOOK_" + secrets.token_hex(10)
        (checkout / "fixture.txt").write_text(original + "\n")
        (session / ".reflex-fixture").write_text("delivery canary only\n")
        gate.write(session / "hook-config.json", dict(arm=arm, marker=marker, original=original))
        shutil.copyfile(__file__, session / "fixture_hook.py")
        # The hook imports repository helpers; agent tools can only read the fixture.
        handler = dict(type="command", command=sys.executable,
            args=[str(Path(__file__).resolve()), "hook", str(session)], timeout=5)
        settings = gate.native_settings(authority, checkout, ["Read"])
        settings.update(disableAllHooks=False, hooks={"PostToolUse":[dict(matcher="Read", hooks=[handler])]})
        settings["permissions"]["deny"] += ["Read(/" + str(session / name) + ")" for name in
            ("hook-config.json", "fixture_hook.py", "settings.json", "manifest.json")]
        gate.write(session / "settings.json", settings)
        prompt = ("Read fixture.txt once using Read. Report its FILE_ marker and any HOOK_ receipt marker "
                  "actually delivered with that read. If no receipt marker is present say ABSENT. "
                  "Do not search for markers elsewhere or use any other tool. This is a delivery diagnostic.")
        argv = ["claude", "--print", "--model", "claude-haiku-4-5-20251001",
            "--max-budget-usd", str(CAP), "--output-format", "stream-json", "--verbose",
            "--include-hook-events", "--debug-file", str(session / "claude-debug.log"),
            "--no-session-persistence", "--no-chrome", "--setting-sources", "",
            "--settings", str(session / "settings.json"), "--strict-mcp-config", "--mcp-config",
            '{"mcpServers":{}}', "--tools", "Read", "--allowedTools", "Read",
            "--permission-mode", "dontAsk", prompt]
        manifest = dict(root=str(session), execution_root=str(checkout), experiment_root=str(authority),
            argv=argv, expected_claude_version=probe.EXPECTED_VERSION, budget_usd=CAP, arm=arm,
            budget_allocation_sha256=gate.sha(authority / "evaluation-allocation.json"),
            protocol_sha256=gate.sha(root / "protocol.json"),
            files={str(p):gate.sha(p) for p in (session / "settings.json", session / "hook-config.json",
                session / "fixture_hook.py", checkout / "fixture.txt")})
        gate.write(session / "manifest.json", manifest)
    return root


def live(session):
    manifest = json.loads((session / "manifest.json").read_text())
    authority = Path(manifest["experiment_root"])
    gate.validate_authority(manifest)
    protocol_path = session.parent / "protocol.json"
    if gate.sha(protocol_path) != manifest["protocol_sha256"]:
        raise ValueError("protocol changed")
    protocol = json.loads(protocol_path.read_text())
    for path, digest in (protocol["sources"] | manifest["files"]).items():
        if gate.sha(Path(path)) != digest:
            raise ValueError("frozen source changed: " + path)
    with execution_lock(authority):
        path = authority / "paid-state.json"
        state = json.loads(path.read_text())
        budget = Budget(state, gate.budget_limit(manifest), CAP, persist=lambda:gate.write(path, state))
        key = str(session.relative_to(authority))
        if key in state["runs"]:
            raise ValueError("assignment already attempted")
        attempt, reason = budget.reserve(key)
        if attempt is None:
            raise ValueError(reason)
        state["runs"][key] = dict(status="reserved", kind="delivery-canary")
        gate.write(path, state)
        result = None
        try:
            if not budget.dispatch(attempt):
                raise ValueError("accounting blocks dispatch")
            result = probe.execute(session / "manifest.json", CAP, 90)
            config = json.loads((session / "hook-config.json").read_text())
            answer = result.get("result_text") or ""
            observed = dict(arm=manifest["arm"], original_received=config["original"] in answer,
                hook_received=config["marker"] in answer, cost_usd=result.get("total_cost_usd"),
                answer=answer, result_is_error=result.get("result_is_error"),
                limitation="one diagnostic sample; not production execution or action avoidance")
            observed["passed"] = (observed["original_received"] and result.get("result_is_error") is False
                and observed["hook_received"] == (manifest["arm"] in ("context", "replacement")))
            gate.write(session / "finding.json", observed)
            state["runs"][key].update(status="complete", summary=result, finding=observed)
            return observed
        except BaseException as error:
            state["runs"][key].update(status="failed", error=str(error))
            raise
        finally:
            budget.settle(attempt, result)
            gate.write(path, state)


if __name__ == "__main__":
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("command", choices=("prepare", "live", "hook"))
    p.add_argument("path", type=Path)
    p.add_argument("destination", type=Path, nargs="?")
    args = p.parse_args()
    result = prepare(args.path, args.destination) if args.command == "prepare" else (
        live(args.path) if args.command == "live" else hook(args.path))
    print(json.dumps(result, default=str))
