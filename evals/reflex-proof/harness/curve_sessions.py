"""Native training and checkpoint preparation for the frozen learning-curve pilot."""
from contextlib import contextmanager
from datetime import datetime, timezone
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import time

import cohort_v5 as native
import curve_tasks
import learning_curve as curve
import real_repo_gate as gate
import real_repo_pairs as pairs


@contextmanager
def registry(root):
    # Scoring imports set cache env vars. They must not change the native sandbox's env denylist.
    before = dict(os.environ)
    try:
        modules = gate.modules(root)
        curve_tasks.install(modules[0])
        os.environ.clear()
        os.environ.update(before)
        yield modules
    finally:
        os.environ.clear()
        os.environ.update(before)


def prepare(root, authority, controls, mode="organic-training"):
    root, authority, controls = root.resolve(), authority.resolve(strict=True), controls.resolve(strict=True)
    if authority not in root.parents:
        raise ValueError("live cohort must share its original ancestor ledger")
    old = pairs.read(controls / "manifest.json")
    validation = pairs.read(controls / "validation.json")
    if not validation["complete"] or not validation["valid"] or len(validation["rows"]) != 19:
        raise ValueError("all 19 trusted task controls must pass")
    with registry(authority):
        m = curve.freeze(root, authority, mode)
    control_hashes = m.get("control_task_sha256", m["task_sha256"])
    if any(old["task_sha256"].get(t) != files for t, files in control_hashes.items()):
        raise ValueError("new tasks differ from validated controls")
    for task, files in control_hashes.items():
        if any(gate.sha(controls / "private/tasks" / task / name) != digest for name, digest in files.items()):
            raise ValueError("validated task control material changed")
    # Keep full old controls and provenance, rather than presenting them as new validation.
    shutil.copyfile(controls / "validation.json", root / "validation.json")
    (root / "private/validation").symlink_to(controls / "private/validation", target_is_directory=True)
    allocation = authority / "evaluation-allocation.json"
    m.update(protocol_revision=5, policy="generic-sandboxed-bash", model=native.MODEL,
             budget_allocation_sha256=gate.sha(allocation),
             controls_reused=dict(root=str(controls), manifest_sha256=gate.sha(controls / "manifest.json"),
                                  validation_sha256=gate.sha(controls / "validation.json")),
             prospective_revision="15 new native sessions; all historical training excluded; user approved paid pilot.")
    if mode == curve.GUIDED:
        m["prospective_revision"] = "Five explicit instruction-guided development demonstrations; no autonomous-discovery or efficacy claim."
    m["budget"]["paid_scope_confirmed"] = True
    m["session_helper_sha256"] = gate.sha(Path(__file__))
    shutil.copyfile(Path(__file__), root / "source/evals/reflex-proof/harness/curve_sessions.py")
    gate.write(root / "manifest.json", m)
    gate.budget_limit(dict(experiment_root=str(authority), budget_allocation_sha256=m["budget_allocation_sha256"]))
    plan = m["families"][0]
    # Same direct sandbox, fresh positive and negative controls for this root.
    gate.confinement_preflight(root, Path(plan["checkout"]))
    return m


def prepare_session(root, task):
    root = root.resolve(strict=True)
    with registry(root):
        m, _ = curve.verify(root)
        if m["session_helper_sha256"] != gate.sha(Path(__file__)):
            raise ValueError("training helper changed after freeze")
    is_native = task == "native-confinement"
    if not is_native:
        review = root / "native-confinement-reviewed.json"
        gate.require_native_review(dict(experiment_root=m["budget_authority"], cohort_root=str(root),
            native_review_path=str(review), native_review_sha256=gate.sha(review)))
    with registry(root):
        value = gate.prepare_session(root, task, m["model"], native_probe=is_native, setup_checkout=pairs.frozen_setup)
    # Policy generation must happen after restoring the caller environment.
    path = native.finalize_recipe(value, root, is_native)
    value = pairs.read(path)
    value["budget_allocation_sha256"] = m["budget_allocation_sha256"]
    value["shared_sources"].update({str(p): gate.sha(p) for p in
        (Path(__file__), Path(curve.__file__), Path(curve_tasks.__file__),
         Path(m["budget_authority"]) / "evaluation-allocation.json")})
    gate.write(path, value)
    return path


