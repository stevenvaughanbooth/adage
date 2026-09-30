"""Prospective two-arm transfer of a frozen instruction-guided learned CPU action.

Only run() dispatches paid calls. No helper creates an execution grant. Training tasks
are repeated demonstrations; transfer tasks were previously observed in cold pilots.
"""
import argparse
import copy
import json
from pathlib import Path
import shutil
import time

import curve_heldout as heldout
import curve_replay

pairs, gate, curve = heldout.pairs, heldout.gate, heldout.curve
PROTOCOL = "learning-curve-guided-transfer-1"
ARMS = ("learned", "automatic")


def sources():
    return {**curve.sources(), **{str(p): pairs.sha(p) for p in (
        Path(__file__), Path(curve_replay.__file__), Path(heldout.__file__),
        Path(heldout.curve_sessions.__file__), Path(heldout.cohort_v5.__file__))}}


def training_inputs(cumulative):
    record = pairs.read(cumulative / "attempt.json")
    rows = curve_replay.validated_inputs(record)
    if (record.get("exit_code") != 0 or record["count"] != 10
            or rows[0].get("kind") != curve.GUIDED
            or rows[0].get("workflow_guidance") != curve.GUIDANCE
            or len({r["task"] for r in rows}) != 5):
        raise ValueError("ten native guided demonstrations over five tasks required")
    files = [cumulative / "attempt.json", cumulative / "condition.stdout.json"]
    for row in rows:
        session = Path(row["source"]).parent
        manifest, provenance = session / "manifest.json", session / "transcript-provenance.json"
        m, p = pairs.read(manifest), pairs.read(provenance)
        if (pairs.sha(manifest) != row["manifest_sha256"]
                or pairs.sha(provenance) != row["provenance_sha256"]
                or pairs.sha(Path(row["source"])) != row["sha256"]
                or p.get("synthetic") is not False or p.get("sha256") != row["sha256"]
                or p.get("session_id") != row["session_id"]
                or m.get("kind") != curve.GUIDED or m.get("training_hooks_enabled") is not False
                or m.get("workflow_guidance") != curve.GUIDANCE):
            raise ValueError("native guided provenance changed or feedback entered training")
        files.extend([manifest, provenance, Path(row["source"]), Path(row["copy"])])
    return record, files


def verify(root):
    m = pairs.read(root / "manifest.json")
    if (m.get("protocol") != PROTOCOL or m["source_sha256"] != sources()
            or pairs.sha(root / "manifest.json") != (root / "manifest.sha256").read_text().strip()):
        raise ValueError("transfer source or manifest changed after freeze")
    for path, digest in m["frozen_inputs"].items():
        if pairs.sha(Path(path)) != digest:
            raise ValueError("transfer frozen input changed: " + path)
    training_inputs(Path(m["cumulative_root"]))
    for task, files in m["task_sha256"].items():
        for name, digest in files.items():
            if pairs.sha(root / "private/tasks" / task / name) != digest:
                raise ValueError("transfer task material changed")
    with heldout.curve_sessions.registry(root) as modules:
        families = modules[0]
    return m, families


