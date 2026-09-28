#!/usr/bin/env python3
"""Live proving-ground eval: real `claude` agents, with and without Adage (Design §12).

  python3 evals/live/runner.py [--config] <config.json> [train] [learn] [eval] [report] [options]

With no `--execute` this is a dry run (`--dry-run` says so explicitly). It prints the plan: every
session, arm, prompt and command line, the number of agent runs per phase and round, the output
layout, and the spend cap. Nothing is executed and nothing is written. With `--execute` it runs the
plan. Every session is a real headless Claude Code agent (`claude -p`), so this spends real tokens,
and each session gets a fresh sandbox and its own empty home directory:

  train   baseline agents on the training tasks, with no Adage hooks installed (round 1 only)
  learn   `adage learn` over the training transcripts (and, in later rounds, earlier eval sessions
          of tasks outside this round's held-out pool)
  eval    held-out tasks; each (task, repeat) runs every arm, in an order drawn from the seed:
            adage    hooks installed, directives delivered
            holdout  hooks installed, directives computed and logged but not delivered (the control)
            off      no hooks at all (to measure hook overhead, if wanted)
  report  summary.json (per-arm aggregates, task-level permutation p-values with Holm adjustment,
          task-bootstrap CIs, per round) and a results page like demo/results:
          <root>/results/{index.html, summary.json, state.json}

Rounds: the config's `rounds` (default 1) are run in order: round 1 is train, learn, eval, report;
each later round learns again, then runs its held-out tasks. With `eval.tasks_by_round` every round
has a fresh held-out pool and never learns from sessions of its own tasks; without it every round
reuses `eval.tasks`, learns from earlier sessions of those same tasks, and is marked `leaky`.
`--round N` runs only round N.

The output root (`--root`, default /tmp/adage-live/<config name>):
runs/<run>/{<workdir>/, home/, stream.jsonl} is what an agent can see; private/{adage-home/,
transcripts/<project>/<session>.jsonl, outcomes.json} is what it must not (the learned index and
every other session). state.json and summary.json sit at the root. After the last session, the root
also gets adage-home, transcripts and outcomes.json links into private/, so it has the layout of a
demo archive: `python3 evals/datasets/tools/build_demo.py <root> <dataset> --name ... --repo ...`
turns a live run into an offline `adage eval` dataset. The live dashboard reads the same state:
`DEMO_ROOT=<root> ADAGE_HOME=<root>/private/adage-home python3 demo/dashboard/serve.py`.

Options:
  --config PATH        the config (or give it as the first positional argument)
  --dry-run            print the plan and exit (the default without --execute)
  --execute            run for real (spends tokens); refused inside an agent session (CLAUDECODE)
  --root DIR           output root
  --model ID           override the config's model
  --round N            run only this learning round (default: every round in the config)
  --seed N             seed for the arm order (default: the config's `seed`, else the root's
                       state.json, else a fresh one; always recorded in state.json)
  --no-isolate         run agents with your own HOME, ~/.claude and environment (not
                       recommended: auto-memory and earlier transcripts leak into the control arm)
  --host-auth          forward the variables of a host-managed login to isolated sessions (HOST_AUTH_ENV:
                       the ingress token file, organisation, account, base URL, proxies and CA
                       bundles; never CLAUDECODE or a session id). Same as `"host_auth": true`
  --json               print the plan as JSON (dry run)
  --adage PATH         the adage binary (default: target/release/adage in this repository)

Config: JSON. See evals/live/configs/ and evals/live/README.md.
"""

import fnmatch
import json
import re
import os
import random
import secrets
import shutil
import subprocess
import sys
import threading
import time
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
sys.path.insert(0, str(REPO / "demo"))
import report as demo_report  # noqa: E402 - demo/report.py: statuses, statistics, results page

EDIT_TOOLS = {"Edit", "Write", "MultiEdit", "NotebookEdit"}
EXPLORE_TOOLS = {"Read", "Glob", "Grep", "LS"}
EXPLORE_COMMANDS = ("ls", "find", "cat", "grep", "rg", "head", "tail", "tree", "sed -n", "wc")
ARMS = ("adage", "holdout", "off", "instincts", "e1")
SPOOL_MAGIC = bytes([0xAD, 0xA6, 0xE5, 0x01])
DEFAULTS = {
    "model": "claude-haiku-4-5-20251001",
    "max_turns": 30,
    "concurrency": 3,
    "rounds": 1,
    "learn_args": ["--demo"],
    "allowed_tools": ["Bash", "Read", "Edit", "Write", "Glob", "Grep", "LS", "MultiEdit"],
    "disallowed_tools": ["WebFetch", "WebSearch", "Agent", "Task"],
    "budget_usd": 10.0,
    "workdir": "repo",
    # Extra environment variables passed to isolated sessions (e.g. Bedrock or Vertex settings).
    "forward_env": [],
    # Forward HOST_AUTH_ENV too (`--host-auth`): for hosts that provide Claude Code's login.
    "host_auth": False,
    # Run each session under bubblewrap (`confine_argv`): see only its run directory, its ADAGE_HOME
    # and what it needs. `confine_hide`, `confine_expose` adjust what is hidden and mounted back.
    "confine": False,
    # How tool calls are categorised for the timeline and the metrics (substrings of a Bash
    # command). The defaults describe the acme-billing demo; other sandboxes set their own.
    "classify": {
        "lookup": ["wi-lookup"],
        "regen": ["make gen", "gen.py"],
        "verify": ["unittest", "make test", "make ci", "pytest"],
        "bash_edit": ["sed -i", "cat >", "cat <<", "tee ", "write_text(", "> acme/", "> schema/", ">acme/"],
        "check_flags": ["--check", "--verify"],
    },
    # The finish-line obligation: edits to these paths must be followed by the regenerate step.
    "obligation": {"paths": "schema/*.json"},
    "labels": {"lookup": "work-item lookup", "regen": "make gen"},
}

# An isolated session sees only these variables of the operator's environment, plus the auth
# variables below and the config's `forward_env`. HOME and every config and data directory point
# into the run's own home, so the agent cannot read the operator's Claude Code memory (MEMORY.md),
# earlier transcripts (~/.claude/projects) or settings.
PASS_ENV = ("PATH", "LANG", "LANGUAGE", "LC_ALL", "LC_CTYPE", "TERM", "SHELL", "USER", "LOGNAME", "TZ",
            "HTTPS_PROXY", "HTTP_PROXY", "NO_PROXY", "https_proxy", "http_proxy", "no_proxy",
            "SSL_CERT_FILE", "SSL_CERT_DIR", "NODE_EXTRA_CA_CERTS", "REQUESTS_CA_BUNDLE")
# How Claude Code authenticates without the operator's config directory: one of these variables
# (an API key, or a token from `claude setup-token`), else a copy of the operator's
# ~/.claude/.credentials.json (Linux; macOS keeps it in the Keychain, so set a variable there).
AUTH_ENV = ("ANTHROPIC_API_KEY", "CLAUDE_CODE_OAUTH_TOKEN", "ANTHROPIC_AUTH_TOKEN")
AUTH_PASS = AUTH_ENV + ("ANTHROPIC_BASE_URL",)
# `--host-auth` (or `"host_auth": true`): the allowlist a host-managed Claude Code login needs when
# the host, not a credentials file, provides auth (Claude Code on the web and similar containers:
# a session-ingress token file and the organisation and account it belongs to). Exactly these, and
# only when set. Never the host session's own identity (CLAUDECODE, CLAUDE_CODE_SESSION_ID, ...):
# those would attach every child session to the operator's session.
HOST_AUTH_ENV = ("ANTHROPIC_BASE_URL", "CLAUDE_CODE_PROVIDER_MANAGED_BY_HOST", "CLAUDE_SESSION_INGRESS_TOKEN_FILE",
                 "CLAUDE_CODE_OAUTH_TOKEN", "CLAUDE_CODE_ORGANIZATION_UUID", "CLAUDE_CODE_ACCOUNT_UUID",
                 "HTTPS_PROXY", "HTTP_PROXY", "NO_PROXY", "https_proxy", "http_proxy", "no_proxy",
                 "SSL_CERT_FILE", "NODE_EXTRA_CA_CERTS", "REQUESTS_CA_BUNDLE")