def review_native(root):
    session = root / "sessions/native-confinement"
    m = pairs.read(session / "manifest.json")
    summary = pairs.read(session / "summary.json")
    run = pairs.read(session / "run.json")
    uses, results = {}, {}
    for line in (session / "native-transcript.jsonl").read_text().splitlines():
        content = json.loads(line).get("message", {}).get("content", [])
        for block in content if isinstance(content, list) else []:
            if block.get("type") == "tool_use":
                uses[block["id"]] = block
            if block.get("type") == "tool_result":
                results[block["tool_use_id"]] = block
    commands = [k for k, v in uses.items() if v["name"] == "Bash" and v["input"].get("command") == ".venv/bin/python native_canary.py"]
    reads = [k for k, v in uses.items() if v["name"] == "Read"
             and v["input"].get("file_path") == str(root / "private/native-read-canary.txt")]
    if len(commands) != 1 or len(reads) != 1:
        raise ValueError("native probe did not perform the two expected operations")
    result = results[commands[0]]
    checks = json.loads(result["content"].split("NATIVE_SANDBOX_CHECKS=", 1)[1])
    expected = {k: True for k in ("read", "outside_write", "dependency_write", "git_write", "network", "inside_write",
                                  "env_sentinel_denied", "env_secret_names_absent")}
    checkout = Path(m["execution_root"])
    control = pairs.read(session / "outside-control.json")
    passed = (checks == expected and result.get("is_error") is False and results[reads[0]].get("is_error") is True
        and gate.sha(checkout / "native_canary.py") == m["canary_sha256"]
        and (checkout / "inside-write-canary").read_text() == "allowed"
        and not any(p.exists() for p in (root / "private/native-write-canary", checkout / ".venv/native-write-canary", checkout / ".git/native-write-canary"))
        and control == dict(network_connected=True, read_canary_exists=True)
        and "ADAGE_CONFINEMENT_SENTINEL" in run.get("launch_environment_names", [])
        and run["exit_code"] == 0 and summary["result_is_error"] is False)
    record = dict(passed=passed, cohort_root=str(root), manifest_sha256=gate.sha(session / "manifest.json"),
        checks=checks, controls=control, cost_usd=summary["total_cost_usd"], model=m["model"],
        reviewed_at=datetime.now(timezone.utc).isoformat(),
        hashes={p.name: gate.sha(p) for p in session.iterdir() if p.is_file()})
    path = root / "native-confinement-reviewed.json"
    if path.exists():
        raise ValueError("review is immutable")
    gate.write(path, record)
    if not passed:
        raise ValueError("native confinement failed")
    return record


