#!/usr/bin/env python3
"""Four-arm curve evaluation. Preparation is offline; only explicit `live` spends.

No command creates grants. Uncollected assignments and unknown shell behavior remain unknown.
Literal command matches are descriptive; this harness does not invent semantic-equivalence proof.
"""
from __future__ import annotations

import argparse
import copy
import hashlib
import json
from pathlib import Path
import re
import shlex
import shutil
import subprocess
import sys
import time

import cohort_v5
import curve_sessions
import learning_curve as curve
import real_repo_pairs as pairs

gate = pairs.substrate
ARMS = curve.ARMS


def verify_curve(root):
    protocol = pairs.read(root / "manifest.json").get("protocol")
    if protocol == "learning-curve-guided-transfer-1":
        import curve_transfer
        return curve_transfer.verify(root)
    if protocol == "learning-curve-cold-1":
        import curve_cold
        return curve_cold.verify(root)
    with curve_sessions.registry(root):
        return curve.verify(root)


def artifact_files(home, names):
    return {name: pairs.sha(pairs.safe_file(home, name)) for name in names}


def prepare(root, task, descriptor):
    root, descriptor = root.resolve(strict=True), descriptor.resolve(strict=True)
    base, _ = verify_curve(root)
    validation = pairs.read(root / "validation.json")
    if not validation.get("complete") or not validation.get("valid"):
        raise ValueError("complete base/gold validation required")
    entries = [r for r in base["matrix"] if r["task"] == task]
    cold_study = base["protocol"] == "learning-curve-cold-1"
    guided_transfer = base["protocol"] == "learning-curve-guided-transfer-1"
    expected_arms = {"agent", "instincts"} if cold_study else {"learned", "automatic"} if guided_transfer else set(ARMS)
    if len(entries) != len(expected_arms) or {r["arm"] for r in entries} != expected_arms:
        raise ValueError("complete registered task arms required")
    plan = base["families"][0]
    artifact = pairs.read(descriptor)
    if cold_study:
        if pairs.sha(descriptor) != base["descriptor_sha256"] or artifact.get("checkpoint") != 0:
            raise ValueError("registered checkpoint-zero descriptor required")
    elif (artifact.get("checkpoint") != (10 if guided_transfer else 15) or artifact.get("family") != plan["family"]
            or artifact.get("training_tasks") != plan["train"] or artifact.get("action_kind") != "verify"):
        raise ValueError("natural checkpoint-15 verification descriptor required")
    # Guided transfer's verifier binds all ten native histories across its two source cohorts.
    for task_id in ([] if cold_study or guided_transfer else plan["train"]):
        session = root / "sessions" / task_id
        manifest = pairs.read(session / "manifest.json")
        provenance = pairs.read(session / "transcript-provenance.json")
        if (manifest.get("kind") != "organic-training" or manifest.get("training_hooks_enabled") is not False
                or provenance.get("synthetic") is not False
                or pairs.sha(session / "native-transcript.jsonl") != provenance["sha256"]):
            raise ValueError("native unexposed training provenance required")
    home = Path(artifact["learned_home"]).resolve(strict=True)
    files = {} if cold_study else artifact_files(home, artifact["files"])
    cold = Path(artifact["checkpoint0_home"]).resolve(strict=True)
    cold_files = artifact_files(cold, artifact["checkpoint0_files"])
    candidates = [] if cold_study else [r for r in pairs.read(home / "index.json")["routes"]
                  if r["track"] + "/" + r["route"]["id"] == artifact["route"]]
    if not cold_study and (len(candidates) != 1 or not candidates[0]["track"].endswith("/learned")):
        raise ValueError("candidate missing from naturally learned index")
    if any(r["track"].endswith("/learned") for r in pairs.read(cold / "index.json")["routes"]):
        raise ValueError("checkpoint zero contains learned knowledge")
    if not artifact.get("condition_command") or not Path(artifact["condition_output"]).is_file():
        raise ValueError("production conditioning output required")
    with curve_sessions.registry(root):
        pairs.prepare_hidden_expectations(root, task)
    hidden = root / "private/hidden-identities" / (task + ".json")
    native_cohort = Path(base.get("native_cohort_root", str(root)))
    review = native_cohort / "native-confinement-reviewed.json"
    authority = Path(base["budget_authority"])
    gate.require_native_review(dict(experiment_root=str(authority), cohort_root=str(native_cohort),
        native_review_path=str(review), native_review_sha256=pairs.sha(review)))
    folder = root / "heldout" / task
    folder.mkdir(parents=True, exist_ok=False)
    binaries = {}
    for name, source in artifact["binaries"].items():
        target = folder / "bin" / name
        target.parent.mkdir(exist_ok=True)
        shutil.copy2(source, target)
        binaries[name] = str(target)
    assignments = []
    for entry in entries:
        arm = entry["arm"]
        session = folder / arm
        session.mkdir()
        checkout, local_home = session / "repo", session / "adage-home"
        gate.create_checkout(root, plan["repo"], plan["base_full"], checkout)
        pairs.frozen_setup(root, checkout, plan, base["python_requested"])
        local_home.mkdir()
        selected = files if arm in ("learned", "automatic") else cold_files if arm == "instincts" else {}
        source_home = home if arm in ("learned", "automatic") else cold
        for name in selected:
            target = local_home / name
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(source_home / name, target)
        settings = cohort_v5.policy(authority, checkout, root / "runtime/python")
        # The reference receives no Adage hook at all. Other non-execution arms use adage,
        # whose production runtime suppresses execution-only candidates without advice fallback.
        normalized_policy = json.dumps(settings, sort_keys=True).replace(str(session), "<assignment>")
        if arm != "agent":
            settings["disableAllHooks"] = False
            settings["hooks"] = {event: [{"hooks": [{"type": "command", "command": shlex.join([
                base["preparation_python"], "-B", str(Path(__file__)), "hook", str(session / "manifest.json")]),
                "timeout": 90}]}] for event in pairs.EVENTS}
        pairs.write(session / "settings.json", settings)
        (session / ".reflex-fixture").write_text("learning curve held-out task\n")
        argv = ["claude", "--print", "--restricted", "--model", artifact["model"], "--max-budget-usd", "1.5",
            "--max-turns", "60", "--output-format", "stream-json", "--verbose", "--include-hook-events",
            "--include-partial-messages", "--debug-file", str(session / "claude-debug.log"), "--no-chrome",
            "--disable-slash-commands", "--setting-sources", "", "--settings", str(session / "settings.json"),
            "--strict-mcp-config", "--mcp-config", '{"mcpServers":{}}', "--tools", "Read,Edit,Write,Glob,Grep,Bash",
            "--permission-mode", "dontAsk", "--permission-prompts", "none", entry["prompt"]]
        frozen = {str(local_home / name): digest for name, digest in selected.items()}
        frozen.update({str(p): pairs.sha(p) for p in [descriptor, Path(artifact["condition_output"]), hidden,
            review, root / "manifest.json", root / "validation.json", session / "settings.json",
            *map(Path, binaries.values())]})
        frozen.update(base.get("frozen_inputs", {}))
        initial = pairs.tree_state(checkout)
        m = dict(protocol=base["protocol"], protocol_revision=5, root=str(session), cohort_root=str(native_cohort),
            experiment_root=str(authority), execution_root=str(checkout), home=str(local_home), arm=arm,
            task=task, order=entry["order"], route=artifact["route"], model=artifact["model"],
            argv=cohort_v5.startup_argv(argv), binaries=binaries, learned_files=selected,
            sources={**curve.sources(), str(Path(__file__)): pairs.sha(Path(__file__)),
                     str(Path(curve_sessions.__file__)): pairs.sha(Path(curve_sessions.__file__)),
                     str(Path(cohort_v5.__file__)): pairs.sha(Path(cohort_v5.__file__)), **artifact.get("source_sha256", {})},
            frozen=frozen, initial_source=initial, budget_usd=1.5, expected_claude_version=pairs.probe.EXPECTED_VERSION,
            budget_allocation_sha256=base["budget_allocation_sha256"], native_review_path=str(review),
            native_review_sha256=pairs.sha(review), hidden_identities=str(hidden),
            matching_command_template="" if cold_study else candidates[0]["route"]["step"]["command"],
            training_tasks=[] if cold_study else plan["train"],
            comparison_identity=dict(model=artifact["model"], base_commit=plan["base_full"],
                binary_hashes={name: pairs.sha(Path(p)) for name, p in binaries.items()},
                initial_source_digest=initial["digest"], policy_sha256=hashlib.sha256(normalized_policy.encode()).hexdigest()))
        if cold_study or guided_transfer:
            m["study_root"] = str(root)
        gate.validate_authority(m)
        gate.budget_limit(m)
        pairs.write(session / "manifest.json", m)
        (session / "manifest.sha256").write_text(pairs.sha(session / "manifest.json") + "\n")
        assignments.append(str(session / "manifest.json"))
    if any(pairs.read(p)["comparison_identity"] != pairs.read(assignments[0])["comparison_identity"] for p in assignments):
        raise ValueError("four-arm source/model/runtime/policy mismatch")
    result = dict(task=task, assignments=assignments, paid_calls=0, grants_written=False)
    pairs.write(folder / "assignments.json", result)
    return result


