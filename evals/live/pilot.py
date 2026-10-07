#!/usr/bin/env python3
"""Operator-local, learned E0 paid pilot. Planning, learning and freezing call no models.

Use --execute only on train/run. A fresh home, honest task groups and fixed stage allocations
connect the existing native runner, canonical learner and frozen evaluator without auto-review.
The bundled tasks are synthetic; even real model results are pilot evidence, not customer claims.
"""
from __future__ import annotations

import argparse
import copy
import json
import os
import shlex
import subprocess
import sys
import tempfile
from decimal import Decimal, InvalidOperation
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import evidence
import frozen
import runner
import training
from budget import Budget, execution_lock

PACK = HERE / "sandboxes" / "pilot"
STAGES = ("train", "validation", "test")
AGENT_MARKERS = runner.AGENT_MARKERS


def micros(value):
    try:
        number = Decimal(str(value)) * 1_000_000
    except InvalidOperation as exc:
        raise ValueError("budgets must be positive exact microdollar amounts") from exc
    if not number.is_finite() or number <= 0 or number != number.to_integral_value():
        raise ValueError("budgets must be positive exact microdollar amounts")
    return int(number)


def subprocess_json(argv, **kwargs):
    result = subprocess.run(list(map(str, argv)), capture_output=True, text=True, timeout=120, **kwargs)
    if result.returncode:
        raise ValueError(result.stderr.strip() or f"command failed: {argv[0]}")
    return json.loads(result.stdout)


def selected_tasks(registry):
    selected = {stage: [] for stage in STAGES}
    seen = {stage: set() for stage in STAGES}
    groups = {}
    for task, row in sorted(registry["tasks"].items()):
        stage, group = row["split"], row["group_id"]
        if stage not in STAGES or (group in groups and groups[group] != stage):
            raise ValueError("task groups must be disjoint across train, validation and test")
        groups[group] = stage
        if stage == "train" or group not in seen[stage]:
            selected[stage].append(task)
        seen[stage].add(group)
    if len(seen["train"]) < 2 or any(len(seen[stage]) < 2 for stage in STAGES[1:]):
        raise ValueError("the pilot needs at least two honest groups in every split")
    return selected


