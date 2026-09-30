"""Replay frozen native training after a code change; never overwrite the prospective checkpoint."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import time

import real_repo_gate as gate
import real_repo_pairs as pairs


def validated_inputs(original):
    """Replay one complete frozen checkpoint without mixing training regimes."""
    count, rows = original.get("count"), original.get("inputs", [])
    if (original.get("status") != "completed" or type(count) is not int or count <= 0
            or len(rows) != count):
        raise ValueError("complete nonempty chronological checkpoint required")
    kind = rows[0].get("kind", "organic-training")
    guidance = rows[0].get("workflow_guidance")
    if kind not in ("organic-training", "instruction-guided-training"):
        raise ValueError("unknown training regime")
    if (len({r["session_id"] for r in rows}) != count or len({r["sha256"] for r in rows}) != count
            or any(r.get("kind", "organic-training") != kind
                   or r.get("workflow_guidance") != guidance for r in rows)):
        raise ValueError("duplicate or mixed training history")
    for row in rows:
        if (row.get("synthetic") is not False or row.get("hooks_enabled") is not False
                or gate.sha(Path(row["copy"])) != row["sha256"]):
            raise ValueError("frozen native history changed or exposed")
    return rows


def replay(checkpoint, output, binary):
    checkpoint, output, binary = checkpoint.resolve(strict=True), output.resolve(), binary.resolve(strict=True)
    original = pairs.read(checkpoint / "attempt.json")
    rows = validated_inputs(original)
    output.mkdir(parents=True, exist_ok=False)
    history = output / "history"
    history.mkdir()
    inputs = []
    for row in rows:
        dest = history / Path(row["copy"]).name
        shutil.copyfile(row["copy"], dest)
        if gate.sha(dest) != row["sha256"]:
            raise ValueError("history copy changed")
        inputs.append(dict(row, copy=str(dest)))
    home = output / "adage-home"
    executable = output / "adage"
    shutil.copy2(binary, executable)
    command = [str(executable), "condition", "--plain", "--json", "--transcripts", *[r["copy"] for r in inputs]]
    sources = {str(p.relative_to(gate.REPO)): gate.sha(p) for p in
        [*sorted((gate.REPO / "crates").rglob("*")), gate.REPO / "Cargo.toml", gate.REPO / "Cargo.lock"] if p.is_file()}
    record = dict(kind="development-replay", status="started", original_checkpoint=str(checkpoint),
        original_attempt_sha256=gate.sha(checkpoint / "attempt.json"), count=original["count"], inputs=inputs,
        training_kind=rows[0].get("kind", "organic-training"), workflow_guidance=rows[0].get("workflow_guidance"),
        production_source_sha256=sources, binary_sha256=gate.sha(executable), command=command,
        paid_calls=0, grants_written=False, efficacy_result=False,
        limitation="Code changed after observing training. This is development evidence, not held-out efficacy.")
    gate.write(output / "attempt.json", record)
    env = {k: v for k, v in os.environ.items() if not k.startswith("ADAGE_")}
    env.update(ADAGE_HOME=str(home), ADAGE_AUTO_LEARN="0")
    started = time.monotonic_ns()
    result = subprocess.run(command, cwd=output, env=env, capture_output=True, text=True, timeout=600)
    (output / "condition.stdout.json").write_text(result.stdout)
    (output / "condition.stderr.txt").write_text(result.stderr)
    record.update(status="completed" if result.returncode == 0 else "failed", exit_code=result.returncode,
                  elapsed_ns=time.monotonic_ns() - started)
    if result.returncode == 0:
        index = pairs.read(home / "index.json")
        record.update(route_count=len(index["routes"]), execution_candidates=[r for r in index["routes"]
            if r["route"].get("evidence", {}).get("execution")])
        record["artifact_sha256"] = {str(p.relative_to(home)): gate.sha(p) for p in
            [home / "index.json", *sorted((home / "tracks").rglob("*"))] if p.is_file()}
    gate.write(output / "attempt.json", record)
    return record


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("checkpoint", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("binary", type=Path)
    args = parser.parse_args()
    result = replay(args.checkpoint, args.output, args.binary)
    print(json.dumps({k: result.get(k) for k in ("status", "route_count", "execution_candidates", "paid_calls")}))