def hook(manifest, payload):
    if pairs.read(manifest)["arm"] == "agent":
        raise ValueError("reference arm must never invoke Adage")
    return pairs.hook(manifest, payload)


def read_jsonl(path):
    return [json.loads(line) for line in path.read_text().splitlines() if line.strip()]


def collect(manifest):
    m = pairs.read(manifest)
    pairs.verify(m)
    session, root, checkout = Path(m["root"]), Path(m.get("study_root", m["cohort_root"])), Path(m["execution_root"])
    if (session / "collection.json").exists():
        raise ValueError("collection is immutable")
    verify_curve(root)  # Registers the dedicated task family for the shared scorer.
    errors = []
    final = pairs.tree_state(checkout)
    pairs.write(session / "final-source.json", final)
    (session / "final-tracked.patch").write_bytes(subprocess.run(["git", "diff", "--binary", "HEAD"],
        cwd=checkout, capture_output=True, check=True).stdout)
    def attempt(label, call, fallback):
        try:
            return call()
        except Exception as error:
            errors.append(dict(stage=label, error=str(error)))
            return fallback
    provenance = attempt("native-transcript", lambda: gate.retain_transcript(session, checkout), {})
    stream = attempt("stream", lambda: read_jsonl(session / "stdout.jsonl"), [])
    hooks = [] if m["arm"] == "agent" else attempt("hooks", lambda: read_jsonl(session / "production-hooks.jsonl"), [])
    metrics = pairs.stream_metrics(stream)
    matching = [a for a in metrics["agent_actions"] if a.get("name") == "Bash"
        and pairs.command_matches(a.get("input", {}).get("command", ""), m["matching_command_template"])]
    successful = [a for a in matching if a["id"] in metrics["tool_results"]
        and not metrics["tool_results"][a["id"]].get("is_error", False)]
    def score():
        plan = pairs.read(root / "manifest.json")["families"][0]
        scoring = session / "private-score"
        if any("symlink" in f for f in final["files"].values()):
            raise ValueError("source symlink requires confinement review")
        gate.create_checkout(root, plan["repo"], plan["base_full"], scoring)
        pairs.frozen_setup(root, scoring, plan, pairs.read(root / "manifest.json")["python_requested"])
        for name in set(pairs.tree_state(scoring)["files"]) - set(final["files"]):
            (scoring / name).unlink()
        for name in final["files"]:
            target = scoring / name
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(checkout / name, target)
        if pairs.tree_state(scoring) != final:
            raise ValueError("scorer source generation mismatch")
        with curve_sessions.registry(root):
            return pairs.score_with_integrity(root, m["task"], scoring, pairs.read(m["hidden_identities"]))
    quality = attempt("independent-quality", score, dict(passed=False, status="unknown"))
    summary = attempt("summary", lambda: pairs.probe.summary(session), {})
    pattern = re.compile(r"^\S+ \[DEBUG\] \[API REQUEST\] /v1/messages x-client-request-id=([\w-]+) source=(\w+)$")
    def dispatches():
        return [dict(request_id=v[1], source=v[2]) for line in (session / "claude-debug.log").read_text().splitlines()
                if (v := pattern.fullmatch(line))]
    records = []
    receipts = []
    if m["arm"] != "agent":
        receipts = attempt("journal", lambda: pairs.journal(m), [])
        def spool():
            path = Path(m["home"]) / "sessions" / (provenance["session_id"] + ".spool")
            shutil.copyfile(path, session / "production-session.spool")
            return pairs.spool_records(path)
        records = attempt("runtime-spool", spool, [])
    result = dict(protocol=m["protocol"], task=m["task"], arm=m["arm"], order=m["order"], provenance=provenance,
        quality=quality, summary=summary, process=attempt("process", lambda: pairs.read(session / "run.json"), {}),
        **metrics, matching_agent_actions=matching, successful_matching_agent_actions=successful,
        final_generation_receipts=pairs.final_receipts(hooks, m["route"], provenance.get("session_id"), checkout, final["digest"]),
        bash_action_audit=pairs.bash_audit.audit_bash_actions(metrics["agent_actions"]),
        all_journal_receipts=receipts, hooks=hooks, final_generation=final["digest"], runtime_records=records,
        execution_attempts=[e for r in records for e in r.get("exec", [])],
        observed_dispatches=attempt("dispatches", dispatches, []), cost_usd=summary.get("total_cost_usd"),
        learned_files=m["learned_files"], comparison_identity=m["comparison_identity"], prompt=m["argv"][-1],
        template=m["matching_command_template"], route=m["route"], collection_errors=errors,
        semantic_equivalence_status="only identical fixed project-Python pytest commands with literal test scope can establish a conservative match")
    pairs.write(session / "collection.json", result)
    return result


