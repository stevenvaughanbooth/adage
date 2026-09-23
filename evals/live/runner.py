#!/usr/bin/env python3
"""Live proving-ground eval: real `claude` agents, with and without Adage (Design §12).

  python3 evals/live/runner.py <config.json> [train] [learn] [eval] [report] [options]

With no `--execute` this is a dry run. It prints the plan: every session, arm, prompt and
command line, the output layout, and the spend cap. Nothing is executed and nothing is written.
With `--execute` it runs the plan. Every session is a real headless Claude Code agent (`claude -p`),
so this spends real tokens, and each session gets a fresh sandbox:

  train   baseline agents on the training tasks, with no Adage hooks installed
  learn   `adage learn` over the training transcripts (and, in later rounds, every earlier session)
  eval    held-out tasks; the arms are interleaved so both see the same conditions over time:
            adage    hooks installed, directives delivered
            holdout  hooks installed, directives computed and logged but not delivered (the control)
            off      no hooks at all (to measure hook overhead, if wanted)
  report  summary.json: per-arm medians and means with permutation p-values

The output root (`--root`, default /tmp/adage-live/<config name>) has the layout of a demo archive:
runs/<run>/{<workdir>/, stream.jsonl}, transcripts/<project>/<session>.jsonl, adage-home/,
state.json, outcomes.json and summary.json. So
`python3 evals/datasets/tools/build_demo.py <root> <dataset>` turns a live run into an
offline `adage eval` dataset.

Options:
  --execute            run for real (spends tokens); refused inside an agent session (CLAUDECODE)
  --root DIR           output root
  --model ID           override the config's model
  --round N            learning round (round 1 learns from training only; later rounds from all)
  --json               print the plan as JSON (dry run)
  --adage PATH         the adage binary (default: target/release/adage in this repository)

Config: JSON. See evals/live/configs/acme-billing.json (the demo) and evals/live/README.md.
"""

import json
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
ARMS = ("adage", "holdout", "off")
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
}


# ------------------------------------------------------------------------------------- config

def load_config(path, overrides):
    cfg = dict(DEFAULTS)
    cfg.update(json.loads(Path(path).read_text()))
    cfg.update({k: v for k, v in overrides.items() if v is not None})
    missing = [k for k in ("name", "repo_key", "sandbox", "score", "prompts", "train", "eval") if k not in cfg]
    if missing:
        raise SystemExit(f"config {path}: missing {', '.join(missing)}")
    for arm in cfg["eval"].get("arms", ["holdout", "adage"]):
        if arm not in ARMS:
            raise SystemExit(f"config {path}: unknown arm {arm!r} (one of {', '.join(ARMS)})")
    return cfg


def expand(argv, **kv):
    """Fill `{repo}`, `{dir}`, `{task}` placeholders in a command given as an argv list."""
    kv.setdefault("repo", str(REPO))
    return [a.format(**kv) for a in argv]


# --------------------------------------------------------------------------------------- plan

def plan(cfg, phases, round_):
    """Every session the phases would run, in order: (run id, phase, arm, task, prompt)."""
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
    """Environment overrides for one session. Training and `off` sessions have no hooks at all."""
    if arm in ("train", "off"):
        return {}
    return {"ADAGE_HOME": str(home), "ADAGE_ARM": "holdout" if arm == "holdout" else "treated"}


