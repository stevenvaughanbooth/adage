#!/usr/bin/env python3
"""Separate prospective agent/instinct study after a stopped CPU learning-curve study.

Only `run` dispatches paid calls. Existing controls, checkpoint zero and native review are
hash-bound, never rewritten. This comparison cannot establish learned or CPU-execution savings.
"""
import argparse
import copy
from pathlib import Path
import json
import shutil
import time

import curve_heldout as heldout

pairs, gate, curve = heldout.pairs, heldout.gate, heldout.curve
PROTOCOL = "learning-curve-cold-1"


def sources():
    return {**curve.sources(), **{str(p): pairs.sha(p) for p in (
        Path(__file__), Path(heldout.__file__), Path(heldout.curve_sessions.__file__), Path(heldout.cohort_v5.__file__))}}


def verify(root):
    m = pairs.read(root / "manifest.json")
    if m.get("protocol") != PROTOCOL or m["source_sha256"] != sources():
        raise ValueError("cold study source changed after freeze")
    if pairs.sha(root / "manifest.json") != (root / "manifest.sha256").read_text().strip():
        raise ValueError("cold study manifest changed")
    for path, digest in m["frozen_inputs"].items():
        if pairs.sha(Path(path)) != digest:
            raise ValueError("cold study frozen input changed: " + path)
    for task, files in m["task_sha256"].items():
        for name, digest in files.items():
            if pairs.sha(root / "private/tasks" / task / name) != digest:
                raise ValueError("frozen task material changed")
    with heldout.curve_sessions.registry(root) as modules:
        families = modules[0]
    return m, families


def freeze(root, original):
    original, root = original.resolve(strict=True), root.resolve()
    if original not in root.parents:
        raise ValueError("cold study must be a new nested root within the reviewed native cohort")
    old = pairs.read(original / "manifest.json")
    authority = Path(old["budget_authority"])
    checkpoint = original / "checkpoints/0"
    record = pairs.read(checkpoint / "attempt.json")
    final = pairs.read(original / "checkpoints/15/attempt.json")
    if record.get("count") != 0 or record.get("status") != "completed" or record.get("exit_code") != 0:
        raise ValueError("completed original checkpoint zero required")
    if final.get("count") != 15 or final.get("status") != "completed" or final.get("execution_candidates"):
        raise ValueError("this separate protocol requires the original no-candidate stop at checkpoint 15")
    validation = pairs.read(original / "validation.json")
    if not validation.get("complete") or not validation.get("valid"):
        raise ValueError("trusted complete task validation required")
    # Verify the original's preserved source bytes, allowing current documentation to evolve.
    preserved = []
    for source, digest in old["source_sha256"].items():
        archived = original / "source" / Path(source).relative_to(gate.REPO)
        if pairs.sha(archived) != digest:
            raise ValueError("original frozen source copy changed")
        preserved.append(archived)
    tasks = old["families"][0]["heldout"]
    ledger = pairs.read(authority / "paid-state.json")
    if any(row.get("task") in tasks for row in ledger["runs"].values()):
        raise ValueError("held-out tasks were already attempted")
    review = original / "native-confinement-reviewed.json"
    gate.require_native_review(dict(experiment_root=str(authority), cohort_root=str(original),
        native_review_path=str(review), native_review_sha256=pairs.sha(review)))
    archive = original / "production-original/manifest.json"
    binary_record = pairs.read(archive)
    binaries = {name: value["path"] for name, value in binary_record["binaries"].items()}
    for name, path in binaries.items():
        if pairs.sha(Path(path)) != binary_record["binaries"][name]["sha256"]:
            raise ValueError("original binary archive changed")
    if pairs.sha(Path(binaries["adage"])) != record["binary_sha256"]:
        raise ValueError("binary did not generate checkpoint zero")
    home = checkpoint / "adage-home"
    for name, digest in record["artifact_sha256"].items():
        if pairs.sha(pairs.safe_file(home, name)) != digest:
            raise ValueError("checkpoint-zero knowledge changed")
    if any(r["track"].endswith("/learned") for r in pairs.read(home / "index.json")["routes"]):
        raise ValueError("checkpoint zero contains learned knowledge")
    root.mkdir(parents=True, exist_ok=False)
    for relative in ("runtime", "cache", "bases", "private/validation"):
        dest = root / relative
        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.symlink_to((original / relative).resolve(strict=True), target_is_directory=True)
    shutil.copy2(original / "validation.json", root / "validation.json")
    for task in tasks:
        shutil.copytree(original / "private/tasks" / task, root / "private/tasks" / task)
    matrix = []
    for index, task in enumerate(tasks):
        for arm in (("agent", "instincts") if index % 2 == 0 else ("instincts", "agent")):
            matrix.append(dict(task=task, arm=arm, order=len(matrix),
                prompt=(root / "private/tasks" / task / "prompt.txt").read_text()))
    descriptor = dict(checkpoint=0, checkpoint0_home=str(home), checkpoint0_files=record["artifact_sha256"],
        learned_home=str(home), route="", binaries=binaries, model=old["model"],
        condition_command=record["command"], condition_output=str(checkpoint / "condition.stdout.json"),
        source_sha256=sources())
    pairs.write(root / "checkpoint0-descriptor.json", descriptor)
    native = original / "sessions/native-confinement"
    inputs = [original / "manifest.json", original / "validation.json", checkpoint / "attempt.json",
        checkpoint / "condition.stdout.json", original / "checkpoints/15/attempt.json", archive, review,
        native / "manifest.json", native / "settings.json", root / "checkpoint0-descriptor.json",
        root / "validation.json", *preserved, *map(Path, binaries.values()),
        *[home / name for name in record["artifact_sha256"]]]
    m = dict(protocol=PROTOCOL, protocol_revision=5, root=str(root), native_cohort_root=str(original),
        budget_authority=str(authority), budget_allocation_sha256=old["budget_allocation_sha256"],
        preparation_python=old["preparation_python"], python_requested=old["python_requested"], model=old["model"],
        families=copy.deepcopy(old["families"]), matrix=matrix, task_sha256={t: old["task_sha256"][t] for t in tasks},
        source_sha256=sources(), frozen_inputs={str(p): pairs.sha(p) for p in inputs},
        descriptor_sha256=pairs.sha(root / "checkpoint0-descriptor.json"),
        study_revision="Separate prospectively frozen cold-only comparison; original CPU protocol stopped with no executable checkpoint-15 candidate.",
        budget=dict(evaluation_sessions=8, per_session_usd=1.5, reservation_usd=12),
        cpu_savings_claim="not tested", learning_frozen=True, grants_written=False)
    pairs.write(root / "manifest.json", m)
    (root / "manifest.sha256").write_text(pairs.sha(root / "manifest.json") + "\n")
    for source in m["source_sha256"]:
        target = root / "source" / Path(source).relative_to(gate.REPO)
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, target)
    return m