def task_result(rows, expected_arms=ARMS):
    by_arm = {r["arm"]: r for r in rows}
    if tuple(expected_arms) not in (tuple(ARMS), ("learned", "automatic")):
        raise ValueError("unsupported comparison arms")
    if len(rows) != len(expected_arms) or set(by_arm) != set(expected_arms):
        return dict(passed=False, confirmed_elimination=False, reasons=["complete unique four-arm task required"])
    learned, automatic = by_arm["learned"], by_arm["automatic"]
    pair_rows = [copy.deepcopy(learned), copy.deepcopy(automatic)]
    pair_rows[0]["arm"] = "suggest"
    comparison = pairs.pair_result(pair_rows)
    reasons = list(comparison["reasons"])
    for row in rows:
        if row.get("collection_errors") or row.get("status") == "uncollected":
            reasons.append(row["arm"] + ": incomplete evidence")
        if row.get("comparison_identity") != learned.get("comparison_identity") or row.get("prompt") != learned.get("prompt"):
            reasons.append(row["arm"] + ": four-arm comparison mismatch")
        if row["arm"] != "automatic" and (row.get("all_journal_receipts") or row.get("execution_attempts")):
            reasons.append(row["arm"] + ": unexpected CPU execution")
    if not approved_execution_evidence(automatic):
        reasons.append("automatic: execution approval evidence incomplete or foreign")
    scope = fixed_pytest_evidence(learned)
    if not scope:
        reasons.append("identical fixed pytest command on baseline final source generation unverified")
    confirmed = bool(comparison["verification_action_suppressed"] and scope and not reasons)
    failed_only = learned.get("quality", {}).get("passed") is True and automatic.get("quality", {}).get("passed") is False
    return dict(passed=confirmed, confirmed_elimination=confirmed, reasons=reasons, literal_pair_evidence=comparison,
        automatic_only_quality_failure=failed_only,
        quality_by_arm={arm: by_arm[arm].get("quality", {}) for arm in expected_arms},
        contrasts={label: contrast(by_arm[left], by_arm[right]) for label, left, right in (
            ("instincts-agent", "agent", "instincts"), ("learned-instincts", "instincts", "learned"),
            ("automatic-learned", "learned", "automatic"), ("automatic-agent", "agent", "automatic"))
            if left in by_arm and right in by_arm})


