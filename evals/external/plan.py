#!/usr/bin/env python3
"""Plan, harvest and record Adage runs on Harbor datasets (stdlib only; never runs an agent).

  plan.py split    --registry R --dataset swebenchpro@1.0 --tasks-dir D --clones C --out split.json
  plan.py phases   --registry R --dataset swebenchpro@1.0 [--split split.json] [--pilot 20] [--write-configs DIR]
  plan.py harvest  --jobs JOBDIR... --split split.json --out harvest/
  plan.py emit     --jobs JOBDIR... --suite swebenchpro [--split split.json] --out records.jsonl
  plan.py check-leakage --home <ADAGE_HOME> --split split.json --tasks-dir D --harvest harvest/<repo> --repo <owner/name>
  plan.py check-patches --jobs JOBDIR...
  plan.py analyse  --records records.jsonl [--b 5000] [--seed 0]
  plan.py power    [--discordance 0.1,0.2,0.3] [--delta 0.03,0.05,0.1]

`phases` prints every command of the protocol, in order, with a cost estimate from PER_TRIAL_USD.
Those figures are ESTIMATES (list-price guesses per trial, not measurements); the pilot replaces
them with measured cost per trial before the full run is approved.
"""

from __future__ import annotations

import argparse
import json
import random
import shutil
import sys
from collections import defaultdict
from datetime import datetime
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import protocol  # noqa: E402
import stats  # noqa: E402

AGENT = "adage_harbor:AdageClaudeCode"
ARMS = ("off", "holdout", "adage")
# ESTIMATED agent cost per trial in USD (list prices, typical SWE-agent turn counts). Not measured.
PER_TRIAL_USD = {
    "swebenchpro": {"haiku": 0.40, "sonnet": 1.50, "opus": 4.00},
    "swebench-verified": {"haiku": 0.25, "sonnet": 0.90, "opus": 2.50},
    "swegym": {"haiku": 0.25, "sonnet": 0.90, "opus": 2.50},
    "swesmith": {"haiku": 0.15, "sonnet": 0.60, "opus": 1.60},
    "swe-gen-js": {"haiku": 0.30, "sonnet": 1.10, "opus": 3.00},
    "terminal-bench": {"haiku": 0.30, "sonnet": 1.20, "opus": 3.50},
}
SPOOL_MAGIC = bytes([0xAD, 0xA6, 0xE5, 0x01])
CENSOR_EXC = {"AgentTimeoutError"}
SCORER_EXC = {"VerifierTimeoutError", "RewardFileNotFoundError", "RewardFileEmptyError", "VerifierOutputParseError",
              "AddTestsDirError", "DownloadVerifierDirError"}
CENSOR_SUBTYPES = {"error_max_turns", "error_max_budget_usd"}


def model_family(model: str) -> str:
    m = model.lower()
    return next((f for f in ("haiku", "sonnet", "opus") if f in m), "sonnet")


def parse_dataset(s: str) -> tuple[str, str | None]:
    name, _, ver = s.partition("@")
    return name, ver or None


def repo_key(suite: str, repo: str) -> str:
    """The canonical key an index is learned under (adage condition --repo)."""
    return f"adage-eval:{suite}/{protocol.repo_slug(repo)}"


# -------------------------------------------------------------------------------------- split

def cmd_split(a) -> int:
    reg = json.loads(Path(a.registry).read_text())
    name, ver = parse_dataset(a.dataset)
    entry = protocol.registry_entry(reg, name, ver)
    metas = []
    missing = []
    for t in entry["tasks"]:
        d = Path(a.tasks_dir) / t["name"]
        m = protocol.load_task_meta(d, t["name"]) if d.exists() else None
        if m is None or not m.base_commit:
            missing.append(t["name"])
            continue
        m.repo = m.repo or protocol.repo_from_task_name(t["name"])
        metas.append(m)
    if missing and not a.allow_missing:
        print(f"{len(missing)} task(s) have no tests/config.json under {a.tasks_dir}, e.g. {missing[:3]}.\n"
              f"Fetch the metadata first (see `phases`), or pass --allow-missing to split what is there.",
              file=sys.stderr)
        return 2
    dates = protocol.CommitDates(a.clones, a.dates_cache)
    split = protocol.chronological_split(metas, dates, a.train_frac, a.buffer, a.min_test)
    split.update({"dataset": a.dataset, "digest": protocol.split_digest(split), "missing": missing})
    Path(a.out).write_text(json.dumps(split, indent=1, sort_keys=True))
    for r, v in split["repos"].items():
        print(f"{r:<40} train {len(v['train']):>3}  buffer {len(v['buffer'])}  test {len(v['test']):>3}  cutoff {v['cutoff']}")
    for r, v in split["excluded"].items():
        print(f"{r:<40} excluded: {v['reason']} (n={v['n']})")
    print(f"split digest {split['digest']}  (publish before running eval jobs)")
    return 0