def checkpoint(root, count, binary):
    root, binary = root.resolve(strict=True), binary.resolve(strict=True)
    with registry(root):
        m, _ = curve.verify(root)
    if m["session_helper_sha256"] != gate.sha(Path(__file__)):
        raise ValueError("training helper changed after freeze")
    authority = Path(m["budget_authority"])
    state = pairs.read(authority / "paid-state.json")
    records = []
    for task in m["families"][0]["train"][:count]:
        session = root / "sessions" / task
        manifest, provenance = pairs.read(session / "manifest.json"), pairs.read(session / "transcript-provenance.json")
        key = str(session.relative_to(authority))
        terminal = state["runs"][key]
        if (terminal["status"] != "completed" or manifest["kind"] != m.get("training_kind", "organic-training")
                or terminal.get("kind") != manifest["kind"]
                or manifest["source_sha256"] != gate.sha(Path(gate.__file__))
                or any(gate.sha(Path(p)) != digest for p, digest in manifest["shared_sources"].items())
                or terminal["transcript"] != provenance
                or provenance["sha256"] != gate.sha(session / "native-transcript.jsonl")
                or manifest["settings_sha256"] != gate.sha(session / "settings.json")
                or Path(provenance["cwd"]).resolve() != Path(manifest["execution_root"]).resolve()
                or not pairs.read(session / "settings.json")["disableAllHooks"]):
            raise ValueError("native history provenance or ledger mismatch")
        gate.validate_training_recipe(manifest)
        records.append(dict(task=task, session_id=provenance["session_id"], sha256=provenance["sha256"],
            kind=manifest["kind"], workflow_guidance=manifest.get("workflow_guidance"),
            synthetic=provenance["synthetic"], hooks_enabled=manifest["training_hooks_enabled"],
            source=str(session / "native-transcript.jsonl"), outcome=terminal.get("result"),
            manifest_sha256=gate.sha(session / "manifest.json"), provenance_sha256=gate.sha(session / "transcript-provenance.json")))
    records = curve.history_inputs(m, count, records)
    folder = root / "checkpoints" / str(count)
    folder.mkdir(parents=True, exist_ok=False)
    home = folder / "adage-home"
    history = folder / "history"
    history.mkdir()
    for row in records:
        dest = history / (row["session_id"] + ".jsonl")
        shutil.copyfile(row["source"], dest)
        if gate.sha(dest) != row["sha256"]:
            raise ValueError("history copy changed")
        row["copy"] = str(dest)
    command = [str(binary), "condition", "--plain", "--json"]
    if count:
        command += ["--transcripts", *[r["copy"] for r in records]]
    else:
        command += ["--instincts-only", "--repo-path", m["families"][0]["checkout"]]
    env = {k: v for k, v in os.environ.items() if not k.startswith("ADAGE_")}
    env.update(ADAGE_HOME=str(home), ADAGE_AUTO_LEARN="0")
    record = dict(count=count, inputs=records, command=command, binary=str(binary), binary_sha256=gate.sha(binary),
                  protocol=m["protocol"], training_kind=m.get("training_kind", "organic-training"),
                  workflow_guidance=m.get("workflow_guidance"),
                  source_sha256=m["source_sha256"], status="started", started_ns=time.time_ns(), paid_calls=0)
    record["production_source_sha256"] = {str(p.relative_to(gate.REPO)): gate.sha(p)
        for p in [*sorted((gate.REPO / "crates").rglob("*")), gate.REPO / "Cargo.toml", gate.REPO / "Cargo.lock"] if p.is_file()}
    record["git_head"] = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=gate.REPO, text=True).strip()
    gate.write(folder / "attempt.json", record)
    result = subprocess.run(command, cwd=folder, env=env, capture_output=True, text=True, timeout=600)
    (folder / "condition.stdout.json").write_text(result.stdout)
    (folder / "condition.stderr.txt").write_text(result.stderr)
    record.update(exit_code=result.returncode, elapsed_ns=time.time_ns()-record["started_ns"],
                  status="completed" if result.returncode == 0 else "failed")
    if result.returncode == 0:
        index = pairs.read(home / "index.json")
        record.update(route_count=len(index["routes"]), execution_candidates=[r for r in index["routes"]
            if r["route"].get("evidence", {}).get("execution")])
        record["artifact_sha256"] = {str(p.relative_to(home)): gate.sha(p) for p in [home / "index.json", *sorted((home / "tracks").rglob("*"))] if p.is_file()}
    if gate.sha(binary) != record["binary_sha256"] or (home / "approvals.json").exists():
        record["status"] = "invalid-binary-drift-or-grant"
    gate.write(folder / "attempt.json", record)
    return record


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("prepare", "session", "run", "review-native", "checkpoint"))
    parser.add_argument("root", type=Path)
    parser.add_argument("--authority", type=Path)
    parser.add_argument("--controls", type=Path)
    parser.add_argument("--mode", choices=("organic-training", curve.GUIDED), default="organic-training")
    parser.add_argument("--task", default="native-confinement")
    parser.add_argument("--count", type=int)
    parser.add_argument("--binary", type=Path)
    args = parser.parse_args()
    if args.command == "prepare":
        result = prepare(args.root, args.authority, args.controls, args.mode)
    elif args.command == "session":
        result = str(prepare_session(args.root, args.task))
    elif args.command == "run":
        result = gate.run_session(args.root / "sessions" / args.task / "manifest.json", wall_seconds=600)
    elif args.command == "review-native":
        result = review_native(args.root)
    else:
        result = checkpoint(args.root, args.count, args.binary)
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
