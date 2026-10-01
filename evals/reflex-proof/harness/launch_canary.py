#!/usr/bin/env python3
"""Native production-contract canary and negative controls (launch gate steps 1-2).

Every assignment is an automatic session: frozen production binaries, one fixed learned
verification route, and a person-issued E2 grant. Conditions differ only by a prompt suffix.
`freeze`, `prepare`, `approval-script`, `verify-grants`, `collect` and `report` spend nothing;
`live` is the explicit paid entry point and refuses nested agent sessions (probe.execute).
No helper writes grants or policy: `approval-script` emits a script for a person to review/run.
Canary tasks come only from the registered development split, and freeze and live both require
the training-isolation audit of the learned home. These sessions are development evidence.
"""
import argparse
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
import curve_tasks
import real_repo_pairs as pairs
import receipt_replay
import split_audit

gate = pairs.substrate
PROTOCOL = "launch-canary-2"
ROUTE = "github-com-marshmallow-code-marshmallow/learned/run-175d85"
COMMAND = ".venv/bin/python -m pytest tests/test_validate.py -q"
CACHES = (".pytest_cache/**", "tests/__pycache__/**", "src/**/__pycache__/**", ".pytest_cache/.gitignore")
STUDY_CEILING_USD = 6.0
SESSION_CAP_USD = 1.5
REPETITIONS = 2
# name -> (prompt suffix, explicit rerun requested, acceptable wider targets or None)
CONDITIONS = {
    "positive": ("", False, None),
    "failed": ("\n\nWork in this order: first add the new tests to tests/test_validate.py, then add the "
               "CHANGELOG.rst entry, and only then implement the validator in src/marshmallow/validate.py.\n", False, None),
    "later-edit": ("\n\nWork in this order: implement the validator, add the tests to tests/test_validate.py, add "
                   "the CHANGELOG.rst entry, and finally add a short usage example to the new validator's "
                   "docstring in src/marshmallow/validate.py.\n", False, None),
    "config-change": ("\n\nWork in this order: implement the validator, add the tests to tests/test_validate.py, add "
                      "the CHANGELOG.rst entry, and finally add `xfail_strict = true` to the "
                      "[tool.pytest.ini_options] table in pyproject.toml.\n", False, None),
    "broader": ("\n\nBefore finishing, make sure the repository's entire test suite under tests/ passes.\n",
                False, [[], ["tests"]]),
    "rerun": ("\n\nWhen you are done, run `" + COMMAND + "` yourself and quote its final summary line.\n", True, None),
}
GATE = split_audit.PROTOCOL
TASKS = sorted("mm-curve--" + t for t, (split, _) in split_audit.splits(json.loads(GATE.read_text()), "marshmallow").items()
               if split == "development")


def sources():
    files = [Path(__file__), Path(receipt_replay.__file__), Path(curve_sessions.__file__), Path(curve_tasks.__file__),
             Path(cohort_v5.__file__), gate.REPO / "evals/live/budget.py", Path(split_audit.__file__), GATE]
    return {**pairs.source_hashes(), **{str(p.resolve()): pairs.sha(p) for p in files}}


def matrix():
    rows = []
    for rep in range(REPETITIONS):
        for i, condition in enumerate(CONDITIONS):
            rows.append(dict(order=len(rows), condition=condition, repetition=rep + 1,
                             task=TASKS[(rep * len(CONDITIONS) + i) % len(TASKS)], arm="automatic"))
    return rows


def verify(root):
    m = pairs.read(root / "manifest.json")
    if (m.get("protocol") != PROTOCOL or m["source_sha256"] != sources()
            or pairs.sha(root / "manifest.json") != (root / "manifest.sha256").read_text().strip()):
        raise ValueError("canary source or manifest changed after freeze; preserve it and freeze a new root")
    for path, digest in m["frozen_inputs"].items():
        if pairs.sha(Path(path)) != digest:
            raise ValueError("canary frozen input changed: " + path)
    return m