# ------------------------------------------------------------------------------------- phases

def pilot_subset(split: dict, n: int, seed: int) -> dict:
    """The pilot: the two largest repositories; the latest n train tasks of each (closest to the
    cutoff); n test tasks in all, drawn round-robin from their test sets in seeded order."""
    repos = sorted(split["repos"], key=lambda r: (-len(split["repos"][r]["test"]), r))[:2]
    rng = random.Random(seed)
    pools = {r: rng.sample(split["repos"][r]["test"], len(split["repos"][r]["test"])) for r in repos}
    test = {r: [] for r in repos}
    i = 0
    while sum(map(len, test.values())) < n and any(pools.values()):
        r = repos[i % len(repos)]
        if pools[r]:
            test[r].append(pools[r].pop())
        i += 1
    out = {k: v for k, v in split.items() if k != "repos"}
    out["repos"] = {r: {**split["repos"][r], "train": split["repos"][r]["train"][-n:], "test": test[r]} for r in repos}
    return out


def task_configs(entry: dict, names: list[str], source: str) -> list[dict]:
    by = {t["name"]: t for t in entry["tasks"]}
    out = []
    for n in names:
        t = by[n]
        out.append({"path": t["path"], "git_url": t["git_url"], "git_commit_id": t["git_commit_id"], "source": source})
    return out


def agent_config(model: str, arm: str, a, repo: str | None = None, home_repo: str | None = None,
                 label: str | None = None) -> dict:
    kw = {"arm": arm, "disable_web_search": True}
    if a.max_turns:
        kw["max_turns"] = a.max_turns
    if arm in ("holdout", "adage", "agents-md"):
        src = home_repo or repo
        kw["adage_home"] = str(Path(a.artifacts).resolve() / f"{protocol.repo_slug(src)}.tgz")
        kw["adage_repo_key"] = repo_key(a.suite, src)
        if arm != "agents-md":
            kw["adage_bin"] = str(Path(a.adage_bin).resolve())
    if label:
        kw["arm_label"] = label
    return {"import_path": AGENT, "model_name": model, "kwargs": kw}


def job(name: str, a, agents: list[dict], tasks: list[dict], attempts: int = 1) -> dict:
    return {"job_name": name, "jobs_dir": a.jobs_dir, "n_attempts": attempts, "n_concurrent_trials": a.concurrency,
            "agents": agents, "tasks": tasks}