def fixed_pytest_evidence(row):
    """A deliberately narrow sufficient match; aliases and selectors remain unconfirmed.

    A concrete command from the actual learned route, identical in both arms, names the same
    frozen project Python, literal repository-relative targets, and non-selection flags.
    Require its successful agent execution on the baseline's final generation. Independent
    whole-suite hidden scoring verifies task quality separately. This is a subset of equivalent
    operations: every automatic-arm Bash still goes through the strict unknown audit.
    """
    command = row.get("template", "")
    if not isinstance(command, str) or pairs.bash_audit._shell_syntax(command):
        return False
    try:
        argv = shlex.split(command)
    except ValueError:
        return False
    if argv[:3] != [".venv/bin/python", "-m", "pytest"]:
        return False
    index = 3
    while index < len(argv):
        arg = argv[index]
        index += 1
        if arg in ("-q", "-v", "-vv", "-s", "-x", "--disable-warnings"):
            continue
        if arg == "-p":
            if index == len(argv) or argv[index] != "no:cacheprovider":
                return False
            index += 1
            continue
        if (arg.startswith(("-", "/")) or not re.fullmatch(r"[A-Za-z0-9_./-]+", arg)
                or ".." in arg.split("/")):
            return False
    final = row.get("final_generation")
    session_id = row.get("provenance", {}).get("session_id")
    if not final or not session_id:
        return False
    for action in row.get("successful_matching_agent_actions", []):
        if not pairs.command_matches(action.get("input", {}).get("command", ""), row["template"]):
            continue
        for hook_row in row.get("hooks", []):
            payload = hook_row.get("payload", {})
            if (hook_row.get("event") == "PostToolUse" and hook_row.get("exit_code") == 0
                    and payload.get("tool_use_id") == action.get("id") and payload.get("session_id") == session_id
                    and hook_row.get("before", {}).get("digest") == final and hook_row.get("after", {}).get("digest") == final):
                return True
    return False


