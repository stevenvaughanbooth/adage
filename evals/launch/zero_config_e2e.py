#!/usr/bin/env python3
"""Zero-config end to end with real agents: the staged marketplace plugin, a scratch clone of a
RepoBench repository, and a fake HOME whose ~/.claude/projects already holds that repository's
real earlier sessions (rewritten to the clone's path, as a user's own history would be).

  python3 evals/launch/zero_config_e2e.py setup  --repo click --history /tmp/adage-repobench/runs/click
  python3 evals/launch/zero_config_e2e.py run    --repo click --task <task-id> --n <k> --execute
  python3 evals/launch/zero_config_e2e.py status

Nothing Adage-specific is configured: no ADAGE_* variable, no `adage` command before the first
session, no `adage condition`. Claude Code loads the plugin with `--plugin-dir` (the staged
marketplace layout from scripts/stage-plugin.mjs, binary bundled under dist/). Sessions are
isolated like the live runner's (fresh environment, host-auth allowlist, --tools pinned) but not
confined: bubblewrap's PID namespace would kill the detached learners the hooks start, which is
exactly what this checks. Each run records the stream (hook events included), the Adage data
directory's learn.log, spool and ledger, and `adage` (bare) before and after.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
sys.path.insert(0, str(REPO / "evals" / "live"))
sys.path.insert(0, str(REPO / "evals" / "repobench"))
import runner  # noqa: E402

BASE = Path(os.environ.get("ZC_ROOT", "/tmp/adage-zc"))
HOME = BASE / "home"
CLONE = BASE / "clone"
PLUGIN = BASE / "stage" / "plugin" / "claude-code"
OUT = BASE / "out"
ADAGE_DATA = HOME / ".local" / "share" / "adage"


def slug(p: Path) -> str:
    return re.sub(r"[^A-Za-z0-9]", "-", str(p))


def env() -> dict:
    base = dict(os.environ)
    e = runner.isolation_env(HOME, base, runner.HOST_AUTH_ENV)
    assert not any(runner.forbidden_forward(k) and k not in runner.HOST_AUTH_ENV for k in e), "leaked marker"
    return e


def sandbox(repo: str, task: str) -> None:
    import common
    import sandbox as sb
    r, t = common.repo(repo), common.task(repo, task)
    sb.create(r, t["base"], CLONE)


def setup(a) -> int:
    HOME.mkdir(parents=True, exist_ok=True)
    (HOME / ".gitconfig").write_text("[user]\n\tname = adage-eval\n\temail = adage-eval@localhost\n")
    for d in runner.home_dirs(HOME).values():
        Path(d).mkdir(parents=True, exist_ok=True)
    proj = HOME / ".claude" / "projects" / slug(CLONE)
    proj.mkdir(parents=True, exist_ok=True)
    src = Path(a.history)
    state = json.loads((src / "state.json").read_text())
    n = 0
    for run in state["runs"].values():
        sid = (run.get("metrics") or {}).get("session_id")
        if not sid:
            continue
        hits = list((src / "private" / "transcripts").glob(f"*/{sid}.jsonl"))
        if not hits:
            continue
        old = f"{src}/runs/{run['id']}/{run.get('workdir', 'repo')}"
        text = hits[0].read_text().replace(old, str(CLONE))
        (proj / hits[0].name).write_text(text)
        n += 1
    print(f"history: {n} real sessions of {a.repo} under {proj}")
    return 0


def bare_adage(tag: str) -> str:
    # The plugin's own copy (what its hooks run: ${CLAUDE_PLUGIN_DATA}/bin/adage), once a session
    # staged it; before that, the same release build.
    staged = sorted((HOME / ".claude").glob("plugins/data/*/bin/adage"))
    bin_ = str(staged[0]) if staged else str(REPO / "target" / "release" / "adage")
    out = subprocess.run([bin_], cwd=CLONE, env=env(), capture_output=True, text=True, timeout=60)
    text = out.stdout + (("\n[stderr]\n" + out.stderr) if out.stderr.strip() else "")
    (OUT / f"adage-{tag}.txt").write_text(text)
    return text


def wait_learner(timeout=300) -> None:
    """Wait for the background learner the hooks start (its lock is held while it runs)."""
    t0 = time.time()
    time.sleep(3)
    while time.time() - t0 < timeout:
        busy = subprocess.run(["pgrep", "-f", "condition --auto"], capture_output=True, text=True).stdout.strip()
        if not busy:
            return
        time.sleep(2)


def run(a) -> int:
    if not a.execute:
        print("dry run: pass --execute")
        return 0
    OUT.mkdir(parents=True, exist_ok=True)
    tag = f"s{a.n}"
    sandbox(a.repo, a.task)
    import common
    prompt = common.task(a.repo, a.task)["prompt"]
    if a.n == 1:
        bare_adage("before")
    log_before = (ADAGE_DATA / "learn.log").read_text().splitlines() if (ADAGE_DATA / "learn.log").exists() else []
    cmd = ["claude", "-p", prompt, "--model", a.model, "--max-turns", str(a.max_turns),
           "--output-format", "stream-json", "--verbose", "--include-hook-events",
           "--plugin-dir", str(PLUGIN), "--strict-mcp-config",
           "--tools", ",".join(runner.DEFAULTS["allowed_tools"]),
           "--allowedTools", *runner.DEFAULTS["allowed_tools"],
           "--disallowedTools", *runner.DEFAULTS["disallowed_tools"]]
    t0 = time.time()
    with open(OUT / f"{tag}.stream.jsonl", "w") as f:
        subprocess.run(cmd, cwd=CLONE, env=env(), stdout=f, stderr=subprocess.DEVNULL, timeout=3600)
    elapsed = time.time() - t0
    wait_learner()
    lines = [json.loads(x) for x in (OUT / f"{tag}.stream.jsonl").read_text().splitlines() if x.startswith("{")]
    result = next((m for m in lines if m.get("type") == "result"), {})
    hooks = [m for m in lines if m.get("type") == "system" and "hook" in str(m.get("subtype", ""))]
    reflex = [h for h in hooks if "adage-reflex" in json.dumps(h)]
    notices = [h for h in hooks if "systemMessage" in json.dumps(h) or "adage:" in json.dumps(h)]
    score = subprocess.run(["python3", str(REPO / "evals/repobench/score.py"), a.repo, a.task, str(CLONE)],
                           capture_output=True, text=True)
    try:
        sc = json.loads(score.stdout)
    except ValueError:
        sc = {"error": score.stderr[-400:]}
    log_after = (ADAGE_DATA / "learn.log").read_text().splitlines() if (ADAGE_DATA / "learn.log").exists() else []
    new_log = [json.loads(x) for x in log_after[len(log_before):] if x.startswith("{")]
    card = bare_adage(tag)
    rec = {"session": a.n, "task": a.task, "model": a.model, "elapsed_s": round(elapsed, 1),
           "cost_usd": result.get("total_cost_usd"), "turns": result.get("num_turns"), "subtype": result.get("subtype"),
           "session_id": result.get("session_id"), "passed": sc.get("passed"),
           "hook_events": len(hooks), "hook_notices": [json.dumps(h)[:600] for h in notices][:6],
           "reflex_directives": [json.dumps(h)[:800] for h in reflex][:10], "n_reflex_directives": len(reflex),
           "learn_log_new": [{k: v for k, v in x.items() if k in ("trigger", "repo", "sessions_read", "changes", "error",
                                                                 "duration_ms", "kind", "stage")} for x in new_log],
           "ledger": sorted(str(p.relative_to(ADAGE_DATA)) for p in (ADAGE_DATA / "ledger").glob("*"))
           if (ADAGE_DATA / "ledger").exists() else [],
           "status_card": card}
    with open(OUT / "sessions.jsonl", "a") as f:
        f.write(json.dumps(rec) + "\n")
    print(json.dumps({k: v for k, v in rec.items() if k != "status_card"}, indent=1)[:6000])
    print(card)
    return 0


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("cmd", choices=("setup", "run"))
    ap.add_argument("--repo", default="click")
    ap.add_argument("--history")
    ap.add_argument("--task")
    ap.add_argument("--n", type=int, default=1)
    ap.add_argument("--model", default="claude-haiku-4-5-20251001")
    ap.add_argument("--max-turns", type=int, default=40)
    ap.add_argument("--execute", action="store_true")
    a = ap.parse_args(argv)
    return setup(a) if a.cmd == "setup" else run(a)


if __name__ == "__main__":
    sys.exit(main())
