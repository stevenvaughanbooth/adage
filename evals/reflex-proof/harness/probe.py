#!/usr/bin/env python3
"""Prepare a dry-run fixture or explicitly execute a budgeted Claude substrate probe."""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import queue
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import threading
import time

from fixture_hook import monotonic_ns

HERE = Path(__file__).resolve().parent
EXPECTED_VERSION = "2.1.282"
EVENTS = ("SessionStart", "UserPromptSubmit", "PreToolUse", "PostToolUse", "PostToolBatch", "Stop", "SessionEnd")
# The launch environment is an allowlist, so a frozen sandbox policy never depends on the shell
# that freezes or runs it. Unlisted names are dropped before Claude starts. Credential names may
# reach Claude for login but every one is denied to tool children (DENIED_ENV, a fixed list).
# A random sentinel joins every launch and is denied too; the native canary proves it is absent.
LAUNCH_PASS = ("PATH", "HOME", "USER", "LOGNAME", "SHELL", "LANG", "LC_ALL", "LC_CTYPE", "TERM", "TMPDIR")
LAUNCH_SECRETS = ("ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_BASE_URL", "CLAUDE_CODE_OAUTH_TOKEN",
                  "CLAUDE_CONFIG_DIR")
SENTINEL = "ADAGE_CONFINEMENT_SENTINEL"
# Names Claude Code itself gives tool children. Denial is attempted for each; the native canary
# reports which survive. A survivor is tolerated only when the same canary proves it grants nothing:
# the sandbox proxy credential only if the proxy refuses an external request, the inbox token only if
# a tool cannot connect to the session's inbox socket. Never a pattern; never a host credential.
RUNTIME_INJECTED = {"CLOUDSDK_PROXY_PASSWORD": "proxy_denies_external", "CLAUDE_CODE_MESSAGING_TOKEN": "inbox"}
CREDENTIAL_LIKE = re.compile(r"TOKEN|SECRET|PASSWORD|CREDENTIAL|API_KEY|AUTH", re.I)
DENIED_ENV = tuple(sorted({*LAUNCH_SECRETS, SENTINEL, *RUNTIME_INJECTED, "CLAUDE_CODE_MESSAGING_SOCKET",
    "ANTHROPIC_CUSTOM_HEADERS", "AWS_ACCESS_KEY_ID", "AWS_SECRET_ACCESS_KEY", "AWS_SESSION_TOKEN", "GH_TOKEN",
    "GITHUB_TOKEN", "GOOGLE_APPLICATION_CREDENTIALS", "NPM_TOKEN", "OPENAI_API_KEY", "PYTHONPATH", "PYTHONSTARTUP",
    "SSH_AUTH_SOCK"}))


def proven_harmless(name, checks):
    """The single exemption rule: a named runtime-injected variable whose proof passed this run."""
    return name in RUNTIME_INJECTED and checks.get(RUNTIME_INJECTED[name]) is True


def unexplained_credentials(names, checks):
    """Credential-like names visible to tools that no passing canary check accounts for."""
    return sorted(n for n in names if CREDENTIAL_LIKE.search(n) and not proven_harmless(n, checks))


def visible_denied(names, checks):
    """Denied names a tool still saw, other than proven-harmless runtime-injected ones."""
    return sorted(n for n in names if n in DENIED_ENV and not proven_harmless(n, checks))


def canary_output(content):
    """(names, checks) from a native canary tool result. Claude Code may prefix an exit-code line
    and append blocks (for example <sandbox_violations>); each value is decoded where it starts."""
    text = content if isinstance(content, str) else "".join(
        b.get("text", "") for b in content if isinstance(b, dict))
    decoder = json.JSONDecoder()
    def value(marker):
        if text.count(marker) != 1:
            raise ValueError("native canary output must contain exactly one " + marker)
        return decoder.raw_decode(text.split(marker, 1)[1])[0]
    names, checks = value("NATIVE_ENV_NAMES="), value("NATIVE_SANDBOX_CHECKS=")
    if not (isinstance(names, list) and all(isinstance(n, str) for n in names) and isinstance(checks, dict)):
        raise ValueError("malformed native canary output")
    return names, checks


NATIVE_REQUIRED = ("read", "outside_write", "dependency_write", "git_write", "network", "inside_write", "inbox",
                   "env_sentinel_denied", "python_on_path")


