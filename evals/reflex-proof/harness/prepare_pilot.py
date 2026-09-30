#!/usr/bin/env python3
"""Prepare an outcome-assisted production-path pilot offline; never grant or launch a model."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile


def write(path, value):
    path.write_text(json.dumps(value, indent=2) + "\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path)
    args = parser.parse_args()
    repo = Path(__file__).resolve().parents[3]
    root = args.root or Path(tempfile.mkdtemp(prefix="adage-production-pilot-"))
    root.mkdir(parents=True, exist_ok=True)
    if any(root.iterdir()):
        parser.error("root must be empty")
    root = root.resolve()
    binary = repo / "target/debug/adage"
    data = repo / "evals/datasets/live-20260928-py-clinic-s12"
    manifest = json.loads((data / "manifest.json").read_text())
    train = [s for s in manifest["sessions"] if s["phase"] == "train"]
    assert all(s["task"] != "CARE-279" for s in train)
    private = root / "private"
    private.mkdir()
    outcomes = private / "outcomes.json"
    write(outcomes, {s["id"]: s.get("passed") for s in train})
    suite = json.loads((repo / "evals/gen/specs/smoke.json").read_text())
    spec = next(s["spec"] for s in suite["sandboxes"] if s["seed"] == 12)
    write(private / "spec.json", spec)
    subprocess.run(["python3", "-B", str(repo / "evals/gen/gen.py"), "--spec",
                    str(private / "spec.json"), "--seed", "12", "--out", str(private / "bundle")], check=True)
    home = private / "adage-home"
    command = [str(binary), "condition", "--json", "--repo", "wardline-clinic",
               "--outcomes", str(outcomes), "--transcripts", *[str(data / s["file"]) for s in train]]
    result = subprocess.run(command, env=dict(os.environ, ADAGE_HOME=str(home)), capture_output=True, text=True)
    (private / "learn.stdout.json").write_text(result.stdout)
    (private / "learn.stderr.txt").write_text(result.stderr)
    result.check_returncode()
    shutil.copytree(home, private / "e1-home")  # Same learned index, deliberately no grants.
    index = json.loads((home / "index.json").read_text())
    routes = [{"track": r["track"], "id": r["route"]["id"],
               "step": r["route"]["step"]["do"],
               "command": r["route"]["step"].get("command"),
               "trigger": r["route"]["trigger"]["on"]} for r in index["routes"]]
    config = json.loads((private / "bundle/config.json").read_text())
    config.update(name="reflex-proof-production-pilot", model="claude-haiku-4-5-20251001",
                  budget_usd=3.0, concurrency=1, max_turns=30, confine=True, host_auth=False,
                  seed=20260929, learn_args=[])
    config["eval"] = {"tasks": ["CARE-279"], "repeats": 1, "arms": ["adage", "e1"]}
    write(root / "pilot.json", config)
    record = {
        "revision": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repo, text=True).strip(),
        "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
        "training_sessions": len(train), "training_only": True, "default_thresholds": True,
        "learning_protocol": "outcome-assisted-condition", "shipped_default_bootstrap": False,
        "inputs": [{"file": s["file"], "session_id": s["id"], "task": s["task"],
                    "sha256": hashlib.sha256((data / s["file"]).read_bytes()).hexdigest()}
                   for s in train],
        "routes": routes, "heldout_task": "CARE-279", "grants_written": False,
        "learned_index_sha256": hashlib.sha256((home / "index.json").read_bytes()).hexdigest(),
        "learned_files": [{"file": str(p.relative_to(home)), "sha256": hashlib.sha256(p.read_bytes()).hexdigest()}
                          for p in sorted((home / "tracks").rglob("*")) if p.is_file()],
        "paid_calls": 0, "cost_usd": 0,
        "fixture_script_sha256": hashlib.sha256((private / "bundle/repo/tools/case").read_bytes()).hexdigest(),
        "condition_argv": command,
    }
    write(root / "preparation.json", record)
    print(json.dumps({"root": str(root), "routes": routes, "grants_written": False}, indent=2))


if __name__ == "__main__":
    main()