def build_phases(a, entry: dict, split: dict) -> dict:
    suite = a.suite
    configs, cmds = {}, []
    repos = sorted(split["repos"])
    train_names = [t for r in repos for t in split["repos"][r]["train"]]
    tj = f"{suite}-train"
    configs[f"{tj}.json"] = job(tj, a, [agent_config(a.model, "off", a)], task_configs(entry, train_names, suite),
                                a.train_repeats)
    cfg_dir = Path(a.write_configs or "configs")
    env = f"PYTHONPATH={HERE}"
    cmds.append(("1. train (arm off, no hooks)", [f"{env} harbor run -c {cfg_dir / (tj + '.json')}"]))
    cmds.append(("2. harvest train transcripts per repository",
                 [f"python3 {HERE / 'plan.py'} harvest --jobs {a.jobs_dir}/{tj} --split {a.split} --out {a.harvest}"]))
    learn, freeze = [], []
    for r in repos:
        s = protocol.repo_slug(r)
        home = Path(a.artifacts) / s / "home"
        learn.append(f"rm -rf {home} && mkdir -p {home} && ADAGE_HOME={home} {a.adage_host} learn "
                     f"--transcripts {a.harvest}/{s}/*.jsonl --outcomes {a.harvest}/{s}/outcomes.json "
                     f"--repo {repo_key(suite, r)} --no-cache --json > {Path(a.artifacts) / s}/learn.json")
        freeze.append(f"tar -C {home} -czf {Path(a.artifacts) / (s + '.tgz')} index.json tracks && "
                      f"python3 {HERE / 'plan.py'} check-leakage --home {home} --split {a.split} "
                      f"--tasks-dir {a.tasks_dir} --harvest {a.harvest}/{s} --repo {r}")
    freeze.append(f"(cd {a.artifacts} && sha256sum *.tgz > MANIFEST.sha256)   # the frozen artifact; no learning after this")
    cmds.append(("3. adage condition, one ADAGE_HOME per repository (default thresholds, frozen afterwards)", learn))
    cmds.append(("4. freeze + leakage check (a finding blocks the eval)", freeze))
    rng = random.Random(a.seed)
    lro = protocol.leave_repo_out(repos, a.seed) if a.leave_repo_out and len(repos) > 1 else {}
    eval_jobs = []
    for rep in range(1, a.repeats + 1):
        for r in repos:
            arms = list(a.arms)
            rng.shuffle(arms)
            agents = [agent_config(a.model, arm, a, r) for arm in arms]
            if r in lro:
                agents.append(agent_config(a.model, "adage", a, r, home_repo=lro[r], label="adage-lro"))
                rng.shuffle(agents)
            tests = list(split["repos"][r]["test"])
            rng.shuffle(tests)
            name = f"{suite}-eval-{protocol.repo_slug(r)}-r{rep}"
            configs[f"{name}.json"] = job(name, a, agents, task_configs(entry, tests, suite))
            eval_jobs.append(name)
    rng.shuffle(eval_jobs)
    cmds.append((f"5. eval: {len(eval_jobs)} jobs (repository x repeat), arms interleaved within each task, "
                 f"job order seeded ({a.seed})", [f"{env} harbor run -c {cfg_dir / (n + '.json')}" for n in eval_jobs]))
    jobs = " ".join(f"{a.jobs_dir}/{n}" for n in eval_jobs)
    cmds.append(("6. patch cleanliness, records, analysis", [
        f"python3 {HERE / 'plan.py'} check-patches --jobs {jobs}",
        f"python3 {HERE / 'plan.py'} emit --jobs {jobs} --suite {suite} --split {a.split} --out {a.records}",
        f"python3 {HERE / 'plan.py'} analyse --records {a.records}"]))
    n_train = len(train_names) * a.train_repeats
    n_eval = sum(len(c["tasks"]) * len(c["agents"]) for k, c in configs.items() if "-eval-" in k)
    per = PER_TRIAL_USD.get(suite, PER_TRIAL_USD["swebenchpro"])[model_family(a.model)]
    return {"configs": configs, "commands": cmds, "trials": {"train": n_train, "eval": n_eval},
            "estimate_usd": {"per_trial": per, "train": n_train * per, "eval": n_eval * per,
                             "total": (n_train + n_eval) * per}}