def initialize(root, adage, model, total_budget, session_budget, *, max_turns=30, timeout=600, pack=PACK):
    root, adage, pack = Path(root).resolve(), Path(adage).resolve(), Path(pack).resolve()
    if root.exists():
        raise ValueError("pilot root already exists; use plan/train/learn/freeze/run to resume it")
    if not runner.under(root, runner.CONFINE_HIDE):
        raise ValueError("keep the pilot under a standard hidden root such as /tmp or /workspace")
    if not model.strip() or type(max_turns) is not int or max_turns <= 0 or type(timeout) is not int or timeout <= 0:
        raise ValueError("supply a resolved model revision and positive execution bounds")
    total, cap = micros(total_budget), micros(session_budget)
    registry = subprocess_json([sys.executable, pack / "generate.py", "--list"])
    chosen = selected_tasks(registry)
    counts = {stage: len(tasks) * (1 if stage == "train" else 2) for stage, tasks in chosen.items()}
    if sum(counts.values()) * cap > total:
        raise ValueError(f"the {sum(counts.values())} scheduled runs need at least {sum(counts.values()) * cap / 1_000_000:g} USD in reservations")
    closure = sorted(p for p in pack.rglob("*") if p.is_file() and "__pycache__" not in p.parts)
    collector = list(frozen.collector_sources()) + [HERE / name for name in ("pilot.py", "training.py")]
    inputs = {str(path): frozen.file_digest(path) for path in sorted(set(closure + collector))}
    configs = {}
    for stage in STAGES:
        cfg = copy.deepcopy(runner.DEFAULTS)
        cfg.update(name=f"learned-e0-pilot-{stage}", repo_key=registry["repo"], workdir="pilot",
                   sandbox=[sys.executable, str(pack / "generate.py"), "--out", "{dir}", "--task", "{task}"],
                   score=[sys.executable, str(pack / "grade.py"), "--workdir", "{dir}", "--task", "{task}"],
                   prompts=["Read TASK.md and complete its request. Report your changes and verification."],
                   train={"tasks": chosen["train"] if stage == "train" else [], "repeats": 1},
                   eval={"tasks": [] if stage == "train" else chosen[stage], "repeats": 1, "arms": ["off", "adage"]},
                   model=model, max_turns=max_turns, timeout_seconds=timeout, concurrency=1, rounds=1, seed=42,
                   capture_usage=True, setting_sources="", confine=True, learn_args=[],
                   classify={**runner.DEFAULTS["classify"], "lookup": [],
                             "verify": ["make check", "tools/project.py check"],
                             "regen": ["make generate", "tools/project.py generate"], "check_flags": [" check"]},
                   obligation={"paths": "src/*.py"}, labels={"lookup": "repository context", "regen": "make generate"},
                   budget_usd=counts[stage] * cap / 1_000_000, session_budget_usd=cap / 1_000_000,
                   execution_cwd=str(runner.REPO))
        configs[stage] = cfg
    manifest = {"schema": 1, "evidence_kind": "synthetic_paid_pilot", "treatment": "learned_e0_advice",
                "root": str(root), "adage": str(adage), "adage_sha256": frozen.file_digest(adage),
                "harness_version": runner.harness_version(), "registry": registry, "selected": chosen,
                "counts": counts, "configs": configs, "inputs": inputs, "pack_inputs": list(map(str, closure)),
                "budget_microusd": total, "session_cap_microusd": cap,
                "policy": {"min_groups": 2, "max_quality_loss": 0.02, "min_savings": 0.2, "min_baseline_success": 0.8}}
    root.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".adage-pilot-", dir=root.parent) as temporary:
        staging = Path(temporary) / "pilot"
        staging.mkdir(mode=0o700)
        frozen.publish(staging / "registry.json", registry)
        training.registry(staging / "registry.json", registry["repo"])
        for stage, cfg in configs.items():
            frozen.publish(staging / f"{stage}-config.json", cfg)
        frozen.publish(staging / "pilot.json", manifest)
        os.rename(staging, root)
    return manifest


def load(root):
    root = Path(root).resolve()
    manifest = frozen.read_json(root / "pilot.json")
    if manifest.get("schema") != 1 or manifest.get("root") != str(root):
        raise ValueError("pilot manifest/root mismatch")
    if frozen.file_digest(manifest["adage"]) != manifest["adage_sha256"]:
        raise ValueError("Adage build changed; restore the pinned pilot build")
    for path, expected in manifest["inputs"].items():
        if frozen.file_digest(path) != expected:
            raise ValueError(f"pilot input changed: {path}; retain original inputs or start a new pilot")
    for stage in STAGES:
        if frozen.read_json(root / f"{stage}-config.json") != manifest["configs"][stage]:
            raise ValueError("pilot stage configuration changed")
    if frozen.read_json(root / "registry.json") != manifest["registry"]:
        raise ValueError("pilot registry changed")
    chosen = selected_tasks(manifest["registry"])
    counts = {stage: len(tasks) * (1 if stage == "train" else 2) for stage, tasks in chosen.items()}
    cap = manifest["session_cap_microusd"]
    if (manifest["selected"] != chosen or manifest["counts"] != counts
            or sum(counts.values()) * cap > manifest["budget_microusd"]
            or any(micros(manifest["configs"][stage]["budget_usd"]) != counts[stage] * cap
                   or micros(manifest["configs"][stage]["session_budget_usd"]) != cap for stage in STAGES)):
        raise ValueError("pilot allocations no longer match the fixed schedule and absolute budget")
    return manifest


def pilot_sha(manifest):
    return frozen.digest(frozen.encoded(manifest))


