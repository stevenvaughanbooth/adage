#!/usr/bin/env python3
"""Prepare a fresh policy cohort offline, sharing the original paid ledger."""
import argparse
import copy
import json
import os
from pathlib import Path
import shutil
import tempfile

import real_repo_gate as gate
import real_repo_pairs as pairs

MODEL = "claude-haiku-4-5-20251001"


def policy(authority, checkout, runtime, protected=()):
    settings = gate.native_settings(authority, checkout,
        ["Read", "Edit", "Write", "Glob", "Grep", "Bash"], protected)
    settings["sandbox"]["filesystem"]["allowRead"] = [str(checkout.resolve()), str(runtime.resolve())]
    return settings


def startup_argv(argv):
    """Identical startup isolation for native, training and paired assignments."""
    inbox = Path(tempfile.mkdtemp(prefix="adage-np5-", dir="/tmp")).resolve()
    inbox.chmod(0o700)
    socket = inbox / "inbox.sock"
    if len(os.fsencode(socket)) >= 100:
        raise ValueError("messaging socket exceeds portable native limit")
    argv = list(argv)
    argv[-1:-1] = ["--messaging-socket-path", str(socket)]
    return ["/usr/bin/env", "CLAUDE_CODE_PROFILE_STARTUP=1", *argv]


def prepare_cohort(authority, destination):
    authority, destination = authority.resolve(strict=True), destination.resolve()
    if authority not in destination.parents:
        raise ValueError("cohort must remain under its original budget authority")
    destination.mkdir(parents=True, exist_ok=False, mode=0o700)
    original = authority / "manifest.json"
    value = copy.deepcopy(pairs.read(original))
    value.update(root=str(destination), protocol_revision=5, budget_authority=str(authority),
        policy="generic-sandboxed-bash", prior_manifest=str(original), prior_manifest_sha256=gate.sha(original),
        prior_cohort_eligibility="Entire previous policy cohort excluded from learning regardless of outcome; retained as infrastructure attempts.",
        prospective_revision="Fresh same-matrix cohort after bare python pytest permission denials; no held-out outcomes observed.",
        training_gate="Five fresh unsteered sessions per family under protocol 5; no old sessions admitted.",
        source_files={str(p):gate.sha(p) for p in (Path(__file__), Path(gate.__file__), Path(pairs.__file__))})
    # Setup/scoring alone can access these trusted inputs. Native agent tools deny their parent.
    for relative in ("runtime/python", "cache/uv", "cache/repobench", "bases", "private/validation"):
        link = destination / relative
        link.parent.mkdir(parents=True, exist_ok=True)
        link.symlink_to((authority / relative).resolve(strict=True), target_is_directory=True)
    for name in ("validation.json", "confinement.json"):
        shutil.copy2(authority / name, destination / name)
    value["trusted_reuse"] = {relative:str((destination / relative).resolve()) for relative in
                             ("runtime/python", "cache/uv", "cache/repobench", "bases", "private/validation")}
    gate.write(destination / "manifest.json", value)
    gate.append(destination / "attempts.jsonl", dict(status="cohort-prepared", paid_calls=0,
        protocol_revision=5, budget_authority=str(authority), old_training_excluded=True))
    return destination / "manifest.json"


def finalize_recipe(value, root, native=False):
    control = pairs.read(root / "manifest.json")
    authority, session = Path(control["budget_authority"]), Path(value["root"])
    checkout = Path(value["execution_root"])
    protected = [checkout / "native_canary.py"] if native else []
    settings = policy(authority, checkout, root / "runtime/python", protected)
    if native:
        settings["permissions"]["allow"] = ["Read", "Bash"]
    gate.write(session / "settings.json", settings)
    value.update(protocol_revision=5, cohort_root=str(root), experiment_root=str(authority),
        policy=control["policy"], settings_sha256=gate.sha(session / "settings.json"),
        source_sha256=gate.sha(Path(gate.__file__)),
        native_review_path=str(root / "native-confinement-reviewed.json"),
        budget_authority_manifest_sha256=gate.sha(authority / "manifest.json"))
    if not native:
        review = root / "native-confinement-reviewed.json"
        value["native_review_sha256"] = gate.sha(review)
        gate.require_native_review(value)
    value["argv"] = startup_argv(value["argv"])
    sources = (Path(__file__), Path(pairs.__file__), root / "manifest.json", authority / "manifest.json",
               root / "confinement.json", root / "validation.json")
    value["shared_sources"].update({str(p):gate.sha(p) for p in sources})
    gate.validate_authority(value)
    gate.write(session / "manifest.json", value)
    return session / "manifest.json"


def prepare_session(root, task="native-confinement"):
    root = root.resolve(strict=True)
    native = task == "native-confinement"
    if not native:
        # Fail before materializing any training assignment if this cohort is unreviewed.
        control = pairs.read(root / "manifest.json")
        review = root / "native-confinement-reviewed.json"
        gate.require_native_review(dict(experiment_root=control["budget_authority"], cohort_root=str(root),
            native_review_path=str(review), native_review_sha256=gate.sha(review)))
    value = gate.prepare_session(root, task, MODEL, native_probe=native, setup_checkout=pairs.frozen_setup)
    return finalize_recipe(value, root, native)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    cohort = sub.add_parser("prepare-cohort")
    cohort.add_argument("authority", type=Path)
    cohort.add_argument("destination", type=Path)
    session = sub.add_parser("prepare-session")
    session.add_argument("root", type=Path)
    session.add_argument("--task", default="native-confinement")
    args = parser.parse_args()
    print(prepare_cohort(args.authority, args.destination) if args.command == "prepare-cohort"
          else prepare_session(args.root, args.task))
