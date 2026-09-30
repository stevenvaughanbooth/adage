#!/usr/bin/env python3
"""Offline-only preparation of a diagnostic revision; never invokes Claude or clears accounting.

Reuse the unchanged disposable canary checkout from the startup-censored attempt, but preserve
its logs/manifests and allocate new logs and a short private cross-session inbox path. The shared
original live runner remains the sole dispatcher and still refuses unresolved accounting.
"""
import argparse
import copy
import hashlib
import json
import os
from pathlib import Path
import tempfile


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def prepare(original, destination):
    original = original.resolve(strict=True)
    old = json.loads(original.read_text())
    old_root = Path(old["root"])
    if old.get("kind") != "native-confinement" or old.get("budget_usd") != .5 or old.get("max_turns") != 5:
        raise ValueError("requires the bounded original native canary recipe")
    run = json.loads((old_root / "run.json").read_text())
    if not run.get("cancelled") or (old_root / "stdout.jsonl").stat().st_size:
        raise ValueError("only a startup-censored attempt with no stream output can use this revision")
    checkout = Path(old["execution_root"])
    if (checkout / "inside-write-canary").exists() or sha(checkout / "native_canary.py") != old["canary_sha256"]:
        raise ValueError("the original disposable canary changed or executed")
    if sha(old_root / "settings.json") != old["settings_sha256"]:
        raise ValueError("original settings changed")
    destination = destination.resolve()
    if destination.parent != old_root.parent:
        raise ValueError("new logs must be a sibling assignment in the same experiment ledger")
    destination.mkdir(mode=0o700, parents=False, exist_ok=False)
    inbox = Path(tempfile.mkdtemp(prefix="adage-np3-", dir="/tmp")).resolve()
    inbox.chmod(0o700)
    socket = inbox / "inbox.sock"
    if len(os.fsencode(socket)) >= 100:
        raise ValueError("native Unix socket path unexpectedly long")
    settings = destination / "settings.json"
    settings.write_bytes((old_root / "settings.json").read_bytes())
    (destination / ".reflex-fixture").write_text("native confinement diagnostic protocol revision 3\n")
    value = copy.deepcopy(old)
    value.update(root=str(destination), protocol_revision=3, prepared_by=str(Path(__file__).resolve()),
        supersedes_for_diagnostics=str(original), previous_manifest_sha256=sha(original),
        startup_changes=["fresh short private messaging socket", "startup profiler enabled"],
        unchanged=["model", "tool permissions", "sandbox confinement", "canary", "host login", "agent guards"],
        live_admission="unchanged shared ledger; unknown prior cost remains blocking",
        settings_sha256=sha(settings), paid_calls=0)
    argv = value["argv"]
    argv[argv.index("--settings") + 1] = str(settings)
    argv[argv.index("--debug-file") + 1] = str(destination / "claude-debug.log")
    argv[-1:-1] = ["--messaging-socket-path", str(socket)]
    value["argv"] = ["/usr/bin/env", "CLAUDE_CODE_PROFILE_STARTUP=1", *argv]
    value["shared_sources"][str(Path(__file__).resolve())] = sha(Path(__file__))
    value["shared_sources"][str(original)] = sha(original)
    target = destination / "manifest.json"
    target.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n")
    return target


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("original", type=Path)
    parser.add_argument("destination", type=Path)
    args = parser.parse_args()
    print(prepare(args.original, args.destination))