def cmd_phases(a) -> int:
    reg = json.loads(Path(a.registry).read_text())
    name, ver = parse_dataset(a.dataset)
    entry = protocol.registry_entry(reg, name, ver)
    a.suite = a.suite or name
    print(f"# Adage on {a.dataset} via Harbor: protocol {protocol.PROTOCOL_VERSION}, model {a.model}, "
          f"arms {','.join(a.arms)}{' + adage-lro' if a.leave_repo_out else ''}, repeats {a.repeats}, seed {a.seed}")
    print("# 0. metadata + split (once; the split digest is the pre-registration)")
    t0 = entry["tasks"][0]
    parent = Path(t0["path"]).parent
    src = f"{a.tasks_dir}-src"
    print(f"git clone --filter=blob:none --no-checkout {t0['git_url']} {src} && "
          f"git -C {src} sparse-checkout set --no-cone '/{parent}/*/tests/config.json' && "
          f"git -C {src} checkout {t0['git_commit_id']} && ln -sfn {src}/{parent} {a.tasks_dir}")
    print(f"python3 {HERE / 'plan.py'} split --registry {a.registry} --dataset {a.dataset} --tasks-dir {a.tasks_dir} "
          f"--clones {a.clones} --dates-cache {a.clones}/dates.json --out {a.split}")
    if not a.split or not Path(a.split).exists():
        by = defaultdict(int)
        for t in entry["tasks"]:
            by[protocol.repo_from_task_name(t["name"])] += 1
        n = sum(by.values())
        per = PER_TRIAL_USD.get(a.suite, PER_TRIAL_USD["swebenchpro"])[model_family(a.model)]
        if a.pilot:
            n_train, n_test = 2 * a.pilot, a.pilot
        else:
            n_train = int(n * protocol.DEFAULT_TRAIN_FRAC)
            n_test = max(0, n - n_train - protocol.DEFAULT_BUFFER * len(by))
        n_eval = n_test * a.repeats * (len(a.arms) + (1 if a.leave_repo_out else 0))
        print(f"\n# no split yet ({a.split}): rough counts from the registry only: {len(by)} repositories, {n} tasks")
        print(f"# ESTIMATE (not measured): train {n_train} trials + eval {n_eval} trials at ~${per:.2f}/trial "
              f"= ~${(n_train + n_eval) * per:,.0f}")
        return 0
    split = json.loads(Path(a.split).read_text())
    if a.pilot:
        split = pilot_subset(split, a.pilot, a.seed)
        print(f"# PILOT: {len(split['repos'])} repositories, {sum(len(v['train']) for v in split['repos'].values())} "
              f"train and {sum(len(v['test']) for v in split['repos'].values())} test tasks")
    print(f"# split digest {split.get('digest') or protocol.split_digest(split)}")
    p = build_phases(a, entry, split)
    if a.write_configs:
        d = Path(a.write_configs)
        d.mkdir(parents=True, exist_ok=True)
        for fn, c in p["configs"].items():
            (d / fn).write_text(json.dumps(c, indent=1))
        print(f"# wrote {len(p['configs'])} Harbor job configs to {d}")
    else:
        print("# (add --write-configs DIR to write the job configs these commands name)")
    for title, lines in p["commands"]:
        print(f"\n# {title}")
        for line in lines:
            print(line)
    e = p["estimate_usd"]
    print(f"\n# COST ESTIMATE (list-price guess ${e['per_trial']:.2f}/trial for {a.suite}/{model_family(a.model)}; "
          f"NOT measured): train {p['trials']['train']} trials ~${e['train']:,.0f}, eval {p['trials']['eval']} trials "
          f"~${e['eval']:,.0f}, total ~${e['total']:,.0f}")
    return 0


# ------------------------------------------------------------------------------- trial reading

def trial_dirs(job_dirs: list[str]) -> list[Path]:
    """Trial directories: those holding a result.json with a task_name (the job's own result.json
    has none)."""
    out = []
    for j in job_dirs:
        for rp in sorted(Path(j).rglob("result.json")):
            try:
                d = json.loads(rp.read_text())
            except ValueError:
                continue
            if isinstance(d, dict) and "task_name" in d:
                out.append(rp.parent)
    return out


def match_task(name: str, known: set[str]) -> str:
    """The split's task id for a Harbor task name (which may carry an org prefix)."""
    if name in known:
        return name
    base = name.rsplit("/", 1)[-1]
    if base in known:
        return base
    hits = [k for k in known if name.endswith(k) or base.endswith(k)]
    return max(hits, key=len) if hits else base


def split_index(split: dict | None) -> dict[str, tuple[str, str]]:
    """task id -> (repo, part) for every task in the split."""
    out = {}
    for repo, v in (split or {}).get("repos", {}).items():
        for part in ("train", "buffer", "test"):
            for t in v[part]:
                out[t] = (repo, part)
    return out


def session_files(trial: Path) -> list[Path]:
    """Claude Code transcripts Harbor brought back: agent/sessions/projects/<slug>/*.jsonl, without
    subagent transcripts (claude_code.py `_session_dirs`)."""
    root = trial / "agent" / "sessions" / "projects"
    return sorted(f for f in root.rglob("*.jsonl") if "subagents" not in f.parts) if root.is_dir() else []


def reward_of(res: dict):
    rw = (res.get("verifier_result") or {}).get("rewards")
    if not rw:
        return None
    return rw.get("reward", max(rw.values()))


def stream_result(trial: Path) -> dict:
    """The terminal `result` event of Claude's stream-json output (agent/claude-code.txt)."""
    p = trial / "agent" / "claude-code.txt"
    last = {}
    if p.exists():
        for line in p.read_text(errors="replace").splitlines():
            if '"type":"result"' in line.replace(" ", ""):
                try:
                    m = json.loads(line)
                except ValueError:
                    continue
                if m.get("type") == "result":
                    last = m
    return last