def train_jobs(manifest):
    return [{"id": run_id, "task": task, "prompt": prompt}
            for run_id, _, _, task, prompt in runner.plan(manifest["configs"]["train"], ["train"], 1)]


def training_sources(root, run_id, run, stream_sha256):
    """Pin original native evidence before admitting another training session."""
    archive = root / "train" / "private" / "transcripts"
    stream = root / "train" / "runs" / run_id / "stream.jsonl"
    sources, reasons = {}, []
    sid = (run.get("metrics") or {}).get("session_id")
    valid_sid = isinstance(sid, str) and bool(sid) and "/" not in sid and "\\" not in sid
    paths = sorted(path for path in archive.glob("*/*.jsonl") if valid_sid and path.name == f"{sid}.jsonl")
    if not valid_sid:
        reasons.append("missing or invalid native session identity")
    if len(paths) != 1:
        reasons.append("training run needs exactly one archived native history")
    for path in [stream, *paths]:
        boundary = archive if path != stream else stream.parent
        if path.is_symlink() or not path.resolve().is_relative_to(boundary.resolve()):
            reasons.append("native evidence must not link outside its archive")
            continue
        try:
            sources[str(path)] = frozen.file_digest(path)
            if path != stream:
                training.check_native_session(path, sid)
        except (OSError, ValueError) as exc:
            reasons.append(f"native evidence is unavailable or invalid: {exc}")
    if sources.get(str(stream)) != stream_sha256:
        reasons.append("native request stream changed or could not be fully captured")
    return {"complete": not reasons, "session_id": sid, "sources": sources, "reasons": reasons}


def training_journal(manifest, root, objects):
    jobs = train_jobs(manifest)
    expected, receipt_map, started = {job["id"] for job in jobs}, {}, set()
    state_path = root / "train" / "state.json"
    for path in (root / "train" / "started").glob("*.json"):
        if frozen.read_json(path) != {"pilot_sha256": pilot_sha(manifest), "job_id": path.stem} or path.stem not in expected:
            raise ValueError("training start journal mismatch")
        started.add(path.stem)
    for path in (root / "train" / "receipts").glob("*.json"):
        pointer = frozen.read_json(path)
        receipt = objects.get(pointer["sha256"])
        if (path.stem not in expected or pointer.get("pilot_sha256") != pilot_sha(manifest)
                or receipt.get("job_id") != path.stem or receipt.get("pilot_sha256") != pilot_sha(manifest)):
            raise ValueError("training receipt journal mismatch")
        receipt_map[path.stem] = receipt
    if (started or receipt_map) and not state_path.exists():
        raise ValueError("training accounting ledger is missing; restore the original ledger")
    state = frozen.read_json(state_path) if state_path.exists() else {}
    if (started or receipt_map) and not isinstance(state.get("accounting"), dict):
        raise ValueError("training accounting ledger is missing; restore the original ledger")
    cfg = manifest["configs"]["train"]
    ledger = Budget(state, cfg["budget_usd"], cfg["session_budget_usd"])
    frozen.validate_admission_journal({"jobs": jobs, "protocol": {"budget": {"max_cost_microusd": manifest["session_cap_microusd"]}}},
                                      objects, started | receipt_map.keys(), receipt_map, ledger)
    for run, receipt in receipt_map.items():
        if state.get("runs", {}).get(run) != receipt["run"]:
            raise ValueError("training run/grade differs from its immutable receipt")
        for sha in receipt["evidence"].values():
            objects.body(sha)
        accounting = objects.get(receipt["evidence"]["accounting"])
        sources = objects.get(receipt["evidence"]["sources"])
        if training_sources(root, run, receipt["run"], accounting["stream_sha256"]) != sources:
            raise ValueError("training native history or request stream differs from its immutable receipt; restore the original evidence")
    if started - receipt_map.keys():
        raise ValueError("training job started without a receipt; reconcile the original attempt without rerunning")
    return jobs, receipt_map, ledger


