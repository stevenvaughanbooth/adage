#!/usr/bin/env python3
"""The learning curve: does Adage get cheaper as you use it?

  python3 evals/repobench/curve.py <repo-id> --root DIR [--n 20] [--budget USD] --execute

One repository's selected tasks (build.py's latest `--n`), in the chronological order of their
fixes, are worked one after another by two sequences of real agents, run side by side:

  adage    hooks on, directives delivered, learning after every session: the driver conditions
           the repository on every earlier session of this sequence (`adage condition --demo`,
           the thresholds `learn.thresholds = demo` gives automatic learning; no outcomes, as
           automatic learning has none), then the next task starts. Session k sees what sessions
           1..k-1 taught.
  control  hooks on, directives withheld (ADAGE_ARM=control); nothing is learned.

Why the driver and not the hook's own learner: under `confine` each session sees only its own run
directory, and the learner the Stop hook starts inside it (ADAGE_AUTO_LEARN=1) drops every
transcript it cannot see from `learn-queue/<repo>.sources.json` and relearns from the current
session alone (the first click curve, run with `--in-session-learner`, went from 7 remembered
sessions back to 1). Outside a sandbox the hook's learner reads the transcripts the driver passes.
`--in-session-learner` restores the hook's learner, and then runs `adage condition --auto
--repo-root <sandbox>` after each session, as the first click curve did. Every session is a runner
session (evals/live/runner.py `Live`): isolated, confined, scored by the hidden tests. Results:
<root>/<arm>/state.json and <root>/curve.json (per position: task, arm, cost, turns, passed,
reflexes live before it).
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import threading
import time
from pathlib import Path

import build
import common

sys.path.insert(0, str(common.REPO / "evals" / "live"))
import runner  # noqa: E402

ADAGE = common.REPO / "target" / "release" / "adage"
_env_for = runner.env_for


IN_SESSION_LEARNER = False


def env_for(arm, home):
    env = _env_for(arm, home)
    if arm == "adage" and IN_SESSION_LEARNER:
        env["ADAGE_AUTO_LEARN"] = "1"
    return env


runner.env_for = env_for


def adage_env(home: Path) -> dict:
    env = dict(os.environ, ADAGE_HOME=str(home))
    env.pop("CLAUDECODE", None)
    # The learner's own sources: none but the transcripts the hooks named (no operator history).
    env["HOME"] = str(home / "operator-home")
    (home / "operator-home").mkdir(parents=True, exist_ok=True)
    return env


def prepare_home(home: Path) -> None:
    home.mkdir(parents=True, exist_ok=True)
    env = adage_env(home)
    # `adage config set` is a decision for a person and wants a terminal: give it one (`script`),
    # as the operator who set this experiment up.
    for k, v in (("learn.thresholds", "demo"), ("learn.auto", "true")):
        out = subprocess.run(["script", "-qec", f"{ADAGE} config set {k} {v}", "/dev/null"], env=env,
                             capture_output=True, text=True, stdin=subprocess.DEVNULL)
        if out.returncode != 0:
            raise SystemExit(f"adage config set {k} {v}: {out.stdout[-300:]} {out.stderr[-300:]}")


def index_routes(home: Path, key: str) -> list[dict]:
    p = home / "index.json"
    if not p.exists():
        return []
    return [r for r in json.loads(p.read_text()).get("routes", []) if r.get("repo") == key]


def learner_state(home: Path) -> list[dict]:
    q = home / "learn-queue"
    return [json.loads(p.read_text()) for p in sorted(q.glob("*.state.json"))] if q.exists() else []


def condition_after(home: Path, sandbox: Path, key: str, transcripts: list[str]) -> dict:
    t0 = time.time()
    if IN_SESSION_LEARNER:
        argv = [str(ADAGE), "condition", "--auto", "--repo-root", str(sandbox)]
    else:
        argv = [str(ADAGE), "condition", "--demo", "--json", "--repo", key, "--transcripts", *transcripts]
    out = subprocess.run(argv, env=adage_env(home), capture_output=True, text=True, timeout=600)
    return {"exit": out.returncode, "seconds": round(time.time() - t0, 2), "stderr": out.stderr[-400:],
            "transcripts": len(transcripts)}


def sequence(live: runner.Live, arm: str, tasks: list[dict], key: str, log: list, lock: threading.Lock) -> None:
    for i, t in enumerate(tasks, 1):
        before = index_routes(live.home, key)
        run_id = f"seq-{i:02d}-{t['id']}-{arm}"
        existing = live.state["runs"].get(run_id)
        if not runner.finished(existing, "eval", arm, t["id"], 1):
            live.run_one(run_id, "eval", arm, t["id"], t["prompt"].replace("{", "{{").replace("}", "}}"))
        r = live.state["runs"].get(run_id, {})
        entry = {"pos": i, "task": t["id"], "arm": arm, "run": run_id,
                 "reflexes_before": sum(1 for x in before if not common.is_instinct(x)),
                 "instincts_before": sum(1 for x in before if common.is_instinct(x)),
                 "metrics": {k: (r.get("metrics") or {}).get(k) for k in
                             ("cost_usd", "turns", "passed", "turns_to_first_edit", "elapsed", "dead_end_hits", "status")}}
        if arm == "adage" and r.get("metrics"):
            entry["in_session_learner"] = learner_state(live.home)
            sids = [(x.get("metrics") or {}).get("session_id") for k, x in sorted(live.state["runs"].items())
                    if k.startswith("seq-") and int(k[4:6]) <= i]
            transcripts = [str(p) for sid in sids if sid for p in live.transcripts.glob(f"*/{sid}.jsonl")]
            entry["condition"] = condition_after(live.home, live.root / "runs" / run_id / live.cfg["workdir"], key,
                                                 transcripts)
            entry["reflexes_after"] = [x["route"]["id"] for x in index_routes(live.home, key)
                                       if not common.is_instinct(x)]
        with lock:
            log.append(entry)
        print(f"  [{arm}] {i:2d} {t['id']}: passed={entry['metrics']['passed']} turns={entry['metrics']['turns']} "
              f"cost={entry['metrics']['cost_usd']} reflexes_before={entry['reflexes_before']}", flush=True)


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("repo")
    ap.add_argument("--root", required=True)
    ap.add_argument("--n", type=int, default=20)
    ap.add_argument("--budget", type=float, default=10.0)
    ap.add_argument("--model", default="claude-haiku-4-5-20251001")
    ap.add_argument("--max-turns", type=int, default=30)
    ap.add_argument("--in-session-learner", action="store_true",
                    help="ADAGE_AUTO_LEARN=1 in the sessions, and `condition --auto` after each (see above)")
    ap.add_argument("--execute", action="store_true")
    a = ap.parse_args(argv)
    global IN_SESSION_LEARNER
    IN_SESSION_LEARNER = a.in_session_learner
    tasks = build.select(a.repo, a.n)
    r = common.repo(a.repo)
    key = common.repo_key(r)
    root = Path(a.root).resolve()
    print(f"{a.repo}: {len(tasks)} tasks in order, arms adage (learning) and control; key {key}")
    for i, t in enumerate(tasks, 1):
        print(f"  {i:2d} {t['date'][:10]} {t['id']}")
    if not a.execute:
        print("dry run: add --execute")
        return 0
    if os.environ.get("CLAUDECODE"):
        raise SystemExit("refusing to run live agents from inside an agent session (CLAUDECODE is set)")
    lives = {}
    for arm in ("adage", "control"):
        cfg = runner.load_config(common.HERE / "configs" / f"{a.repo}.json", {})
        cfg.update(name=f"curve-{a.repo}-{arm}", model=a.model, max_turns=a.max_turns, budget_usd=a.budget / 2,
                   task_prompts={t["id"]: t["prompt"] for t in tasks}, seed=build.SEED)
        cfg["train"] = {"tasks": [], "repeats": 1}
        cfg["eval"] = {"tasks": [t["id"] for t in tasks], "repeats": 1, "arms": [arm]}
        sub = root / arm
        sub.mkdir(parents=True, exist_ok=True)
        live = runner.Live(cfg, sub, ADAGE, 1)
        live.state["curve"] = {"repo": a.repo, "arm": arm, "order": [t["id"] for t in tasks]}
        if arm == "adage":
            prepare_home(live.home)
        lives[arm] = live
    log, lock = [], threading.Lock()
    threads = [threading.Thread(target=sequence, args=(lives[arm], arm, tasks, key, log, lock)) for arm in lives]
    for th in threads:
        th.start()
    for th in threads:
        th.join()
    log.sort(key=lambda e: (e["arm"], e["pos"]))
    (root / "curve.json").write_text(json.dumps({"repo": a.repo, "key": key, "model": a.model,
                                                 "in_session_learner": IN_SESSION_LEARNER, "entries": log},
                                                indent=1))
    spent = sum((e["metrics"].get("cost_usd") or 0) for e in log)
    print(f"curve: {len(log)} sessions, ${spent:.2f} -> {root / 'curve.json'}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