def freeze(root, cumulative_root, controls):
    root = root.resolve()
    cumulative, controls = cumulative_root.resolve(strict=True), controls.resolve(strict=True)
    native = cumulative.parent
    if native not in root.parents:
        raise ValueError("transfer must nest within its reviewed native cohort")
    parent, old = pairs.read(native / "manifest.json"), pairs.read(controls / "manifest.json")
    authority = Path(parent["budget_authority"]).resolve(strict=True)
    validation = pairs.read(controls / "validation.json")
    if (not validation.get("complete") or not validation.get("valid") or len(validation.get("rows", [])) != 19
            or Path(old["budget_authority"]).resolve() != authority
            or (native / "runtime/python").resolve() != (controls / "runtime/python").resolve()):
        raise ValueError("same-budget/runtime validated nineteen-task controls required")
    record, inputs = training_inputs(cumulative)
    for name, digest in record["production_source_sha256"].items():
        if pairs.sha(gate.REPO / name) != digest:
            raise ValueError("production source changed after conditioning")
    home = cumulative / "adage-home"
    for name, digest in record["artifact_sha256"].items():
        if pairs.sha(pairs.safe_file(home, name)) != digest:
            raise ValueError("learned artifact changed")
    candidates = sorted((r for r in pairs.read(home / "index.json")["routes"]
        if r["track"].endswith("/learned") and r["route"].get("evidence", {}).get("execution")
        and r["route"]["trigger"].get("on") == "context"), key=lambda r: r["track"] + "/" + r["route"]["id"])
    if not candidates:
        raise ValueError("certified learned context candidate required")
    candidate = candidates[0]
    command = candidate["route"].get("step", {}).get("command")
    if command != ".venv/bin/python -m pytest tests/test_validate.py -q":
        raise ValueError("registered fixed verification command required")
    route = candidate["track"] + "/" + candidate["route"]["id"]
    cold = native / "checkpoints/0"
    zero = pairs.read(cold / "attempt.json")
    if zero.get("status") != "completed" or zero.get("count") != 0 or zero.get("exit_code") != 0:
        raise ValueError("completed checkpoint zero required")
    cold_home = cold / "adage-home"
    for name, digest in zero["artifact_sha256"].items():
        if pairs.sha(pairs.safe_file(cold_home, name)) != digest:
            raise ValueError("checkpoint zero changed")
    if any(r["track"].endswith("/learned") for r in pairs.read(cold_home / "index.json")["routes"]):
        raise ValueError("checkpoint zero has learned knowledge")
    # The replacement replay preserves the original cohort's executables. Both rebuilt
    # binaries remain explicitly pinned and enter the new study's frozen input hashes.
    binaries = {name: str((cumulative / name).resolve(strict=True)) for name in ("adage", "adage-exec")}
    expected = {"adage": "64dee0b72e8721df7c7ccb23777eeb816310f64139f6af4bd06bdb365b40d2a7",
                "adage-exec": "39acd547b28a08d4daec615c83ddfd373acc970e7b30442f9e7213f58f7a898d"}
    if any(pairs.sha(Path(path)) != expected[name] for name, path in binaries.items()) or record["binary_sha256"] != expected["adage"]:
        raise ValueError("conditioned release binary mismatch")
    review = native / "native-confinement-reviewed.json"
    gate.require_native_review(dict(experiment_root=str(authority), cohort_root=str(native),
        native_review_path=str(review), native_review_sha256=pairs.sha(review)))
    tasks = old["families"][0]["heldout"]
    train = parent["families"][0]["train"]
    if len(tasks) != 4 or set(tasks) & {r["task"] for r in record["inputs"]}:
        raise ValueError("four transfer tasks disjoint from training required")
    for task in tasks:
        for name, digest in old["task_sha256"][task].items():
            if pairs.sha(controls / "private/tasks" / task / name) != digest:
                raise ValueError("control task material changed")
    root.mkdir(parents=True, exist_ok=False)
    for relative in ("runtime", "cache", "bases", "private/validation"):
        dest = root / relative
        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.symlink_to((controls / relative).resolve(strict=True), target_is_directory=True)
    shutil.copy2(controls / "validation.json", root / "validation.json")
    for task in tasks:
        shutil.copytree(controls / "private/tasks" / task, root / "private/tasks" / task)
    matrix = []
    for i, task in enumerate(tasks):
        for arm in (ARMS if i % 2 == 0 else ARMS[::-1]):
            matrix.append(dict(task=task, arm=arm, order=len(matrix),
                prompt=(root / "private/tasks" / task / "prompt.txt").read_text()))
    descriptor = dict(checkpoint=10, family="mm-curve", training_tasks=train, action_kind="verify",
        files=record["artifact_sha256"], learned_home=str(home), route=route, binaries=binaries,
        checkpoint0_home=str(cold_home), checkpoint0_files=zero["artifact_sha256"], model=parent["model"],
        condition_command=record["command"], condition_output=str(cumulative / "condition.stdout.json"), source_sha256=sources())
    pairs.write(root / "checkpoint10-descriptor.json", descriptor)
    inputs.extend([native / "manifest.json", controls / "manifest.json", controls / "validation.json",
        cold / "attempt.json", root / "validation.json", root / "checkpoint10-descriptor.json", review,
        native / "sessions/native-confinement/manifest.json", native / "sessions/native-confinement/settings.json",
        *map(Path, binaries.values()), *[home / name for name in record["artifact_sha256"]],
        *[cold_home / name for name in zero["artifact_sha256"]]])
    families = copy.deepcopy(parent["families"])
    families[0]["heldout"] = tasks
    m = dict(protocol=PROTOCOL, protocol_revision=5, root=str(root), native_cohort_root=str(native),
        cumulative_root=str(cumulative), budget_authority=str(authority), budget_allocation_sha256=parent["budget_allocation_sha256"],
        preparation_python=parent["preparation_python"], python_requested=parent["python_requested"], model=parent["model"],
        families=families, matrix=matrix, task_sha256={t: old["task_sha256"][t] for t in tasks},
        source_sha256=sources(), frozen_inputs={str(p): pairs.sha(p) for p in inputs},
        descriptor_sha256=pairs.sha(root / "checkpoint10-descriptor.json"), training_kind=curve.GUIDED,
        candidate_selection="lexicographically first certified context route, before transfer dispatch",
        study_revision="Ten guided native demonstrations, five repeated tasks; transfer on four distinct-from-training tasks already observed in two cold pilots. No workflow guidance in transfer prompts.",
        budget=dict(evaluation_sessions=8, per_session_usd=1.5, reservation_usd=12),
        learning_frozen=True, grants_written=False)
    pairs.write(root / "manifest.json", m)
    (root / "manifest.sha256").write_text(pairs.sha(root / "manifest.json") + "\n")
    for source in m["source_sha256"]:
        target = root / "source" / Path(source).relative_to(gate.REPO)
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, target)
    return m


