"""One isolated, tool-less `claude -p` call (prompt writing, the agents-md baseline), with its cost
appended to the spend ledger.

The call runs as the live runner's sessions do: a fresh HOME and CLAUDE_CONFIG_DIR, only the
runner's allowlisted environment (`--host-auth`: HOST_AUTH_ENV, never CLAUDECODE or a session id),
no tools, no settings sources but the empty project. It reads nothing but its prompt.
"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

import common

sys.path.insert(0, str(common.REPO / "evals" / "live"))
import runner  # noqa: E402

LEDGER = "spend.jsonl"


def ledger_path() -> Path:
    return common.cache_dir() / LEDGER


def record_spend(kind: str, usd: float, **kv) -> None:
    p = ledger_path()
    p.parent.mkdir(parents=True, exist_ok=True)
    with open(p, "a") as f:
        f.write(json.dumps({"t": time.time(), "kind": kind, "usd": usd, **kv}) + "\n")


def ask(prompt: str, model: str = "claude-sonnet-5", kind: str = "llm", max_turns: int = 1) -> tuple[str, float]:
    """The model's reply to `prompt`, and what it cost."""
    home = Path(tempfile.mkdtemp(prefix="rb-llm-", dir=str(common.cache_dir())))
    try:
        forward = [k for k in runner.HOST_AUTH_ENV]
        env = runner.isolation_env(home, dict(os.environ), forward)
        for d in runner.home_dirs(home).values():
            Path(d).mkdir(parents=True, exist_ok=True)
        cmd = ["claude", "-p", "--model", model, "--output-format", "json", "--max-turns", str(max_turns),
               "--setting-sources", "project", "--strict-mcp-config", "--tools", ""]
        out = subprocess.run(cmd, input=prompt, capture_output=True, text=True, env=env, cwd=home, timeout=900)
        try:
            res = json.loads(out.stdout)
        except ValueError:
            raise RuntimeError(f"claude -p printed no JSON: {out.stdout[-400:]} {out.stderr[-400:]}") from None
        cost = float(res.get("total_cost_usd") or 0.0)
        record_spend(kind, cost, model=model)
        if res.get("is_error"):
            raise RuntimeError(f"claude -p error: {res.get('result')}")
        return res.get("result") or "", cost
    finally:
        shutil.rmtree(home, ignore_errors=True)


def spent(kinds=None) -> float:
    p = ledger_path()
    if not p.exists():
        return 0.0
    tot = 0.0
    for line in p.read_text().splitlines():
        rec = json.loads(line)
        if kinds is None or rec["kind"] in kinds:
            tot += rec["usd"]
    return tot