def runtime_blockers(manifest, root):
    blockers = []
    if sys.platform != "linux":
        blockers.append("paid pilot execution requires Linux")
    if any(os.environ.get(key) for key in AGENT_MARKERS):
        blockers.append("run paid stages from an operator terminal, outside an agent session")
    if not manifest["harness_version"] or runner.harness_version() != manifest["harness_version"]:
        blockers.append("install/restore the pinned Claude Code version and initialize a new pilot if none was pinned")
    if not runner.auth_source(os.environ):
        blockers.append("configure Claude Code authentication locally; plan cannot verify whether credentials are valid or expired")
    error = frozen.confinement_probe()
    if error:
        blockers.append(error)
    for tool in ("git", "make"):
        if not runner.shutil.which(tool):
            blockers.append(f"install {tool} on the execution host")
    env = runner.isolation_env(root / "private-home", os.environ)
    if frozen.exposed_private_inputs({"private_inputs": manifest["pack_inputs"]}, root, frozen.Objects(root / "objects"), env):
        blockers.append("mounted tool/authentication directories expose pilot evidence or the grader; move the pilot/private inputs")
    return blockers


def plan(root, *, runtime=True):
    root = Path(root).resolve()
    manifest = load(root)
    objects = frozen.Objects(root / "objects")
    blockers, stages = [], {}
    try:
        jobs, receipts, ledger = training_journal(manifest, root, objects)
        stages["train"] = {"scheduled": len(jobs), "completed": len(receipts), "accounted_usd": ledger.spent,
                           "unknown_costs": sum(a.get("status") == "unknown" for a in ledger.data["attempts"])}
        if ledger.data["blocked_reason"]:
            blockers.append("training accounting: " + ledger.data["blocked_reason"])
        for run_id, receipt in receipts.items():
            accounting = objects.get(receipt["evidence"]["accounting"])
            grade = objects.get(receipt["evidence"]["grade"])
            sources = objects.get(receipt["evidence"]["sources"])
            if not sources["complete"]:
                blockers.append(f"training {run_id} has incomplete native histories: " + "; ".join(sources["reasons"])
                                + "; retain the original attempt and diagnose before spending again")
            if (not accounting["complete"] or any(call["model"] != manifest["configs"]["train"]["model"] for call in accounting["calls"])
                    or evidence.independent_grade(grade["stdout"], grade["returncode"], grade["timed_out"]) is None):
                blockers.append(f"training {run_id} has incomplete usage, a different model or an unknown independent grade; retain the attempt and diagnose before spending again")
    except (ValueError, OSError, KeyError, TypeError) as exc:
        blockers.append(str(exc))
    for stage in STAGES[1:]:
        path = root / f"{stage}-frozen.json"
        if path.exists():
            study = frozen.adage_json(manifest["adage"], "validate", path, "--objects", objects.root)
            readiness = frozen.preflight(study, objects, root / stage, Path(manifest["adage"]))
            stages[stage] = readiness
            # Source/journal/accounting failures in one cohort block paid dispatch everywhere.
            blockers.extend(f"{stage}: {item['message']}" for item in readiness["blockers"]
                            if item["code"] not in ("authentication", "harness", "confinement", "platform", "agent_session"))
        else:
            stages[stage] = {"scheduled": manifest["counts"][stage], "completed": 0, "frozen": False}
    if runtime:
        blockers.extend(runtime_blockers(manifest, root))
    return {"schema": 1, "evidence_kind": manifest["evidence_kind"], "customer_savings_claim": False,
            "ready_for_paid_dispatch": not blockers, "blockers": blockers, "stages": stages,
            "budget_usd": manifest["budget_microusd"] / 1_000_000,
            "maximum_reserved_usd": sum(manifest["counts"].values()) * manifest["session_cap_microusd"] / 1_000_000,
            "per_run_cap_usd": manifest["session_cap_microusd"] / 1_000_000,
            "accounting_basis": "CLI estimates/reservations, not provider invoices; an unknown or overrun stops dispatch",
            "review_command": shlex.join(["env", f"ADAGE_HOME={root / 'learner-home'}", manifest["adage"], "review"]),
            "next": "train --execute, learn, human review, freeze --reviewed-by NAME, then run --split validation|test --execute"}


