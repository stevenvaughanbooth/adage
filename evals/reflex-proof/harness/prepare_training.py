#!/usr/bin/env python3
"""Prepare unsteered training with pinned offline dependencies and proven startup settings.

Never dispatches a model. Keeps the original recipe and writes a separately frozen revision.
"""
import argparse
import copy
import json
from pathlib import Path
import tempfile

import real_repo_gate as gate
import real_repo_pairs as pairs


def revise(manifest):
    manifest = Path(manifest).resolve(strict=True)
    value = json.loads(manifest.read_text())
    root = Path(value["root"])
    if value["kind"] != "organic-training" or value["training_hooks_enabled"]:
        raise ValueError("only unsteered organic training is eligible")
    if (root / "run.json").exists() or (root / "events.jsonl").exists():
        raise ValueError("cannot revise an attempted assignment")
    target = root / "manifest-v4.json"
    if target.exists():
        raise ValueError("revised recipe already exists")
    value = copy.deepcopy(value)
    inbox = Path(tempfile.mkdtemp(prefix="adage-train-", dir="/tmp")).resolve()
    inbox.chmod(0o700)
    argv = value["argv"]
    argv[-1:-1] = ["--messaging-socket-path", str(inbox / "inbox.sock")]
    value["argv"] = ["/usr/bin/env", "CLAUDE_CODE_PROFILE_STARTUP=1", *argv]
    value.update(protocol_revision=4, previous_manifest_sha256=gate.sha(manifest),
                 previous_manifest=str(manifest),
                 preparation="Pinned offline dependencies; native probe v4 startup settings; no guidance")
    for source in (Path(__file__).resolve(), Path(pairs.__file__).resolve(), manifest):
        value["shared_sources"][str(source)] = gate.sha(source)
    gate.write(target, value)
    return target


def prepare(root, task, model):
    root = Path(root).resolve(strict=True)
    if not json.loads((root / "native-confinement-reviewed.json").read_text()).get("passed"):
        raise ValueError("reviewed native confinement required before training preparation")
    base = json.loads((root / "manifest.json").read_text())
    plans = [p for p in base["families"] if task in p["train"]]
    if len(plans) != 1:
        raise ValueError("task must be in the prospectively assigned training split")
    # Reuse the paired scorer's trusted-base, pinned offline installation. The original
    # frozen helper and recipe remain unchanged; every setup command is still journaled.
    original = gate.setup
    try:
        gate.setup = lambda r, checkout, repo, python: pairs.frozen_setup(r, checkout, plans[0], python)
        value = gate.prepare_session(root, task, model)
    finally:
        gate.setup = original
    return revise(Path(value["root"]) / "manifest.json")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", type=Path)
    parser.add_argument("task")
    parser.add_argument("--model", default="claude-haiku-4-5-20251001")
    args = parser.parse_args()
    print(prepare(args.root, args.task, args.model))