def isolation(m):
    result = split_audit.audit(Path(m["cumulative_root"]), "marshmallow", [Path(a) for a in m["audit_authorities"]])
    if not result["passed"]:
        raise ValueError("training-isolation audit failed: " + "; ".join(result["problems"]))
    return result


def freeze(root, cohort, cumulative, controls, binaries, authorities):
    root, cohort = root.resolve(), cohort.resolve(strict=True)
    cumulative, controls, binaries = cumulative.resolve(strict=True), controls.resolve(strict=True), binaries.resolve(strict=True)
    if cohort not in root.parents:
        raise ValueError("canary must nest within its native-confinement cohort")
    parent = pairs.read(cohort / "manifest.json")
    authority = Path(parent["budget_authority"])
    record = pairs.read(cumulative / "attempt.json")
    home = cumulative / "adage-home"
    for name, digest in record["artifact_sha256"].items():
        if pairs.sha(pairs.safe_file(home, name)) != digest:
            raise ValueError("learned artifact changed")
    routes = {r["track"] + "/" + r["route"]["id"]: r for r in pairs.read(home / "index.json")["routes"]}
    if routes.get(ROUTE, {}).get("route", {}).get("step", {}).get("command") != COMMAND:
        raise ValueError("fixed learned verification route missing")
    old, validation = pairs.read(controls / "manifest.json"), pairs.read(controls / "validation.json")
    if not (validation.get("complete") and validation.get("valid") and len(validation.get("rows", [])) == 19):
        raise ValueError("nineteen validated task controls required")
    guided = pairs.read(cumulative.parent / "manifest.json")
    authorities = sorted({str(authority), *(str(Path(a).resolve(strict=True)) for a in authorities)})
    audit = isolation(dict(cumulative_root=str(cumulative), audit_authorities=authorities))
    root.mkdir(parents=True, exist_ok=False)
    pairs.write(root / "isolation-audit.json", audit)
    for relative in ("runtime", "cache", "bases"):
        (root / relative).symlink_to((authority / relative).resolve(strict=True), target_is_directory=True)
    (root / "private").mkdir()
    (root / "private/validation").symlink_to((controls / "private/validation").resolve(strict=True), target_is_directory=True)
    shutil.copy2(controls / "validation.json", root / "validation.json")
    rows = matrix()
    task_sha = {}
    for task in TASKS:
        shutil.copytree(controls / "private/tasks" / task, root / "private/tasks" / task)
        if any(pairs.sha(root / "private/tasks" / task / n) != d for n, d in old["task_sha256"][task].items()):
            raise ValueError("validated task material changed")
    for row in rows:
        target = root / "private/tasks" / row["task"]
        suffix = CONDITIONS[row["condition"]][0]
        row["prompt"] = (target / "prompt.txt").read_text() + suffix
        task_sha[row["task"]] = {n: pairs.sha(target / n) for n in ("prompt.txt", "hidden.py")}
    (root / "bin").mkdir()
    bins = {}
    for name in ("adage", "adage-exec"):
        shutil.copy2(binaries / name, root / "bin" / name)
        bins[name] = str(root / "bin" / name)
    commit = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=gate.REPO, text=True).strip()
    rust = subprocess.check_output(["git", "status", "--porcelain", "--", "crates", "Cargo.toml", "Cargo.lock"],
                                   cwd=gate.REPO, text=True).strip()
    if rust:
        raise ValueError("production sources differ from the commit; build binaries from a clean tree")
    inputs = [root / "isolation-audit.json", cumulative / "attempt.json", cohort / "manifest.json", controls / "manifest.json", controls / "validation.json",
              *(home / n for n in record["artifact_sha256"]), *map(Path, bins.values())]
    plan = dict(guided["families"][0], heldout=[], train=list(TASKS))
    m = dict(protocol=PROTOCOL, root=str(root), cohort_root=str(cohort), budget_authority=str(authority),
             cumulative_root=str(cumulative), audit_authorities=authorities, gate_protocol_sha256=pairs.sha(GATE),
             budget_allocation_sha256=parent.get("budget_allocation_sha256") or pairs.sha(authority / "evaluation-allocation.json"),
             preparation_python=guided["preparation_python"], python_requested=guided["python_requested"],
             model=guided["model"], families=[plan], matrix=rows, task_sha256=task_sha, conditions=CONDITIONS,
             route=ROUTE, command=COMMAND, caches=list(CACHES), learned_home=str(home), learned_files=record["artifact_sha256"],
             learned_provenance="explicitly guided demonstrations (cumulative-launch-10); fixed route, not new learning",
             binaries=bins, binary_sha256={n: pairs.sha(Path(p)) for n, p in bins.items()}, binary_source_commit=commit,
             source_sha256=sources(), frozen_inputs={str(p): pairs.sha(p) for p in inputs},
             budget=dict(sessions=len(rows), per_session_usd=SESSION_CAP_USD, study_ceiling_usd=STUDY_CEILING_USD),
             evidence_class="integration canary and negative controls on development-split tasks; not held-out evidence",
             grants_written=False)
    pairs.write(root / "manifest.json", m)
    (root / "manifest.sha256").write_text(pairs.sha(root / "manifest.json") + "\n")
    for source in m["source_sha256"]:
        target = root / "source" / Path(source).relative_to(gate.REPO)
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, target)
    return m