class TrainingObserver:
    def __init__(self, manifest, root):
        self.manifest, self.root = manifest, root
        self.version = None

    def prepared(self, live, run_id, task, arm, workdir, settings, env):
        load(self.root)
        if arm != "train" or live.cfg != self.manifest["configs"]["train"] or not live.isolate:
            raise ValueError("training must use the pinned untreated isolated configuration")
        if live.state["harness_version"] != self.manifest["harness_version"]:
            raise ValueError("training harness version changed")
        if frozen.exposed_private_inputs({"private_inputs": self.manifest["pack_inputs"]}, self.root,
                                         frozen.Objects(self.root / "objects"), env):
            raise ValueError("training exposes private grader or pilot evidence")
        env["ADAGE_AUTO_LEARN"], env["ADAGE_DISABLE"] = "0", "1"

    def event(self, value):
        if value.get("type") == "system" and value.get("subtype") == "init":
            self.version = value.get("claude_code_version")

    def graded(self, stdout, returncode, timed_out):
        self.grade = {"stdout": stdout, "returncode": returncode, "timed_out": timed_out}


def train(root, *, execute=False, live_factory=runner.Live):
    root = Path(root).resolve()
    readiness = plan(root)
    if not execute:
        return readiness
    if readiness["blockers"]:
        raise ValueError("pilot is not ready: " + "; ".join(readiness["blockers"]))
    manifest, objects = load(root), frozen.Objects(root / "objects")
    with execution_lock(root):
        # Preflight is a snapshot: another invocation may finish before this lock is acquired.
        readiness = plan(root)
        if readiness["blockers"]:
            raise ValueError("pilot is not ready: " + "; ".join(readiness["blockers"]))
        jobs, receipts, _ = training_journal(manifest, root, objects)
        for job in jobs:
            if job["id"] in receipts:
                continue
            observer = TrainingObserver(manifest, root)
            live = live_factory(copy.deepcopy(manifest["configs"]["train"]), root / "train", Path(manifest["adage"]),
                                1, isolate=True, resume=False, observer=observer)
            if live.budget.data["blocked_reason"]:
                break
            frozen.publish(root / "train" / "started" / f"{job['id']}.json",
                           {"pilot_sha256": pilot_sha(manifest), "job_id": job["id"]})
            live.run_one(job["id"], "train", "train", job["task"], job["prompt"])
            load(root)
            if observer.version != manifest["harness_version"].split()[0]:
                raise ValueError("actual training harness version was not observed")
            accounting = evidence.read_stream(root / "train" / "runs" / job["id"] / "stream.jsonl")
            sources = training_sources(root, job["id"], live.state["runs"][job["id"]], accounting.stream_sha256)
            receipt = {"pilot_sha256": pilot_sha(manifest), "job_id": job["id"], "run": live.state["runs"][job["id"]],
                       "evidence": {"accounting": objects.put({"complete": accounting.complete, "calls": accounting.calls,
                           "reasons": accounting.reasons, "terminal": accounting.terminal, "stream_sha256": accounting.stream_sha256}),
                           "grade": objects.put(observer.grade), "sources": objects.put(sources)}}
            frozen.publish(root / "train" / "receipts" / f"{job['id']}.json",
                           {"pilot_sha256": pilot_sha(manifest), "sha256": objects.put(receipt)})
            if (not sources["complete"] or not accounting.complete
                    or any(call["model"] != manifest["configs"]["train"]["model"] for call in accounting.calls)
                    or evidence.independent_grade(observer.grade["stdout"], observer.grade["returncode"], observer.grade["timed_out"]) is None):
                break
    result = plan(root, runtime=False)
    result["returncode"] = 0 if not result["blockers"] and result["stages"]["train"]["completed"] == len(jobs) else 2
    return result