# Of those, the ones that actually carry a credential.
HOST_AUTH_TOKENS = ("CLAUDE_SESSION_INGRESS_TOKEN_FILE", "CLAUDE_CODE_OAUTH_TOKEN")


def forbidden_forward(name):
    """Variables `forward_env` must never pass: an agent session's own markers and identity (a
    child session carrying them attaches to, or reports into, the operator's session) and Adage's
    own settings (the runner sets those per arm)."""
    return (name == "CLAUDECODE" or name.startswith("ADAGE_") or "SESSION_ID" in name
            or name in ("CLAUDE_CODE_CHILD_SESSION", "CLAUDE_CODE_ENTRYPOINT", "CLAUDE_PID")
            or name.startswith("CLAUDE_CODE_MESSAGING_") or name.startswith("CLAUDE_CODE_REMOTE"))


# ------------------------------------------------------------------------------------- config

def load_config(path, overrides):
    cfg = dict(DEFAULTS)
    raw = json.loads(Path(path).read_text())
    for k in ("classify", "labels"):
        if k in raw:
            raw[k] = {**DEFAULTS[k], **raw[k]}
    cfg.update(raw)
    cfg.update({k: v for k, v in overrides.items() if v is not None})
    missing = [k for k in ("name", "repo_key", "sandbox", "score", "prompts", "train", "eval") if k not in cfg]
    if missing:
        raise SystemExit(f"config {path}: missing {', '.join(missing)}")
    for arm in cfg["eval"].get("arms", ["holdout", "adage"]):
        if arm not in ARMS:
            raise SystemExit(f"config {path}: unknown arm {arm!r} (one of {', '.join(ARMS)})")
    if not isinstance(cfg["rounds"], int) or cfg["rounds"] < 1:
        raise SystemExit(f"config {path}: rounds must be a positive integer")
    if cfg.get("host_auth"):
        cfg["forward_env"] = list(dict.fromkeys([*(cfg.get("forward_env") or []), *HOST_AUTH_ENV]))
    bad = [k for k in cfg.get("forward_env") or [] if forbidden_forward(k)]
    if bad:
        raise SystemExit(f"config {path}: forward_env may not pass {', '.join(bad)} (an agent session's own "
                         "markers, or Adage's settings)")
    tbr = cfg["eval"].get("tasks_by_round")
    if tbr is None and "tasks" not in cfg["eval"]:
        raise SystemExit(f"config {path}: eval needs tasks or tasks_by_round")
    pools = []
    if tbr is not None:
        for r in range(1, cfg["rounds"] + 1):
            pool = tasks_for_round(cfg, r)
            if not pool:
                raise SystemExit(f"config {path}: eval.tasks_by_round has no tasks for round {r}")
            pools.append(set(pool))
        for i, p in enumerate(pools):
            for j, q in enumerate(pools[:i]):
                if p & q:
                    raise SystemExit(f"config {path}: rounds {j + 1} and {i + 1} share held-out tasks "
                                     f"{sorted(p & q)}; each round needs a fresh pool")
    else:
        pools.append(set(cfg["eval"]["tasks"]))
    both = set(cfg["train"]["tasks"]) & set().union(*pools)
    if both:
        raise SystemExit(f"config {path}: tasks both trained on and held out: {sorted(both)}")
    return cfg


def expand(argv, **kv):
    """Fill `{repo}`, `{dir}`, `{task}` placeholders in a command given as an argv list."""
    kv.setdefault("repo", str(REPO))
    return [a.format(**kv) for a in argv]


def fresh_pools(cfg):
    """Does every round have its own held-out tasks (`eval.tasks_by_round`)?"""
    return cfg["eval"].get("tasks_by_round") is not None


def tasks_for_round(cfg, round_):
    """The held-out tasks of one round: `eval.tasks_by_round` (a list, one pool per round, or a
    dict keyed by round number), else `eval.tasks` for every round."""
    tbr = cfg["eval"].get("tasks_by_round")
    if tbr is None:
        return list(cfg["eval"]["tasks"])
    try:
        return list(tbr[str(round_)] if isinstance(tbr, dict) else tbr[round_ - 1])
    except (KeyError, IndexError):
        raise SystemExit(f"eval.tasks_by_round has no pool for round {round_}") from None


# --------------------------------------------------------------------------------------- plan

def arm_order(arms, seed, round_, task, repeat):
    """The order the arms of one (task, repeat) run in: a shuffle seeded by (seed, round, task,
    repeat), so it is random across pairs and the same on every machine and rerun."""
    order = list(arms)
    random.Random(f"{seed}/{round_}/{task}/{repeat}").shuffle(order)
    return order


def plan(cfg, phases, round_):
    """Every session one round would run, in order: (run id, phase, arm, task, prompt)."""
    out = []
    prompts = cfg["prompts"]
    if "train" in phases and round_ == 1:
        t = cfg["train"]
        for r in range(t.get("repeats", 1)):
            for i, task in enumerate(t["tasks"]):
                out.append((f"train-{task}-r{r + 1}", "train", "train", task, prompts[(i + r) % len(prompts)]))
    if "eval" in phases:
        e = cfg["eval"]
        tag = "eval" if round_ == 1 else f"eval{round_}"
        for r in range(e.get("repeats", 1)):
            for i, task in enumerate(tasks_for_round(cfg, round_)):
                prompt = prompts[(i + r) % len(prompts)]
                for arm in arm_order(e.get("arms", ["holdout", "adage"]), cfg.get("seed") or 0, round_, task, r + 1):
                    out.append((f"{tag}-{task}-r{r + 1}-{arm}", "eval", arm, task, prompt))
    return out


def claude_command(cfg, prompt, settings):
    return [
        "claude", "-p", prompt,
        "--model", cfg["model"],
        "--max-turns", str(cfg["max_turns"]),
        "--output-format", "stream-json", "--verbose", "--include-hook-events",
        "--setting-sources", "project,local",
        "--strict-mcp-config",
        "--settings", str(settings),
        # The tool set itself, not only what is pre-approved: newer harnesses offer more built-in
        # tools (TaskGet/TaskList, Cron*, ToolSearch, ...), and an agent asked to "pick up W-1042"
        # looked for it with TaskGet, found no such task and asked what to do. `tools` (default:
        # the allowed tools) keeps every session, of every harness version, on the same tools.
        "--tools", ",".join(cfg.get("tools") or cfg["allowed_tools"]),
        "--allowedTools", *cfg["allowed_tools"],
        "--disallowedTools", *cfg["disallowed_tools"],
    ]


def instincts_home(home):
    """The `instincts` arm's ADAGE_HOME, next to the learned one: Adage's built-in defaults bound
    from the repository's files alone (`adage learn --instincts-only`), no learned reflex."""
    return Path(home).parent / "instincts-home"


def e1_home(home):
    """The `e1` arm's ADAGE_HOME: a copy of the learned home in which a person approved E1
    prefetches (`adage review allow-exec ... --level e1`, which needs a terminal). Its hooks run
    `adage-exec`, not `adage`. The runner never approves anything itself."""
    return Path(home).parent / "e1-home"


def settings_kind(arm):
    """Which settings file a session loads: no hooks, Adage's hooks, or adage-exec's (E1)."""
    return "exec" if arm == "e1" else "hooks" if env_for(arm, "/x") else "none"