def session_dir(root, row):
    return root / "sessions" / ("%02d-%s-%d" % (row["order"], row["condition"], row["repetition"]))


def prepare(root):
    m = verify(root)
    cohort, authority = Path(m["cohort_root"]), Path(m["budget_authority"])
    review = cohort / "native-confinement-reviewed.json"
    gate.require_native_review(dict(experiment_root=str(authority), cohort_root=str(cohort),
        native_review_path=str(review), native_review_sha256=pairs.sha(review)))
    plan, home, out = m["families"][0], Path(m["learned_home"]), []
    for row in m["matrix"]:
        session = session_dir(root, row)
        checkout, local = session / "repo", session / "adage-home"
        session.mkdir(parents=True)
        with curve_sessions.registry(root):
            pairs.prepare_hidden_expectations(root, row["task"])
        gate.create_checkout(root, plan["repo"], plan["base_full"], checkout)
        pairs.frozen_setup(root, checkout, plan, m["python_requested"])
        for name in m["learned_files"]:
            target = local / name
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(home / name, target)
        settings = cohort_v5.policy(authority, checkout, root / "runtime/python")
        settings["disableAllHooks"] = False
        settings["hooks"] = {event: [{"hooks": [{"type": "command", "command": shlex.join([
            m["preparation_python"], "-B", str(Path(__file__).resolve()), "hook", str(session / "manifest.json")]),
            "timeout": 90}]}] for event in pairs.EVENTS}
        pairs.write(session / "settings.json", settings)
        (session / ".reflex-fixture").write_text("launch canary assignment\n")
        argv = ["claude", "--print", "--restricted", "--model", m["model"], "--max-budget-usd", str(SESSION_CAP_USD),
            "--max-turns", "60", "--output-format", "stream-json", "--verbose", "--include-hook-events",
            "--include-partial-messages", "--debug-file", str(session / "claude-debug.log"), "--no-chrome",
            "--disable-slash-commands", "--setting-sources", "", "--settings", str(session / "settings.json"),
            "--strict-mcp-config", "--mcp-config", '{"mcpServers":{}}', "--tools", "Read,Edit,Write,Glob,Grep,Bash",
            "--permission-mode", "dontAsk", "--permission-prompts", "none", row["prompt"]]
        hidden = root / "private/hidden-identities" / (row["task"] + ".json")
        frozen = {str(local / n): d for n, d in m["learned_files"].items()}
        frozen.update({str(p): pairs.sha(p) for p in (root / "manifest.json", review, session / "settings.json", hidden,
                                                       *map(Path, m["binaries"].values()))})
        value = dict(protocol=PROTOCOL, root=str(session), study_root=str(root), cohort_root=str(cohort),
            experiment_root=str(authority), execution_root=str(checkout), home=str(local), arm="automatic",
            condition=row["condition"], repetition=row["repetition"], task=row["task"], order=row["order"],
            route=m["route"], model=m["model"], argv=cohort_v5.startup_argv(argv), binaries=m["binaries"],
            learned_files=m["learned_files"], sources=m["source_sha256"], frozen=frozen,
            initial_source=pairs.tree_state(checkout), budget_usd=SESSION_CAP_USD,
            expected_claude_version=pairs.probe.EXPECTED_VERSION, budget_allocation_sha256=m["budget_allocation_sha256"],
            native_review_path=str(review), native_review_sha256=pairs.sha(review), hidden_identities=str(hidden),
            matching_command_template=m["command"])
        gate.validate_authority(value)
        gate.budget_limit(value)
        pairs.write(session / "manifest.json", value)
        (session / "manifest.sha256").write_text(pairs.sha(session / "manifest.json") + "\n")
        out.append(str(session / "manifest.json"))
    return dict(assignments=out, paid_calls=0, grants_written=False)