def learner_command(root, manifest, *args):
    return subprocess_json([manifest["adage"], *args], cwd=root,
                           env=dict(os.environ, ADAGE_HOME=str(root / "learner-home"), ADAGE_AUTO_LEARN="0", ADAGE_EMBED_MODEL="none"))


def learn(root):
    root, manifest = Path(root).resolve(), load(root)
    readiness = plan(root, runtime=False)
    if readiness["blockers"]:
        raise ValueError("reconcile pilot evidence before learning: " + "; ".join(readiness["blockers"]))
    objects = frozen.Objects(root / "objects")
    jobs, receipts, ledger = training_journal(manifest, root, objects)
    if len(jobs) != len(receipts) or ledger.data["blocked_reason"]:
        raise ValueError("finish and reconcile every training attempt before learning")
    record = root / "learning.json"
    if record.exists():
        return frozen.read_json(record)
    bundle = root / "training-bundle"
    if not bundle.exists():
        training.export([root / "train"], root / "registry.json", manifest["registry"]["repo"], manifest["adage"], bundle)
    export = frozen.read_json(bundle / "report.json")
    if any(frozen.file_digest(path) != sha for path, sha in export["sources"].items()):
        raise ValueError("training bundle sources changed")
    if frozen.file_digest(bundle / "corpus.jsonl") != export["corpus_sha256"]:
        raise ValueError("training corpus changed")
    home = root / "learner-home"
    marker = home / "pilot-owner.json"
    if home.exists() and not marker.exists() and any(home.iterdir()):
        raise ValueError("pilot learning requires its own fresh ADAGE_HOME")
    frozen.publish(marker, {"pilot_sha256": pilot_sha(manifest)})
    repo = manifest["registry"]["repo"]
    imported = learner_command(root, manifest, "corpus", "import", bundle / "corpus.jsonl", "--repo", repo)
    learned = learner_command(root, manifest, "corpus", "learn", "--repo", repo)
    audit = learner_command(root, manifest, "corpus", "inspect", "--repo", repo, "--evidence")
    graph = learner_command(root, manifest, "graph", "--json", "--events", "0")
    (home / "tracks").mkdir(exist_ok=True)
    sources = frozen.adage_json(manifest["adage"], "sources", home / "tracks")
    queue = learner_command(root, manifest, "review", "--json")
    result = {"pilot_sha256": pilot_sha(manifest), "training_generation": learned["corpus_generation"],
              "import_sha256": objects.put(imported), "learn_sha256": objects.put(learned),
              "audit_sha256": objects.put(audit), "routes_sha256": objects.put(graph["routes"]),
              "sources_sha256": objects.put(sources),
              "review_sha256": objects.put(queue), "candidate_count": len(graph["routes"]),
              "next": "Review the recorded candidates in an operator terminal; no candidate is a valid stop. Do not inject an authored index or relax learning thresholds."}
    frozen.publish(record, result)
    return result


def check_lineage(original, current, index, repo):
    def routes(source):
        return {(track["manifest"]["scope"]["repo"], track["manifest"]["track"]["name"], route["id"]): route
                for track in source["tracks"] for route in track["routes"]}
    before, after = routes(original), routes(current)
    if current["index"] != index:
        raise ValueError("compiled treatment differs from the canonical reviewed source; recompile before freezing")
    for entry in index["routes"]:
        key = (entry["repo"], entry["track"], entry["route"]["id"])
        old, new = before.get(key), after.get(key)
        if (not old or not new or key[0] != repo or entry["route"].get("authored")
                or entry.get("exec") or new.get("evidence", {}).get("execution")
                or {k: v for k, v in old.items() if k not in ("state", "strength")}
                != {k: v for k, v in new.items() if k not in ("state", "strength")}):
            raise ValueError("treatment contains an authored, added or changed reflex outside this pilot's production learner output")