def approved_execution_evidence(row):
    """Inspect all receipts, including executions that did not establish an elimination."""
    for receipt in row.get("all_journal_receipts", []):
        if (receipt.get("phase") != "completed" or not receipt.get("approved_on")
                or receipt.get("session") != row.get("provenance", {}).get("session_id")
                or receipt.get("route") != row.get("route")
                or receipt.get("outcome", {}).get("outcome") == "unknown"):
            return False
    return True


def contrast(before, after):
    def delta(key):
        a, b = before.get(key), after.get(key)
        return a - b if isinstance(a, (int, float)) and isinstance(b, (int, float)) else None
    costs = [pairs.known_cost(row.get("cost_usd")) for row in (before, after)]
    return dict(cost_usd_saved=costs[0] - costs[1] if all(c is not None for c in costs) else None,
        tool_calls_saved=delta("agent_action_count"),
        observed_model_requests_saved=(len(before["observed_dispatches"]) - len(after["observed_dispatches"]))
            if before.get("observed_dispatches") and after.get("observed_dispatches") else None)


def report(root):
    base, _ = verify_curve(root)
    if base["protocol"] == "learning-curve-cold-1":
        import curve_cold
        return curve_cold.report(root)
    ledger_path = Path(base["budget_authority"]) / "paid-state.json"
    ledger = pairs.read(ledger_path) if ledger_path.exists() else {"runs": {}}
    rows = []
    for entry in base["matrix"]:
        path = root / "heldout" / entry["task"] / entry["arm"] / "collection.json"
        key = str(path.parent.relative_to(Path(base["budget_authority"])))
        attempt = ledger["runs"].get(key, {})
        row = pairs.read(path) if path.exists() else dict(task=entry["task"], arm=entry["arm"],
            order=entry["order"], status="uncollected", cost_usd=attempt.get("summary", {}).get("total_cost_usd"))
        row["ledger_attempt"] = attempt
        rows.append(row)
    expected_arms = ("learned", "automatic") if base["protocol"] == "learning-curve-guided-transfer-1" else ARMS
    tasks = {task: task_result([r for r in rows if r["task"] == task], expected_arms)
             for task in base["families"][0]["heldout"]}
    costs = [pairs.known_cost(r.get("cost_usd")) for r in rows]
    confirmed = sum(t.get("confirmed_elimination", False) for t in tasks.values())
    complete = all(r.get("status") != "uncollected" and not r.get("collection_errors") for r in rows)
    quality_safe = all(r.get("quality", {}).get("passed") is True for r in rows if r["arm"] in ("learned", "automatic"))
    execution_safe = all(approved_execution_evidence(r) if r["arm"] == "automatic"
        else not (r.get("all_journal_receipts") or r.get("execution_attempts")) for r in rows)
    uncertain_shell = any(a["classification"] == pairs.bash_audit.UNKNOWN for r in rows if r["arm"] == "automatic"
        for a in pairs.bash_audit.audit_bash_actions(r.get("agent_actions", []))["actions"])
    result = dict(protocol=base["protocol"], passed=confirmed >= 3 and complete and quality_safe and execution_safe
        and not uncertain_shell and all(c is not None for c in costs),
        assignment_denominator=len(base["matrix"]),
        collected=sum(r.get("status") != "uncollected" for r in rows), rows=rows, tasks=tasks,
        known_cost_usd=sum(c for c in costs if c is not None), unknown_cost_assignments=sum(c is None for c in costs),
        confirmed_eliminations=confirmed, automatic_only_quality_failures=sum(t.get("automatic_only_quality_failure", False) for t in tasks.values()),
        limitations=["Confirmed lower bounds admit only identical fixed project-Python pytest commands with literal test targets and allowlisted flags; other semantic equivalence remains unverified.",
            f"All {len(base['matrix'])} assignments, including failures, censored and missing runs, remain in the denominator.",
            "API requests and tokens do not measure GPU utilization or energy; four tasks are a feasibility screen."])
    output = root / "reports" / ("heldout-" + str(time.time_ns()) + ".json")
    pairs.write(output, result)
    return result