def describe(cfg, phases, round_, root, adage):
    sessions = plan(cfg, phases, round_)
    rows = []
    for run_id, phase, arm, task, prompt in sessions:
        text = prompt.format(id=task)
        workdir = root / "runs" / run_id / cfg["workdir"]
        rows.append({
            "run": run_id, "phase": phase, "arm": arm, "task": task, "prompt": text,
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
    return {
        "config": cfg["name"], "model": cfg["model"], "round": round_, "phases": phases, "root": str(root),
        "adage": str(adage), "sessions": rows, "learn": learn,
        "totals": {
            "sessions": len(rows),
            "by_arm": {a: sum(1 for r in rows if r["arm"] == a) for a in sorted({r["arm"] for r in rows})},
            "max_turns_each": cfg["max_turns"], "concurrency": cfg["concurrency"],
            "budget_usd": cfg["budget_usd"],
        },
    }


def print_plan(p):
    print(f"DRY RUN: nothing is executed. Add --execute to run {p['totals']['sessions']} real `claude` sessions.")
    print(f"config {p['config']}  model {p['model']}  round {p['round']}  phases {' '.join(p['phases'])}")
    print(f"root {p['root']}  (runs/, transcripts/, adage-home/, state.json, summary.json)")
    print(f"sessions {p['totals']['sessions']} {p['totals']['by_arm']}  concurrency {p['totals']['concurrency']}  "
          f"max {p['totals']['max_turns_each']} turns each  spend cap ${p['totals']['budget_usd']}")
    for r in p["sessions"]:
        env = " ".join(f"{k}={v}" for k, v in r["env"].items()) or "(no hooks)"
        print(f"  {r['run']:<28} {r['arm']:<8} {r['prompt']!r:<40} {env}")
    if p["sessions"]:
        r = p["sessions"][0]
        print("\nper session, e.g. the first:")
        print("  sandbox: " + " ".join(r["sandbox"]))
        print("  agent:   " + " ".join(repr(a) if " " in a else a for a in r["command"]))
        print("  score:   " + " ".join(r["score"]))
    if p["learn"]:
        print("learn:     " + " ".join(p["learn"]))


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

    def save(self):
        tmp = self.root / "state.json.tmp"
        tmp.write_text(json.dumps(self.state))
        tmp.replace(self.root / "state.json")

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
                    status="running", t0=time.time())
        env = dict(os.environ, **env_for(arm, self.home))
        cmd = claude_command(self.cfg, prompt, self.settings(bool(env_for(arm, self.home))))
        t0 = time.time()
        events, sid, result, turns = [], None, {}, 0
        with open(run_dir / "stream.jsonl", "w") as raw:
            proc = subprocess.Popen(cmd, cwd=workdir, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True, env=env)
            for line in proc.stdout:
                t = round(time.time() - t0, 2)
                raw.write(json.dumps({"t": t, "line": line.rstrip("\n")}) + "\n")
                try:
                    m = json.loads(line)
                except ValueError:
                    continue
                if m.get("type") == "system" and m.get("subtype") == "init":
                    sid = m.get("session_id")
                elif m.get("type") == "assistant":
                    uses = [c for c in m.get("message", {}).get("content", []) if c.get("type") == "tool_use"]
                    turns += bool(uses)
                    events += [{"t": t, "turn": turns, "tool": u.get("name"), "edit": u.get("name") in EDIT_TOOLS} for u in uses]
                elif m.get("type") == "result":
                    result = m
            proc.wait()
        score = json.loads(subprocess.run(expand(self.cfg["score"], dir=str(workdir), task=task),
                                          capture_output=True, text=True).stdout or "{}")
        first_edit = next((e for e in events if e["edit"]), None)
        cost = result.get("total_cost_usd") or 0.0
        with self.lock:
            self.spent += cost
        if sid:
            self.archive_transcript(sid)
        metrics = {
            "session_id": sid, "elapsed": round(time.time() - t0, 2), "turns": result.get("num_turns") or turns,
            "tool_turns": turns, "tool_calls": len(events),
            "turns_to_first_edit": first_edit["turn"] - 1 if first_edit else None,
            "time_to_first_edit": first_edit["t"] if first_edit else None,
            "cost_usd": cost, "passed": bool(score.get("passed")), "stale_generated": bool(score.get("stale_generated")),
            "result_subtype": result.get("subtype"),
        }
        self.update(run_id, status="passed" if metrics["passed"] else "failed", metrics=metrics, t_end=time.time())
        print(f"  {run_id}: passed={metrics['passed']} turns={metrics['turns']} first_edit={metrics['turns_to_first_edit']} "
              f"cost={cost:.3f} (total ${self.spent:.2f})", flush=True)

    def phase(self, sessions):
        with ThreadPoolExecutor(max_workers=self.cfg["concurrency"]) as ex:
            for f in [ex.submit(self.run_one, *s) for s in sessions]:
                try:
                    f.result()
                except Exception as e:  # noqa: BLE001 - one failed session must not stop the others
                    print(f"  session failed: {e}", flush=True)

    def learn(self):
        runs = [r for r in self.state["runs"].values() if r.get("metrics") and (r["phase"] == "train" or self.round > 1)]
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
        self.state["rounds"][str(self.round)] = {"learn": rep, "tracks": json.loads(idx.read_text()).get("routes", []) if idx.exists() else []}
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


def summarise(state, round_):
    runs = [r for r in state["runs"].values() if r.get("metrics") and r["phase"] == "eval" and r.get("round", 1) == round_]
    arms = sorted({r["arm"] for r in runs})
    metrics = {
        "turns_to_first_edit": (lambda m: m["turns_to_first_edit"], statistics.median),
        "time_to_first_edit_s": (lambda m: m["time_to_first_edit"], statistics.median),
        "total_turns": (lambda m: m["turns"], statistics.median),
        "cost_usd": (lambda m: m["cost_usd"], mean),
        "pass_rate": (lambda m: 1.0 if m["passed"] else 0.0, mean),
        "stale_generated_rate": (lambda m: 1.0 if m["stale_generated"] else 0.0, mean),
    }
    out = {"round": round_, "runs": {a: sum(1 for r in runs if r["arm"] == a) for a in arms}, "metrics": {}}
    for name, (f, agg) in metrics.items():
        vals = {a: [f(r["metrics"]) for r in runs if r["arm"] == a] for a in arms}
        row = {a: (agg([x for x in v if x is not None]) if any(x is not None for x in v) else None) for a, v in vals.items()}
        if "adage" in vals and "holdout" in vals:
            row["p_value"] = perm_p(vals["adage"], vals["holdout"], agg)
        out["metrics"][name] = row
    return out


# --------------------------------------------------------------------------------------- main

def parse(argv):
    opts = {"execute": False, "json": False, "root": None, "model": None, "round": 1, "adage": None, "phases": []}
    it = iter(argv)
    config = None
    for a in it:
        if a == "--execute":
            opts["execute"] = True
        elif a == "--json":
            opts["json"] = True
        elif a in ("--root", "--model", "--round", "--adage"):
            opts[a[2:]] = next(it, None)
        elif a in ("train", "learn", "eval", "report"):
            opts["phases"].append(a)
        elif a in ("-h", "--help"):
            print(__doc__)
            sys.exit(0)
        elif config is None:
            config = a
        else:
            raise SystemExit(f"unexpected argument {a!r}")
    if config is None:
        raise SystemExit("usage: runner.py <config.json> [train] [learn] [eval] [report] [--execute] (see --help)")
    opts["round"] = int(opts["round"])
    opts["phases"] = opts["phases"] or ["train", "learn", "eval", "report"]
    return config, opts


def main(argv):
    config, opts = parse(argv)
    cfg = load_config(config, {"model": opts["model"]})
    root = Path(opts["root"] or f"/tmp/adage-live/{cfg['name']}").resolve()
    adage = Path(opts["adage"] or REPO / "target" / "release" / "adage")
    p = describe(cfg, opts["phases"], opts["round"], root, adage)
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
    live = Live(cfg, root, adage, opts["round"])
    sessions = plan(cfg, opts["phases"], opts["round"])
    if "train" in opts["phases"]:
        live.phase([s for s in sessions if s[1] == "train"])
    if "learn" in opts["phases"]:
        live.learn()
    if "eval" in opts["phases"]:
        live.phase([s for s in sessions if s[1] == "eval"])
    if "report" in opts["phases"]:
        summary = summarise(live.state, opts["round"])
        (root / "summary.json").write_text(json.dumps(summary, indent=2))
        print(json.dumps(summary, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