def transcript_turns(files: list[Path]) -> int | None:
    """Assistant turns: distinct assistant message ids across the session transcripts."""
    ids = set()
    for f in files:
        for line in f.read_text(errors="replace").splitlines():
            try:
                m = json.loads(line)
            except ValueError:
                continue
            if m.get("type") == "assistant":
                ids.add((m.get("message") or {}).get("id") or m.get("uuid"))
    return len(ids) or None


def spool_count(trial: Path) -> int:
    n = 0
    for f in (trial / "agent" / "adage-home" / "sessions").glob("*.spool"):
        n += f.read_bytes().count(SPOOL_MAGIC)
    return n


def status_of(res: dict, sr: dict) -> str:
    exc = (res.get("exception_info") or {}).get("exception_type")
    if exc in CENSOR_EXC or sr.get("subtype") in CENSOR_SUBTYPES:
        return "censored" if reward_of(res) is not None else "infra_error"
    if exc in SCORER_EXC:
        return "scorer_error"
    if exc:
        return "infra_error"
    if reward_of(res) is None:
        return "scorer_error"
    return "ok"


def _secs(t: dict | None):
    try:
        return (datetime.fromisoformat(t["finished_at"]) - datetime.fromisoformat(t["started_at"])).total_seconds()
    except (TypeError, KeyError, ValueError):
        return None


def trial_record(trial: Path, suite: str, idx: dict) -> dict:
    res = json.loads((trial / "result.json").read_text())
    agent_cfg = (res.get("config") or {}).get("agent") or {}
    kw = agent_cfg.get("kwargs") or {}
    arm = kw.get("arm_label") or kw.get("arm") or "off"
    tid = match_task(res["task_name"], set(idx))
    repo = idx.get(tid, (None,))[0] or protocol.repo_from_task_name(tid)
    sr = stream_result(trial)
    ar = res.get("agent_result") or {}
    sessions = session_files(trial)
    ai = res.get("agent_info") or {}
    setup_p = trial / "agent" / "adage" / "setup.json"
    setup = json.loads(setup_p.read_text()) if setup_p.exists() else {}
    status = status_of(res, sr)
    reward = reward_of(res)
    rec = {
        "run_id": f"{trial.parent.name}/{trial.name}",
        "tier": "l4", "suite": suite, "task_id": tid,
        "repo_key": protocol.repo_slug(repo) if repo else None,
        "arm": arm, "repeat": None, "status": status,
        "outcome": {"passed": None if reward is None else bool(reward >= 1), "reward": reward},
        "cost": {"usd": ar.get("cost_usd") if ar.get("cost_usd") is not None else sr.get("total_cost_usd"),
                 "input_tokens": ar.get("n_input_tokens"), "cache_tokens": ar.get("n_cache_tokens"),
                 "output_tokens": ar.get("n_output_tokens")},
        "efficiency": {"turns": sr.get("num_turns") or transcript_turns(sessions),
                       "agent_seconds": _secs(res.get("agent_execution"))},
        "harness": {"name": "harbor", "agent": ai.get("name"), "agent_version": ai.get("version"),
                    "claude_code_version": setup.get("claude_code_version") or ai.get("version"),
                    "adage_version": setup.get("adage_version")},
        "model": ((ai.get("model_info") or {}).get("name")) or agent_cfg.get("model_name"),
        "adage": {"spool_records": spool_count(trial), "generation": (setup.get("home") or {}).get("generation"),
                  "sessions": [f.stem for f in sessions]},
        "started_at": res.get("started_at"),
        "exception": (res.get("exception_info") or {}).get("exception_type"),
    }
    if arm in ("holdout", "adage", "adage-lro") and status == "ok" and rec["adage"]["spool_records"] == 0:
        rec["adage"]["hooks_inert"] = True
    return rec


def assign_repeats(recs: list[dict]) -> None:
    """repeat = 1-based order of (task, arm) trials by start time."""
    groups = defaultdict(list)
    for r in recs:
        groups[(r["task_id"], r["arm"])].append(r)
    for g in groups.values():
        g.sort(key=lambda r: (r.get("started_at") or "", r["run_id"]))
        for i, r in enumerate(g, 1):
            r["repeat"] = i