def prepare(root):
    m, _ = verify(root)
    return [heldout.prepare(root, task, root / "checkpoint0-descriptor.json") for task in m["families"][0]["heldout"]]


def run(root):
    m, _ = verify(root)
    # No retries or reordering: one bounded eight-assignment dispatch, original shared ledger.
    return [heldout.live(root / "heldout" / e["task"] / e["arm"] / "manifest.json", wall_seconds=600)
            for e in m["matrix"]]


def report(root):
    m, _ = verify(root)
    authority = Path(m["budget_authority"])
    ledger = pairs.read(authority / "paid-state.json")
    rows = []
    for entry in m["matrix"]:
        session = root / "heldout" / entry["task"] / entry["arm"]
        attempt = ledger["runs"].get(str(session.relative_to(authority)), {})
        path = session / "collection.json"
        row = pairs.read(path) if path.exists() else dict(**entry, status="uncollected",
            cost_usd=attempt.get("summary", {}).get("total_cost_usd"))
        row["ledger_attempt"] = attempt
        rows.append(row)
    contrasts = {}
    for task in m["families"][0]["heldout"]:
        arms = {r["arm"]: r for r in rows if r["task"] == task}
        contrasts[task] = heldout.contrast(arms["agent"], arms["instincts"])
    result = dict(protocol=PROTOCOL, assignment_denominator=8, rows=rows, contrasts=contrasts,
        collected=sum(r.get("status") != "uncollected" for r in rows), cpu_savings_claim="not tested",
        learned_savings_claim="not tested", limitations=["Four tasks are descriptive cold-start evidence, not a population efficacy estimate.",
            "Original learning and CPU-execution protocol remains stopped with zero executable candidates."])
    pairs.write(root / "reports" / (str(time.time_ns()) + ".json"), result)
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("freeze", "prepare", "run", "report"))
    parser.add_argument("root", type=Path)
    parser.add_argument("--original", type=Path)
    args = parser.parse_args()
    if args.command == "freeze":
        if args.original is None:
            parser.error("freeze needs --original")
        result = freeze(args.root, args.original)
    else:
        result = globals()[args.command](args.root.resolve(strict=True))
    print(json.dumps(result, indent=2))