def live(manifest, wall_seconds=300):
    m = pairs.read(manifest)
    pairs.verify(m)
    root, session = Path(m["experiment_root"]), Path(m["root"])
    if pairs.tree_state(Path(m["execution_root"])) != m["initial_source"]:
        raise ValueError("prepared checkout changed before dispatch")
    gate.validate_authority(m)
    gate.require_native_review(m)
    if m["arm"] == "automatic" and not (Path(m["home"]) / "approvals.json").is_file():
        raise ValueError("human must grant exact automatic candidate")
    with pairs.execution_lock(root):
        state_path = root / "paid-state.json"
        state = pairs.read(state_path) if state_path.exists() else {"runs": {}}
        budget = pairs.Budget(state, gate.budget_limit(m), 1.5, persist=lambda: pairs.write(state_path, state))
        key = str(session.relative_to(root))
        if key in state["runs"]:
            raise ValueError("assignment already attempted; no outcome retries")
        study_root = Path(m.get("study_root", m["cohort_root"]))
        control, _ = verify_curve(study_root)
        assigned = [r for r in control["matrix"] if r["task"] == m["task"] and r["arm"] == m["arm"]]
        if len(assigned) != 1 or assigned[0]["order"] != m["order"]:
            raise ValueError("assignment does not match frozen matrix")
        for earlier in control["matrix"]:
            if earlier["order"] < m["order"]:
                prior = study_root / "heldout" / earlier["task"] / earlier["arm"]
                if str(prior.relative_to(root)) not in state["runs"]:
                    raise ValueError("prospectively fixed assignment order must be retained")
        attempt, reason = budget.reserve(key)
        if attempt is None:
            raise ValueError(reason)
        state["runs"][key] = dict(status="reserved", task=m["task"], arm=m["arm"], protocol=m["protocol"])
        pairs.write(state_path, state)
        result = None
        try:
            if not budget.dispatch(attempt):
                raise ValueError("accounting blocked dispatch")
            result = pairs.probe.execute(manifest, 1.5, wall_seconds)
            budget.settle(attempt, result)
            state["runs"][key].update(status="complete", summary=result)
            state["runs"][key]["quality"] = collect(manifest)["quality"]
        except BaseException as error:
            state["runs"][key].update(status="failed", error=str(error))
            raise
        finally:
            budget.settle(attempt, result)
            pairs.write(state_path, state)
            pairs.append(root / "attempts.jsonl", dict(status="curve-heldout-finished", run=key,
                accounting=attempt, evidence=state["runs"][key], timestamp_ns=time.time_ns()))
    return state["runs"][key]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("prepare", "hook", "collect", "live", "report"))
    parser.add_argument("path", type=Path)
    parser.add_argument("--task")
    parser.add_argument("--descriptor", type=Path)
    args = parser.parse_args()
    if args.command == "hook":
        print(hook(args.path, json.load(sys.stdin)), end="")
        return
    if args.command == "prepare":
        if not args.task or not args.descriptor:
            parser.error("prepare needs --task and --descriptor")
        result = prepare(args.path, args.task, args.descriptor)
    else:
        result = globals()[args.command](args.path)
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