def assess_canary(content):
    """Content-level verdict: every required check true; no unexplained denied or credential names."""
    names, checks = canary_output(content)
    failed = [k for k in NATIVE_REQUIRED if checks.get(k) is not True]
    denied, credentials = visible_denied(names, checks), unexplained_credentials(names, checks)
    return dict(passed=not (failed or denied or credentials), checks=checks, tool_environment_names=names,
                failed_checks=failed, visible_denied_names=denied, unexplained_credential_names=credentials,
                tolerated_runtime_names=sorted(n for n in names if proven_harmless(n, checks)))


def launch_env(source=None):
    """The exact Claude environment: allowlisted names from `source`, plus a fresh sentinel."""
    source = os.environ if source is None else source
    env = {k: source[k] for k in LAUNCH_PASS + LAUNCH_SECRETS if k in source}
    env[SENTINEL] = os.urandom(16).hex()
    return env


def project_launch(env, manifest):
    """A realistic activated project environment (launch-gate-1 revision 7): the checkout's own
    `.venv` first on PATH and as VIRTUAL_ENV, so bare `python`/`pytest` resolve as they would for
    a developer. It adds no readable or writable path: the sandbox policy is unchanged."""
    project = manifest.get("project_environment")
    if not project:
        return env, LAUNCH_PASS
    expected = Path(manifest["execution_root"]) / ".venv"
    if Path(project) != expected or not (expected / "bin/python").exists():
        raise ValueError("project environment must be the checkout's own .venv")
    env = dict(env, VIRTUAL_ENV=project, PATH=str(expected / "bin") + ":" + env.get("PATH", "/usr/bin:/bin"))
    return env, LAUNCH_PASS + ("VIRTUAL_ENV",)


def check_launch(env, settings, plain=LAUNCH_PASS):
    """Fail closed unless every launched name is a plain allowlisted name or denied to tools."""
    denied = {x.get("name") for x in settings.get("sandbox", {}).get("credentials", {}).get("envVars", [])
              if x.get("mode") == "deny"}
    exposed = sorted(set(env) - set(plain) - denied)
    if exposed:
        raise ValueError("launch would expose undenied environment names to tools: " + ", ".join(exposed))
    return sorted(env)


def prepare(model, budget, delay=0):
    root = Path(tempfile.mkdtemp(prefix="adage-reflex-harness-"))
    root.chmod(0o700)
    (root / ".reflex-fixture").write_text("fixture only\n")
    shutil.copyfile(HERE / "fixture_hook.py", root / "fixture_hook.py")
    (root / "a.txt").write_text("fixture alpha\n")
    (root / "b.txt").write_text("fixture beta\n")
    subprocess.run(["git", "init", "-q", str(root)], check=True)
    handler = {"type": "command", "command": sys.executable,
               "args": [str(root / "fixture_hook.py"), str(root), str(delay)], "timeout": max(5, delay + 3)}
    settings = {"autoMemoryEnabled": False, "hooks": {event: [{"hooks": [handler]}] for event in EVENTS}}
    (root / "settings.json").write_text(json.dumps(settings, indent=2))
    prompt = ("This is an isolated hook-boundary fixture. Read a.txt and b.txt using Read, preferably in one parallel "
              "batch. Do not use other tools. Then report both file contents and whether you received any "
              "REFLEX_FIXTURE_OK receipts. Do not invoke the fixture script yourself.")
    argv = ["claude", "--print", "--model", model, "--max-budget-usd", str(budget),
            "--output-format", "stream-json", "--verbose", "--include-hook-events", "--include-partial-messages",
            "--debug-file", str(root / "claude-debug.log"), "--no-session-persistence", "--no-chrome",
            "--setting-sources", "", "--settings", str(root / "settings.json"), "--strict-mcp-config",
            "--mcp-config", '{"mcpServers":{}}', "--tools", "Read", "--allowedTools", "Read",
            "--permission-mode", "dontAsk", prompt]
    manifest = {"schema": 1, "evidence": "not_run", "root": str(root), "argv": argv,
                "expected_claude_version": EXPECTED_VERSION, "budget_usd": budget, "model": model,
                "fixture_script_sha256": hashlib.sha256((root / "fixture_hook.py").read_bytes()).hexdigest(),
                "fixture_delay_seconds": delay, "notes": [
                    "No model call is made by prepare.", "Managed settings may still apply.",
                    "Stream timestamps are arrival times, not API dispatch times.",
                    "Assistant message IDs are not API request IDs.",
                    "This is a fixture executor, not a test of production Adage grants or scheduling."]}
    path = root / "manifest.json"
    path.write_text(json.dumps(manifest, indent=2))
    return path