def cmd_emit(a) -> int:
    split = json.loads(Path(a.split).read_text()) if a.split else None
    idx = split_index(split)
    recs = [trial_record(t, a.suite, idx) for t in trial_dirs(a.jobs)]
    assign_repeats(recs)
    out = open(a.out, "w") if a.out != "-" else sys.stdout
    for r in recs:
        out.write(json.dumps(r, sort_keys=True) + "\n")
    if out is not sys.stdout:
        out.close()
        by = defaultdict(lambda: defaultdict(int))
        for r in recs:
            by[r["arm"]][r["status"]] += 1
        print(f"{len(recs)} records -> {a.out}  {json.dumps({k: dict(v) for k, v in by.items()})}")
        inert = sum(1 for r in recs if r["adage"].get("hooks_inert"))
        if inert:
            print(f"WARNING: {inert} hook-arm trial(s) left no spool record: Adage never ran there "
                  f"(repository key mismatch or hooks not loaded)")
    return 0


# ------------------------------------------------------------------------------------ harvest

def cmd_harvest(a) -> int:
    split = json.loads(Path(a.split).read_text())
    idx = split_index(split)
    out = Path(a.out)
    manifest = defaultdict(list)
    outcomes = defaultdict(dict)
    skipped = defaultdict(int)
    for t in trial_dirs(a.jobs):
        res = json.loads((t / "result.json").read_text())
        tid = match_task(res["task_name"], set(idx))
        repo, part = idx.get(tid, (None, None))
        kw = ((res.get("config") or {}).get("agent") or {}).get("kwargs") or {}
        if part != "train":
            skipped["not a train task" if part else "not in split"] += 1
            continue
        if kw.get("arm", "off") != "off":
            skipped["not arm off"] += 1
            continue
        reward = reward_of(res)
        slug = protocol.repo_slug(repo)
        for f in session_files(t):
            dest = out / slug / f.name
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(f, dest)
            if reward is not None:
                outcomes[slug][f.stem] = bool(reward >= 1)
            manifest[slug].append({"task_id": tid, "session": f.stem, "trial": str(t), "passed": outcomes[slug].get(f.stem)})
    for slug in manifest:
        (out / slug / "outcomes.json").write_text(json.dumps(outcomes[slug], indent=1, sort_keys=True))
        (out / slug / "manifest.json").write_text(json.dumps(manifest[slug], indent=1))
        print(f"{slug}: {len(manifest[slug])} transcripts, {sum(outcomes[slug].values())} resolved")
    for k, v in skipped.items():
        print(f"skipped {v} trial(s): {k}")
    return 0


# ------------------------------------------------------------------------------------- checks

def cmd_check_leakage(a) -> int:
    split = json.loads(Path(a.split).read_text())
    test_ids = split["repos"][a.repo]["test"]
    metas = [protocol.load_task_meta(Path(a.tasks_dir) / t, t) for t in test_ids]
    train_texts = [f.read_text(errors="replace") for f in sorted(Path(a.harvest).glob("*.jsonl"))]
    rep = protocol.check_leakage(protocol.artifact_texts(a.home), metas, train_texts)
    print(json.dumps(rep, indent=1))
    return 0 if rep["ok"] else 1


def cmd_check_patches(a) -> int:
    bad = 0
    n = 0
    for t in trial_dirs(a.jobs):
        d = t / "agent" / "adage"
        read = lambda name: (d / name).read_text(errors="replace") if (d / name).exists() else None  # noqa: E731
        diff, status, untracked = read("patch.diff"), read("status.txt"), read("untracked.txt")
        if diff is None and status is None:
            continue
        n += 1
        rep = protocol.check_patch_clean(diff, status, untracked)
        if not rep["ok"]:
            bad += 1
            print(f"{t}: {rep['violations']}")
    print(f"{n} trial worktree(s) checked, {bad} with Adage/harness files")
    return 0 if bad == 0 else 1


def cmd_analyse(a) -> int:
    recs = [json.loads(line) for line in Path(a.records).read_text().splitlines() if line.strip()]
    print(json.dumps(stats.analyse(recs, b=a.b, seed=a.seed), indent=1))
    return 0


def cmd_power(a) -> int:
    ds = [float(x) for x in a.discordance.split(",")]
    es = [float(x) for x in a.delta.split(",")]
    print(f"pairs (tasks) needed, paired binary resolve, alpha {a.alpha} two-sided, power {a.power} (McNemar):")
    print("discordance  " + "  ".join(f"d={e:<6}" for e in es))
    for d in ds:
        cells = []
        for e in es:
            try:
                cells.append(f"{stats.pairs_for_binary(d, e, a.alpha, a.power):<8}")
            except ValueError:
                cells.append(f"{'-':<8}")
        print(f"{d:<11}  " + "  ".join(cells))
    print(f"continuous (log cost ratio), SD {a.sd}: 10% change needs "
          f"{stats.pairs_for_continuous(a.sd, 0.10536, a.alpha, a.power)} pairs, 20% needs "
          f"{stats.pairs_for_continuous(a.sd, 0.22314, a.alpha, a.power)}")
    return 0