def freeze(root, reviewed_by):
    root, manifest = Path(root).resolve(), load(root)
    readiness = plan(root, runtime=False)
    if readiness["blockers"]:
        raise ValueError("reconcile pilot evidence before freezing: " + "; ".join(readiness["blockers"]))
    if not reviewed_by.strip():
        raise ValueError("record the operator who reviewed the candidates with --reviewed-by")
    objects = frozen.Objects(root / "objects")
    record = frozen.read_json(root / "learning.json")
    if record["pilot_sha256"] != pilot_sha(manifest):
        raise ValueError("learning provenance does not belong to this pilot")
    repo, index = manifest["registry"]["repo"], root / "learner-home" / "index.json"
    frozen.artifact(manifest["adage"], index, repo)
    current = frozen.adage_json(manifest["adage"], "sources", root / "learner-home" / "tracks")
    check_lineage(objects.get(record["sources_sha256"]), current, frozen.read_json(index), repo)
    current_audit = learner_command(root, manifest, "corpus", "inspect", "--repo", repo, "--evidence")
    if current_audit["training_generation"] != record["training_generation"]:
        raise ValueError("the learner corpus changed after the recorded production learning run")
    audit = objects.get(record["audit_sha256"])
    if audit["training_generation"] != record["training_generation"]:
        raise ValueError("learner and training audit generations differ")
    frozen.publish_bytes(root / "reviewed-index.json", index.read_bytes())
    frozen.publish(root / "review.json", {"reviewed_by": reviewed_by, "index_sha256": frozen.file_digest(index),
                                         "learning": record, "review": learner_command(root, manifest, "review", "--json")})
    identities = {task: {"task_id": task, "group_id": row["group_id"]} for task, row in manifest["registry"]["tasks"].items()}
    for stage in STAGES[1:]:
        path = root / f"{stage}-frozen.json"
        if path.exists():
            study = frozen.adage_json(manifest["adage"], "validate", path, "--objects", objects.root)
            if study["protocol"]["intervention"]["reflex_set_sha256"] != frozen.file_digest(index):
                raise ValueError("reviewed artifact changed after a cohort was frozen; create a new pilot")
        else:
            frozen.prepare(root / f"{stage}-config.json", objects.root / f"{record['audit_sha256']}.json", index,
                           objects, path, Path(manifest["adage"]), [*manifest["pack_inputs"], HERE / "pilot.py", HERE / "training.py"],
                           learner_revision=f"{manifest['adage_sha256']}:production-defaults:corpus={record['training_generation']}",
                           policy=manifest["policy"], identities=identities,
                           timeout=manifest["configs"][stage]["timeout_seconds"])
    return plan(root)


def run(root, split, *, execute=False):
    root, manifest = Path(root).resolve(), load(root)
    readiness = plan(root)
    if not execute:
        return readiness
    if readiness["blockers"]:
        raise ValueError("pilot is not ready: " + "; ".join(readiness["blockers"]))
    with execution_lock(root):
        # Hold the pilot lock across cross-stage checks and the validation release gate.
        readiness = plan(root)
        if readiness["blockers"]:
            raise ValueError("pilot is not ready: " + "; ".join(readiness["blockers"]))
        if not all((root / f"{stage}-frozen.json").exists() for stage in STAGES[1:]):
            raise ValueError("freeze both held-out cohorts before any evaluation dispatch")
        if split == "test":
            validation = readiness["stages"]["validation"]
            if validation["pending_runs"]:
                raise ValueError("collect the full validation cohort before starting the untouched test cohort")
            validation_path = root / "validation-frozen.json"
            objects = frozen.Objects(root / "objects")
            validation_study = frozen.adage_json(manifest["adage"], "validate", validation_path, "--objects", objects.root)
            with execution_lock(root / "validation"):
                collected = frozen.report(validation_path, validation_study, objects, root / "validation", Path(manifest["adage"]))
            if collected.returncode or not json.loads(collected.stdout)["evidence_complete"]:
                raise ValueError("validation evidence is incomplete; diagnose its original usage/grades before spending on the untouched test cohort")
        path, objects = root / f"{split}-frozen.json", frozen.Objects(root / "objects")
        study = frozen.adage_json(manifest["adage"], "validate", path, "--objects", objects.root)
        result = frozen.execute(path, study, objects, root / split, Path(manifest["adage"]))
    return {"returncode": result.returncode, "report": json.loads(result.stdout), "stderr": result.stderr}