def env_for(arm, home):
    """Adage's environment overrides for one session. Training and `off` sessions have no hooks at
    all. The `instincts` arm is treated, reading the day-0 index (instincts only). Automatic
    learning is off: the rounds decide when learning happens, so the index stays fixed within a
    round."""
    if arm in ("train", "off"):
        return {}
    if arm == "instincts":
        return {"ADAGE_HOME": str(instincts_home(home)), "ADAGE_ARM": "treated", "ADAGE_AUTO_LEARN": "0"}
    if arm == "e1":
        return {"ADAGE_HOME": str(e1_home(home)), "ADAGE_ARM": "treated", "ADAGE_AUTO_LEARN": "0"}
    return {"ADAGE_HOME": str(home), "ADAGE_ARM": "holdout" if arm == "holdout" else "treated",
            "ADAGE_AUTO_LEARN": "0"}


def home_dirs(run_home):
    """Where an isolated session's HOME, Claude Code config directory (CLAUDE_CONFIG_DIR: its
    transcripts, auto-memory and settings), XDG directories, CODEX_HOME and TMPDIR point."""
    h = Path(run_home)
    return {
        "HOME": str(h), "CLAUDE_CONFIG_DIR": str(h / ".claude"),
        "XDG_CONFIG_HOME": str(h / ".config"), "XDG_DATA_HOME": str(h / ".local" / "share"),
        "XDG_STATE_HOME": str(h / ".local" / "state"), "XDG_CACHE_HOME": str(h / ".cache"),
        "CODEX_HOME": str(h / ".codex"), "TMPDIR": str(h / "tmp"),
    }


def isolation_env(run_home, base, forward=()):
    """The whole environment of an isolated session: the run's own home directories, and from
    `base` only PASS_ENV, the auth variables and `forward`. Nothing else of the operator's (no
    CLAUDE_CONFIG_DIR, no CLAUDECODE, no ADAGE_*) gets through."""
    env = {k: base[k] for k in (*PASS_ENV, *AUTH_PASS, *forward) if k in base}
    env.update(home_dirs(run_home))
    env["DISABLE_AUTOUPDATER"] = "1"  # the harness version must not change mid-experiment
    return env


# `confine`: run each session in a bubblewrap mount namespace in which the hidden directories are
# empty and only what the session needs is mounted back. Without it an agent can walk out of its
# sandbox: real Haiku sessions ran `find /tmp -name "*hidden*"`, read the hidden test of their own
# task, and read the runner's state.json (every other session's commands and prompts).
CONFINE_HIDE = ("/tmp", "/home", "/root", "/var/tmp", "/mnt", "/workspace")
# Environment variables whose values are files a session needs (CA bundles, the host's token file).
CONFINE_FILE_ENV = ("SSL_CERT_FILE", "NODE_EXTRA_CA_CERTS", "REQUESTS_CA_BUNDLE", "CLAUDE_SESSION_INGRESS_TOKEN_FILE",
                    "SSL_CERT_DIR")


def under(path, dirs):
    p = str(path)
    return any(p == d or p.startswith(d.rstrip("/") + "/") for d in dirs)


def confine_argv(cfg, run_dir, env, ro=(), rw=()):
    """The bubblewrap prefix for one session: the whole filesystem read-only, the directories in
    `confine_hide` (default CONFINE_HIDE) empty, then mounted back: the run directory (read-write:
    the sandbox and the session's own home), `rw` (the arm's ADAGE_HOME), and read-only `ro` (the
    settings file and the hook binaries), the files named by CONFINE_FILE_ENV, the PATH entries
    under a hidden directory, and the config's `confine_expose`. A fresh /proc (own PID namespace)
    and /dev. The network is shared."""
    hide = list(cfg.get("confine_hide") or CONFINE_HIDE)
    argv = ["bwrap", "--die-with-parent", "--unshare-pid", "--ro-bind", "/", "/", "--proc", "/proc", "--dev", "/dev"]
    for d in hide:
        if Path(d).is_dir():
            argv += ["--tmpfs", d]
    seen = set()

    def mount(kind, path):
        path = str(path)
        if path in seen or not Path(path).exists() or not under(path, hide):
            return
        seen.add(path)
        argv.extend([kind, path, path])

    for x in rw:
        mount("--bind", x)
    mount("--bind", run_dir)
    # Their directories, not the files: a bind mount pins the file's inode, and the host replaces
    # its token file by rename (a session confined with the file alone was "Not logged in").
    ro = list(ro) + [str(Path(env[k]).parent) for k in CONFINE_FILE_ENV if env.get(k)]
    ro += [x for x in (env.get("PATH") or "").split(":") if x]
    ro += list(cfg.get("confine_expose") or [])
    for x in ro:
        mount("--ro-bind", x)
    return argv


def session_env(arm, run_home, adage_home, isolate, base, forward=()):
    """The environment one session runs with: isolated (the default) or the operator's own."""
    env = isolation_env(run_home, base, forward) if isolate else dict(base)
    env.update(env_for(arm, adage_home))
    return env


def operator_config_dir(base=None):
    base = os.environ if base is None else base
    return Path(base.get("CLAUDE_CONFIG_DIR") or Path(base.get("HOME") or Path.home()) / ".claude")


def auth_source(base, forward=(), host=False):
    """How isolated sessions will authenticate: with `host_auth`, ("host", [names]) when a host token
    variable is set (no credentials file is ever copied then); else ("env", [names]) when an auth variable is set,
    ("file", path) for the operator's credentials file (copied into each run's config directory
    for the session and removed after it), ("forward", [names]) when only `forward_env` variables
    are set (Bedrock, Vertex: trusted as given), or None."""
    if host:
        tok = [k for k in HOST_AUTH_TOKENS if base.get(k)]
        return ("host", tok) if tok else None
    names = [k for k in AUTH_ENV if base.get(k)]
    if names:
        return "env", names
    creds = operator_config_dir(base) / ".credentials.json"
    if creds.is_file():
        return "file", str(creds)
    fwd = [k for k in forward if base.get(k)]
    return ("forward", fwd) if fwd else None


def rounds_for(cfg, round_):
    return [round_] if round_ else list(range(1, cfg["rounds"] + 1))


def estimate(cfg, phases, rounds):
    """How many agent runs the plan makes, as the product that produces the number."""
    t, e = cfg["train"], cfg["eval"]
    arms = e.get("arms", ["holdout", "adage"])
    train_runs = len(t["tasks"]) * t.get("repeats", 1) if ("train" in phases and 1 in rounds) else 0
    eval_rounds = rounds if "eval" in phases else []
    sizes = [len(tasks_for_round(cfg, r)) for r in (eval_rounds or rounds[:1])]
    eval_runs = sum(sizes) * e.get("repeats", 1) * len(arms) if eval_rounds else 0
    return {
        "train": {"tasks": len(t["tasks"]) if train_runs else 0, "repeats": t.get("repeats", 1), "runs": train_runs},
        "eval": {"tasks": sizes[0] if len(set(sizes)) == 1 else sizes, "repeats": e.get("repeats", 1), "arms": arms,
                 "rounds": len(eval_rounds), "runs": eval_runs, "fresh_pools": fresh_pools(cfg)},
        "total_runs": train_runs + eval_runs,
    }


def leaky_rounds(cfg, phases, rounds):
    """Rounds whose learning would read earlier sessions of their own held-out tasks."""
    return [r for r in rounds if r > 1 and "learn" in phases and not fresh_pools(cfg)]