# --------------------------------------------------------------------------------------- main

def parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = p.add_subparsers(dest="cmd", required=True)

    s = sub.add_parser("split")
    s.add_argument("--registry", required=True)
    s.add_argument("--dataset", required=True)
    s.add_argument("--tasks-dir", required=True)
    s.add_argument("--clones", required=True)
    s.add_argument("--dates-cache")
    s.add_argument("--out", required=True)
    s.add_argument("--train-frac", type=float, default=protocol.DEFAULT_TRAIN_FRAC)
    s.add_argument("--buffer", type=int, default=protocol.DEFAULT_BUFFER)
    s.add_argument("--min-test", type=int, default=protocol.MIN_TEST)
    s.add_argument("--allow-missing", action="store_true")
    s.set_defaults(fn=cmd_split)

    s = sub.add_parser("phases")
    s.add_argument("--registry", required=True)
    s.add_argument("--dataset", required=True)
    s.add_argument("--suite")
    s.add_argument("--split", default="split.json")
    s.add_argument("--pilot", type=int, default=0)
    s.add_argument("--model", default="anthropic/claude-sonnet-4-5")
    s.add_argument("--arms", type=lambda x: x.split(","), default=list(ARMS))
    s.add_argument("--leave-repo-out", action="store_true")
    s.add_argument("--repeats", type=int, default=3)
    s.add_argument("--train-repeats", type=int, default=1)
    s.add_argument("--max-turns", type=int, default=0)
    s.add_argument("--seed", type=int, default=0)
    s.add_argument("--concurrency", type=int, default=8)
    s.add_argument("--jobs-dir", default="jobs")
    s.add_argument("--tasks-dir", default="harbor-tasks")
    s.add_argument("--clones", default="clones")
    s.add_argument("--harvest", default="harvest")
    s.add_argument("--artifacts", default="artifacts")
    s.add_argument("--records", default="records.jsonl")
    s.add_argument("--adage-bin", default="target/x86_64-unknown-linux-musl/release/adage")
    s.add_argument("--adage-host", default="target/release/adage")
    s.add_argument("--write-configs")
    s.set_defaults(fn=cmd_phases)

    s = sub.add_parser("harvest")
    s.add_argument("--jobs", nargs="+", required=True)
    s.add_argument("--split", required=True)
    s.add_argument("--out", required=True)
    s.set_defaults(fn=cmd_harvest)

    s = sub.add_parser("emit")
    s.add_argument("--jobs", nargs="+", required=True)
    s.add_argument("--suite", required=True)
    s.add_argument("--split")
    s.add_argument("--out", default="-")
    s.set_defaults(fn=cmd_emit)

    s = sub.add_parser("check-leakage")
    s.add_argument("--home", required=True)
    s.add_argument("--split", required=True)
    s.add_argument("--tasks-dir", required=True)
    s.add_argument("--harvest", required=True)
    s.add_argument("--repo", required=True)
    s.set_defaults(fn=cmd_check_leakage)

    s = sub.add_parser("check-patches")
    s.add_argument("--jobs", nargs="+", required=True)
    s.set_defaults(fn=cmd_check_patches)

    s = sub.add_parser("analyse")
    s.add_argument("--records", required=True)
    s.add_argument("--b", type=int, default=5000)
    s.add_argument("--seed", type=int, default=0)
    s.set_defaults(fn=cmd_analyse)

    s = sub.add_parser("power")
    s.add_argument("--discordance", default="0.1,0.2,0.3")
    s.add_argument("--delta", default="0.03,0.05,0.1")
    s.add_argument("--sd", type=float, default=0.5)
    s.add_argument("--alpha", type=float, default=0.05)
    s.add_argument("--power", type=float, default=0.8)
    s.set_defaults(fn=cmd_power)
    return p


def main(argv=None) -> int:
    a = parser().parse_args(argv)
    if getattr(a, "cmd", None) == "split" and not a.dates_cache:
        a.dates_cache = str(Path(a.clones) / "dates.json")
    return a.fn(a)


if __name__ == "__main__":
    sys.exit(main())
