#!/usr/bin/env python3
"""The live runner (evals/live/runner.py) for the GO validation: the shipped product, used as a user
uses it.

  python3 gv_run.py <config.json> <phases...> [--execute] --host-auth --root R --adage BIN [--eval-repeats N]
                    [runner options]

It is the unchanged runner (isolation, `--host-auth`, `--confine`, arm order, scoring, state.json,
spend cap, resume) with these differences, each switched on by the config's `go_validation` block:

`train_as_user: true` (the family config). The training sessions ARE the user's history:
  - they run with Adage's hooks installed (`adage hooks-config`, as `adage init` installs them) and
    Adage's data directory (ADAGE_HOME = <root>/private/adage-home, which persists across sessions
    as a user's does). Nothing else is set: no ADAGE_ARM (the product assigns its own arm, with its
    default control share), no ADAGE_AUTO_LEARN (learning is on by default), no `adage condition`.
    The product's own background learner (`adage condition --auto`, started by the Stop/SessionEnd
    hooks) forms the family and compiles the card, exactly as after a user's sessions.
  - they run one at a time (a history is sequential), and after each one the driver waits until the
    learner the hooks started has finished (the queue files in ADAGE_HOME/learn-queue) before the
    scorer runs and the next session starts. Waiting is not an intervention: a user's next session
    also starts after the last one's learner, which takes seconds.
  - confinement keeps the runner's mounts but drops `--unshare-pid` and `--die-with-parent`, which
    would kill the detached learner when the session exits (as in evals/launch/zero_config_e2e.py).
    The learner then runs inside the session's own mount namespace: it sees this session's
    transcript and ADAGE_HOME; earlier sessions' transcripts it cannot see are served from the
    product's parse cache (adage-learn cache.rs: a transcript out of sight is not "gone").
  - the runner's `learn` phase is refused: the harness does not condition anything.
  Held-out sessions (`eval`) then use the runner's arms unchanged: `control` (hooks installed,
  directives withheld: ADAGE_ARM=control) and `adage` (ADAGE_ARM=treated), both reading the same
  ADAGE_HOME with learning frozen (ADAGE_AUTO_LEARN=0), so every pair sees the same index: the one
  the product learned from the 4 training sessions.

`learn_outcomes: false` (the regression slice). The runner's `learn` phase runs `adage condition`
  over the training transcripts WITHOUT the hidden-test outcomes (a user has none), with the
  config's `learn_args` (default thresholds).

`seed_train_from: <root>` (the regression slice). A new root is seeded with that root's finished
  training runs and their transcripts (as evals/repobench/rerun.py does), so no training is re-run.

`global_cap_usd` / `spend_roots`: before `--execute`, the spend already recorded in every root of
  this validation is summed; the driver refuses to start when it reaches the global cap ($15).

Before `--execute` it also refuses when CLAUDECODE or CLAUDE_CODE_SESSION_ID is set (run it with
`env -u CLAUDECODE -u CLAUDE_CODE_SESSION_ID`), and checks that the binary is the product under
test: `adage config` must list `learn.cards` (procedure cards). Without `--execute` it is the
runner's dry run (the plan), plus these checks' results.
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

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[2]
sys.path.insert(0, str(REPO / "evals" / "live"))
import runner  # noqa: E402

FORBIDDEN_ENV = ("CLAUDECODE", "CLAUDE_CODE_SESSION_ID")
# learn-queue files that are not a request (adage-hook autolearn.rs: `<slug>.json` is the request).
QUEUE_OTHER = (".state.json", ".spawn.json", ".sources.json", ".notice.json", ".notice-shown.json", ".lock")


def config_path(argv: list[str]) -> Path:
    if "--config" in argv:
        return Path(argv[argv.index("--config") + 1])
    for a in argv:
        if a.endswith(".json"):
            return Path(a)
    raise SystemExit("usage: gv_run.py <config.json> <phases...> [runner options]")


def opt(argv: list[str], name: str) -> str | None:
    return argv[argv.index(name) + 1] if name in argv and argv.index(name) + 1 < len(argv) else None


def root_of(cfg: dict, argv: list[str]) -> Path:
    return Path(opt(argv, "--root") or f"/tmp/adage-live/{cfg['name']}").resolve()


# ------------------------------------------------------------------------------ learner waiting

def learner_busy(home: Path) -> list[str]:
    """The repositories whose automatic learn is pending or running (ADAGE_HOME/learn-queue:
    `<slug>.json` is the latest request, `<slug>.state.json` the learner's last run)."""
    q = home / "learn-queue"
    busy = []
    for req in q.glob("*.json"):
        if req.name.endswith(QUEUE_OTHER):  # the learner's and the hook's other files
            continue
        slug = req.name[:-5]
        try:
            r = json.loads(req.read_text())
            st = json.loads((q / f"{slug}.state.json").read_text()) if (q / f"{slug}.state.json").exists() else {}
        except (OSError, ValueError):
            busy.append(slug)
            continue
        if st.get("outcome") == "running" or int(r.get("requested_at") or 0) > int(st.get("processed") or 0):
            busy.append(slug)
    return busy


def wait_learner(home: Path, timeout: float, log: Path, run: str) -> dict:
    t0 = time.time()
    time.sleep(3)  # the Stop/SessionEnd hooks write their request as the session exits
    state = "idle"
    while learner_busy(home):
        if time.time() - t0 > timeout:
            state = "timeout"
            break
        time.sleep(2)
    q = home / "learn-queue"
    states = {}
    for p in q.glob("*.state.json"):
        try:
            states[p.name[:-11]] = json.loads(p.read_text())
        except (OSError, ValueError):
            pass
    rec = {"run": run, "waited_s": round(time.time() - t0, 1), "result": state, "learner_states": states,
           "learn_log_lines": len((home / "learn.log").read_text().splitlines()) if (home / "learn.log").exists() else 0}
    with open(log, "a") as f:
        f.write(json.dumps(rec) + "\n")
    print(f"  learner after {run}: {state} ({rec['waited_s']} s)", flush=True)
    return rec


# ------------------------------------------------------------------------------------ patching

def install(gv: dict) -> None:
    as_user = bool(gv.get("train_as_user"))
    wait_s = float(gv.get("learner_wait_s", 900))

    if as_user:
        _kind, _env, _confine = runner.settings_kind, runner.env_for, runner.confine_argv

        def settings_kind(arm):
            return "hooks" if arm == "train" else _kind(arm)

        def env_for(arm, home):
            # The user's data directory, and nothing else: the product's defaults decide the rest.
            return {"ADAGE_HOME": str(home)} if arm == "train" else _env(arm, home)

        def confine_argv(cfg, run_dir, env, ro=(), rw=()):
            argv = _confine(cfg, run_dir, env, ro, rw)
            if Path(run_dir).name.startswith("train-"):
                argv = [x for x in argv if x not in ("--die-with-parent", "--unshare-pid")]
            return argv

        runner.settings_kind, runner.env_for, runner.confine_argv = settings_kind, env_for, confine_argv

        _score, _phase = runner.Live.score, runner.Live.phase

        def score(self, workdir, task):
            run = Path(workdir).parent.name
            if run.startswith("train-"):
                wait_learner(self.home, wait_s, self.private / "learner-waits.jsonl", run)
            return _score(self, workdir, task)

        def phase(self, name, sessions):
            if name != "train":
                return _phase(self, name, sessions)
            # The confinement mounts ADAGE_HOME back only when it exists (runner.confine_argv); on
            # the first training session nothing has created it yet, so the hooks and the learner
            # wrote to the tmpfs over /tmp and everything they learned vanished with the sandbox.
            # Found by the first live training run (2026-09-29), voided and re-run.
            self.home.mkdir(parents=True, exist_ok=True)
            saved = self.cfg["concurrency"]
            self.cfg["concurrency"] = 1
            try:
                return _phase(self, name, sessions)
            finally:
                self.cfg["concurrency"] = saved

        def learn(self):
            raise SystemExit("train_as_user: the product conditions itself after each training session; the "
                             "runner's `learn` phase would be a harness intervention (run train, then eval)")

        runner.Live.score, runner.Live.phase, runner.Live.learn = score, phase, learn

    if gv.get("learn_outcomes") is False:
        def learn_user(self):
            """Live.learn without the hidden-test outcomes (a user's `adage condition` has none)."""
            self.set_phase("learn")
            runs = runner.learning_runs(self.state["runs"].values(), self.round, None)
            transcripts = []
            for r in runs:
                hits = list(self.transcripts.glob(f"*/{r['metrics']['session_id']}.jsonl"))
                if hits:
                    transcripts.append(str(hits[0]))
            if self.round == 1:
                shutil.rmtree(self.home / "tracks", ignore_errors=True)
            cmd = [str(self.adage), "condition", *self.cfg["learn_args"], "--json", "--repo", self.cfg["repo_key"],
                   "--transcripts", *transcripts]
            out = subprocess.run(cmd, capture_output=True, text=True, env=dict(os.environ, ADAGE_HOME=str(self.home)))
            if out.returncode != 0:
                raise SystemExit(out.stderr)
            rep = json.loads(out.stdout)
            idx = self.home / "index.json"
            tracks = json.loads(idx.read_text()).get("routes", []) if idx.exists() else []
            self.state["rounds"][str(self.round)] = {"learn": rep, "tracks": tracks, "sessions": len(transcripts),
                                                     "leaky": False, "pool": None, "outcomes": "none (as a user)"}
            self.state["learn"], self.state["tracks"] = rep, tracks
            self.save()
            print(json.dumps(rep, indent=2))

        runner.Live.learn = learn_user
        _describe = runner.describe

        def describe(*a, **k):
            p = _describe(*a, **k)
            if p.get("learn") and "--outcomes" in p["learn"]:
                i = p["learn"].index("--outcomes")
                p["learn"] = p["learn"][:i] + p["learn"][i + 2:]
            return p

        runner.describe = describe


# ------------------------------------------------------------------------------------- seeding

def seed(root: Path, src: Path, cfg: dict) -> None:
    """Seed a new root with a finished root's training runs and their transcripts (rerun.py)."""
    if (root / "state.json").exists():
        return
    state = json.loads((src / "state.json").read_text())
    train = {k: v for k, v in state["runs"].items() if v.get("phase") == "train" and v["task"] in cfg["train"]["tasks"]}
    tdir = root / "private" / "transcripts"
    tdir.mkdir(parents=True, exist_ok=True)
    n = 0
    for r in train.values():
        sid = (r.get("metrics") or {}).get("session_id")
        for p in (src / "private" / "transcripts").glob(f"*/{sid}.jsonl"):
            (tdir / p.parent.name).mkdir(exist_ok=True)
            shutil.copy(p, tdir / p.parent.name / p.name)
            n += 1
    cost = sum((r.get("metrics") or {}).get("cost_usd") or 0.0 for r in train.values())
    (root / "state.json").write_text(json.dumps({"config": cfg["name"], "runs": train, "rounds": {},
                                                 "seed": state.get("seed"), "seeded_from": str(src),
                                                 "train_cost_usd": round(cost, 4)}))
    print(f"seeded {root} with {len(train)} training runs ({n} transcripts, ${cost:.2f} already spent) from {src}")


# ------------------------------------------------------------------------------------- checks

def spent(roots: list[str]) -> dict:
    """Spend recorded in each root's state.json, excluding runs seeded from elsewhere."""
    out = {}
    for r in roots:
        p = Path(r) / "state.json"
        if not p.exists():
            out[r] = 0.0
            continue
        st = json.loads(p.read_text())
        total = sum((x.get("metrics") or {}).get("cost_usd") or 0.0 for x in st.get("runs", {}).values())
        out[r] = round(total - float(st.get("train_cost_usd") or 0.0), 4)
    return out


def product_check(adage: Path) -> dict:
    """Is this binary the product under test (procedure cards) and what does it install?"""
    if not adage.exists():
        return {"ok": False, "why": f"missing {adage}"}
    with tempfile.TemporaryDirectory() as t:
        env = dict(os.environ, ADAGE_HOME=t, HOME=t)
        cfg = subprocess.run([str(adage), "config"], capture_output=True, text=True, env=env).stdout
        hooks = subprocess.run([str(adage), "hooks-config", str(adage)], capture_output=True, text=True, env=env).stdout
    try:
        events = sorted(json.loads(hooks)["hooks"])
    except (ValueError, KeyError):
        events = []
    ok = "learn.cards" in cfg and "UserPromptSubmit" in events and "Stop" in events
    return {"ok": ok, "cards": "learn.cards" in cfg, "auto_learn_default_on": "learn.auto               = true" in cfg,
            "hook_events": events, "version": subprocess.run([str(adage), "--version"], capture_output=True,
                                                             text=True).stdout.strip()}


def overrides(eval_repeats: int | None, budget: float | None) -> None:
    """Config overrides applied as the runner loads the config: the eval repeats (`--eval-repeats`,
    for the R4 interim after repeat 1; a later call without it resumes and runs only repeat 2) and
    the spend cap clamped to what remains of the global cap."""
    _load = runner.load_config

    def load_config(path, ovr):
        cfg = _load(path, ovr)
        if eval_repeats is not None:
            cfg["eval"] = {**cfg["eval"], "repeats": eval_repeats}
        if budget is not None and budget < cfg["budget_usd"]:
            print(f"spend cap clamped to ${budget:.2f} by the global cap (was ${cfg['budget_usd']})", flush=True)
            cfg["budget_usd"] = round(budget, 2)
        return cfg

    runner.load_config = load_config


def main(argv: list[str]) -> int:
    eval_repeats = None
    if "--eval-repeats" in argv:
        i = argv.index("--eval-repeats")
        eval_repeats = int(argv[i + 1])
        argv = argv[:i] + argv[i + 2:]
    cpath = config_path(argv)
    cfg = json.loads(cpath.read_text())
    gv = cfg.get("go_validation") or {}
    root = root_of(cfg, argv)
    execute = "--execute" in argv
    phases = [a for a in argv if a in ("train", "learn", "eval", "report")]
    if gv.get("train_as_user") and "learn" in phases:
        raise SystemExit("train_as_user: do not run the runner's `learn` phase (the product learns by itself)")
    adage = Path(opt(argv, "--adage") or REPO / "target" / "release" / "adage").resolve()
    pc = product_check(adage)
    print(f"product check: {json.dumps(pc)}")
    if execute:
        bad = [k for k in FORBIDDEN_ENV if k in os.environ]
        if bad:
            raise SystemExit(f"refusing: {', '.join(bad)} set; run with `env -u CLAUDECODE -u CLAUDE_CODE_SESSION_ID`")
        for k in ("--host-auth",):
            if k not in argv and not cfg.get("host_auth"):
                raise SystemExit("the validation runs with --host-auth")
        if not cfg.get("confine"):
            raise SystemExit("the validation runs confined (config confine: true)")
        if not pc["ok"]:
            raise SystemExit(f"{adage} is not the product under test (needs learn.cards and the prompt/stop hooks)")
        cap = float(gv.get("global_cap_usd", 15.0))
        sp = spent(gv.get("spend_roots") or [str(root)])
        print(f"spend so far across the validation's roots: {sp} = ${sum(sp.values()):.2f} of ${cap}")
        if sum(sp.values()) >= cap:
            raise SystemExit("global cap reached; stopping (prereg stopping rule R1)")
        others = sum(v for k, v in sp.items() if Path(k).resolve() != root)
        remaining = cap - others - sp.get(str(root), 0.0)
        if remaining < float(gv.get("min_remaining_usd", 0.0)):
            raise SystemExit(f"only ${remaining:.2f} of the global cap remains (< ${gv['min_remaining_usd']}); "
                             "not starting (prereg stopping rule R6)")
        if gv.get("seed_train_from"):
            seed(root, Path(gv["seed_train_from"]), cfg)
        # The runner counts every run in the root's state (seeded training included) against its cap.
        seeded = float((json.loads((root / "state.json").read_text()) if (root / "state.json").exists() else {})
                       .get("train_cost_usd") or 0.0)
        overrides(eval_repeats, seeded + cap - others)
    else:
        overrides(eval_repeats, None)
    install(gv)
    return runner.main(argv)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
