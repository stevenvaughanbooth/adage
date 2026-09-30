#!/usr/bin/env python3
"""Freeze and validate the prospective cross-session pilot without spending model tokens.

Reuses real_repo_gate confinement, frozen dependencies, and the showcase scorer. This preparation
entry point does not dispatch paid agents, change budget admission, or create execution grants.
"""
from __future__ import annotations
import argparse
import json
from pathlib import Path
import subprocess
import shutil
import time

import curve_tasks
import real_repo_gate as gate
import real_repo_pairs as pairs

ARMS = ("agent", "instincts", "learned", "automatic")
CHECKPOINTS = (0, 3, 5, 10, 15)
GUIDED = "instruction-guided-training"
GUIDANCE = ("\n\nRepository workflow for this instruction-guided demonstration: after implementing "
            "the code, tests, and documentation, run `.venv/bin/python -m pytest tests/test_validate.py -q` "
            "from the repository root. Report its actual result; do not claim success if it fails.\n")
HERE = Path(__file__).resolve().parent


def sources():
    paths = [HERE / "curve_tasks.py", HERE / "learning_curve.py", HERE / "LEARNING-CURVE.md"]
    return {**pairs.source_hashes(), **{str(p): gate.sha(p) for p in paths}}


def freeze(root, authority, mode="organic-training"):
    if mode not in ("organic-training", GUIDED):
        raise ValueError("unknown training mode")
    guided = mode == GUIDED
    train = curve_tasks.TRAIN[:5] if guided else curve_tasks.TRAIN
    heldout = [] if guided else curve_tasks.HELDOUT
    authority = authority.resolve(strict=True)
    prior = pairs.read(authority / "manifest.json")
    plan = next(p for p in prior["families"] if p["family"] == "mm-validator")
    root.mkdir(parents=True, exist_ok=False)
    for relative in ("runtime", "cache", "bases"):
        (root / relative).symlink_to(authority / relative, target_is_directory=True)
    # Do not reuse a native agent sandbox approval: this is only the offline scoring substrate.
    families, _, _, _ = gate.modules(root)
    curve_tasks.install(families)
    rows = []
    for i, task in enumerate(heldout):
        for position in range(4):
            rows.append(dict(task=task, arm=ARMS[(i + position) % 4], order=len(rows),
                             prompt=families.prompt(task)))
    task_hashes, control_hashes = {}, {}
    for task in train + heldout:
        family, variant = families.parse_task(task)
        private = root / "private/tasks" / task
        private.mkdir(parents=True)
        (private / "prompt.txt").write_text(families.prompt(task))
        (private / "hidden.py").write_text(family["hidden"](variant))
        task_hashes[task] = {name: gate.sha(private / name) for name in ("prompt.txt", "hidden.py")}
        control_hashes[task] = dict(task_hashes[task])
        if guided:
            (private / "prompt.txt").write_text(families.prompt(task) + GUIDANCE)
            task_hashes[task]["prompt.txt"] = gate.sha(private / "prompt.txt")
    manifest = dict(schema=1, protocol="learning-curve-1", status="offline-preparation",
                    root=str(root), budget_authority=str(authority), source_sha256=sources(),
                    source_revision=subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=HERE, text=True).strip(),
                    preparation_python=prior["preparation_python"], python_requested=prior["python_requested"],
                    families=[dict(plan, family="mm-curve", train=train, heldout=heldout)],
                    checkpoints=list(CHECKPOINTS), matrix=rows, task_sha256=task_hashes,
                    budget=dict(training_sessions=15, evaluation_sessions=16, per_session_usd=1.5,
                                reservation_usd=46.5, new_probe_reservation_usd=0.5,
                                paid_scope_confirmed=False),
                    live_dispatch_enabled=False, grants_written=False,
                    candidate_selection="checkpoint-15 only; no candidate means stop, never hand-author one",
                    claims="Authored related feature tasks on one pinned upstream repository; screening only.")
    manifest.update(training_kind=mode, workflow_guidance=GUIDANCE if guided else None)
    if guided:
        manifest.update(protocol="instruction-guided-development-1", checkpoints=[0, 5],
            control_task_sha256=control_hashes,
            candidate_selection="checkpoint-5 only; no candidate means stop; unchanged production thresholds",
            claims="Development transfer from an explicitly instructed routine, not autonomous discovery or efficacy.")
        manifest["budget"].update(training_sessions=5, evaluation_sessions=0, reservation_usd=7.5)
    gate.write(root / "manifest.json", manifest)
    for name in manifest["source_sha256"]:
        source = Path(name)
        target = root / "source" / source.relative_to(gate.REPO)
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, target)
    return manifest