def approval_script(root):
    """A person-run script: E2 opt-in for these homes only, auto approval off, sandbox required."""
    m = verify(root)
    track, reflex = m["route"].rsplit("/", 1)
    policy = '[exec]\nmax_level = "E2"\nauto_approve = "off"\nrequire_sandbox = true\n'
    lines = ["#!/bin/sh", "set -eu",
             "printf '%%s\\n' 'Launch canary: enable E2 only in %d new temporary homes and approve one learned pytest verification.'" % len(m["matrix"]),
             "printf '%s\\n' 'Sandbox required; automatic approval off; declared pytest/bytecode caches only; no global changes.'"]
    caches = " ".join("--cache " + shlex.quote(c) for c in m["caches"])
    for row in m["matrix"]:
        session = session_dir(root, row)
        home, checkout = session / "adage-home", session / "repo"
        p = shlex.quote(str(home / "policy.toml"))
        lines += ["test -d %s && test -f %s || { echo 'Prepare the canary first.' >&2; exit 1; }" % (
                      shlex.quote(str(checkout)), shlex.quote(str(home / "index.json"))),
                  "test ! -e %s || { echo 'Grant already present; stop and review.' >&2; exit 1; }" % shlex.quote(str(home / "approvals.json")),
                  "if [ -e %s ]; then printf '%%s' %s | cmp -s - %s || { echo 'Existing policy differs; stop.' >&2; exit 1; }; "
                  "else printf '%%s' %s > %s; fi" % (p, shlex.quote(policy), p, shlex.quote(policy), p),
                  "ADAGE_HOME=%s %s review delegate %s %s --level e2 --effect verify %s --timeout-ms 30000 --repo %s --by launch-evaluation-human" % (
                      shlex.quote(str(home)), shlex.quote(m["binaries"]["adage"]), track, reflex, caches, shlex.quote(str(checkout)))]
    text = "\n".join(lines) + "\n"
    path = root / "approve-canary.sh"
    if path.exists() and path.read_text() != text:
        raise ValueError("approval script already written with different content")
    path.write_text(text)
    return dict(path=str(path), sha256=pairs.sha(path), homes=len(m["matrix"]))