def prepare(root):
    m, _ = verify(root)
    return [heldout.prepare(root, task, root / "checkpoint10-descriptor.json") for task in m["families"][0]["heldout"]]


def run(root):
    m, _ = verify(root)
    # Preflight every assignment before spending on the first arm. Human grants are external.
    manifests = [root / "heldout" / e["task"] / e["arm"] / "manifest.json" for e in m["matrix"]]
    for manifest in manifests:
        assignment = pairs.read(manifest)
        pairs.verify(assignment)
        if assignment["arm"] == "automatic" and not (Path(assignment["home"]) / "approvals.json").is_file():
            raise ValueError("all exact automatic candidates need human grants before dispatch")
    return [heldout.live(manifest, wall_seconds=600) for manifest in manifests]


def report(root):
    m, _ = verify(root)
    authority = Path(m["budget_authority"])
    ledger = pairs.read(authority / "paid-state.json")
    rows = []
    for entry in m["matrix"]:
        session = root / "heldout" / entry["task"] / entry["arm"]
        attempt = ledger["runs"].get(str(session.relative_to(authority)), {})
        path = session / "collection.json"
        row = pairs.read(path) if path.exists() else dict(**entry, status="uncollected", cost_usd=attempt.get("summary", {}).get("total_cost_usd"))
        row["ledger_attempt"] = attempt
        rows.append(row)
    tasks = {task: heldout.task_result([r for r in rows if r["task"] == task], expected_arms=ARMS)
             for task in m["families"][0]["heldout"]}
    costs = [pairs.known_cost(r.get("cost_usd")) for r in rows]
    confirmed = sum(t.get("confirmed_elimination", False) for t in tasks.values())
    complete = all(r.get("status") != "uncollected" and not r.get("collection_errors") for r in rows)
    quality = all(r.get("quality", {}).get("passed") is True for r in rows)
    execution = all(heldout.approved_execution_evidence(r) if r["arm"] == "automatic"
        else not (r.get("all_journal_receipts") or r.get("execution_attempts")) for r in rows)
    uncertain = any(a["classification"] == pairs.bash_audit.UNKNOWN for r in rows if r["arm"] == "automatic"
        for a in pairs.bash_audit.audit_bash_actions(r.get("agent_actions", []))["actions"])
    result = dict(protocol=PROTOCOL, passed=confirmed >= 3 and complete and quality and execution
        and not uncertain and all(c is not None for c in costs), assignment_denominator=8, rows=rows, tasks=tasks,
        collected=sum(r.get("status") != "uncollected" for r in rows), confirmed_eliminations=confirmed,
        known_cost_usd=sum(c for c in costs if c is not None), unknown_cost_assignments=sum(c is None for c in costs),
        limitations=["Ten instruction-guided demonstrations across five repeated tasks; not autonomous discovery.",
            "Four transfer tasks excluded from learning but previously observed in two cold pilots; not globally unseen benchmarks.",
            "All eight assignments remain in the denominator, including failures and missing runs.",
            "Only identical fixed pytest commands qualify for confirmed elimination; unknown shell behavior disqualifies.",
            "API requests and tokens do not measure GPU utilization or energy."])
    pairs.write(root / "reports" / ("transfer-" + str(time.time_ns()) + ".json"), result)
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("freeze", "prepare", "run", "report"))
    parser.add_argument("root", type=Path)
    parser.add_argument("--cumulative", type=Path)
    parser.add_argument("--controls", type=Path)
    args = parser.parse_args()
    if args.command == "freeze":
        if args.cumulative is None or args.controls is None:
            parser.error("freeze requires --cumulative and --controls")
        result = freeze(args.root, args.cumulative, args.controls)
    else:
        result = globals()[args.command](args.root.resolve(strict=True))
    print(json.dumps(result, indent=2))
