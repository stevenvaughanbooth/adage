#!/usr/bin/env python3
"""Post-hoc, read-only audit of environment denial in archived native sessions. Names only.

For each archived native session settings.json: the denied environment names (never values),
which environment family froze it, whether a native canary ever tested environment denial, and
whether any agent tool call in its native transcript could have read the environment.
"""
import hashlib
import json
import os
from pathlib import Path
import re
import sys

READS_ENV = re.compile(r"(^|[\s;&|(])(env|printenv|set|export)(\s|$)|os\.environ|getenv|\$\{?[A-Z_][A-Z0-9_]*|/proc/self/environ")
DUMPS_ENV = re.compile(r"(^|[;&|]\s*)(env|printenv|set|declare -x|export -p)\s*($|[;&|>])|os\.environ\s*\)|dict\(os\.environ|/proc/self/environ")
PRUNE = {"repo", ".venv", "private-score", "cache", "runtime", "bases", "adage-home", "history", "source", "bin", ".git"}


def family(names):
    if any(n.startswith("CLAUDE_CODE_") or n == "CLAUDE_PID" for n in names):
        return "claude-code-agent"
    if any(n.startswith("CODEX_") for n in names):
        return "codex-agent"
    return "terminal-or-unknown"


def tool_calls(path):
    calls = []
    if not path.exists():
        path = path.with_name("stdout.jsonl")  # Diagnostics kept the stream, not the native copy.
    if not path.exists():
        return None
    for line in path.read_text(errors="replace").splitlines():
        try:
            row = json.loads(line)
        except json.JSONDecodeError:
            continue
        message = row.get("message") if isinstance(row, dict) else None
        content = message.get("content", []) if isinstance(message, dict) else []
        for block in content if isinstance(content, list) else []:
            if block.get("type") == "tool_use" and block.get("name") == "Bash":
                calls.append(block.get("input", {}).get("command", ""))
    return calls


def audit(roots):
    rows = []
    for authority in roots:
        for directory, dirs, files in os.walk(authority):
            dirs[:] = [d for d in dirs if d not in PRUNE]
            if "settings.json" not in files:
                continue
            d = Path(directory)
            try:
                settings = json.loads((d / "settings.json").read_text())
            except (json.JSONDecodeError, OSError):
                continue
            envvars = settings.get("sandbox", {}).get("credentials", {}).get("envVars")
            if envvars is None:
                continue
            names = sorted(x["name"] for x in envvars)
            calls = tool_calls(d / "native-transcript.jsonl")
            env_reads = [c[:160] for c in calls or [] if READS_ENV.search(c)]
            rows.append(dict(session=str(d), launched=(d / "run.json").exists(), denied_names=len(names),
                denied_set=hashlib.sha256(json.dumps(names).encode()).hexdigest()[:12],
                calls_that_dump_environment=[c[:160] for c in calls or [] if DUMPS_ENV.search(c)],
                frozen_in=family(names), transcript=calls is not None, bash_calls=len(calls or []),
                calls_that_may_read_environment=env_reads,
                environment_names_referenced=sorted({n for c in calls or [] for n in re.findall(r"\$\{?([A-Za-z_][A-Za-z0-9_]*)", c)}),
                python_environment_access=sum(bool(re.search(r"os\.environ|getenv", c)) for c in calls or [])))
    launched = [r for r in rows if r["launched"]]
    return dict(kind="post-hoc environment-denial audit; names only, no values", sessions=len(rows),
        launched=len(launched), frozen_in={f: sum(r["frozen_in"] == f for r in launched) for f in
                                           ("claude-code-agent", "codex-agent", "terminal-or-unknown")},
        denied_sets={h: sum(r["denied_set"] == h for r in launched) for h in sorted({r["denied_set"] for r in launched})},
        launched_with_environment_dump=[r["session"] for r in launched if r["calls_that_dump_environment"]],
        launched_without_transcript=[r["session"] for r in launched if not r["transcript"]],
        launched_with_possible_environment_reads=[r for r in launched if r["calls_that_may_read_environment"]],
        environment_names_referenced=sorted({n for r in launched for n in r["environment_names_referenced"]}),
        python_environment_access=sum(r["python_environment_access"] for r in launched),
        native_canary_tested_environment_denial=False, rows=rows)


if __name__ == "__main__":
    print(json.dumps(audit([Path(p) for p in sys.argv[1:]]), indent=1))
