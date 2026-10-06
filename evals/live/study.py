#!/usr/bin/env python3
"""Audit and seal a private launch-study inventory. Never dispatches a model or authorizes spend."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import tempfile

HERE = Path(__file__).resolve().parent
PHASES = ("train", "development", "confirmation")
CONTROL_ROLES = {"reference", "base", "partial", "forged_report"}


def encoded(value):
    return (json.dumps(value, sort_keys=True, indent=2, allow_nan=False) + "\n").encode()


def digest(path):
    value = hashlib.sha256()
    with Path(path).open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            value.update(chunk)
    return value.hexdigest()


def decoded(data):
    def unique(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise ValueError(f"duplicate JSON key: {key}")
            result[key] = value
        return result
    return json.loads(data, object_pairs_hook=unique,
                      parse_constant=lambda value: (_ for _ in ()).throw(ValueError(value)))


def read(path):
    return decoded(Path(path).read_bytes())


def require(condition, message):
    if not condition:
        raise ValueError(message)


def nonempty(value):
    return isinstance(value, str) and bool(value.strip())


def regular(path):
    path = Path(path)
    return path.is_absolute() and path.is_file() and not any(p.is_symlink() for p in (path, *path.parents))


def validate(spec):
    """Reject ambiguity; incomplete but well-formed inventories can still be audited."""
    require(isinstance(spec, dict), "study must be an object")
    require(spec.get("schema") == 1, "unsupported study schema")
    for field in ("name", "scenario", "repo", "population", "sampling_rule"):
        require(nonempty(spec.get(field)), f"missing {field}")
    require(spec.get("treatment") == "production_learned_e0", "only production-learned E0 is supported")
    require(spec.get("policy") == read(HERE / "policies" / "claim.json"), "launch policy must match policies/claim.json")
    require(isinstance(spec.get("inputs"), dict), "inputs must map absolute paths to SHA-256 digests")
    for path, sha in spec["inputs"].items():
        require(Path(path).is_absolute() and isinstance(sha, str) and re.fullmatch(r"[0-9a-f]{64}", sha),
                "invalid pinned input")
    require(isinstance(spec.get("tasks"), list), "tasks must be a list")
    require(isinstance(spec.get("prior_exposure_keys"), list)
            and all(nonempty(key) and key == key.strip() for key in spec["prior_exposure_keys"]), "declare prior exposure keys")
    ids, groups, keys = set(), {}, {}
    for task in spec["tasks"]:
        require(isinstance(task, dict), "task must be an object")
        for key in ("id", "group", "prompt", "controls"):
            require(nonempty(task.get(key)), f"task missing {key}")
        require(all(task[key] == task[key].strip() for key in ("id", "group")), "task identities must be canonical")
        require(task["id"] not in ids, "duplicate task id")
        ids.add(task["id"])
        require(task.get("phase") in PHASES, "invalid task phase")
        require(type(task.get("exposed")) is bool, "task exposure must be explicit")
        require(task.get("provenance") in ("historical", "prospective", "synthetic"), "invalid provenance")
        require(isinstance(task.get("base_commit"), str) and re.fullmatch(r"[0-9a-f]{40}", task["base_commit"]),
                "pin the full base commit")
        require(isinstance(task.get("related_keys"), list) and task["related_keys"]
                and all(nonempty(key) and key == key.strip() for key in task["related_keys"]), "declare issue/feature relationship keys")
        require(task["group"] not in groups or groups[task["group"]] == task["phase"], "group crosses phases")
        groups[task["group"]] = task["phase"]
        for key in task["related_keys"]:
            require(key not in keys or keys[key] == task["group"], "related issues must share one group")
            keys[key] = task["group"]
        require(task["prompt"] in spec["inputs"] and task["controls"] in spec["inputs"],
                "task prompt and control report must be pinned")
    require(set(spec.get("targets", {})) == set(PHASES), "declare targets for every phase")
    require(set(spec.get("allocations_microusd", {})) == set(PHASES), "declare allocation for every phase")
    for phase in PHASES:
        for value in (spec["targets"][phase], spec["allocations_microusd"][phase]):
            require(value is None or type(value) is int and value > 0, "targets/allocations must be positive integers or null")
    cap = spec.get("session_cap_microusd")
    require(cap is None or type(cap) is int and cap > 0, "session cap must be positive integer or null")
    require(isinstance(spec.get("readiness"), dict), "declare readiness evidence")


def audit(spec, phase, *, input_paths=None):
    validate(spec)
    require(phase in PHASES, "unknown phase")
    blockers = []
    verified = set()
    input_paths = input_paths or {}
    for path, sha in spec["inputs"].items():
        resolved = input_paths.get(path, path)
        if not regular(resolved) or digest(resolved) != sha:
            blockers.append(f"missing, linked or changed input: {path}")
        else:
            verified.add(path)
    tasks = [task for task in spec["tasks"] if task["phase"] == phase]
    groups = {task["group"] for task in tasks}
    counts = {p: len({t["group"] for t in spec["tasks"] if t["phase"] == p}) for p in PHASES}
    target = spec["targets"][phase]
    if target is None:
        blockers.append(f"{phase} independent-group target is not registered")
    elif len(groups) != target:
        blockers.append(f"{phase} has {len(groups)} groups; fixed target is {target}")
    if phase != "train" and len(tasks) != len(groups):
        blockers.append("paired evaluation needs exactly one representative per independent group")
    exposures = set(spec["prior_exposure_keys"])
    for task in spec["tasks"]:
        if task["exposed"] or task["phase"] != "confirmation":
            exposures.update([task["group"], *task["related_keys"]])
    for task in tasks:
        label = task["id"]
        if task["provenance"] == "synthetic":
            blockers.append(f"{label}: synthetic tasks cannot substantiate this real-workload claim")
        if phase == "confirmation" and exposures.intersection([task["group"], *task["related_keys"]]):
            blockers.append(f"{label}: confirmation group was previously exposed")
        if task["controls"] not in verified:
            continue
        # Decode exactly the bytes whose hash is checked. Reopening a report
        # after a separate digest can otherwise audit different mutable bytes.
        report_bytes = Path(input_paths.get(task["controls"], task["controls"])).read_bytes()
        if hashlib.sha256(report_bytes).hexdigest() != spec["inputs"][task["controls"]]:
            blockers.append(f"{label}: control report changed while auditing")
            continue
        report = decoded(report_bytes)
        require(isinstance(report, dict), "control report must be an object")
        cases = report.get("cases", [])
        require(isinstance(cases, list), "control cases must be a list")
        if (report.get("quality_authority") != "external_parent" or report.get("kind") != "semantic"
                or report.get("task_id") != label or report.get("base_commit") != task["base_commit"]):
            blockers.append(f"{label}: semantic external-parent controls do not match the task")
        roles = {case.get("role") for case in cases if isinstance(case, dict)}
        if not CONTROL_ROLES <= roles:
            blockers.append(f"{label}: missing reference/base/partial/forged-report controls")
        if any(not isinstance(case, dict) or type(case.get("accepted")) is not bool
               or case["accepted"] != (case.get("role") == "reference") for case in cases):
            blockers.append(f"{label}: a control produced an unexpected grade")
        if report.get("remaining_omissions") != [] or not nonempty(report.get("coverage_reviewed_by")):
            blockers.append(f"{label}: semantic coverage review is incomplete")
        dependencies = report.get("inputs")
        if not isinstance(dependencies, list) or not dependencies or any(path not in verified for path in dependencies):
            blockers.append(f"{label}: source, grader, environment and observation closure is incomplete")
    required = ["source_closure", "runtime", "parent_grader", "provider_only_egress", "group_review"]
    if phase != "train":
        required += ["learned_lineage", "operator_review", "delivery_controls"]
    if phase == "confirmation":
        required += ["development_decision", "power_analysis", "multiplicity_policy"]
        if target is None or target < spec["policy"]["min_groups"]:
            blockers.append("confirmation needs at least 200 independent groups; power may require more")
    for gate in required:
        evidence = spec["readiness"].get(gate)
        if not isinstance(evidence, dict) or evidence.get("status") != "pass" or evidence.get("artifact") not in verified:
            blockers.append(f"readiness evidence missing: {gate}")
    if not nonempty(spec.get("model_revision")) or not nonempty(spec.get("harness_version")):
        blockers.append("exact model revision and harness version must be pinned")
    runs = len(tasks) * (1 if phase == "train" else 2)
    cap, allocation = spec["session_cap_microusd"], spec["allocations_microusd"][phase]
    reservation = runs * cap if cap else None
    if cap is None or allocation is None or not runs or allocation < reservation:
        blockers.append("fixed allocation cannot yet reserve every scheduled attempt")
    return {"schema": 1, "name": spec["name"], "phase": phase, "groups": counts, "scheduled_runs": runs,
            "reservation_microusd": reservation, "allocation_microusd": allocation,
            "ready_for_sealing": not blockers, "paid_dispatch_authorized": False,
            "customer_savings_claim": False, "billed_cost": None, "blockers": blockers}


def seal(spec, phase, out):
    """Audit frozen objects, then publish a no-clobber registration last."""
    validate(spec)
    require(phase in PHASES, "unknown phase")
    out = Path(out).absolute()
    require(not out.exists() and not out.is_symlink(), "seal destination already exists")
    require(not any(parent.is_symlink() for parent in out.parents),
            "seal destination must not have linked ancestors")
    require(not any((parent / ".git").exists() for parent in (out, *out.parents)), "keep private studies outside Git")
    out.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".adage-study-", dir=out.parent) as temp:
        stage = Path(temp) / "sealed"
        (stage / "objects").mkdir(parents=True, mode=0o700)
        os.chmod(stage, 0o700)
        frozen = {}
        for path, sha in spec["inputs"].items():
            require(regular(path), f"missing or linked input: {path}")
            target = stage / "objects" / sha
            if not target.exists():
                shutil.copyfile(path, target)
                os.chmod(target, 0o600)
            require(digest(target) == sha, "input changed while sealing")
            frozen[path] = target
        # Keep original manifest paths for provenance, but resolve every audit
        # read against the private copied bytes that will actually be sealed.
        result = audit(spec, phase, input_paths=frozen)
        require(result["ready_for_sealing"], "study is not ready for sealing: " + "; ".join(result["blockers"]))
        tool = HERE / "study.py"
        tool_sha = digest(tool)
        shutil.copyfile(tool, stage / "objects" / tool_sha)
        os.chmod(stage / "objects" / tool_sha, 0o600)
        require(digest(stage / "objects" / tool_sha) == tool_sha, "tool changed while sealing")
        body = {"schema": 1, "phase": phase, "spec": spec, "audit": result, "tool_sha256": tool_sha}
        registration = {"sha256": hashlib.sha256(encoded(body)).hexdigest(), "registration": body}
        (stage / "registration.json").write_bytes(encoded(registration))
        os.chmod(stage / "registration.json", 0o600)
        # mkdir is exclusive even if another process creates an empty directory
        # after the initial check. rename(stage, out) could replace that directory.
        os.mkdir(out, 0o700)
        os.rename(stage / "registration.json", out / "registration.pending.json")
        os.rename(stage / "objects", out / "objects")
        # An interruption preserves inspectable pending metadata/objects while
        # verify fails closed until this final atomic publication succeeds.
        os.rename(out / "registration.pending.json", out / "registration.json")
    return registration


def verify(root):
    root = Path(root).absolute()
    record = read(root / "registration.json")
    require(isinstance(record, dict) and isinstance(record.get("registration"), dict), "invalid registration")
    body = record["registration"]
    require(record["sha256"] == hashlib.sha256(encoded(body)).hexdigest(), "registration digest mismatch")
    validate(body["spec"])
    for sha in [*body["spec"]["inputs"].values(), body["tool_sha256"]]:
        path = root / "objects" / sha
        require(regular(path) and digest(path) == sha, "sealed input is missing or changed")
    return {"sha256": record["sha256"], "integrity_verified": True,
            "paid_dispatch_authorized": False, "customer_savings_claim": False}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    for command in ("audit", "seal"):
        entry = sub.add_parser(command)
        entry.add_argument("plan", type=Path)
        entry.add_argument("--phase", choices=PHASES, required=True)
        if command == "seal":
            entry.add_argument("--out", type=Path, required=True)
    sub.add_parser("verify").add_argument("root", type=Path)
    args = parser.parse_args()
    try:
        if args.command == "verify":
            result = verify(args.root)
        else:
            spec = read(args.plan)
            result = audit(spec, args.phase) if args.command == "audit" else seal(spec, args.phase, args.out)
        print(encoded(result).decode(), end="")
    except (ValueError, KeyError, TypeError, OSError) as exc:
        parser.exit(2, f"study: {exc}\n")


if __name__ == "__main__":
    main()