def describe(cfg, phases, rounds, root, adage, isolate=True):
    private = root / "private"
    rows = []
    for round_ in rounds:
        for run_id, phase, arm, task, prompt in plan(cfg, phases, round_):
            text = prompt.format(id=task)
            workdir = root / "runs" / run_id / cfg["workdir"]
            rows.append({
                "run": run_id, "round": round_, "phase": phase, "arm": arm, "task": task, "prompt": text,
                "cwd": str(workdir),
                "home": str(root / "runs" / run_id / "home") if isolate else None,
                "sandbox": expand(cfg["sandbox"], dir=str(workdir), task=task),
                "command": claude_command(cfg, text, private / f"settings-{settings_kind(arm)}.json"),
                "env": env_for(arm, private / "adage-home"),
                "score": expand(cfg["score"], dir=str(workdir), task=task),
            })
    learn = None
    if "learn" in phases:
        learn = [str(adage), "learn", *cfg["learn_args"], "--json", "--repo", cfg["repo_key"],
                 "--outcomes", str(private / "outcomes.json"), "--transcripts", "<each learnable earlier session's transcript>"]
    est = estimate(cfg, phases, rounds)
    assert est["total_runs"] == len(rows), (est, len(rows))
    return {
        "config": cfg["name"], "model": cfg["model"], "rounds": rounds, "round": rounds[0] if len(rounds) == 1 else None,
        "phases": phases, "root": str(root), "adage": str(adage), "sessions": rows, "learn": learn,
        "learn_rounds": rounds if learn else [],
        "seed": cfg.get("seed"),
        "pools": {str(r): tasks_for_round(cfg, r) for r in rounds},
        "leaky_rounds": leaky_rounds(cfg, phases, rounds),
        "isolation": {
            "enabled": isolate,
            "per_run": sorted(home_dirs("")) if isolate else [],
            "passed_through": list(PASS_ENV + AUTH_PASS) + list(cfg.get("forward_env") or []) if isolate else "everything",
        },
        "estimate": est,
        "totals": {
            "sessions": len(rows),
            "by_arm": {a: sum(1 for r in rows if r["arm"] == a) for a in sorted({r["arm"] for r in rows})},
            "by_round": {str(rd): sum(1 for r in rows if r["round"] == rd) for rd in rounds},
            "max_turns_each": cfg["max_turns"], "concurrency": cfg["concurrency"],
            "budget_usd": cfg["budget_usd"],
        },
    }


def estimate_line(est):
    parts = []
    if est["train"]["runs"]:
        parts.append(f"train {est['train']['tasks']} tasks x {est['train']['repeats']} repeats = {est['train']['runs']}")
    if est["eval"]["runs"]:
        e = est["eval"]
        if isinstance(e["tasks"], list):
            parts.append(f"eval ({' + '.join(map(str, e['tasks']))}) tasks over {e['rounds']} rounds x {e['repeats']} repeats"
                         f" x {len(e['arms'])} arms ({', '.join(e['arms'])}) = {e['runs']}")
        else:
            parts.append(f"eval {e['tasks']} tasks x {e['repeats']} repeats x {len(e['arms'])} arms ({', '.join(e['arms'])})"
                         f" x {e['rounds']} round{'s' if e['rounds'] != 1 else ''} = {e['runs']}")
    return "; ".join(parts) + f"; total {est['total_runs']} agent runs"


def leak_warning(rounds):
    return ("!" * 100 + f"\nWARNING: round(s) {', '.join(map(str, rounds))} learn from earlier sessions of the SAME held-out "
            "tasks they are then evaluated on.\nTheir results are marked `leaky: true` and are biased toward Adage. "
            "Give the config eval.tasks_by_round (a fresh pool per round).\n" + "!" * 100)


def print_plan(p):
    t = p["totals"]
    print(f"DRY RUN: nothing is executed. Add --execute to run {t['sessions']} real `claude` sessions.")
    print(f"config {p['config']}  model {p['model']}  rounds {' '.join(map(str, p['rounds']))}  phases {' '.join(p['phases'])}")
    print(f"root {p['root']}  (runs/, private/{{adage-home,transcripts}}, state.json, summary.json, results/)")
    print(f"runs: {estimate_line(p['estimate'])}")
    print(f"sessions {t['sessions']} by arm {t['by_arm']} by round {t['by_round']}  concurrency {t['concurrency']}  "
          f"max {t['max_turns_each']} turns each  spend cap ${t['budget_usd']}")
    print(f"arm order: shuffled per (task, repeat) with seed {p['seed']} (pass --seed {p['seed']} to run this exact order)")
    iso = p["isolation"]
    print("isolation: " + (f"each session gets its own {', '.join(iso['per_run'])} under runs/<run>/home; "
                           f"only {len(iso['passed_through'])} environment variables pass through"
                           if iso["enabled"] else "OFF (--no-isolate): sessions share your HOME, ~/.claude and environment"))
    if p["leaky_rounds"]:
        print(leak_warning(p["leaky_rounds"]))
    for rd in p["rounds"]:
        steps = [ph for ph in ("train", "learn", "eval", "report") if ph in p["phases"] and not (ph == "train" and rd != 1)]
        print(f"\nround {rd}: {' -> '.join(steps) or '(nothing)'}"
              + ("   (learns from the training sessions)" if rd == 1 and "learn" in steps else
                 "   (learns from training and earlier eval sessions of other tasks)" if "learn" in steps and rd not in p["leaky_rounds"] else
                 "   (learns from every earlier session: LEAKY)" if "learn" in steps else ""))
        for r in (s for s in p["sessions"] if s["round"] == rd):
            env = " ".join(f"{k}={v}" for k, v in r["env"].items()) or "(no hooks)"
            print(f"  {r['run']:<30} {r['arm']:<8} {r['prompt']!r:<44} {env}")
    if p["sessions"]:
        r = p["sessions"][0]
        print("\nper session, e.g. the first:")
        print("  sandbox: " + " ".join(r["sandbox"]))
        print("  agent:   " + " ".join(repr(a) if " " in a else a for a in r["command"]))
        print("  score:   " + " ".join(r["score"]))
    if p["learn"]:
        print("learn:     " + " ".join(p["learn"]))


# ----------------------------------------------------------------------------- classification

def classify(cfg, name, inp, failed=False):
    """A coarse category for the timeline and the metrics."""
    c = cfg["classify"]
    if name in EDIT_TOOLS:
        return "edit"
    cmd = (inp or {}).get("command", "") if isinstance(inp, dict) else ""
    if name == "Bash":
        if any(s in cmd for s in c["bash_edit"]):
            return "edit"
        if any(s in cmd for s in c["lookup"]):
            return "lookup"
        checking = any(f in cmd for f in c.get("check_flags", []))
        if any(s in cmd for s in c["regen"]) and not checking:
            return "regen"
        if any(s in cmd for s in c["verify"]) or (checking and any(s in cmd for s in c["regen"])):
            return "verify-fail" if failed else "verify"
        if any(cmd.lstrip().startswith(x) for x in EXPLORE_COMMANDS):
            return "explore"
        return "exec"
    if name in EXPLORE_TOOLS:
        return "explore"
    return "other"


def touches_obligation(cfg, ev, workdir):
    """Does this edit event change a file the finish-line obligation covers?"""
    pat = (cfg.get("obligation") or {}).get("paths")
    if not pat or ev["cat"] != "edit":
        return False
    target = ev.get("cmd") or ""
    if ev["tool"] in EDIT_TOOLS:
        try:
            rel = str(Path(target).resolve().relative_to(Path(workdir).resolve()))
        except ValueError:
            rel = target
        return fnmatch.fnmatch(rel, pat)
    d, _, ext = pat.rpartition("/")
    return (not d or f"{d}/" in target) and ext.replace("*", "") in target


def spool_records(path):
    data = path.read_bytes()
    i, out = 0, []
    while True:
        j = data.find(SPOOL_MAGIC, i)
        if j < 0 or j + 12 > len(data):
            break
        n = int.from_bytes(data[j + 4:j + 8], "little")
        try:
            out.append(json.loads(data[j + 12:j + 12 + n]))
        except ValueError:
            pass
        i = j + 12 + n
    return out