def verify_grants(root):
    """Read-only check of the person's grants: exactly the reviewed route, terms and policy."""
    m = verify(root)
    problems = []
    for row in m["matrix"]:
        home = session_dir(root, row) / "adage-home"
        try:
            store = pairs.read(home / "approvals.json")
            policy = (home / "policy.toml").read_text()
        except OSError as error:
            problems.append(dict(order=row["order"], problem=str(error)))
            continue
        grants = store.get("approvals", [])
        g = grants[0] if len(grants) == 1 else {}
        expected = dict(route=m["route"], by="launch-evaluation-human", auto=False)
        if (len(grants) != 1 or any(g.get(k) != v for k, v in expected.items())
                or g["key"].get("level") != "e2" or g["key"].get("effect") != "verify"
                or g["key"].get("argv_templates") != [shlex.split(m["command"])]
                or g.get("terms", {}).get("cache") != m["caches"] or g["terms"].get("timeout_ms") != 30000
                or policy != '[exec]\nmax_level = "E2"\nauto_approve = "off"\nrequire_sandbox = true\n'):
            problems.append(dict(order=row["order"], problem="grant or policy differs from the reviewed script"))
    return dict(passed=not problems, problems=problems, checked=len(m["matrix"]))


def hook(manifest, payload):
    return pairs.hook(manifest, payload)


def read_jsonl(path):
    return [json.loads(line) for line in path.read_text().splitlines() if line.strip()] if path.exists() else []


def native_contract(session):
    """Whether the execution contract reached the native transcript (not just hook stdout)."""
    hits = 0
    for line in (session / "native-transcript.jsonl").read_text().splitlines() if (session / "native-transcript.jsonl").exists() else []:
        if 'kind=\\"execution-contract\\"' in line or 'kind="execution-contract"' in line:
            hits += 1
    return hits


def collect(manifest):
    m = pairs.read(manifest)
    pairs.verify(m)
    session, root, checkout = Path(m["root"]), Path(m["study_root"]), Path(m["execution_root"])
    if (session / "collection.json").exists():
        raise ValueError("collection is immutable")
    study = verify(root)
    errors = []
    def attempt(label, call, fallback):
        try:
            return call()
        except Exception as error:
            errors.append(dict(stage=label, error=str(error)))
            return fallback
    final = pairs.tree_state(checkout)
    pairs.write(session / "final-source.json", final)
    (session / "final-tracked.patch").write_bytes(subprocess.run(["git", "diff", "--binary", "HEAD"],
        cwd=checkout, capture_output=True, check=True).stdout)
    provenance = attempt("native-transcript", lambda: gate.retain_transcript(session, checkout), {})
    stream = attempt("stream", lambda: read_jsonl(session / "stdout.jsonl"), [])
    hooks = attempt("hooks", lambda: read_jsonl(session / "production-hooks.jsonl"), [])
    metrics = pairs.stream_metrics(stream)
    def score():
        plan = study["families"][0]
        scoring = session / "private-score"
        gate.create_checkout(root, plan["repo"], plan["base_full"], scoring)
        pairs.frozen_setup(root, scoring, plan, study["python_requested"])
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
    dispatches = attempt("dispatches", lambda: [dict(request_id=v[1], source=v[2]) for line in
        (session / "claude-debug.log").read_text().splitlines() if (v := pattern.fullmatch(line))], [])
    receipts = attempt("journal", lambda: pairs.journal(m), [])
    rerun, wider = CONDITIONS[m["condition"]][1:]
    timeline = attempt("timeline", lambda: receipt_replay.extract(session / "production-hooks.jsonl", "automatic"), {})
    replay = attempt("replay", lambda: receipt_replay.replay(timeline, rerun, wider), {})
    starts = [h for h in hooks if h.get("event") == "SessionStart"]
    outputs = [h.get("output") or "" for h in hooks if h.get("journal_new")]
    result = dict(protocol=PROTOCOL, condition=m["condition"], repetition=m["repetition"], task=m["task"],
        order=m["order"], provenance=provenance, quality=quality, summary=summary, cost_usd=summary.get("total_cost_usd"),
        usage=summary.get("usage"), observed_dispatches=dispatches, agent_action_count=metrics["agent_action_count"],
        agent_actions=metrics["agent_actions"], all_journal_receipts=receipts, timeline=timeline, replay=replay,
        contract_emitted=any('kind=\\"execution-contract\\"' in (h.get("output") or "") for h in starts),
        contract_native_hits=attempt("contract", lambda: native_contract(session), 0),
        structured_receipts=sum("COMPLETED CPU ACTION" in o and "operation_id" in o and "declared_input_digest" in o for o in outputs),
        final_generation=final["digest"], process=attempt("process", lambda: pairs.read(session / "run.json"), {}),
        collection_errors=errors)
    pairs.write(session / "collection.json", result)
    return result


