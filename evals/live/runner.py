#!/usr/bin/env python3
"""Live proving-ground eval: real `claude` agents, with and without Adage (Design §12).

  python3 evals/live/runner.py [--config] <config.json> [train] [learn] [eval] [report] [options]

With no `--execute` this is a dry run (`--dry-run` says so explicitly). It prints the plan: every
session, arm, prompt and command line, the number of agent runs per phase and round, the output
layout, and the spend cap. Nothing is executed and nothing is written. With `--execute` it runs the
plan. Every session is a real headless Claude Code agent (`claude -p`), so this spends real tokens,
and each session gets a fresh sandbox:

  train   baseline agents on the training tasks, with no Adage hooks installed (round 1 only)
  learn   `adage learn` over the training transcripts (and, in later rounds, every earlier session)
  eval    held-out tasks; the arms are interleaved so both see the same conditions over time:
            adage    hooks installed, directives delivered
            holdout  hooks installed, directives computed and logged but not delivered (the control)
            off      no hooks at all (to measure hook overhead, if wanted)
  report  summary.json (per-arm medians and means with permutation p-values, per round) and a
          results page like demo/results: <root>/results/{index.html, summary.json, state.json}

Rounds: the config's `rounds` (default 1) are run in order: round 1 is train, learn, eval, report;
each later round learns from every earlier session, then runs the held-out tasks again. `--round N`
runs only round N.

The output root (`--root`, default /tmp/adage-live/<config name>) has the layout of a demo archive:
runs/<run>/{<workdir>/, stream.jsonl}, transcripts/<project>/<session>.jsonl, adage-home/,
state.json, outcomes.json and summary.json. So
`python3 evals/datasets/tools/build_demo.py <root> <dataset> --name ... --repo ...` turns a live run
into an offline `adage eval` dataset. The live dashboard reads the same state:
`DEMO_ROOT=<root> python3 demo/dashboard/serve.py`.

Options:
  --config PATH        the config (or give it as the first positional argument)
  --dry-run            print the plan and exit (the default without --execute)
  --execute            run for real (spends tokens); refused inside an agent session (CLAUDECODE)
  --root DIR           output root
  --model ID           override the config's model
  --round N            run only this learning round (default: every round in the config)
  --json               print the plan as JSON (dry run)
  --adage PATH         the adage binary (default: target/release/adage in this repository)

Config: JSON. See evals/live/configs/ and evals/live/README.md.
"""

import fnmatch
import json
import re
import os
import random
import shutil
import statistics
import subprocess
import sys
import threading
import time
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
EDIT_TOOLS = {"Edit", "Write", "MultiEdit", "NotebookEdit"}
EXPLORE_TOOLS = {"Read", "Glob", "Grep", "LS"}
EXPLORE_COMMANDS = ("ls", "find", "cat", "grep", "rg", "head", "tail", "tree", "sed -n", "wc")
ARMS = ("adage", "holdout", "off")
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
    return cfg


def expand(argv, **kv):
    """Fill `{repo}`, `{dir}`, `{task}` placeholders in a command given as an argv list."""
    kv.setdefault("repo", str(REPO))
    return [a.format(**kv) for a in argv]


# --------------------------------------------------------------------------------------- plan

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
            for i, task in enumerate(e["tasks"]):
                prompt = prompts[(i + r) % len(prompts)]
                for arm in e.get("arms", ["holdout", "adage"]):
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
        "--allowedTools", *cfg["allowed_tools"],
        "--disallowedTools", *cfg["disallowed_tools"],
    ]


def env_for(arm, home):
    """Environment overrides for one session. Training and `off` sessions have no hooks at all.
    Automatic learning is off: the rounds decide when learning happens, so the index stays fixed
    within a round."""
    if arm in ("train", "off"):
        return {}
    return {"ADAGE_HOME": str(home), "ADAGE_ARM": "holdout" if arm == "holdout" else "treated",
            "ADAGE_AUTO_LEARN": "0"}


def rounds_for(cfg, round_):
    return [round_] if round_ else list(range(1, cfg["rounds"] + 1))


def estimate(cfg, phases, rounds):
    """How many agent runs the plan makes, as the product that produces the number."""
    t, e = cfg["train"], cfg["eval"]
    arms = e.get("arms", ["holdout", "adage"])
    train_runs = len(t["tasks"]) * t.get("repeats", 1) if ("train" in phases and 1 in rounds) else 0
    eval_rounds = len(rounds) if "eval" in phases else 0
    eval_runs = len(e["tasks"]) * e.get("repeats", 1) * len(arms) * eval_rounds
    return {
        "train": {"tasks": len(t["tasks"]) if train_runs else 0, "repeats": t.get("repeats", 1), "runs": train_runs},
        "eval": {"tasks": len(e["tasks"]), "repeats": e.get("repeats", 1), "arms": arms, "rounds": eval_rounds,
                 "runs": eval_runs},
        "total_runs": train_runs + eval_runs,
    }