def spool_directives(home, session_id):
    """What Adage emitted (or would have) in this session, and its judgements, from its spool."""
    out = []
    p = home / "sessions" / f"{session_id}.spool"
    if not session_id or not p.exists():
        return out
    for rec in spool_records(p):
        d = rec.get("decision", {})
        for k in ("directives", "would_have"):
            for x in d.get(k, []):
                out.append({"ts": rec.get("ts"), "event": rec.get("event"), "kind": k, "route": x.get("route"),
                            "aspect": x.get("aspect"), "text": x.get("text", "")[:600]})
        for jd in d.get("judgements", []):
            out.append({"ts": rec.get("ts"), "event": rec.get("event"), "kind": jd.get("kind"), "route": jd.get("route")})
        if d.get("continue_reason"):
            out.append({"ts": rec.get("ts"), "event": rec.get("event"), "kind": "stop_request", "text": d["continue_reason"]})
    return out


def follow_counts(spool):
    """Routes the session was exposed to (would-have), and how many of them it followed."""
    exposed = {d["route"] for d in spool if d["kind"] == "would_have" and d.get("route")}
    followed = {d["route"] for d in spool if d["kind"] in ("followed", "obligation_met") and d.get("route")} & exposed
    derailed = {d["route"] for d in spool if d["kind"] == "derailed" and d.get("route")}
    return {"exposed": len(exposed), "followed": len(followed), "derailed": len(derailed)}


def shell_segments(cmd):
    """Split a shell command on `&&`, `||`, `;` and newlines outside quotes (a sed script such as
    `'4a\\<newline>row'` keeps its newline). Coarse, for metrics only."""
    segs, cur, quote, i = [], [], None, 0
    while i < len(cmd):
        ch = cmd[i]
        if quote:
            if ch == quote:
                quote = None
            elif ch == "\\" and quote == '"' and i + 1 < len(cmd):
                cur.append(ch)
                i += 1
                ch = cmd[i]
        elif ch in "'\"":
            quote = ch
        elif cmd.startswith("&&", i) or cmd.startswith("||", i):
            segs.append("".join(cur))
            cur = []
            i += 2
            continue
        elif ch in ";\n":
            segs.append("".join(cur))
            cur = []
            i += 1
            continue
        cur.append(ch)
        i += 1
    segs.append("".join(cur))
    return [x.strip() for x in segs if x.strip()]


def regen_after_obligation(cfg, events, workdir):
    """Did a regenerate step run after the last edit the finish-line obligation covers?

    A single shell command may do both: `sed -i … registry/x.csv && npm run codegen && npm run check`
    is one event, classified "edit", yet the regeneration follows the edit. So a Bash event's command
    is read segment by segment, in order."""
    ob = [i for i, e in enumerate(events) if touches_obligation(cfg, e, workdir)]
    if not ob:
        return False
    last = events[ob[-1]]
    c = cfg["classify"]
    if last["tool"] == "Bash" and not last.get("failed"):
        segs = shell_segments(last.get("cmd") or "")
        d, _, _ = ((cfg.get("obligation") or {}).get("paths") or "").rpartition("/")
        # The last segment that writes the covered files (not one that merely reads them, like the
        # `cat registry/x.csv` that often follows the edit).
        writes = lambda x: d and f"{d}/" in x and (any(b in x for b in c["bash_edit"]) or re.search(rf">>?\s*{re.escape(d)}/", x))
        edit_at = max((i for i, x in enumerate(segs) if writes(x)), default=None)
        if edit_at is not None:
            for x in segs[edit_at + 1:]:
                if any(r in x for r in c["regen"]) and not any(f in x for f in c.get("check_flags", [])):
                    return True
    return any(e["cat"] == "regen" and not e.get("failed") for e in events[ob[-1] + 1:])


def run_metrics(cfg, events, result, score, workdir, elapsed, sid, spool):
    first_edit = next((e for e in events if e["cat"] == "edit"), None)
    before = [e for e in events if not first_edit or e["t"] < first_edit["t"]]
    lookup = next((e for e in events if e["cat"] == "lookup" and not e.get("failed")), None)
    ob_edits = [i for i, e in enumerate(events) if touches_obligation(cfg, e, workdir)]
    ob_applies = bool(ob_edits)
    regen_after = ob_applies and regen_after_obligation(cfg, events, workdir)
    stale = bool(score.get("stale_generated"))
    passed = bool(score.get("passed"))
    return {
        "session_id": sid, "elapsed": elapsed,
        "turns": result.get("num_turns") or max((e["turn"] for e in events), default=0),
        "tool_turns": max((e["turn"] for e in events), default=0), "tool_calls": len(events),
        "turns_to_first_edit": first_edit["turn"] - 1 if first_edit else None,
        "time_to_first_edit": first_edit["t"] if first_edit else None,
        "explore_calls_before_edit": sum(1 for e in before if e["cat"] == "explore"),
        "lookup_turn": lookup["turn"] if lookup else None,
        "dead_end_hits": sum(1 for e in events if e["cat"] == "verify-fail"),
        "ran_regen": any(e["cat"] == "regen" for e in events),
        "obligation_applies": ob_applies,
        "obligation_met": (regen_after and not stale) if ob_applies else None,
        "cost_usd": result.get("total_cost_usd") or 0.0,
        "passed": passed, "stale_generated": stale,
        "ci_passed": bool(score.get("ci_passed", passed)), "hidden_passed": bool(score.get("hidden_passed", passed)),
        "result_subtype": result.get("subtype"),
        "adage": spool, "follow": follow_counts(spool),
    }


# ------------------------------------------------------------------------------------ execute

ARCHIVE = ("adage-home", "transcripts", "outcomes.json")


def learning_runs(runs, round_, pool=None):
    """The finished runs round `round_` learns from: in round 1 the training sessions; in later
    rounds also every earlier round's eval sessions. With a held-out `pool` (the round's tasks,
    under eval.tasks_by_round), no session of a task in it is used: a round never learns from the
    tasks it is scored on. Without one (None) nothing is filtered, and the round is leaky."""
    out = []
    for r in runs:
        if not (r.get("metrics") or {}).get("session_id"):
            continue
        if not ((r.get("phase") == "train" or round_ > 1) and r.get("round", 1) < max(round_, 2)):
            continue
        if pool is not None and r.get("task") in pool:
            continue
        out.append(r)
    return out


def link_archive(root):
    """Links at the root to private/ so the root has a demo archive's layout (build_demo.py,
    the dashboard). Made only when no session is running."""
    for name in ARCHIVE:
        link, target = root / name, root / "private" / name
        if target.exists() and not link.exists() and not link.is_symlink():
            link.symlink_to(Path("private") / name)


def unlink_archive(root):
    for name in ARCHIVE:
        if (root / name).is_symlink():
            (root / name).unlink()


def harness_version(env=None):
    """`claude --version`, recorded so results say which harness produced them."""
    try:
        out = subprocess.run(["claude", "--version"], capture_output=True, text=True, timeout=60, env=env)
    except (OSError, subprocess.SubprocessError):
        return None
    return out.stdout.strip() or None