def realized(row):
    """Whether the session actually produced the condition it was assigned (all rows are kept)."""
    r = row.get("replay") or {}
    events = (row.get("timeline") or {}).get("events", [])
    receipts = [e for e in events if e.get("kind") == "receipt"]
    ok = [e for e in receipts if e.get("phase") == "completed" and e.get("outcome") == "applied"]
    final = (row.get("timeline") or {}).get("final_digest")
    c = row["condition"]
    if c == "failed":
        return bool(receipts) and not ok
    if c in ("later-edit", "config-change"):
        return bool(ok) and all(e["digest"] != final for e in ok)
    return bool(ok) and bool(r)


def live(root, wall_seconds=600):
    root = root.resolve(strict=True)
    m = verify(root)
    grants = verify_grants(root)
    if not grants["passed"]:
        raise ValueError("person-issued grants are missing or differ from the reviewed script")
    isolation(m)
    authority = Path(m["budget_authority"])
    results = []
    for row in m["matrix"]:
        manifest = session_dir(root, row) / "manifest.json"
        a = pairs.read(manifest)
        pairs.verify(a)
        gate.validate_authority(a)
        gate.require_native_review(a)
        if pairs.tree_state(Path(a["execution_root"])) != a["initial_source"]:
            raise ValueError("prepared checkout changed before dispatch")
    for row in m["matrix"]:
        manifest = session_dir(root, row) / "manifest.json"
        a = pairs.read(manifest)
        key = str(Path(a["root"]).relative_to(authority))
        with pairs.execution_lock(authority):
            state_path = authority / "paid-state.json"
            state = pairs.read(state_path)
            if key in state["runs"]:
                results.append(dict(order=row["order"], skipped="already attempted; never retried"))
                continue
            spent = sum(pairs.known_cost(v.get("summary", {}).get("total_cost_usd")) or 0 for k, v in state["runs"].items()
                        if k.startswith(str(root.relative_to(authority)) + "/"))
            if spent + SESSION_CAP_USD > m["budget"]["study_ceiling_usd"]:
                raise ValueError("canary study ceiling reached; stop and report")
            budget = pairs.Budget(state, gate.budget_limit(a), SESSION_CAP_USD, persist=lambda: pairs.write(state_path, state))
            attempt, reason = budget.reserve(key)
            if attempt is None:
                raise ValueError(reason)
            state["runs"][key] = dict(status="reserved", task=a["task"], arm="automatic", condition=a["condition"], protocol=PROTOCOL)
            pairs.write(state_path, state)
            result = None
            try:
                if not budget.dispatch(attempt):
                    raise ValueError("accounting blocked dispatch")
                result = pairs.probe.execute(manifest, SESSION_CAP_USD, wall_seconds)
                budget.settle(attempt, result)
                state["runs"][key].update(status="complete", summary=result)
                state["runs"][key]["quality"] = collect(manifest)["quality"]
            except BaseException as error:
                state["runs"][key].update(status="failed", error=str(error))
                raise
            finally:
                budget.settle(attempt, result)
                pairs.write(state_path, state)
                pairs.append(authority / "attempts.jsonl", dict(status="launch-canary-finished", run=key,
                    accounting=attempt, evidence=state["runs"][key], timestamp_ns=time.time_ns()))
        results.append(dict(order=row["order"], status=state["runs"][key]["status"]))
    return results