def explicit_request_ids(value):
    """Only literal request-id fields: never substitute message ids."""
    found = []
    if isinstance(value, dict):
        for key, item in value.items():
            if key in ("request_id", "requestId") and isinstance(item, str):
                found.append(item)
            else:
                found.extend(explicit_request_ids(item))
    elif isinstance(value, list):
        for item in value:
            found.extend(explicit_request_ids(item))
    return found


def summary(root):
    def records(name):
        path = root / name
        return [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []
    events = records("events.jsonl")
    stream = records("stream-observations.jsonl")
    ids = sorted({rid for item in stream for rid in explicit_request_ids(item.get("value"))})
    messages = sorted({item["value"]["message"]["id"] for item in stream
                       if isinstance(item.get("value"), dict) and isinstance(item["value"].get("message"), dict)
                       and item["value"]["message"].get("id")})
    results = [item["value"] for item in stream
               if isinstance(item.get("value"), dict) and item["value"].get("type") == "result"]
    result = results[-1] if results else {}
    return {"evidence": "live_attempt" if (root / "run.json").exists() else "fixture_only_or_not_run",
            "hook_events": [item["event"] for item in events if item["kind"] == "hook_enter"],
            "receipts": [item for item in events if item["kind"] == "receipt"],
            "duplicate_suppressions": sum(item["kind"] == "duplicate_suppressed" for item in events),
            "explicit_stream_request_ids": ids, "api_request_count": None,
            "observed_stream_request_id_count": len(ids),
            "assistant_message_ids": messages,
            "total_cost_usd": result.get("total_cost_usd"), "usage": result.get("usage"),
            "num_turns": result.get("num_turns"), "result_status": result.get("subtype"),
            "result_is_error": result.get("is_error"),
            "terminal_reason": result.get("terminal_reason"), "result_text": result.get("result"),
            "duration_api_ms": result.get("duration_api_ms"),
            "accounting": "reported" if result.get("total_cost_usd") is not None else "unknown_stop_before_another_run",
            "limitation": "No API-request count inferred. Inspect version-specific debug logs and raw stream; "
                          "arrival timestamps do not prove request dispatch timing. Fixture selftests are not live hook evidence."}


def export_evidence(root, destination):
    """Export fixture-only artifacts; keep broad debug logs out of the repository."""
    destination.mkdir(parents=True, exist_ok=False)
    for name in ("events.jsonl", "stdout.jsonl", "stream-observations.jsonl", "manifest.json", "run.json",
                 "settings.json", "fixture_hook.py", "effects.txt"):
        source = root / name
        if source.exists():
            shutil.copyfile(source, destination / name)
    milestones = []
    debug = root / "claude-debug.log"
    if debug.exists():
        for line in debug.read_text().splitlines():
            request = re.fullmatch(r"(\S+) \[DEBUG\] \[API REQUEST\] /v1/messages x-client-request-id=([\w-]+) source=(\w+)", line)
            context = re.fullmatch(r"(\S+) \[DEBUG\] Hook (UserPromptSubmit|PostToolBatch) .* provided additionalContext \((\d+) chars\)", line)
            if request:
                milestones.append(dict(timestamp=request[1], kind="api_request_dispatch", client_request_id=request[2], source=request[3]))
            elif context:
                milestones.append(dict(timestamp=context[1], kind="hook_context_accepted", event=context[2], characters=int(context[3])))
    (destination / "debug-milestones.json").write_text(json.dumps(milestones, indent=2))
    report = summary(root)
    report["fixture_script_sha256_at_export"] = hashlib.sha256((root / "fixture_hook.py").read_bytes()).hexdigest()
    report["observed_debug_dispatch_count"] = sum(item["kind"] == "api_request_dispatch" for item in milestones)
    report["export_note"] = "Request dispatches extracted from version-specific real diagnostics; no unsupported total billing/API-count claim."
    (destination / "summary.json").write_text(json.dumps(report, indent=2))
    return destination


def execute(manifest_path, budget, wall_seconds):
    if os.environ.get("CLAUDECODE"):
        raise ValueError("Nested Claude marker is present. Run from an ordinary human terminal; do not unset protection markers.")
    manifest = json.loads(manifest_path.read_text())
    root = Path(manifest["root"]).resolve(strict=True)
    if not (root / ".reflex-fixture").is_file():
        raise ValueError("fixture marker missing")
    if not (math.isfinite(budget) and 0 < budget <= manifest["budget_usd"]):
        raise ValueError("live budget must be positive and no greater than the prepared budget")
    if (root / "run.json").exists() or (root / "events.jsonl").exists():
        raise ValueError("use a fresh prepared fixture for each live run")
    version = subprocess.check_output(["claude", "--version"], text=True).strip()
    if version.split()[0] != manifest["expected_claude_version"]:
        raise ValueError("Claude version changed; revalidate the recipe before changing the expected version")
    argv = manifest["argv"][:]
    argv[argv.index("--max-budget-usd") + 1] = str(budget)
    settings = json.loads(Path(argv[argv.index("--settings") + 1]).read_text())
    env, plain = project_launch(launch_env(), manifest)
    names = check_launch(env, settings, plain)
    run = {"version": version, "budget_usd": budget, "start_monotonic_ns": monotonic_ns(), "argv": argv,
           "launch_environment_names": names}
    (root / "run.json").write_text(json.dumps(run, indent=2))
    lines = queue.Queue()
    with (root / "stderr.log").open("w") as err, (root / "stdout.jsonl").open("w") as raw, \
            (root / "stream-observations.jsonl").open("w") as observations:
        # The prompt is already an argv item. Inheriting a launcher's stdin (for example
        # a Python heredoc) can feed extra instructions to Claude's print-mode prompt.
        child = subprocess.Popen(argv, cwd=manifest.get("execution_root", root), env=env, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                                 stderr=err, text=True, start_new_session=True)
        def read_lines():
            for line in child.stdout:
                lines.put((monotonic_ns(), line))
            lines.put(None)
        reader = threading.Thread(target=read_lines, daemon=True)
        reader.start()
        deadline = time.monotonic() + wall_seconds
        try:
            while True:
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise TimeoutError("live probe wall-clock limit")
                item = lines.get(timeout=remaining)
                if item is None:
                    break
                timestamp, line = item
                raw.write(line)
                raw.flush()
                try:
                    value = json.loads(line)
                except json.JSONDecodeError:
                    value = {"unparsed_line": line}
                observations.write(json.dumps({"arrival_monotonic_ns": timestamp, "value": value}) + "\n")
                observations.flush()
            run["exit_code"] = child.wait(timeout=max(.1, deadline - time.monotonic()))
        except (KeyboardInterrupt, TimeoutError, queue.Empty, subprocess.TimeoutExpired) as error:
            run["cancelled"] = type(error).__name__
            try:
                os.killpg(child.pid, signal.SIGTERM)
                child.wait(timeout=3)
            except ProcessLookupError:
                child.wait()
            except subprocess.TimeoutExpired:
                try:
                    os.killpg(child.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                child.wait()
        finally:
            run["end_monotonic_ns"] = monotonic_ns()
            (root / "run.json").write_text(json.dumps(run, indent=2))
    result = summary(root)
    (root / "summary.json").write_text(json.dumps(result, indent=2))
    return result


def positive(value):
    value = float(value)
    if not math.isfinite(value) or value <= 0:
        raise argparse.ArgumentTypeError("must be finite and positive")
    return value


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    prep = sub.add_parser("prepare", help="dry run only: create isolated fixture and manifest")
    prep.add_argument("--model", required=True)
    prep.add_argument("--budget-usd", type=positive, required=True, help="proposed cap; prepare spends nothing")
    prep.add_argument("--delay", type=float, default=0)
    live = sub.add_parser("live", help="explicit paid execution, after budget approval")
    live.add_argument("manifest", type=Path)
    live.add_argument("--budget-usd", type=positive, required=True)
    live.add_argument("--wall-seconds", type=positive, default=90)
    inspect = sub.add_parser("inspect")
    inspect.add_argument("root", type=Path)
    export = sub.add_parser("export")
    export.add_argument("root", type=Path)
    export.add_argument("destination", type=Path)
    args = parser.parse_args()
    if args.command == "prepare":
        if not math.isfinite(args.delay) or args.delay < 0 or args.delay > 30:
            parser.error("delay must be 0..30 seconds")
        print(prepare(args.model, args.budget_usd, args.delay))
    elif args.command == "inspect":
        print(json.dumps(summary(args.root), indent=2))
    elif args.command == "export":
        print(export_evidence(args.root, args.destination))
    else:
        try:
            print(json.dumps(execute(args.manifest, args.budget_usd, args.wall_seconds), indent=2))
        except ValueError as error:
            parser.error(str(error))