class Live:
    def __init__(self, cfg, root, adage, round_, isolate=True):
        self.cfg, self.root, self.adage, self.round, self.isolate = cfg, root, adage, round_, isolate
        self.private = root / "private"
        self.private.mkdir(parents=True, exist_ok=True)
        # A root written before private/ existed: move its archive in, once.
        for name in ARCHIVE:
            old = root / name
            if old.exists() and not old.is_symlink() and not (self.private / name).exists():
                shutil.move(str(old), str(self.private / name))
        unlink_archive(root)
        self.home = self.private / "adage-home"
        self.transcripts = self.private / "transcripts"
        self.lock = threading.Lock()
        self.written = set()
        self.adage_exec = Path(cfg.get("adage_exec") or Path(adage).parent / "adage-exec")
        self.spent = 0.0
        self.base_env = dict(os.environ)
        self.forward = list(cfg.get("forward_env") or [])
        self.auth = auth_source(self.base_env, self.forward, bool(cfg.get("host_auth"))) if isolate else None
        p = root / "state.json"
        self.state = json.loads(p.read_text()) if p.exists() else {"config": cfg["name"], "runs": {}, "rounds": {}}
        self.state["model"] = cfg["model"]
        self.state["round"] = round_
        self.state["labels"] = cfg["labels"]
        self.state["max_turns"] = cfg["max_turns"]
        self.state["seed"] = cfg.get("seed")
        self.state["isolated"] = isolate
        self.state["auth"] = self.auth[0] if self.auth else None
        self.state["fresh_pools"] = fresh_pools(cfg)
        self.state.setdefault("leaky", False)
        self.state["harness_version"] = harness_version()
        self.state.setdefault("started", time.time())
        self.spent = sum((r.get("metrics") or {}).get("cost_usd") or 0.0 for r in self.state["runs"].values())

    def save(self):
        tmp = self.root / "state.json.tmp"
        tmp.write_text(json.dumps(self.state))
        tmp.replace(self.root / "state.json")

    def set_phase(self, phase):
        with self.lock:
            self.state["phase"] = phase
            self.state["round"] = self.round
            self.save()

    def update(self, run_id, **kv):
        with self.lock:
            self.state["runs"].setdefault(run_id, {}).update(kv)
            self.save()

    def settings(self, kind):
        """The settings file of one kind (`settings_kind`). Written once per invocation and
        atomically: concurrent sessions once rewrote the same file while others started reading it."""
        p = self.private / f"settings-{kind}.json"
        with self.lock:
            if kind not in self.written:
                if kind == "none":
                    body = "{}"
                else:
                    bin_ = self.adage if kind == "hooks" else self.adage_exec
                    body = subprocess.run([str(self.adage), "hooks-config", str(bin_)], capture_output=True, text=True,
                                          check=True).stdout
                tmp = p.with_suffix(".tmp")
                tmp.write_text(body)
                tmp.replace(p)
                self.written.add(kind)
        return p

    def prepare_home(self, run_home):
        """Create an isolated session's empty home: its directories, a git identity (agents may
        commit), and, if that is how auth works, a copy of the operator's credentials file."""
        for d in home_dirs(run_home).values():
            Path(d).mkdir(parents=True, exist_ok=True)
        (run_home / ".gitconfig").write_text("[user]\n\tname = adage-eval\n\temail = adage-eval@localhost\n")
        if self.auth and self.auth[0] == "file":
            dest = Path(home_dirs(run_home)["CLAUDE_CONFIG_DIR"]) / ".credentials.json"
            shutil.copy(self.auth[1], dest)
            dest.chmod(0o600)

    def archive_transcript(self, session_id, config_dir):
        """Copy the session's Claude Code transcript under private/transcripts (the learn input).
        Isolated sessions write theirs into their own config directory; the operator's is read
        only with --no-isolate."""
        for p in (Path(config_dir) / "projects").glob(f"*/{session_id}.jsonl"):
            dest = self.transcripts / p.parent.name / p.name
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy(p, dest)
            return dest
        return None

    def score(self, workdir, task):
        """The scorer's verdict, and whether it produced one."""
        try:
            out = subprocess.run(expand(self.cfg["score"], dir=str(workdir), task=task), capture_output=True, text=True)
            score = json.loads(out.stdout or "{}")
        except (OSError, ValueError):
            return {}, False
        ok = isinstance(score, dict) and "passed" in score
        return (score if isinstance(score, dict) else {}), ok

    def run_one(self, run_id, phase, arm, task, prompt_t):
        if self.spent >= self.cfg["budget_usd"]:
            self.update(run_id, id=run_id, phase=phase, arm=arm, task=task, round=self.round, status="skipped")
            print(f"  {run_id}: skipped, spend cap ${self.cfg['budget_usd']} reached", flush=True)
            return
        run_dir = self.root / "runs" / run_id
        shutil.rmtree(run_dir, ignore_errors=True)
        workdir = run_dir / self.cfg["workdir"]
        workdir.parent.mkdir(parents=True)
        run_home = run_dir / "home"
        self.update(run_id, id=run_id, phase=phase, arm=arm, task=task, prompt=prompt_t.format(id=task), round=self.round,
                    seed=self.cfg.get("seed"), status="running", t0=time.time(), events=[], directives=[])
        subprocess.run(expand(self.cfg["sandbox"], dir=str(workdir), task=task), check=True, capture_output=True)
        prompt = prompt_t.format(id=task)
        env = session_env(arm, run_home, self.home, self.isolate, self.base_env, self.forward)
        if self.isolate:
            self.prepare_home(run_home)
            config_dir = Path(env["CLAUDE_CONFIG_DIR"])
        else:
            config_dir = operator_config_dir(self.base_env)
        settings = self.settings(settings_kind(arm))
        cmd = claude_command(self.cfg, prompt, settings)
        if self.cfg.get("confine"):
            rw = [env["ADAGE_HOME"]] if env.get("ADAGE_HOME") else []
            cmd = confine_argv(self.cfg, run_dir, env, ro=[settings, self.adage, self.adage_exec], rw=rw) + cmd
        t0 = time.time()
        events, directives, uses_by_id, sid, result, turns, version = [], [], {}, None, {}, 0, None
        try:
            with open(run_dir / "stream.jsonl", "w") as raw:
                proc = subprocess.Popen(cmd, cwd=workdir, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True, env=env)
                for line in proc.stdout:
                    t = round(time.time() - t0, 2)
                    raw.write(json.dumps({"t": t, "line": line.rstrip("\n")}) + "\n")
                    try:
                        m = json.loads(line)
                    except ValueError:
                        continue
                    typ = m.get("type")
                    if typ == "system" and m.get("subtype") == "init":
                        sid = m.get("session_id")
                        version = m.get("claude_code_version")
                    elif typ == "system" and "hook" in str(m.get("subtype", "")):
                        text = json.dumps(m)
                        if "adage-signal" in text or "finish-line check" in text:
                            directives.append({"t": t, "raw": text[:4000]})
                            self.update(run_id, directives=directives)
                    elif typ == "assistant":
                        uses = [c for c in m.get("message", {}).get("content", []) if c.get("type") == "tool_use"]
                        turns += bool(uses)
                        for u in uses:
                            uses_by_id[u.get("id")] = u
                            inp = u.get("input") or {}
                            events.append({"t": t, "turn": turns, "tool": u.get("name"), "id": u.get("id"),
                                           "cat": classify(self.cfg, u.get("name"), inp),
                                           "cmd": inp.get("command") or inp.get("file_path") or ""})
                        if uses:
                            self.update(run_id, events=events)
                    elif typ == "user":
                        for c in m.get("message", {}).get("content", []) or []:
                            if isinstance(c, dict) and c.get("type") == "tool_result" and c.get("is_error"):
                                u = uses_by_id.get(c.get("tool_use_id"))
                                for ev in events:
                                    if u and ev["id"] == u.get("id"):
                                        ev["failed"] = True
                                        ev["cat"] = classify(self.cfg, u.get("name"), u.get("input"), failed=True)
                        self.update(run_id, events=events)
                    elif typ == "result":
                        result = m
                proc.wait()
        finally:
            if self.isolate:  # the copied credentials never outlive the session
                (config_dir / ".credentials.json").unlink(missing_ok=True)
        score, scorer_ok = self.score(workdir, task)
        cost = result.get("total_cost_usd") or 0.0
        with self.lock:
            self.spent += cost
        if sid:
            self.archive_transcript(sid, config_dir)
        # The spool is in the arm's own ADAGE_HOME (instincts-home, e1-home), not always adage-home.
        spool = spool_directives(Path(env.get("ADAGE_HOME") or self.home), sid) if arm not in ("train", "off") else []
        metrics = run_metrics(self.cfg, events, result, score, workdir, round(time.time() - t0, 2), sid, spool)
        status, reason = demo_report.classify_status(result, len(events), metrics["turns"], score, scorer_ok,
                                                     self.cfg["max_turns"])
        metrics.update(status=status, status_reason=reason, exit_code=proc.returncode, harness_version=version)
        # `status` at the top stays passed/failed for the dashboard; the run's classification is
        # metrics.status (and run_status).
        self.update(run_id, status="passed" if metrics["passed"] else "failed", run_status=status, metrics=metrics,
                    t_end=time.time(), score={k: v for k, v in score.items() if not k.endswith("_tail")})
        print(f"  {run_id}: {status}{' (' + reason + ')' if reason else ''} passed={metrics['passed']} turns={metrics['turns']} "
              f"first_edit={metrics['turns_to_first_edit']} dead_ends={metrics['dead_end_hits']} cost={cost:.3f} "
              f"(total ${self.spent:.2f})", flush=True)

    def phase(self, name, sessions):
        unlink_archive(self.root)
        self.set_phase(name)
        with ThreadPoolExecutor(max_workers=self.cfg["concurrency"]) as ex:
            futures = [(s, ex.submit(self.run_one, *s)) for s in sessions]
            for s, f in futures:
                try:
                    f.result()
                except Exception as e:  # noqa: BLE001 - one failed session must not stop the others
                    # Recorded as an infra error, so the summary counts it instead of losing it.
                    self.update(s[0], status="failed", run_status="infra_error", t_end=time.time(),
                                metrics={"status": "infra_error", "status_reason": f"runner: {e}"[:300]})
                    print(f"  {s[0]}: infra_error (runner: {e})", flush=True)

    def learn(self):
        self.set_phase("learn")
        pool = set(tasks_for_round(self.cfg, self.round)) if fresh_pools(self.cfg) else None
        leaky = self.round > 1 and pool is None
        if leaky:
            print(leak_warning([self.round]), flush=True)
            self.state["leaky"] = True
        runs = learning_runs(self.state["runs"].values(), self.round, pool)
        transcripts, outcomes = [], {}
        for r in runs:
            sid = r["metrics"]["session_id"]
            hits = list(self.transcripts.glob(f"*/{sid}.jsonl"))
            if hits:
                transcripts.append(str(hits[0]))
                outcomes[sid] = r["metrics"].get("passed")
        (self.private / "outcomes.json").write_text(json.dumps(outcomes))
        if self.round == 1:
            shutil.rmtree(self.home / "tracks", ignore_errors=True)
        out = subprocess.run([str(self.adage), "learn", *self.cfg["learn_args"], "--json", "--repo", self.cfg["repo_key"],
                              "--outcomes", str(self.private / "outcomes.json"), "--transcripts", *transcripts],
                             capture_output=True, text=True, env=dict(os.environ, ADAGE_HOME=str(self.home)))
        if out.returncode != 0:
            raise SystemExit(out.stderr)
        rep = json.loads(out.stdout)
        idx = self.home / "index.json"
        tracks = json.loads(idx.read_text()).get("routes", []) if idx.exists() else []
        self.state["rounds"][str(self.round)] = {"learn": rep, "tracks": tracks, "sessions": len(transcripts),
                                                 "leaky": leaky, "pool": sorted(pool) if pool is not None else None}
        self.state["learn"], self.state["tracks"] = rep, tracks
        if "instincts" in self.cfg["eval"].get("arms", []):
            self.state["rounds"][str(self.round)]["instincts_only"] = self.bind_instincts()
        self.save()
        print(json.dumps(rep, indent=2))

    def bind_instincts(self):
        """The `instincts` arm's index: `adage learn --instincts-only` over a fresh sandbox (of the
        round's first held-out task), in its own ADAGE_HOME. No session is read."""
        home = instincts_home(self.home)
        shutil.rmtree(home, ignore_errors=True)
        home.mkdir(parents=True)
        task = tasks_for_round(self.cfg, self.round)[0]
        work = self.private / "instincts-sandbox" / self.cfg["workdir"]
        shutil.rmtree(work.parent, ignore_errors=True)
        work.parent.mkdir(parents=True)
        subprocess.run(expand(self.cfg["sandbox"], dir=str(work), task=task), check=True, capture_output=True)
        out = subprocess.run([str(self.adage), "learn", "--instincts-only", "--repo-path", str(work), "--json"],
                             capture_output=True, text=True, env=dict(os.environ, ADAGE_HOME=str(home)))
        if out.returncode != 0:
            raise SystemExit(out.stderr)
        idx = home / "index.json"
        routes = json.loads(idx.read_text()).get("routes", []) if idx.exists() else []
        print(f"instincts-only index: {len(routes)} route(s)", flush=True)
        return {"report": json.loads(out.stdout), "routes": routes}