def report(root):
    root, manifest = Path(root).resolve(), load(root)
    objects, result = frozen.Objects(root / "objects"), plan(root, runtime=False)
    result["usage"], result["billed_cost_usd"] = {}, None
    for stage in STAGES:
        calls, estimates, complete = [], [], True
        receipt_paths = list((root / stage / "receipts").glob("*.json"))
        for path in receipt_paths:
            receipt = objects.get(frozen.read_json(path)["sha256"])
            source = objects.get(receipt["evidence"]["accounting"])
            complete &= source["complete"]
            calls.extend(source["calls"])
            estimates.append(frozen.known_cost((source.get("terminal") or {}).get("total_cost_usd")))
        usages = [call.get("usage") for call in calls]
        known = [usage for usage in usages if usage is not None]
        result["usage"][stage] = {"recorded_model_calls": len(calls), "request_usage_complete":
                                  len(receipt_paths) == manifest["counts"][stage] and bool(calls) and complete and len(known) == len(usages),
                                  "known_cli_estimate_usd": sum(value for value in estimates if value is not None),
                                  "unknown_cli_estimates": sum(value is None for value in estimates),
                                  "input_tokens": sum(u["input_tokens"] for u in known),
                                  "output_tokens": sum(u["output_tokens"] for u in known),
                                  "cached_input_tokens": sum(u["cached_input_tokens"] for u in known)}
        if stage != "train" and (root / f"{stage}-frozen.json").exists():
            path = root / f"{stage}-frozen.json"
            study = frozen.adage_json(manifest["adage"], "validate", path, "--objects", objects.root)
            with execution_lock(root / stage):
                measured = frozen.report(path, study, objects, root / stage, Path(manifest["adage"]))
            result["stages"][stage]["report"] = json.loads(measured.stdout) if measured.stdout else {"error": measured.stderr}
    return result


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    init = commands.add_parser("init")
    init.add_argument("--adage", type=Path, default=runner.REPO / "target" / "release" / "adage")
    init.add_argument("--model", required=True)
    init.add_argument("--budget-usd", required=True)
    init.add_argument("--session-budget-usd", default="1")
    init.add_argument("--max-turns", type=int, default=30)
    init.add_argument("--timeout-seconds", type=int, default=600)
    for name in ("plan", "train", "learn", "freeze", "run", "report"):
        command = commands.add_parser(name)
        if name in ("train", "run"):
            command.add_argument("--execute", action="store_true")
        if name == "run":
            command.add_argument("--split", choices=STAGES[1:], required=True)
        if name == "freeze":
            command.add_argument("--reviewed-by", required=True)
    for command in commands.choices.values():
        command.add_argument("--root", type=Path, required=True)
    args = parser.parse_args(argv)
    try:
        if args.command == "init":
            initialize(args.root, args.adage, args.model, args.budget_usd, args.session_budget_usd,
                       max_turns=args.max_turns, timeout=args.timeout_seconds)
            result = plan(args.root)
        elif args.command == "train":
            result = train(args.root, execute=args.execute)
        elif args.command == "run":
            result = run(args.root, args.split, execute=args.execute)
        elif args.command == "freeze":
            result = freeze(args.root, args.reviewed_by)
        else:
            result = {"plan": plan, "learn": learn, "report": report}[args.command](args.root)
    except (ValueError, OSError, KeyError, subprocess.SubprocessError) as exc:
        parser.exit(2, f"pilot: {exc}\n")
    print(json.dumps(result, indent=2, sort_keys=True))
    return result.get("returncode", 0)


if __name__ == "__main__":
    raise SystemExit(main())