def describe(cfg, phases, rounds, root, adage):
    rows = []
    for round_ in rounds:
        for run_id, phase, arm, task, prompt in plan(cfg, phases, round_):
            text = prompt.format(id=task)
            workdir = root / "runs" / run_id / cfg["workdir"]
            rows.append({
                "run": run_id, "round": round_, "phase": phase, "arm": arm, "task": task, "prompt": text,
                "cwd": str(workdir),
                "sandbox": expand(cfg["sandbox"], dir=str(workdir), task=task),
                "command": claude_command(cfg, text, root / f"settings-{'hooks' if env_for(arm, '') else 'none'}.json"),
                "env": env_for(arm, root / "adage-home"),
                "score": expand(cfg["score"], dir=str(workdir), task=task),
            })
    learn = None
    if "learn" in phases:
        learn = [str(adage), "learn", *cfg["learn_args"], "--json", "--repo", cfg["repo_key"],
                 "--outcomes", str(root / "outcomes.json"), "--transcripts", "<each earlier session's transcript>"]
    est = estimate(cfg, phases, rounds)
    assert est["total_runs"] == len(rows), (est, len(rows))
    return {
        "config": cfg["name"], "model": cfg["model"], "rounds": rounds, "round": rounds[0] if len(rounds) == 1 else None,
        "phases": phases, "root": str(root), "adage": str(adage), "sessions": rows, "learn": learn,
        "learn_rounds": rounds if learn else [],
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
        parts.append(f"eval {e['tasks']} tasks x {e['repeats']} repeats x {len(e['arms'])} arms ({', '.join(e['arms'])})"
                     f" x {e['rounds']} round{'s' if e['rounds'] != 1 else ''} = {e['runs']}")
    return "; ".join(parts) + f"; total {est['total_runs']} agent runs"


def print_plan(p):
    t = p["totals"]
    print(f"DRY RUN: nothing is executed. Add --execute to run {t['sessions']} real `claude` sessions.")
    print(f"config {p['config']}  model {p['model']}  rounds {' '.join(map(str, p['rounds']))}  phases {' '.join(p['phases'])}")
    print(f"root {p['root']}  (runs/, transcripts/, adage-home/, state.json, summary.json, results/)")
    print(f"runs: {estimate_line(p['estimate'])}")
    print(f"sessions {t['sessions']} by arm {t['by_arm']} by round {t['by_round']}  concurrency {t['concurrency']}  "
          f"max {t['max_turns_each']} turns each  spend cap ${t['budget_usd']}")
    for rd in p["rounds"]:
        steps = [ph for ph in ("train", "learn", "eval", "report") if ph in p["phases"] and not (ph == "train" and rd != 1)]
        print(f"\nround {rd}: {' -> '.join(steps) or '(nothing)'}"
              + ("   (learns from the training sessions)" if rd == 1 and "learn" in steps else
                 "   (learns from every earlier session)" if "learn" in steps else ""))
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

class Live:
    def __init__(self, cfg, root, adage, round_):
        self.cfg, self.root, self.adage, self.round = cfg, root, adage, round_
        self.home = root / "adage-home"
        self.lock = threading.Lock()
        self.spent = 0.0
        p = root / "state.json"
        self.state = json.loads(p.read_text()) if p.exists() else {"config": cfg["name"], "runs": {}, "rounds": {}}
        self.state["model"] = cfg["model"]
        self.state["round"] = round_
        self.state["labels"] = cfg["labels"]
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

    def settings(self, hooks):
        p = self.root / f"settings-{'hooks' if hooks else 'none'}.json"
        if hooks:
            out = subprocess.run([str(self.adage), "hooks-config", str(self.adage)], capture_output=True, text=True, check=True)
            p.write_text(out.stdout)
        else:
            p.write_text("{}")
        return p

    def archive_transcript(self, session_id):
        """Copy the session's Claude Code transcript under root/transcripts (the learn input)."""
        for p in (Path.home() / ".claude" / "projects").glob(f"*/{session_id}.jsonl"):
            dest = self.root / "transcripts" / p.parent.name / p.name
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy(p, dest)
            return dest
        return None

    def run_one(self, run_id, phase, arm, task, prompt_t):
        if self.spent >= self.cfg["budget_usd"]:
            print(f"  {run_id}: skipped, spend cap ${self.cfg['budget_usd']} reached", flush=True)
            return
        run_dir = self.root / "runs" / run_id
        shutil.rmtree(run_dir, ignore_errors=True)
        workdir = run_dir / self.cfg["workdir"]
        workdir.parent.mkdir(parents=True)
        subprocess.run(expand(self.cfg["sandbox"], dir=str(workdir), task=task), check=True, capture_output=True)
        prompt = prompt_t.format(id=task)
        self.update(run_id, id=run_id, phase=phase, arm=arm, task=task, prompt=prompt, round=self.round,
                    status="running", t0=time.time(), events=[], directives=[])
        env = dict(os.environ, **env_for(arm, self.home))
        cmd = claude_command(self.cfg, prompt, self.settings(bool(env_for(arm, self.home))))
        t0 = time.time()
        events, directives, uses_by_id, sid, result, turns = [], [], {}, None, {}, 0
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
        score = json.loads(subprocess.run(expand(self.cfg["score"], dir=str(workdir), task=task),
                                          capture_output=True, text=True).stdout or "{}")
        cost = result.get("total_cost_usd") or 0.0
        with self.lock:
            self.spent += cost
        if sid:
            self.archive_transcript(sid)
        spool = spool_directives(self.home, sid) if arm not in ("train", "off") else []
        metrics = run_metrics(self.cfg, events, result, score, workdir, round(time.time() - t0, 2), sid, spool)
        self.update(run_id, status="passed" if metrics["passed"] else "failed", metrics=metrics, t_end=time.time(),
                    score={k: v for k, v in score.items() if not k.endswith("_tail")})
        print(f"  {run_id}: passed={metrics['passed']} turns={metrics['turns']} first_edit={metrics['turns_to_first_edit']} "
              f"dead_ends={metrics['dead_end_hits']} cost={cost:.3f} (total ${self.spent:.2f})", flush=True)

    def phase(self, name, sessions):
        self.set_phase(name)
        with ThreadPoolExecutor(max_workers=self.cfg["concurrency"]) as ex:
            for f in [ex.submit(self.run_one, *s) for s in sessions]:
                try:
                    f.result()
                except Exception as e:  # noqa: BLE001 - one failed session must not stop the others
                    print(f"  session failed: {e}", flush=True)

    def learn(self):
        self.set_phase("learn")
        runs = [r for r in self.state["runs"].values() if r.get("metrics") and (r["phase"] == "train" or self.round > 1)
                and r.get("round", 1) < max(self.round, 2)]
        transcripts, outcomes = [], {}
        for r in runs:
            sid = r["metrics"]["session_id"]
            hits = list((self.root / "transcripts").glob(f"*/{sid}.jsonl")) if sid else []
            if hits:
                transcripts.append(str(hits[0]))
                outcomes[sid] = r["metrics"]["passed"]
        (self.root / "outcomes.json").write_text(json.dumps(outcomes))
        if self.round == 1:
            shutil.rmtree(self.home / "tracks", ignore_errors=True)
        out = subprocess.run([str(self.adage), "learn", *self.cfg["learn_args"], "--json", "--repo", self.cfg["repo_key"],
                              "--outcomes", str(self.root / "outcomes.json"), "--transcripts", *transcripts],
                             capture_output=True, text=True, env=dict(os.environ, ADAGE_HOME=str(self.home)))
        if out.returncode != 0:
            raise SystemExit(out.stderr)
        rep = json.loads(out.stdout)
        idx = self.home / "index.json"
        tracks = json.loads(idx.read_text()).get("routes", []) if idx.exists() else []
        self.state["rounds"][str(self.round)] = {"learn": rep, "tracks": tracks, "sessions": len(transcripts)}
        self.state["learn"], self.state["tracks"] = rep, tracks
        self.save()
        print(json.dumps(rep, indent=2))


# ------------------------------------------------------------------------------------- report

def perm_p(a, b, stat, n=10000, seed=7):
    """Two-sided permutation test for a difference in `stat` between samples a and b."""
    a, b = [x for x in a if x is not None], [x for x in b if x is not None]
    if len(a) < 2 or len(b) < 2:
        return None
    rng = random.Random(seed)
    obs, pool, hits = abs(stat(a) - stat(b)), a + b, 0
    for _ in range(n):
        rng.shuffle(pool)
        if abs(stat(pool[:len(a)]) - stat(pool[len(a):])) >= obs - 1e-12:
            hits += 1
    return (hits + 1) / (n + 1)


def mean(v):
    return sum(v) / len(v)


def rate(flag):
    return lambda m: None if m.get(flag) is None else (1.0 if m[flag] else 0.0)


def follow_rate(m):
    f = m.get("follow") or {}
    return f["followed"] / f["exposed"] if f.get("exposed") else None


METRICS = {
    "turns_to_first_edit": (lambda m: m["turns_to_first_edit"], statistics.median),
    "time_to_first_edit_s": (lambda m: m["time_to_first_edit"], statistics.median),
    "total_turns": (lambda m: m["turns"], statistics.median),
    "explore_calls_before_edit": (lambda m: m.get("explore_calls_before_edit"), statistics.median),
    "cost_usd": (lambda m: m["cost_usd"], mean),
    "pass_rate": (rate("passed"), mean),
    "hidden_pass_rate": (rate("hidden_passed"), mean),
    "dead_end_hits": (lambda m: m.get("dead_end_hits"), mean),
    "obligation_met_rate": (rate("obligation_met"), mean),
    "stale_generated_rate": (rate("stale_generated"), mean),
    "follow_rate": (follow_rate, mean),
}


def summarise(state, round_):
    runs = [r for r in state["runs"].values() if r.get("metrics") and r["phase"] == "eval" and r.get("round", 1) == round_]
    arms = sorted({r["arm"] for r in runs})
    out = {"round": round_, "runs": {a: sum(1 for r in runs if r["arm"] == a) for a in arms}, "metrics": {}}
    for name, (f, agg) in METRICS.items():
        vals = {a: [f(r["metrics"]) for r in runs if r["arm"] == a] for a in arms}
        row = {a: (agg([x for x in v if x is not None]) if any(x is not None for x in v) else None) for a, v in vals.items()}
        row["n"] = {a: sum(1 for x in v if x is not None) for a, v in vals.items()}
        if "adage" in vals and "holdout" in vals:
            row["p_value"] = perm_p(vals["adage"], vals["holdout"], agg)
        out["metrics"][name] = row
    return out


def results_page(root, state):
    """The results page, as demo/results has it: index.html (dashboard with the state embedded),
    artifact.html, summary.json and state.json, under <root>/results."""
    sys.path.insert(0, str(REPO / "demo"))
    import report  # noqa: E402 - demo/report.py

    return report.build(redact_home(state), root / "results")


def redact_home(state):
    """The state with the person's home directory shown as `~`: results pages are meant to be
    shared, and agents' commands sometimes name paths under it (one listed ~/.claude/projects)."""
    home = str(Path.home())
    if not home or home == "/":
        return state
    return json.loads(json.dumps(state).replace(home, "~"))


# --------------------------------------------------------------------------------------- main

def parse(argv):
    opts = {"execute": False, "dry_run": False, "json": False, "root": None, "model": None, "round": None,
            "adage": None, "phases": [], "config": None}
    it = iter(argv)
    for a in it:
        if a == "--execute":
            opts["execute"] = True
        elif a == "--dry-run":
            opts["dry_run"] = True
        elif a == "--json":
            opts["json"] = True
        elif a in ("--root", "--model", "--round", "--adage", "--config"):
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
    opts["phases"] = opts["phases"] or ["train", "learn", "eval", "report"]
    return opts["config"], opts


def main(argv):
    config, opts = parse(argv)
    cfg = load_config(config, {"model": opts["model"]})
    root = Path(opts["root"] or f"/tmp/adage-live/{cfg['name']}").resolve()
    adage = Path(opts["adage"] or REPO / "target" / "release" / "adage")
    rounds = rounds_for(cfg, opts["round"])
    p = describe(cfg, opts["phases"], rounds, root, adage)
    if opts["phases"] == ["report"]:
        return report_only(cfg, root)
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
    root.mkdir(parents=True, exist_ok=True)
    live = Live(cfg, root, adage, rounds[0])
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
            live.phase("eval", [s for s in sessions if s[1] == "eval"])
        if "report" in opts["phases"]:
            summaries[str(round_)] = summarise(live.state, round_)
    if "report" in opts["phases"]:
        live.set_phase("done")
        latest = max(int(k) for k in summaries)
        summary = {**summaries[str(latest)], "config": cfg["name"], "model": cfg["model"], "rounds": summaries}
        (root / "summary.json").write_text(json.dumps(summary, indent=2))
        print(json.dumps(summary["metrics"], indent=2))
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
    rounds = sorted({int(r.get("round") or 1) for r in state.get("runs", {}).values() if r.get("phase") == "eval"}) or [1]
    summaries = {str(rd): summarise(state, rd) for rd in rounds}
    latest = max(rounds)
    summary = {**summaries[str(latest)], "config": cfg["name"], "model": cfg["model"], "rounds": summaries}
    (root / "summary.json").write_text(json.dumps(summary, indent=2))
    print(f"recomputed the finish-line obligation for {changed} run(s)")
    print(json.dumps(summary["metrics"], indent=2))
    print(f"results page: {results_page(root, state)}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