# ------------------------------------------------------------------------------------- report

# The metrics, run statuses and statistics live in demo/report.py, shared with its results page.
METRICS = demo_report.METRICS
classify_status = demo_report.classify_status
cluster_perm_p = demo_report.cluster_perm_p
cluster_boot_ci = demo_report.cluster_boot_ci
task_signflip_p = demo_report.task_signflip_p
holm = demo_report.holm


def summarise(state, round_, n_perm=10000, n_boot=2000):
    """The held-out runs of one round per arm: aggregates, n per metric (and how many runs had no
    value or a restricted one), the difference between adage and holdout with a task-bootstrap
    95% CI, the task-stratified permutation p-value and its Holm adjustment across the metrics,
    and the task-level sign-flip p-value. Runs whose harness or scorer failed are left out of
    every arm and counted (`status_counts`, `excluded`); censored runs stay, and are counted."""
    runs = [r for r in state["runs"].values() if r.get("phase") == "eval" and r.get("round", 1) == round_]
    done = [r for r in runs if r.get("metrics")]
    cmp = demo_report.compare(done, state.get("max_turns"), n_perm=n_perm, n_boot=n_boot)
    arms = sorted({r["arm"] for r in runs})
    info = (state.get("rounds") or {}).get(str(round_), {})
    return {
        "round": round_,
        "runs": {a: sum(1 for r in done if r["arm"] == a) for a in arms},
        "unfinished": {a: sum(1 for r in runs if r["arm"] == a and not r.get("metrics")) for a in arms},
        "status_counts": cmp["status_counts"], "excluded": cmp["excluded"], "n_censored": cmp["n_censored"],
        "leaky": bool(info.get("leaky", round_ > 1 and not state.get("fresh_pools"))),
        "seed": state.get("seed"), "harness_version": state.get("harness_version"), "isolated": state.get("isolated"),
        "metrics": cmp["metrics"],
    }


def print_summary(summary):
    print(demo_report.table(summary))
    if summary.get("leaky"):
        print(leak_warning([summary["round"]]))


def results_page(root, state):
    """The results page, as demo/results has it: index.html (dashboard with the state embedded),
    artifact.html, summary.json and state.json, under <root>/results."""
    return demo_report.build(redact_home(state), root / "results")


def redact_home(state):
    """The state with the person's home directory shown as `~`: results pages are meant to be
    shared, and agents' commands sometimes name paths under it (one listed ~/.claude/projects)."""
    return demo_report.redact_home(state)


# --------------------------------------------------------------------------------------- main

