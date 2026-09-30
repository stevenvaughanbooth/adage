#!/usr/bin/env python3
"""Protocol 2: paired real-code preparation, production hook observation and strict evidence.

The original real_repo_gate helper and existing manifests remain immutable. This revision imports
its offline preparation/confinement helpers, never its paid admission shortcut. No command here
creates, copies, edits or revokes execution grants.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shlex
import shutil
import subprocess
import sys
import time
import xml.etree.ElementTree as ET

import real_repo_gate as substrate
import probe
import bash_action_audit as bash_audit

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[2]
sys.path.insert(0, str(REPO / "evals/live"))
from budget import Budget, execution_lock, known_cost
from runner import spool_records

PROTOCOL = 2
EVENTS = ("SessionStart", "UserPromptSubmit", "PreToolUse", "PostToolUse", "Stop", "SessionEnd")
IGNORED = {".git", ".venv", "__pycache__", ".pytest_cache", ".mypy_cache", ".ruff_cache"}
write, sha, append = substrate.write, substrate.sha, substrate.append


def read(path):
    return json.loads(Path(path).read_text())


def source_hashes():
    files = [Path(__file__), HERE / "bash_action_audit.py", HERE / "REAL-REPO-PAIRS.md", HERE / "real_repo_gate.py", HERE / "probe.py", REPO / "evals/live/budget.py",
             REPO / "evals/live/runner.py", REPO / "evals/showcase/sc_score.py",
             REPO / "evals/showcase/families.py", REPO / "evals/showcase/fam_python.py",
             REPO / "evals/lab/research/recurring-workflows/families.py", REPO / "evals/repobench/score.py",
             REPO / "evals/repobench/common.py", REPO / "evals/repobench/sandbox.py", REPO / "evals/repobench/repos.json"]
    return {str(p):sha(p) for p in files}


def hidden_cases(path):
    tree = ET.parse(path)
    cases = []
    for case in tree.iter("testcase"):
        identity = (case.get("classname", ""), case.get("name", ""))
        if not all(identity):
            raise ValueError("JUnit testcase identity missing")
        cases.append(dict(classname=identity[0], name=identity[1],
                          passed=not any(case.find(tag) is not None for tag in ("skipped", "failure", "error"))))
    return cases


def prepare_hidden_expectations(root, task):
    """Run only trusted existing gold code, offline, to freeze exact parametrized identities."""
    families, _, _, _ = substrate.modules(root)
    f, variant = families.parse_task(task)
    expected = root / "private/hidden-identities" / (task + ".json")
    if expected.exists():
        return read(expected)
    golds = sorted((root / "private/validation").glob("*/" + task + "/gold"))
    if not golds:
        raise ValueError("validated trusted gold checkout missing")
    gold = golds[-1]
    hidden = gold / f["hidden_path"]
    if hidden.read_text() != f["hidden"](variant):
        raise ValueError("gold hidden tests differ from unchanged showcase definition")
    report = gold / "adage-hidden-control.xml"
    result = substrate.confined(root, "hidden-control-" + task + "-" + str(time.time_ns()),
        [gold / ".venv/bin/python", "-m", "pytest", "-q", "-p", "no:cacheprovider", f["hidden_path"],
         "--junitxml=" + str(report)], gold)
    cases = hidden_cases(report)
    identities = sorted((c["classname"], c["name"]) for c in cases)
    if result.returncode or not identities or len(identities) != len(set(identities)) or not all(c["passed"] for c in cases):
        raise ValueError("trusted gold hidden-test identity control failed")
    value = dict(task=task, identities=identities, hidden_sha256=sha(hidden), control_report=str(report),
                 control_report_sha256=sha(report), control_exit=0, paid_calls=0)
    write(expected, value)
    return value


def collection_integrity(report, expected):
    try:
        cases = hidden_cases(report)
    except (OSError, ValueError, ET.ParseError):
        return dict(passed=False, reason="missing or invalid independent JUnit report")
    identities = {tuple(identity) for identity in expected["identities"]}
    classes = {identity[0] for identity in identities}
    relevant = [case for case in cases if case["classname"] in classes]
    seen = [(case["classname"], case["name"]) for case in relevant]
    missing = sorted(identities - set(seen))
    extra = sorted(set(seen) - identities)
    unsuccessful = [(case["classname"], case["name"]) for case in relevant if not case["passed"]]
    passed = bool(identities) and not missing and not extra and not unsuccessful and len(seen) == len(set(seen))
    return dict(passed=passed, expected_count=len(identities), observed_count=len(seen),
                missing=missing, extra=extra, unsuccessful=unsuccessful)


def frozen_setup(root, checkout, plan, python):
    """Offline install from trusted base with exact already-prepared dependency versions."""
    env = substrate.environment(root)
    env["UV_OFFLINE"] = "1"
    pins = [line for line in plan["dependencies"] if not line.startswith("-e ")
            and not re.fullmatch(r"Using Python [0-9.]+ environment at: .+", line)]
    if any(not re.fullmatch(r"[A-Za-z0-9_.-]+==[^\s]+", pin) for pin in pins):
        raise ValueError("review nonstandard frozen dependency before preparation")
    substrate.command(root, "paired-venv-" + plan["repo"], ["uv", "venv", "--offline", "--python", python, checkout / ".venv"], env=env)
    substrate.command(root, "paired-deps-" + plan["repo"], ["uv", "pip", "install", "--offline", "--python",
        checkout / ".venv/bin/python", "-e", ".", *pins], cwd=checkout, env=env)


def tree_state(root):
    """Conservative whole-source generation; no following symlinks or ignoring source edits."""
    rows = {}
    for directory, dirs, files in os.walk(root, followlinks=False):
        dirs[:] = sorted(d for d in dirs if d not in IGNORED)
        for name in sorted(files + [d for d in dirs if (Path(directory) / d).is_symlink()]):
            path = Path(directory) / name
            relative = str(path.relative_to(root))
            if path.is_symlink():
                rows[relative] = {"symlink": os.readlink(path)}
            elif path.is_file():
                rows[relative] = {"sha256":sha(path), "exec": bool(path.stat().st_mode & 0o111)}
            else:
                raise ValueError("unsupported source file type: " + relative)
    digest = hashlib.sha256(json.dumps(rows, sort_keys=True, separators=(",", ":")).encode()).hexdigest()
    return {"digest":digest, "files":rows}


def safe_file(root, relative):
    path = root / relative
    if Path(relative).is_absolute() or ".." in Path(relative).parts or not path.is_file() or path.is_symlink():
        raise ValueError("artifact files must be regular relative paths")
    if root.resolve() not in path.resolve().parents:
        raise ValueError("artifact escapes its home")
    if any(p in {"approvals.json", "sessions", "exec"} for p in Path(relative).parts):
        raise ValueError("grants and runtime state are never copied as learned artifacts")
    return path


def verify(manifest):
    session = Path(manifest["root"])
    if sha(session / "manifest.json") != (session / "manifest.sha256").read_text().strip():
        raise ValueError("prepared assignment manifest changed")
    argv = manifest["argv"]
    if argv[argv.index("--model") + 1] != manifest["model"]:
        raise ValueError("argv model differs from comparison identity")
    for path, expected in manifest["sources"].items():
        if sha(Path(path)) != expected:
            raise ValueError("protocol source changed; prepare an explicit new revision")
    for path, expected in manifest["frozen"].items():
        if sha(Path(path)) != expected:
            raise ValueError("frozen learned artifact, binary or settings changed")


def prepare_pair(root, task, descriptor):
    """Prepare both arms; descriptor is produced from actual production learning, never a route template."""
    root = root.resolve()
    base = read(root / "manifest.json")
    validation = read(root / "validation.json")
    if not validation.get("complete") or not validation.get("valid"):
        raise ValueError("all held-out base/gold sensitivity checks must pass first")
    entries = [e for e in base["matrix"] if e["task"] == task]
    if len(entries) != 2 or {e["arm"] for e in entries} != {"suggest", "automatic"}:
        raise ValueError("task must be an assigned held-out pair")
    plan = next(p for p in base["families"] if task in p["heldout"])
    prepare_hidden_expectations(root, task)
    hidden_path = root / "private/hidden-identities" / (task + ".json")
    artifact = read(descriptor)
    if artifact["family"] != plan["family"] or artifact["action_kind"] != "verify":
        raise ValueError("first coding protocol admits the learned verification family only")
    # Native provenance is mandatory. Production condition output remains part of the audit.
    if set(artifact["training_tasks"]) != set(plan["train"]):
        raise ValueError("requires exactly the five prospectively assigned training tasks")
    for training in plan["train"]:
        session = root / "sessions" / training
        m, provenance = read(session / "manifest.json"), read(session / "transcript-provenance.json")
        if m["kind"] != "organic-training" or m["training_hooks_enabled"]:
            raise ValueError("intervened training is ineligible")
        if provenance.get("synthetic") is not False or sha(session / "native-transcript.jsonl") != provenance["sha256"]:
            raise ValueError("native training transcript provenance mismatch")
    home = Path(artifact["learned_home"]).resolve()
    files = {p:sha(safe_file(home, p)) for p in artifact["files"]}
    if "index.json" not in files:
        raise ValueError("learned index required")
    candidates = [r for r in read(home / "index.json")["routes"]
                  if r["track"] + "/" + r["route"]["id"] == artifact["route"]]
    if len(candidates) != 1 or not candidates[0]["track"].endswith("/learned"):
        raise ValueError("exact candidate must exist in the production learned index")
    if not artifact.get("condition_command") or not Path(artifact["condition_output"]).is_file():
        raise ValueError("preserve the actual condition invocation and output")
    pair = root / "pairs" / task
    pair.mkdir(parents=True, exist_ok=False)
    shared = pair / "bin"
    shared.mkdir()
    binaries = {}
    for name in ("adage", "adage-exec"):
        path = shared / name
        shutil.copy2(artifact["binaries"][name], path)
        binaries[name] = str(path)
    rows = []
    for entry in entries:
        arm = entry["arm"]
        session = pair / arm
        session.mkdir()
        checkout, local_home = session / "repo", session / "adage-home"
        substrate.create_checkout(root, plan["repo"], plan["base_full"], checkout)
        frozen_setup(root, checkout, plan, base["python_requested"])
        for relative in files:
            dest = local_home / relative
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(home / relative, dest)
        allowed = ["Read", "Edit", "Write", "Glob", "Grep", "Bash(.venv/bin/python -m pytest *)",
                   "Bash(.venv/bin/python -m pytest)", "Bash(.venv/bin/pytest *)", "Bash(.venv/bin/pytest)",
                   "Bash(git status *)", "Bash(git status)", "Bash(git diff *)", "Bash(git diff)", "Bash(ls *)", "Bash(ls)"]
        if base.get("protocol_revision") == 5:
            from cohort_v5 import policy
            settings = policy(Path(base["budget_authority"]), checkout, root / "runtime/python")
        else:
            settings = substrate.native_settings(root, checkout, allowed)
        settings["disableAllHooks"] = False
        settings["hooks"] = {event:[{"hooks":[{"type":"command", "command":shlex.join([
            base["preparation_python"], "-B", str(Path(__file__)), "hook", str(session / "manifest.json")]),
            "timeout":90}]}] for event in EVENTS}
        write(session / "settings.json", settings)
        (session / ".reflex-fixture").write_text("protocol 2 real coding task\n")
        argv = ["claude", "--print", "--restricted", "--model", artifact["model"], "--max-budget-usd", "1.5",
            "--max-turns", "60", "--output-format", "stream-json", "--verbose", "--include-hook-events",
            "--include-partial-messages", "--debug-file", str(session / "claude-debug.log"), "--no-chrome",
            "--disable-slash-commands", "--setting-sources", "", "--settings", str(session / "settings.json"),
            "--strict-mcp-config", "--mcp-config", '{"mcpServers":{}}', "--tools", "Read,Edit,Write,Glob,Grep,Bash",
            "--permission-mode", "dontAsk", "--permission-prompts", "none", entry["prompt"]]
        frozen = {str(local_home / p):digest for p,digest in files.items()}
        frozen.update({str(p):sha(p) for p in [session / "settings.json", *map(Path,binaries.values()),
                                              Path(descriptor), Path(artifact["condition_output"]), hidden_path,
                                              root / "manifest.json", root / "validation.json"]})
        m = dict(protocol=PROTOCOL, root=str(session), experiment_root=str(root), execution_root=str(checkout),
            home=str(local_home), arm=arm, task=task, route=artifact["route"], argv=argv,
            binaries=binaries, learned_files=files, sources=source_hashes(), frozen=frozen,
            budget_usd=1.5, expected_claude_version=probe.EXPECTED_VERSION,
            hidden_identities=str(hidden_path),
            matching_command_template=candidates[0]["route"]["step"]["command"],
            initial_source=tree_state(checkout), training_tasks=plan["train"], model=artifact["model"])
        if base.get("protocol_revision") == 5:
            from cohort_v5 import startup_argv
            m["argv"] = startup_argv(m["argv"])
            review = root / "native-confinement-reviewed.json"
            m.update(cohort_root=str(root), experiment_root=base["budget_authority"], protocol_revision=5,
                     native_review_path=str(review), native_review_sha256=sha(review))
            m["sources"][str(HERE / "cohort_v5.py")] = sha(HERE / "cohort_v5.py")
            substrate.validate_authority(m)
            substrate.require_native_review(m)
        normalized_settings = json.dumps(settings, sort_keys=True).replace(str(session), "<assignment>")
        m["comparison_identity"] = dict(protocol=PROTOCOL, model=artifact["model"], base_commit=plan["base_full"],
            binary_hashes={name:sha(Path(path)) for name,path in binaries.items()}, sources=m["sources"],
            initial_source_digest=m["initial_source"]["digest"], training_tasks=plan["train"],
            settings_sha256=hashlib.sha256(normalized_settings.encode()).hexdigest())
        write(session / "manifest.json", m)
        (session / "manifest.sha256").write_text(sha(session / "manifest.json") + "\n")
        rows.append(str(session / "manifest.json"))
    if read(rows[0])["comparison_identity"] != read(rows[1])["comparison_identity"]:
        raise ValueError("prepared pair differs in source, model, runtime or tool settings")
    write(pair / "pair.json", dict(protocol=PROTOCOL, task=task, assignments=rows, paid_calls=0,
                                   approval="person must grant the automatic home's exact candidate; harness never writes grants"))
    append(root / "attempts.jsonl", dict(status="pair-prepared", protocol=PROTOCOL, task=task,
                                        assignments=rows, paid_calls=0, timestamp_ns=time.time_ns()))
    return read(pair / "pair.json")


def production_env(m):
    return dict(os.environ, ADAGE_HOME=m["home"], ADAGE_AUTO_LEARN="0", ADAGE_ARM="treated")


def journal(m):
    p = subprocess.run([m["binaries"]["adage-exec"], "journal", "--repo", m["execution_root"]],
        cwd=m["execution_root"], env=production_env(m), capture_output=True, text=True, timeout=10)
    if p.returncode:
        raise ValueError("production journal is unreadable: " + p.stderr[:500])
    return json.loads(p.stdout)


def hook(manifest_path, payload):
    """Observation only: output is the unmodified actual production hook output."""
    wrapper_started = time.monotonic_ns()
    m = read(manifest_path)
    verify(m)
    if Path(payload.get("cwd", "")).resolve() != Path(m["execution_root"]).resolve():
        raise ValueError("hook cwd does not match its assignment")
    event = payload.get("hook_event_name")
    if event not in EVENTS:
        raise ValueError("unregistered hook event")
    session = Path(m["root"])
    before = tree_state(Path(m["execution_root"]))
    prior = journal(m)
    name = "adage-exec" if m["arm"] == "automatic" else "adage"
    started = time.monotonic_ns()
    p = subprocess.run([m["binaries"][name], "hook", "claude", event], input=json.dumps(payload),
        cwd=m["execution_root"], env=production_env(m), capture_output=True, text=True, timeout=80)
    after = tree_state(Path(m["execution_root"]))
    current = journal(m)
    prior_rows = {json.dumps(r, sort_keys=True) for r in prior}
    new_receipts = [r for r in current if json.dumps(r, sort_keys=True) not in prior_rows]
    append(session / "production-hooks.jsonl", dict(event=event, payload=payload, output=p.stdout,
        stderr=p.stderr, exit_code=p.returncode, started_ns=started, ended_ns=time.monotonic_ns(),
        wrapper_started_ns=wrapper_started,
        before=before, after=after, journal_new=new_receipts))
    if p.returncode:
        raise RuntimeError("production hook failed; recorded without fabricating output")
    return p.stdout


def stream_metrics(stream):
    calls, results = {}, {}
    for item in stream:
        if item.get("type") == "assistant":
            for block in item.get("message", {}).get("content", []):
                if block.get("type") == "tool_use":
                    calls[block["id"]] = block
        if item.get("type") == "user":
            content = item.get("message", {}).get("content", [])
            for block in content if isinstance(content, list) else []:
                if block.get("type") == "tool_result":
                    results[block["tool_use_id"]] = block
    return dict(agent_actions=list(calls.values()), tool_results=results, agent_action_count=len(calls))


def command_matches(command, template):
    """Literal argv template matching only, never shell interpretation or family heuristics."""
    try:
        actual, expected = shlex.split(command), shlex.split(template)
    except ValueError:
        return False
    if len(actual) != len(expected):
        return False
    slots = {}
    for a,e in zip(actual, expected):
        names = re.findall(r"\{([a-zA-Z_][a-zA-Z_0-9]*)\}", e)
        if not names:
            if a != e:
                return False
            continue
        parts = re.split(r"\{[a-zA-Z_][a-zA-Z_0-9]*\}", e)
        pattern = re.escape(parts[0])
        for suffix in parts[1:]:
            pattern += r"([a-zA-Z0-9_./:=@,+-]+?)" + re.escape(suffix)
        match = re.fullmatch(pattern, a)
        if not match:
            return False
        for name,value in zip(names, match.groups()):
            if value.startswith("-") or ".." in value.split("/") or (name in slots and slots[name] != value):
                return False
            slots[name] = value
    return True


def final_receipts(hooks, route, session_id, checkout, final_digest):
    eligible = []
    for h in hooks:
        # This first protocol admits verify actions only. No mutation during execution,
        # and no subsequent source edit, may be hidden by a correct but stale test run.
        if h.get("exit_code") != 0 or h["before"]["digest"] != final_digest or h["after"]["digest"] != final_digest:
            continue
        if h.get("payload", {}).get("session_id") != session_id:
            continue
        for r in h.get("journal_new", []):
            if (r.get("phase") == "completed" and r.get("route") == route and r.get("session") == session_id
                and r.get("level") == "e2" and r.get("outcome", {}).get("outcome") == "applied"
                and r.get("worktree") == str(Path(checkout).resolve()) and r.get("approved_on")
                and re.fullmatch(r"[0-9a-f]{64}", r.get("input_digest", "")) and r.get("id")
                and not r.get("outcome", {}).get("changed")):
                eligible.append(r)
    return eligible


def score_with_integrity(root, task, scoring, expected):
    junit = scoring / "adage-independent-junit.xml"
    original_confined = substrate.confined
    def report_confined(root_arg, label, argv, checkout_arg, timeout=300):
        return original_confined(root_arg, label, [*argv, "--junitxml=" + str(junit)], checkout_arg, timeout)
    try:
        # Keep the shared scorer untouched; add only independent result instrumentation.
        substrate.confined = report_confined
        quality = substrate.score(root, task, scoring)
    finally:
        substrate.confined = original_confined
    integrity = collection_integrity(junit, expected)
    quality["hidden_collection_integrity"] = integrity
    quality["passed"] = quality["passed"] and integrity["passed"]
    return quality


def collect(manifest_path):
    m = read(manifest_path)
    verify(m)
    session, checkout, root = Path(m["root"]), Path(m["execution_root"]), Path(m.get("cohort_root", m["experiment_root"]))
    if (session / "collection.json").exists():
        raise ValueError("collection is immutable; retain prior attempt")
    provenance = substrate.retain_transcript(session, checkout)
    final = tree_state(checkout)
    write(session / "final-source.json", final)
    diff = subprocess.run(["git", "diff", "--binary", "HEAD"], cwd=checkout, env=substrate.environment(root),
                          capture_output=True, timeout=30, check=True).stdout
    (session / "final-tracked.patch").write_bytes(diff)
    stream = [json.loads(s) for s in (session / "stdout.jsonl").read_text().splitlines()]
    hooks = [json.loads(s) for s in (session / "production-hooks.jsonl").read_text().splitlines()]
    metrics = stream_metrics(stream)
    matching = [a for a in metrics["agent_actions"] if a.get("name") == "Bash"
                and command_matches(a.get("input", {}).get("command", ""), m["matching_command_template"])]
    successful = [a for a in matching if a["id"] in metrics["tool_results"]
                  and not metrics["tool_results"][a["id"]].get("is_error", False)]
    receipts = final_receipts(hooks, m["route"], provenance["session_id"], checkout, final["digest"])
    # Install from the trusted base first: never execute an agent-modified build backend.
    scoring = session / "private-score"
    base = read(root / "manifest.json")
    plan = next(p for p in base["families"] if m["task"] in p["heldout"])
    if any("symlink" in value for value in final["files"].values()):
        raise ValueError("source symlinks require separate confinement review before scoring")
    substrate.create_checkout(root, plan["repo"], plan["base_full"], scoring)
    frozen_setup(root, scoring, plan, base["python_requested"])
    before_score = tree_state(scoring)
    for relative in set(before_score["files"]) - set(final["files"]):
        (scoring / relative).unlink()
    for relative in final["files"]:
        destination = scoring / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(checkout / relative, destination)
    if tree_state(scoring) != final:
        raise ValueError("scorer source differs from the captured final source")
    quality = score_with_integrity(root, m["task"], scoring, read(m["hidden_identities"]))
    summary = probe.summary(session)
    # Extract observed real dispatches; never infer requests from assistant message count.
    request_pattern = re.compile(r"^\S+ \[DEBUG\] \[API REQUEST\] /v1/messages x-client-request-id=([\w-]+) source=(\w+)$")
    dispatches = [dict(request_id=match[1], source=match[2]) for line in (session / "claude-debug.log").read_text().splitlines()
                  if (match := request_pattern.fullmatch(line))]
    spool = Path(m["home"]) / "sessions" / (provenance["session_id"] + ".spool")
    if not spool.is_file():
        raise ValueError("production session spool is missing")
    shutil.copyfile(spool, session / "production-session.spool")
    runtime_records = spool_records(spool)
    output = dict(protocol=PROTOCOL, task=m["task"], arm=m["arm"], provenance=provenance, quality=quality,
        summary=summary, process=read(session / "run.json"), **metrics, matching_agent_actions=matching,
        successful_matching_agent_actions=successful, final_generation_receipts=receipts,
        bash_action_audit=bash_audit.audit_bash_actions(metrics["agent_actions"]),
        all_journal_receipts=journal(m), hooks=hooks, final_generation=final["digest"],
        runtime_records=runtime_records, execution_attempts=[e for r in runtime_records for e in r.get("exec", [])],
        observed_dispatches=dispatches, cost_usd=summary.get("total_cost_usd"), learned_files=m["learned_files"],
        prompt=m["argv"][-1], template=m["matching_command_template"], route=m["route"])
    output["comparison_identity"] = m["comparison_identity"]
    write(session / "collection.json", output)
    return output


def pair_result(rows):
    reasons = []
    if len(rows) != 2 or {r.get("arm") for r in rows} != {"suggest", "automatic"}:
        return dict(passed=False, reasons=["complete unique arm pair required"])
    baseline = next(r for r in rows if r["arm"] == "suggest")
    automatic = next(r for r in rows if r["arm"] == "automatic")
    for key in ("task", "learned_files", "prompt", "template", "route", "comparison_identity"):
        if key not in baseline or key not in automatic or baseline.get(key) != automatic.get(key):
            reasons.append("arm mismatch: " + key)
    automatic_blockers = []
    for r in rows:
        actions = r.get("agent_actions")
        if not isinstance(actions, list) or any(not isinstance(a, dict) for a in actions):
            reasons.append(r["arm"] + ": complete action records missing")
        else:
            # No trusted shell/config attestation exists in this protocol. Recompute with
            # default strict assumptions; a stored inspection label cannot waive uncertainty.
            expected_audit = bash_audit.audit_bash_actions(actions)
            if r.get("bash_action_audit") != expected_audit or r.get("agent_action_count") != len(actions):
                reasons.append(r["arm"] + ": Bash audit missing, inconsistent, or incomplete")
            if r["arm"] == "automatic":
                automatic_blockers = expected_audit["blocking_actions"]
                if automatic_blockers:
                    reasons.append("automatic arm has definite or unclassified potential Bash verification")
        if not r.get("quality", {}).get("passed") or r["quality"].get("n_tests", 0) <= 0:
            reasons.append(r["arm"] + ": independent quality failed")
        if r.get("process", {}).get("exit_code") != 0 or r["process"].get("cancelled"):
            reasons.append(r["arm"] + ": process failed or censored")
        cost = known_cost(r.get("cost_usd"))
        if cost is None or cost > 1.50 or r.get("summary", {}).get("result_is_error") is not False:
            reasons.append(r["arm"] + ": accounting or terminal result failed")
    if baseline.get("all_journal_receipts"):
        reasons.append("suggest arm unexpectedly executed production actions")
    if not baseline.get("successful_matching_agent_actions"):
        reasons.append("baseline did not successfully perform the learned action")
    if not automatic.get("final_generation_receipts"):
        reasons.append("no successful production CPU receipt for final source generation")
    if automatic.get("matching_agent_actions"):
        reasons.append("automatic arm still performed the corresponding agent action")
    if not all(r.get("observed_dispatches") for r in rows):
        reasons.append("actual request-dispatch diagnostics missing")
    saved = len(baseline.get("matching_agent_actions", [])) - len(automatic.get("matching_agent_actions", []))
    if saved < 1:
        reasons.append("corresponding agent action was not eliminated")
    verification_action_suppressed = not reasons
    if baseline.get("agent_action_count", 0) <= automatic.get("agent_action_count", 0):
        reasons.append("total agent actions did not decrease")
    dispatch_delta = len(baseline.get("observed_dispatches", [])) - len(automatic.get("observed_dispatches", []))
    return dict(passed=not reasons, reasons=reasons, matching_agent_action_delta=saved,
                verification_action_suppressed=verification_action_suppressed,
                automatic_bash_blockers=automatic_blockers,
                total_tool_calls_reduced=baseline.get("agent_action_count", 0) > automatic.get("agent_action_count", 0),
                observed_model_dispatches_saved=dispatch_delta,
                observed_inference_reduction=not reasons and dispatch_delta > 0,
                total_agent_action_delta=baseline.get("agent_action_count", 0)-automatic.get("agent_action_count", 0))


def live(manifest_path, wall_seconds=300):
    m = read(manifest_path)
    verify(m)
    root, session = Path(m["experiment_root"]), Path(m["root"])
    if tree_state(Path(m["execution_root"])) != m["initial_source"]:
        raise ValueError("prepared checkout changed before dispatch")
    substrate.validate_authority(m)
    substrate.require_native_review(m)
    if m["arm"] == "automatic" and not (Path(m["home"]) / "approvals.json").is_file():
        raise ValueError("human must grant the exact automatic candidate in its prepared home")
    with execution_lock(root):
        state_path = root / "paid-state.json"
        state = read(state_path) if state_path.exists() else {"runs":{}}
        budget = Budget(state, 27.50, 1.50, persist=lambda:write(state_path, state))
        key = str(session.relative_to(root))
        if key in state["runs"]:
            raise ValueError("assignment already attempted; no outcome retries")
        attempt, reason = budget.reserve(key)
        if attempt is None:
            raise ValueError(reason)
        result = None
        state["runs"][key] = dict(status="reserved", task=m["task"], arm=m["arm"], protocol=PROTOCOL)
        write(state_path, state)
        try:
            if not budget.dispatch(attempt):
                raise ValueError("accounting blocked dispatch")
            result = probe.execute(manifest_path, 1.50, wall_seconds)
            budget.settle(attempt, result)
            state["runs"][key].update(status="complete", summary=result)
            collection = collect(manifest_path)
            state["runs"][key]["quality"] = collection["quality"]
        except BaseException as error:
            state["runs"][key].update(status="failed", error=str(error))
            raise
        finally:
            budget.settle(attempt, result)
            write(state_path, state)
            append(root / "attempts.jsonl", dict(status="paired-attempt-finished", run=key,
                accounting=attempt, evidence=state["runs"][key], timestamp_ns=time.time_ns()))
    return state["runs"][key]


def main():
    p = argparse.ArgumentParser(description=__doc__)
    subs = p.add_subparsers(dest="command", required=True)
    prep = subs.add_parser("prepare")
    prep.add_argument("root", type=Path)
    prep.add_argument("task")
    prep.add_argument("descriptor", type=Path)
    hidden = subs.add_parser("freeze-hidden")
    hidden.add_argument("root", type=Path)
    hidden.add_argument("task")
    for name in ("hook", "collect", "live"):
        sub = subs.add_parser(name)
        sub.add_argument("manifest", type=Path)
    report = subs.add_parser("report")
    report.add_argument("pair", type=Path)
    args = p.parse_args()
    if args.command == "hook":
        print(hook(args.manifest, json.load(sys.stdin)), end="")
        return
    if args.command == "prepare":
        result = prepare_pair(args.root, args.task, args.descriptor)
    elif args.command == "freeze-hidden":
        result = prepare_hidden_expectations(args.root.resolve(), args.task)
    elif args.command == "report":
        result = pair_result([read(Path(path).parent / "collection.json") for path in read(args.pair)["assignments"]])
    else:
        result = (collect if args.command == "collect" else live)(args.manifest)
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
