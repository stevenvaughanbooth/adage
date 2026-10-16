#!/usr/bin/env python3
"""Export train-only runner archives through the real canonical corpus decoder.

This command calls no models, does not learn or approve reflexes, and never rewrites native
transcripts. Keep its bundle private: it contains normalized trace content and source paths.
Use a fresh runner root for each retry; overwritten attempts cannot be reconstructed here.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import subprocess
import sys
import tempfile
from collections import Counter
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from evidence import independent_grade


def sha256(path):
    result = hashlib.sha256()
    with Path(path).open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            result.update(block)
    return result.hexdigest()


def read_json(path):
    def unique_pairs(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise ValueError(f"duplicate JSON key: {key}")
            result[key] = value
        return result
    return json.loads(Path(path).read_text(), object_pairs_hook=unique_pairs)


def write_json(path, value):
    Path(path).write_text(json.dumps(value, indent=2, sort_keys=True, allow_nan=False) + "\n")
    Path(path).chmod(0o600)


def check_native_session(path, sid):
    """A canonical decoder must not silently collapse a mixed or truncated native file."""
    found = False
    with Path(path).open("rb") as handle:
        while True:
            line = handle.readline(8 * 1024 * 1024 + 1)
            if not line:
                break
            if len(line) > 8 * 1024 * 1024:
                raise ValueError("native transcript record exceeds 8 MiB")
            if not line.strip():
                continue
            value = json.loads(line)
            if not isinstance(value, dict):
                raise ValueError("native transcript records must be JSON objects")
            if "sessionId" in value:
                if value["sessionId"] != sid:
                    raise ValueError("native session identity disagrees with runner record")
                found = True
    if not found:
        raise ValueError("native transcript has no recorded session identity")


def registry(path, repo):
    value = read_json(path)
    if not isinstance(value, dict) or value.get("schema") != 1 or value.get("repo") != repo:
        raise ValueError("registry must have schema=1 and the selected repository key")
    tasks = value.get("tasks")
    if not isinstance(tasks, dict) or not tasks:
        raise ValueError("registry needs a nonempty task map")
    groups = {}
    for task, row in tasks.items():
        if not isinstance(task, str) or not task.strip() or not isinstance(row, dict):
            raise ValueError("registry task ids must be nonempty strings with metadata")
        split, group = row.get("split"), row.get("group_id")
        if split not in ("train", "validation", "test") or not isinstance(group, str) or not group.strip():
            raise ValueError(f"task {task}: declare train/validation/test and a nonempty group_id")
        if task != task.strip() or group != group.strip():
            raise ValueError("task and group ids must not have surrounding whitespace")
        if group in groups and groups[group] != split:
            raise ValueError(f"related group spans splits: {group}")
        groups[group] = split
    return tasks


def collect(roots, tasks):
    sources, sessions, runs = {}, {}, []
    resolved = [Path(root).resolve() for root in roots]
    if len(set(resolved)) != len(resolved):
        raise ValueError("duplicate training root")
    for root in resolved:
        state_path = root / "state.json"
        sources[str(state_path)] = sha256(state_path)
        state = read_json(state_path)
        state_runs = state.get("runs") if isinstance(state, dict) else None
        if not isinstance(state_runs, dict) or not state_runs:
            raise ValueError(f"training root has no run records: {root}")
        archive = root / "private" / "transcripts"
        paths = list(archive.glob("*/*.jsonl"))
        if any(path.is_symlink() or not path.resolve().is_relative_to(archive.resolve()) for path in paths):
            raise ValueError("native transcript archive must not link outside itself")
        accounted = set()
        attempts = (state.get("accounting") or {}).get("attempts", [])
        if not isinstance(attempts, list):
            raise ValueError("invalid runner accounting attempts")
        if any(not isinstance(attempt, dict) or attempt.get("run") not in state_runs for attempt in attempts):
            raise ValueError("accounting contains an attempt without its original run record")
        per_run = Counter(attempt["run"] for attempt in attempts)
        if any(count > 1 for count in per_run.values()):
            raise ValueError("runner root contains overwritten/retried attempts; preserve each retry in a fresh root")
        for run_id, run in sorted(state_runs.items()):
            if not isinstance(run, dict) or run.get("phase") != "train" or run.get("arm") != "train":
                raise ValueError("export requires train-only roots; evaluation or treated runs are not training evidence")
            task = run.get("task")
            if task not in tasks or tasks[task]["split"] != "train":
                raise ValueError(f"run {run_id} is not in the registry's training split")
            if run.get("status") not in ("passed", "failed", "skipped"):
                raise ValueError(f"run {run_id} is unfinished; reconcile the original attempt first")
            metrics = run.get("metrics") or {}
            sid = metrics.get("session_id")
            row = {"root": str(root), "run_id": run_id, "task_id": task,
                   "status": run.get("run_status") or metrics.get("status") or run["status"],
                   "session_id": sid, "outcome": None}
            if not sid:
                # No native evidence exists for a never-started or failed-before-session job.
                # Keep its record in provenance; do not invent an empty canonical session.
                if run["status"] != "skipped" and row["status"] != "infra_error":
                    raise ValueError(f"run {run_id} is missing its native session id")
                row["exclusion"] = "no_native_session"
                runs.append(row)
                continue
            if not isinstance(sid, str) or sid in sessions or "/" in sid or "\\" in sid:
                raise ValueError("invalid or reused native session id")
            matches = [path for path in paths if path.name == f"{sid}.jsonl"]
            if len(matches) != 1:
                raise ValueError(f"run {run_id} needs exactly one archived native transcript")
            path = matches[0]
            accounted.add(path)
            sources[str(path)] = sha256(path)
            check_native_session(path, sid)
            # runner.score removes invalid grades using this same validator. Read its independent
            # scorer object, never metrics.passed (which defaults unknown outcomes to false).
            score = run.get("score")
            outcome = independent_grade(json.dumps(score), 0, False) if isinstance(score, dict) else None
            if row["status"] == "scorer_error":
                outcome = None
            row["outcome"] = outcome
            identity = {"task_id": task, "group_id": tasks[task]["group_id"],
                        "attempt_id": f"{run_id}/{sid}"}
            sessions[sid] = {"identity": identity, "outcome": outcome, "path": path}
            runs.append(row)
        if set(paths) != accounted:
            raise ValueError("unaccounted native transcripts: possible overwritten retries or mixed evaluation archive")
    if not sessions:
        raise ValueError("no native training sessions to export")
    return sources, sessions, runs


def export(roots, registry_path, repo, adage, out):
    if not repo.strip():
        raise ValueError("repository key must not be empty")
    out, registry_path, adage = Path(out).resolve(), Path(registry_path).resolve(), Path(adage).resolve()
    if out.exists():
        raise ValueError("output bundle already exists; use a new path to retain the original evidence")
    registry_sha = sha256(registry_path)
    tasks = registry(registry_path, repo)
    sources, sessions, runs = collect(roots, tasks)
    sources[str(registry_path)] = registry_sha
    sources[str(adage)] = sha256(adage)
    out.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".adage-training-", dir=out.parent) as temporary:
        staging = Path(temporary) / "bundle"
        staging.mkdir(mode=0o700)
        identities = {sid: row["identity"] for sid, row in sorted(sessions.items())}
        outcomes = {sid: row["outcome"] for sid, row in sorted(sessions.items()) if row["outcome"] is not None}
        write_json(staging / "identities.json", identities)
        write_json(staging / "outcomes.json", outcomes)
        write_json(staging / "evaluation-identities.json", {
            task: {"task_id": task, "group_id": row["group_id"]}
            for task, row in tasks.items() if row["split"] != "train"})
        corpus = staging / "corpus.jsonl"
        with corpus.open("wb") as output:
            corpus.chmod(0o600)
            for sid, row in sorted(sessions.items()):
                result = subprocess.run([str(adage), "corpus", "export", str(row["path"]), "--repo", repo,
                                         "--identities", str(staging / "identities.json"),
                                         "--outcomes", str(staging / "outcomes.json")],
                                        capture_output=True, timeout=120, check=False)
                if result.returncode:
                    raise ValueError(f"canonical export failed for session {sid}: "
                                     + result.stderr.decode(errors="replace").strip())
                lines = result.stdout.splitlines()
                if len(lines) != 1:
                    raise ValueError(f"canonical export did not yield exactly one session: {sid}")
                canonical = json.loads(lines[0])
                observed = canonical.get("session", {})
                if (canonical.get("schema") != 1 or observed.get("id") != sid or observed.get("repo") != repo
                        or observed.get("identity") != row["identity"] or observed.get("outcome") != row["outcome"]):
                    raise ValueError(f"native session identity/outcome disagrees with runner record: {sid}")
                if observed.get("steered") or observed.get("directed") or observed.get("influence"):
                    raise ValueError("training evidence includes reflex steering; use untouched baseline sessions")
                output.write(lines[0] + b"\n")
        for path, expected in sources.items():
            if sha256(path) != expected:
                raise ValueError(f"training source changed during export: {path}")
        report = {"schema": 1, "repo": repo, "sessions": len(sessions),
                  "training_groups": len({row["identity"]["group_id"] for row in sessions.values()}),
                  "recorded_successes": sum(value is True for value in outcomes.values()),
                  "recorded_failures": sum(value is False for value in outcomes.values()),
                  "unknown_outcomes": len(sessions) - len(outcomes),
                  "runs_without_native_session": sum("exclusion" in row for row in runs),
                  "unobserved_training_tasks": sorted(task for task, row in tasks.items()
                                                       if row["split"] == "train" and task not in {r["task_id"] for r in runs}),
                  "corpus_sha256": sha256(corpus), "sources": sources, "runs": runs,
                  "grade_provenance": "runner independent scorer objects; native transcripts unchanged",
                  "model_calls": 0, "out": str(out)}
        write_json(staging / "report.json", report)
        os.rename(staging, out)
    return report


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, action="append", required=True, help="train-only runner archive; repeat for retained attempts")
    parser.add_argument("--registry", type=Path, required=True)
    parser.add_argument("--repo", required=True)
    parser.add_argument("--adage", type=Path, default=HERE.parent.parent / "target" / "release" / "adage")
    parser.add_argument("--out", type=Path, required=True, help="new private output bundle directory")
    args = parser.parse_args(argv)
    try:
        result = export(args.root, args.registry, args.repo, args.adage, args.out)
    except (ValueError, OSError, subprocess.TimeoutExpired) as exc:
        parser.exit(2, f"training export: {exc}\n")
    print(json.dumps(result, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