def parse(argv):
    opts = {"execute": False, "dry_run": False, "json": False, "root": None, "model": None, "round": None,
            "adage": None, "phases": [], "config": None, "seed": None, "isolate": True,
            "host_auth": None}
    it = iter(argv)
    for a in it:
        if a == "--execute":
            opts["execute"] = True
        elif a == "--dry-run":
            opts["dry_run"] = True
        elif a == "--json":
            opts["json"] = True
        elif a == "--no-isolate":
            opts["isolate"] = False
        elif a == "--host-auth":
            opts["host_auth"] = True
        elif a in ("--root", "--model", "--round", "--adage", "--config", "--seed"):
            v = next(it, None)
            if v is None:
                raise SystemExit(f"{a} needs a value")
            if a == "--config" and opts["config"] is not None:
                raise SystemExit("config given twice")
            opts[a[2:]] = v
        elif a in ("train", "learn", "eval", "report"):
            opts["phases"].append(a)
        elif a in ("-h", "--help"):
            print(__doc__)
            sys.exit(0)
        elif a.startswith("-"):
            raise SystemExit(f"unknown option {a!r} (see --help)")
        elif opts["config"] is None:
            opts["config"] = a
        else:
            raise SystemExit(f"unexpected argument {a!r}")
    if opts["config"] is None:
        raise SystemExit("usage: runner.py [--config] <config.json> [train] [learn] [eval] [report] [--execute] (see --help)")
    if opts["execute"] and opts["dry_run"]:
        raise SystemExit("--execute and --dry-run are mutually exclusive")
    opts["round"] = int(opts["round"]) if opts["round"] is not None else None
    if opts["round"] is not None and opts["round"] < 1:
        raise SystemExit("--round must be 1 or more")
    try:
        opts["seed"] = int(opts["seed"]) if opts["seed"] is not None else None
    except ValueError:
        raise SystemExit("--seed must be an integer") from None
    opts["phases"] = opts["phases"] or ["train", "learn", "eval", "report"]
    return opts["config"], opts


def resolve_seed(cfg, root):
    """The arm-order seed: --seed or the config's `seed`, else the one an earlier invocation
    recorded in the root's state.json (so later rounds continue it), else a fresh one."""
    if cfg.get("seed") is not None:
        return int(cfg["seed"])
    try:
        s = json.loads((root / "state.json").read_text()).get("seed")
    except (OSError, ValueError):
        s = None
    return int(s) if s is not None else secrets.randbelow(2 ** 31)


def main(argv):
    config, opts = parse(argv)
    cfg = load_config(config, {"model": opts["model"], "seed": opts["seed"], "host_auth": opts["host_auth"]})
    root = Path(opts["root"] or f"/tmp/adage-live/{cfg['name']}").resolve()
    adage = Path(opts["adage"] or REPO / "target" / "release" / "adage")
    rounds = rounds_for(cfg, opts["round"])
    if opts["phases"] == ["report"]:
        return report_only(cfg, root)
    cfg["seed"] = resolve_seed(cfg, root)
    p = describe(cfg, opts["phases"], rounds, root, adage, opts["isolate"])
    if not opts["execute"]:
        if opts["json"]:
            print(json.dumps(p, indent=1))
        else:
            print_plan(p)
        return 0
    # Executing spends real tokens: refuse inside an agent session, and check the tools exist.
    if os.environ.get("CLAUDECODE"):
        raise SystemExit("refusing to run live agents from inside an agent session (CLAUDECODE is set)")
    if not shutil.which("claude"):
        raise SystemExit("`claude` is not on PATH")
    if not adage.exists():
        raise SystemExit(f"build first: cargo build --release -p adage-cli  (missing {adage})")
    if cfg.get("confine") and not shutil.which("bwrap"):
        raise SystemExit("confine needs bubblewrap (`bwrap`) on PATH")
    if opts["isolate"] and not auth_source(os.environ, cfg.get("forward_env") or [], bool(cfg.get("host_auth"))):
        raise SystemExit("isolated sessions cannot find your Claude Code login: set ANTHROPIC_API_KEY, or "
                         "CLAUDE_CODE_OAUTH_TOKEN (from `claude setup-token`), or have ~/.claude/.credentials.json "
                         "(see evals/live/README.md, Isolation); where the host manages the login, --host-auth. "
                         "--no-isolate runs without isolation.")
    if not opts["isolate"]:
        print("WARNING: --no-isolate: sessions use your HOME and ~/.claude (auto-memory, earlier transcripts); "
              "the control arm is not isolated.", flush=True)
    if p["leaky_rounds"]:
        print(leak_warning(p["leaky_rounds"]), flush=True)
    root.mkdir(parents=True, exist_ok=True)
    live = Live(cfg, root, adage, rounds[0], opts["isolate"])
    print(f"seed {cfg['seed']}  harness {live.state['harness_version']}  isolation {'on' if opts['isolate'] else 'OFF'}"
          f"{' (auth: ' + live.auth[0] + ')' if live.auth else ''}", flush=True)
    summaries = {}
    for round_ in rounds:
        live.round = round_
        sessions = plan(cfg, opts["phases"], round_)
        print(f"== round {round_}", flush=True)
        if "train" in opts["phases"] and round_ == 1:
            live.phase("train", [s for s in sessions if s[1] == "train"])
        if "learn" in opts["phases"]:
            live.learn()
        if "eval" in opts["phases"]:
            if "e1" in cfg["eval"].get("arms", []) and not (e1_home(live.home) / "index.json").exists():
                raise SystemExit(f"the e1 arm needs {e1_home(live.home)}: copy private/adage-home there and approve "
                                 "its prefetches as a person (adage review allow-exec ... --level e1); see README")
            live.phase("eval", [s for s in sessions if s[1] == "eval"])
        if "report" in opts["phases"]:
            summaries[str(round_)] = summarise(live.state, round_)
    link_archive(root)
    if "report" in opts["phases"]:
        live.set_phase("done")
        latest = max(int(k) for k in summaries)
        summary = {**summaries[str(latest)], "config": cfg["name"], "model": cfg["model"], "rounds": summaries}
        (root / "summary.json").write_text(json.dumps(summary, indent=2))
        for k in sorted(summaries, key=int):
            print(f"\nround {k}:")
            print_summary(summaries[k])
        print(f"results page: {results_page(root, live.state)}")
    return 0


def report_only(cfg, root):
    """Rebuild summary.json and the results page from a finished run's state.json. Spends no tokens
    and runs no agents, so it needs neither --execute nor a terminal session. Metrics that are pure
    functions of the recorded events (the finish-line obligation) are recomputed with the current
    rules, so a metric fix applies to runs already done."""
    p = root / "state.json"
    if not p.exists():
        raise SystemExit(f"no state.json in {root}; run the experiment first")
    state = json.loads(p.read_text())
    state.setdefault("max_turns", cfg["max_turns"])
    changed = 0
    for rid, r in state.get("runs", {}).items():
        m, score = r.get("metrics"), r.get("score") or {}
        if not m or not m.get("obligation_applies"):
            continue
        workdir = root / "runs" / rid / cfg["workdir"]
        met = regen_after_obligation(cfg, r.get("events", []), workdir) and not bool(score.get("stale_generated"))
        if m.get("obligation_met") != met:
            m["obligation_met"] = met
            changed += 1
    tmp = root / "state.json.tmp"
    tmp.write_text(json.dumps(state))
    tmp.replace(p)
    if (root / "private").is_dir():
        link_archive(root)
    rounds = sorted({int(r.get("round") or 1) for r in state.get("runs", {}).values() if r.get("phase") == "eval"}) or [1]
    summaries = {str(rd): summarise(state, rd) for rd in rounds}
    latest = max(rounds)
    summary = {**summaries[str(latest)], "config": cfg["name"], "model": cfg["model"], "rounds": summaries}
    (root / "summary.json").write_text(json.dumps(summary, indent=2))
    print(f"recomputed the finish-line obligation for {changed} run(s)")
    for k in sorted(summaries, key=int):
        print(f"\nround {k}:")
        print_summary(summaries[k])
    print(f"results page: {results_page(root, state)}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
