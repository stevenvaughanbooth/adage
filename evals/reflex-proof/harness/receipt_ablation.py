#!/usr/bin/env python3
"""Fixed-fixture receipt diagnostic. No learned reflexes, grants, or launch claims."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import sys

import delivery_canary as delivery
import real_repo_gate as gate

ARMS = ("baseline", "generic", "typed", "ownership")
def interpreter(authority):
    return authority / "runtime/python/cpython-3.12.14-macos-aarch64-none/bin/python3"


def command(authority):
    return str(interpreter(authority)) + " -B verify.py"
POLICY = ("This diagnostic harness runs verify.py automatically after you read increment.py. "
          "A successful verification receipt records completed work for its stated source hash. "
          "Use a current receipt as verification evidence. Run the verifier yourself if there is no "
          "successful current receipt, or if you need different diagnostic information. "
          "The receipt covers only the listed cases, not universal correctness.")
VERIFY = '''import hashlib, json, runpy
from pathlib import Path
p = Path("increment.py")
f = runpy.run_path(str(p))["increment"]
cases = [(-1, 0), (0, 1), (41, 42)]
assert all(f(x) == y for x, y in cases)
print(json.dumps({"passed": 3, "failed": 0, "cases": cases,
 "source_sha256": hashlib.sha256(p.read_bytes()).hexdigest()}))
'''


def hook(session):
    payload = json.load(sys.stdin)
    config = json.loads((session / "hook-config.json").read_text())
    arm = config["arm"]
    editing = config.get("mode") == "edit"
    event = payload.get("hook_event_name")
    if event == "SessionStart" and arm == "ownership":
        policy = POLICY.replace("read increment.py", "edit CHANGELOG.md") if editing else POLICY
        return {"hookSpecificOutput":{"hookEventName":event, "additionalContext":policy}}
    if arm == "baseline" or event != "PostToolUse" or payload.get("tool_name") != ("Edit" if editing else "Read"):
        return {}
    if Path(payload.get("tool_input", {}).get("file_path", "")).name != ("CHANGELOG.md" if editing else "increment.py"):
        return {}
    checkout = session / "workspace"
    authority = Path(config["authority"])
    result = gate.confined(authority, "receipt-" + session.name,
        [str(interpreter(authority)), "-B", "verify.py"], checkout, timeout=10)
    receipt = dict(command=command(authority), exit_code=result.returncode,
        source_sha256=gate.sha(checkout / "increment.py"), stdout=result.stdout,
        stderr=result.stderr, scope="three cases in verify.py", fixture=True)
    gate.append(session / "cpu-receipts.jsonl", receipt)
    if result.returncode:
        text = "Fixture verification failed; investigate rather than reuse a success receipt."
    elif arm == "generic":
        text = ("Adage ran `" + command(authority) + "` automatically after a learned context matched: "
            "verification succeeded; no repository files changed. This action is complete; do not repeat it.\n"
            "Learned by Adage from this repository's earlier agent sessions; not requested by the user.")
        # Intentionally reproduces current prose for the experiment, not a real learning claim.
        text = "DIAGNOSTIC: simulated current-product receipt wording follows.\n" + text
    else:
        text = "COMPLETED VERIFICATION (fixture harness, actual execution):\n" + json.dumps(receipt, sort_keys=True)
    response = {"hookSpecificOutput":{"hookEventName":event, "additionalContext":text}}
    gate.append(session / "hook-outputs.jsonl", response)
    return response


def prepare(authority, root, editing=False):
    # Reuse the frozen diagnostic runner and budget contract; change fixtures before dispatch.
    delivery.prepare(authority, root)
    authority, root = authority.resolve(), root.resolve()
    for old in delivery.ARMS:
        shutil.rmtree(root / old)
    protocol = json.loads((root / "protocol.json").read_text())
    protocol.update(kind="receipt-consumption ablation", arms=list(ARMS), repetitions=2,
        maximum_new_reservations_usd=8 * delivery.CAP,
        expected="diagnostic only; count all agent Bash requests and actual successful CPU receipts",
        mode="edit" if editing else "read",
        limitation="authored fixture, not learned production reflex or real-code transfer")
    protocol["sources"][str(Path(__file__).resolve())] = gate.sha(Path(__file__))
    gate.write(root / "protocol.json", protocol)
    for repetition in range(2):
        for arm in ARMS:
            session = root / (str(repetition + 1) + "-" + arm)
            checkout = session / "workspace"
            checkout.mkdir(parents=True)
            (checkout / "increment.py").write_text("def increment(n):\n    return n " + ("-" if editing else "+") + " 1\n")
            (checkout / "verify.py").write_text(VERIFY)
            if editing:
                (checkout / "CHANGELOG.md").write_text("# Changes\n\nNo changes yet.\n")
            (session / ".reflex-fixture").write_text("receipt diagnostic only\n")
            gate.write(session / "hook-config.json", dict(arm=arm, authority=str(authority), mode="edit" if editing else "read"))
            settings = gate.native_settings(authority, checkout, ["Read", "Bash(" + command(authority) + ")"])
            if editing:
                settings["permissions"]["allow"] += ["Edit(/" + str(checkout / name) + ")" for name in ("increment.py", "CHANGELOG.md")]
                settings["permissions"]["deny"] += ["Edit(/" + str(checkout / "verify.py") + ")"]
            handler = dict(type="command", command=sys.executable,
                args=[str(Path(__file__).resolve()), "hook", str(session)], timeout=20)
            settings.update(disableAllHooks=False, hooks={event:[dict(hooks=[handler])]
                for event in ("SessionStart", "PostToolUse")})
            gate.write(session / "settings.json", settings)
            argv = ["claude", "--print", "--model", "claude-haiku-4-5-20251001",
                "--max-budget-usd", str(delivery.CAP), "--output-format", "stream-json", "--verbose",
                "--include-hook-events", "--debug-file", str(session / "claude-debug.log"),
                "--no-session-persistence", "--no-chrome", "--setting-sources", "",
                "--settings", str(session / "settings.json"), "--strict-mcp-config", "--mcp-config",
                '{"mcpServers":{}}', "--tools", "Read,Bash", "--allowedTools", "Read", "Bash(" + command(authority) + ")",
                "--permission-mode", "dontAsk",
                "Use Read on " + str(checkout / "increment.py") + " first (no directory search is needed). "
                "Assess whether this file implements n + 1 for integers. Verify it, "
                "and report your evidence and its limits. The repository verifier is `" + command(authority) + "`. Do not edit files."]
            if editing:
                argv[argv.index("--tools")+1] = "Read,Edit,Bash"
                argv[argv.index("--permission-mode"):argv.index("--permission-mode")] = ["Edit"]
                argv[-1] = ("Use Read on " + str(checkout / "increment.py") + " and " + str(checkout / "CHANGELOG.md") +
                    ". Fix increment.py to return n + 1 for integers using Edit. Then update CHANGELOG.md using Edit. "
                    "Verify the implementation and report evidence and limits. The repository verifier is `" + command(authority) + "`. Do not change verify.py.")
            gate.write(session / "manifest.json", dict(root=str(session), execution_root=str(checkout),
                experiment_root=str(authority), arm=arm, repetition=repetition+1, argv=argv,
                expected_claude_version=delivery.probe.EXPECTED_VERSION, budget_usd=delivery.CAP,
                budget_allocation_sha256=gate.sha(authority / "evaluation-allocation.json"),
                protocol_sha256=gate.sha(root / "protocol.json"),
                files={str(p):gate.sha(p) for p in (session / "hook-config.json", session / "settings.json",
                    checkout / "increment.py", checkout / "verify.py")}))
    return root


def assess(session, manifest, result):
    rows = [json.loads(line) for line in (session / "stdout.jsonl").read_text().splitlines()]
    calls = {c["id"]:c for row in rows if row.get("type") == "assistant"
        for c in row.get("message", {}).get("content", []) if c.get("type") == "tool_use"}
    commands = [c["input"].get("command") for c in calls.values() if c.get("name") == "Bash"]
    receipts = session / "cpu-receipts.jsonl"
    cpu = [json.loads(x) for x in receipts.read_text().splitlines()] if receipts.exists() else []
    assessment = dict(arm=manifest["arm"], repetition=manifest["repetition"], agent_commands=commands,
        agent_tool_calls=len(calls), successful_cpu_receipts=sum(r["exit_code"]==0 for r in cpu),
        result_is_error=result.get("result_is_error"), cost_usd=result.get("total_cost_usd"),
        answer=result.get("result_text"), source_unchanged=all(gate.sha(Path(p))==h
            for p,h in manifest["files"].items()),
        limitation="authored diagnostic; not production learned action elimination")
    if json.loads((session / "hook-config.json").read_text()).get("mode") == "edit":
        authority = Path(manifest["experiment_root"])
        quality = gate.confined(authority, "quality-" + session.name,
            [str(interpreter(authority)), "-B", "verify.py"], session / "workspace", timeout=10)
        assessment["quality_exit"] = quality.returncode
        assessment["verifier_unchanged"] = gate.sha(session / "workspace/verify.py") == manifest["files"][str(session / "workspace/verify.py")]
    return assessment


if __name__ == "__main__":
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument("command", choices=("prepare", "prepare-edit", "live", "hook"))
    p.add_argument("path",type=Path)
    p.add_argument("destination",type=Path,nargs="?")
    a=p.parse_args()
    value=prepare(a.path,a.destination,a.command=="prepare-edit") if a.command.startswith("prepare") else (
        hook(a.path) if a.command=="hook" else delivery.live(a.path, assess))
    print(json.dumps(value,default=str))