def verify(root):
    m = pairs.read(root / "manifest.json")
    if m["protocol"] not in ("learning-curve-1", "instruction-guided-development-1") or m["source_sha256"] != sources():
        raise ValueError("protocol source changed after freeze; retain this attempt and freeze a new root")
    guided = m["protocol"] == "instruction-guided-development-1"
    if (m.get("training_kind", "organic-training") != (GUIDED if guided else "organic-training")
            or m.get("workflow_guidance") != (GUIDANCE if guided else None)
            or guided and (m["checkpoints"] != [0, 5] or m["matrix"]
                or m["families"][0]["train"] != curve_tasks.TRAIN[:5] or m["families"][0]["heldout"])):
        raise ValueError("training mode or scope differs from protocol")
    for task, files in m["task_sha256"].items():
        for name, digest in files.items():
            if gate.sha(root / "private/tasks" / task / name) != digest:
                raise ValueError("task material changed after freeze")
    families, _, _, _ = gate.modules(root)
    curve_tasks.install(families)
    return m, families


def validate(root):
    m, families = verify(root)
    plan = m["families"][0]
    folder = root / "private/validation" / str(time.time_ns())
    folder.mkdir(parents=True)
    rows = []
    for task in plan["train"] + plan["heldout"]:
        family, variant = families.parse_task(task)
        arms = {}
        for arm in ("base", "gold"):
            checkout = folder / task / arm
            gate.create_checkout(root, plan["repo"], plan["base_full"], checkout)
            pairs.frozen_setup(root, checkout, plan, m["python_requested"])
            if arm == "gold":
                family["gold"](variant, checkout)
            arms[arm] = gate.score(root, task, checkout)
            gate.write(checkout.parent / (arm + ".json"), arms[arm])
        sensitive = (not arms["base"]["passed"] and arms["base"]["n_tests"] > 0
                     and bool(arms["base"]["hidden_failed"]) and not arms["base"]["suite_failed"])
        valid = sensitive and arms["gold"]["passed"] and arms["gold"]["n_tests"] > 0
        row = dict(task=task, valid=bool(valid), **arms)
        rows.append(row)
        report = dict(protocol=m["protocol"], complete=len(rows) == len(plan["train"] + plan["heldout"]), valid=all(r["valid"] for r in rows),
                      paid_calls=0, rows=rows, source_sha256=m["source_sha256"],
                      manifest_sha256=gate.sha(root / "manifest.json"))
        gate.write(folder / "report.json", report)
        gate.write(root / "validation.json", report)
        print(json.dumps(dict(task=task, valid=valid, completed=len(rows))), flush=True)
    return report


def history_inputs(manifest, count, records):
    """Select the exact chronological prefix; never discard failed tasks or duplicate identities.

    Records must be native provenance, in assigned order, with outcome labels retained. This is a
    pure admission check for checkpoint wiring, not proof that a transcript came from the runner.
    Runner/ledger hash checks remain mandatory before invoking the production learner.
    """
    if count not in manifest.get("checkpoints", CHECKPOINTS):
        raise ValueError("unregistered checkpoint")
    expected = manifest["families"][0]["train"][:count]
    prefix = records[:count]
    if len(prefix) != count or [r["task"] for r in prefix] != expected:
        raise ValueError("missing, reordered, or held-out training task")
    if len({r["session_id"] for r in prefix}) != count:
        raise ValueError("duplicate session identity")
    if len({r["sha256"] for r in prefix}) != count:
        raise ValueError("duplicate transcript content")
    if any(r.get("synthetic") is not False or r.get("hooks_enabled") is not False for r in prefix):
        raise ValueError("training must be native and unexposed")
    kind = manifest.get("training_kind", "organic-training")
    if any(r.get("kind", "organic-training") != kind
           or r.get("workflow_guidance") != manifest.get("workflow_guidance") for r in prefix):
        raise ValueError("guided and organic history must not be mixed or relabeled")
    return prefix


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("command", choices=("freeze", "validate"))
    p.add_argument("root", type=Path)
    p.add_argument("--authority", type=Path)
    args = p.parse_args()
    if args.command == "freeze":
        if args.authority is None:
            p.error("freeze requires --authority")
        result = freeze(args.root.resolve(), args.authority)
    else:
        result = validate(args.root.resolve(strict=True))
    print(json.dumps({k: result[k] for k in ("protocol", "status", "complete", "valid", "paid_calls") if k in result}))


if __name__ == "__main__":
    main()