def report(root):
    root = root.resolve(strict=True)
    m = verify(root)
    rows = []
    for row in m["matrix"]:
        path = session_dir(root, row) / "collection.json"
        rows.append(pairs.read(path) if path.exists() else dict(row, status="uncollected"))
    for row in rows:
        row["condition_realized"] = row.get("status") != "uncollected" and realized(row)
    controls = [r for r in rows if r["condition"] != "positive" and r["condition_realized"]]
    skipped = [r["order"] for r in controls if r["replay"].get("necessary_work_skipped")]
    positive = [r for r in rows if r["condition"] == "positive" and r.get("status") != "uncollected"]
    costs = [pairs.known_cost(r.get("cost_usd")) for r in rows]
    complete = all(r.get("status") != "uncollected" and not r.get("collection_errors") for r in rows)
    contract = all(r.get("contract_emitted") and r.get("contract_native_hits") for r in rows if r.get("status") != "uncollected")
    successes = sum(r.get("replay", {}).get("successful_receipts", 0) for r in rows)
    structured = sum(r.get("structured_receipts", 0) for r in rows)
    foreign = [r["order"] for r in rows if any(x.get("route") != m["route"] or not x.get("approved_on")
                                                 for x in r.get("all_journal_receipts", []))]
    by_condition = {c: sorted(r["order"] for r in controls if r["condition"] == c) for c in CONDITIONS if c != "positive"}
    result = dict(protocol=PROTOCOL, assignments=len(rows), collected=sum(r.get("status") != "uncollected" for r in rows),
        complete=complete, known_cost_usd=sum(c for c in costs if c is not None), unknown_costs=sum(c is None for c in costs),
        contract_delivered_all=contract, successful_cpu_receipts=successes, structured_receipts_delivered=structured,
        realized_controls=by_condition, necessary_work_skipped=skipped, foreign_or_unapproved_receipts=foreign,
        positive_eliminations=sum(r["replay"].get("confirmed_elimination", False) for r in positive),
        positive_redundant_repeats=sum(r["replay"].get("redundant_repeats", 0) for r in positive),
        quality_passed=sum(r.get("quality", {}).get("passed") is True for r in rows),
        passed=bool(complete and contract and not skipped and not foreign and structured == successes
                    and all(by_condition.values()) and all(c is not None for c in costs)),
        rows=[{k: r.get(k) for k in ("order", "condition", "repetition", "task", "condition_realized", "cost_usd",
              "contract_emitted", "contract_native_hits", "structured_receipts", "quality")} |
              dict(replay={k: v for k, v in (r.get("replay") or {}).items() if k != "requests"},
                   requests=[(q["classification"], q.get("segment") or q.get("command")) for q in (r.get("replay") or {}).get("requests", [])],
                   dispatches=len(r.get("observed_dispatches", [])), agent_actions=r.get("agent_action_count")) for r in rows],
        limitations=["Development-split tasks and a guided learned route: integration evidence, not held-out efficacy.",
                     "Prompt suffixes order the work so each negative condition can occur; unrealized rows stay in the record.",
                     "Model requests and tokens are not GPU measurements."])
    pairs.write(root / "reports" / ("canary-" + str(time.time_ns()) + ".json"), result)
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("freeze", "prepare", "approval-script", "verify-grants", "hook", "live",
                                            "collect", "report"))
    parser.add_argument("path", type=Path)
    parser.add_argument("--cohort", type=Path)
    parser.add_argument("--cumulative", type=Path)
    parser.add_argument("--controls", type=Path)
    parser.add_argument("--binaries", type=Path)
    parser.add_argument("--audit-authority", type=Path, action="append", default=[])
    args = parser.parse_args()
    if args.command == "hook":
        print(hook(args.path, json.load(sys.stdin)), end="")
        sys.exit(0)
    if args.command == "freeze":
        value = freeze(args.path, args.cohort, args.cumulative, args.controls, args.binaries, args.audit_authority)
    elif args.command == "collect":
        value = collect(args.path)
    else:
        value = {"prepare": prepare, "approval-script": approval_script, "verify-grants": verify_grants,
                 "live": live, "report": report}[args.command](args.path.resolve(strict=True))
    print(json.dumps(value, indent=2, default=str))
