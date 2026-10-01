#!/usr/bin/env python3
"""Development canary-v8: paired arms for model requests per task (launch-gate-1 revision 8).

Arms, run in paired blocks on the same development task and prompt:
  A0 no Adage hooks at all (the baseline);
  A1 the revision-7 integration (its frozen release binaries, the production hook registration);
  A2 Lever 2: verification ownership (factual contract, compact receipts, failures, Stop gate);
  A3 Levers 1+2: A2 plus the card naming the repository's test command.
A1-A3 share one learned home and the production event registration, and each needs a
person-issued grant. Safety controls (failed, later-edit, config-change, broader, rerun) run in
A2 and A3. `freeze`, `prepare`, `approval-script`, `verify-grants`, `collect` and `report` spend
nothing; `live` is the paid entry point (the nested-session guard stays in probe.execute). No
helper writes grants or policy. The primary metric is model requests per session (distinct
assistant message ids); CPU work is reported separately and never counted as inference saved.
"""
import argparse
import json
from pathlib import Path
import shlex
import shutil
import statistics
import subprocess
import sys
import time

import cohort_v5
import curve_sessions
import launch_canary as canary
import real_repo_pairs as pairs
import receipt_replay

gate = pairs.substrate
PROTOCOL = "dev-arms-1"
ARMS = {
    "A0": dict(hooks=False, binaries=None, card_checks=None, label="no Adage hooks"),
    "A1": dict(hooks=True, binaries="rev7", card_checks=None, label="revision-7 integration"),
    "A2": dict(hooks=True, binaries="current", card_checks=False, label="Lever 2: verification ownership"),
    "A3": dict(hooks=True, binaries="current", card_checks=True, label="Levers 1+2"),
}
BLOCKS = 6
CONTROL_ARMS = ("A2", "A3")
CONTROLS = ("failed", "later-edit", "config-change", "broader", "rerun")
SESSION_CAP_USD = 1.5
STUDY_CEILING_USD = 6.5
# Claude Code's production registration (`adage-hook` harness/claude.rs), each wrapped by the recorder.
EVENTS = (("SessionStart", None), ("UserPromptSubmit", None), ("PostToolBatch", None), ("PostToolUseFailure", None),
          ("PreToolUse", "Bash"), ("Stop", None), ("SessionEnd", None))
POLICY = '[exec]\nmax_level = "E2"\nauto_approve = "off"\nrequire_sandbox = true\n'


def sources():
    files = [Path(__file__), Path(canary.__file__)]
    return {**canary.sources(), **{str(p.resolve()): pairs.sha(p) for p in files}}


def matrix():
    """Six paired positive blocks (arm order rotates per block), then the A2/A3 controls."""
    rows, arms = [], list(ARMS)
    for block in range(BLOCKS):
        for arm in arms[block % len(arms):] + arms[:block % len(arms)]:
            rows.append(dict(order=len(rows), arm=arm, condition="positive", block=block + 1, task=canary.TASKS[0]))
    for condition in CONTROLS:
        for arm in CONTROL_ARMS:
            rows.append(dict(order=len(rows), arm=arm, condition=condition, block=None, task=canary.TASKS[0]))
    return rows


def session_dir(root, row):
    return root / "sessions" / ("%02d-%s-%s%s" % (row["order"], row["arm"], row["condition"],
                                                  "" if row["block"] is None else "-b%d" % row["block"]))


def verify(root):
    m = pairs.read(root / "manifest.json")
    if (m.get("protocol") != PROTOCOL or m["source_sha256"] != sources()
            or pairs.sha(root / "manifest.json") != (root / "manifest.sha256").read_text().strip()):
        raise ValueError("dev-arms source or manifest changed after freeze; preserve it and freeze a new root")
    for path, digest in m["frozen_inputs"].items():
        if pairs.sha(Path(path)) != digest:
            raise ValueError("frozen input changed: " + path)
    return m


def freeze(root, cohort, cumulative, controls, binaries, rev7_binaries, authorities):
    """Freeze the matrix, prompts, learned home, both binary sets and sources. Spends nothing."""
    root, cohort = root.resolve(), cohort.resolve(strict=True)
    cumulative, controls = cumulative.resolve(strict=True), controls.resolve(strict=True)
    parent = pairs.read(cohort / "manifest.json")
    if cohort not in root.parents:
        raise ValueError("the study must nest within its native-confinement cohort")
    authority = Path(parent["budget_authority"])
    record = pairs.read(cumulative / "attempt.json")
    home = cumulative / "adage-home"
    for name, digest in record["artifact_sha256"].items():
        if pairs.sha(pairs.safe_file(home, name)) != digest:
            raise ValueError("learned artifact changed")
    guided = pairs.read(cumulative.parent / "manifest.json")
    old, validation = pairs.read(controls / "manifest.json"), pairs.read(controls / "validation.json")
    if not (validation.get("complete") and validation.get("valid")):
        raise ValueError("validated task controls required")
    authorities = sorted({str(authority), *(str(Path(a).resolve(strict=True)) for a in authorities)})
    audit = canary.isolation(dict(cumulative_root=str(cumulative), audit_authorities=authorities))
    if subprocess.check_output(["git", "status", "--porcelain", "--", "crates", "Cargo.toml", "Cargo.lock"],
                               cwd=gate.REPO, text=True).strip():
        raise ValueError("production sources differ from the commit; build binaries from a clean tree")
    root.mkdir(parents=True, exist_ok=False)
    pairs.write(root / "isolation-audit.json", audit)
    for relative in ("runtime", "cache", "bases"):
        (root / relative).symlink_to((authority / relative).resolve(strict=True), target_is_directory=True)
    (root / "private").mkdir()
    (root / "private/validation").symlink_to((controls / "private/validation").resolve(strict=True), target_is_directory=True)
    shutil.copy2(controls / "validation.json", root / "validation.json")
    task = canary.TASKS[0]
    shutil.copytree(controls / "private/tasks" / task, root / "private/tasks" / task)
    if any(pairs.sha(root / "private/tasks" / task / n) != d for n, d in old["task_sha256"][task].items()):
        raise ValueError("validated task material changed")
    base_prompt = (root / "private/tasks" / task / "prompt.txt").read_text()
    rows = matrix()
    for row in rows:
        row["prompt"] = base_prompt + canary.CONDITIONS[row["condition"]][0]
    bins = {}
    for label, source in (("current", binaries), ("rev7", rev7_binaries)):
        (root / "bin" / label).mkdir(parents=True)
        bins[label] = {}
        for name in ("adage", "adage-exec"):
            shutil.copy2(Path(source) / name, root / "bin" / label / name)
            bins[label][name] = str(root / "bin" / label / name)
    inputs = [root / "isolation-audit.json", cumulative / "attempt.json", cohort / "manifest.json", controls / "manifest.json",
              controls / "validation.json", *(home / n for n in record["artifact_sha256"]),
              *(Path(p) for b in bins.values() for p in b.values())]
    plan = dict(guided["families"][0], heldout=[], train=[task])
    m = dict(protocol=PROTOCOL, root=str(root), cohort_root=str(cohort), budget_authority=str(authority),
             cumulative_root=str(cumulative), audit_authorities=authorities,
             gate_protocol_sha256=pairs.sha(canary.GATE),
             budget_allocation_sha256=parent.get("budget_allocation_sha256") or pairs.sha(authority / "evaluation-allocation.json"),
             preparation_python=guided["preparation_python"], python_requested=guided["python_requested"],
             model=guided["model"], families=[plan], matrix=rows, arms=ARMS, route=canary.ROUTE, command=canary.COMMAND,
             caches=list(canary.CACHES), learned_home=str(home), learned_files=record["artifact_sha256"],
             learned_provenance="explicitly guided demonstrations (cumulative-launch-10); fixed route, not new learning",
             binaries=bins, binary_sha256={k: {n: pairs.sha(Path(p)) for n, p in v.items()} for k, v in bins.items()},
             binary_source_commit=subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=gate.REPO, text=True).strip(),
             rev7_source_commit="c5f896c", source_sha256=sources(), frozen_inputs={str(p): pairs.sha(p) for p in inputs},
             budget=dict(sessions=len(rows), per_session_usd=SESSION_CAP_USD, study_ceiling_usd=STUDY_CEILING_USD),
             evidence_class="development: paired arms on the development task; not held-out evidence",
             grants_written=False)
    pairs.write(root / "manifest.json", m)
    (root / "manifest.sha256").write_text(pairs.sha(root / "manifest.json") + "\n")
    for source in m["source_sha256"]:
        target = root / "source" / Path(source).relative_to(gate.REPO)
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, target)
    return m


def prepare(root):
    m = verify(root)
    cohort, authority = Path(m["cohort_root"]), Path(m["budget_authority"])
    review = cohort / "native-confinement-reviewed.json"
    gate.require_native_review(dict(experiment_root=str(authority), cohort_root=str(cohort),
        native_review_path=str(review), native_review_sha256=pairs.sha(review)))
    plan, home, out = m["families"][0], Path(m["learned_home"]), []
    for row in m["matrix"]:
        arm = ARMS[row["arm"]]
        session = session_dir(root, row)
        checkout, local = session / "repo", session / "adage-home"
        session.mkdir(parents=True)
        with curve_sessions.registry(root):
            pairs.prepare_hidden_expectations(root, row["task"])
        gate.create_checkout(root, plan["repo"], plan["base_full"], checkout)
        pairs.frozen_setup(root, checkout, plan, m["python_requested"])
        settings = cohort_v5.policy(authority, checkout, root / "runtime/python")
        binaries, learned = {}, {}
        if arm["hooks"]:
            for name in m["learned_files"]:
                target = local / name
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(home / name, target)
            learned = dict(m["learned_files"])
            binaries = m["binaries"][arm["binaries"]]
            settings["disableAllHooks"] = False
            command = shlex.join([m["preparation_python"], "-B", str(Path(__file__).resolve()), "hook", str(session / "manifest.json")])
            settings["hooks"] = {event: [dict({"hooks": [{"type": "command", "command": command, "timeout": 90}]},
                                              **({"matcher": matcher} if matcher else {}))] for event, matcher in EVENTS}
        pairs.write(session / "settings.json", settings)
        (session / ".reflex-fixture").write_text("development arms assignment\n")
        argv = ["claude", "--print", "--restricted", "--model", m["model"], "--max-budget-usd", str(SESSION_CAP_USD),
            "--max-turns", "60", "--output-format", "stream-json", "--verbose", "--include-hook-events",
            "--include-partial-messages", "--debug-file", str(session / "claude-debug.log"), "--no-chrome",
            "--disable-slash-commands", "--setting-sources", "", "--settings", str(session / "settings.json"),
            "--strict-mcp-config", "--mcp-config", '{"mcpServers":{}}', "--tools", "Read,Edit,Write,Glob,Grep,Bash",
            "--permission-mode", "dontAsk", "--permission-prompts", "none", row["prompt"]]
        hidden = root / "private/hidden-identities" / (row["task"] + ".json")
        frozen = {str(local / n): d for n, d in learned.items()}
        frozen.update({str(p): pairs.sha(p) for p in (root / "manifest.json", review, session / "settings.json", hidden,
                                                       *map(Path, binaries.values()))})
        value = dict(protocol=PROTOCOL, root=str(session), study_root=str(root), cohort_root=str(cohort),
            experiment_root=str(authority), execution_root=str(checkout), home=str(local),
            # real_repo_pairs.hook runs adage-exec for "automatic"; A0 never runs a hook.
            arm="automatic" if arm["hooks"] else "agent", dev_arm=row["arm"], card_checks=arm["card_checks"],
            condition=row["condition"], block=row["block"], task=row["task"], order=row["order"],
            route=m["route"], model=m["model"], argv=cohort_v5.startup_argv(argv), binaries=binaries,
            learned_files=learned, sources=m["source_sha256"], frozen=frozen,
            initial_source=pairs.tree_state(checkout), budget_usd=SESSION_CAP_USD, project_environment=str(checkout / ".venv"),
            expected_claude_version=pairs.probe.EXPECTED_VERSION, budget_allocation_sha256=m["budget_allocation_sha256"],
            native_review_path=str(review), native_review_sha256=pairs.sha(review), hidden_identities=str(hidden),
            matching_command_template=m["command"])
        gate.validate_authority(value)
        gate.budget_limit(value)
        pairs.write(session / "manifest.json", value)
        (session / "manifest.sha256").write_text(pairs.sha(session / "manifest.json") + "\n")
        out.append(str(session / "manifest.json"))
    return dict(assignments=out, paid_calls=0, grants_written=False)


def granted(m):
    return [row for row in m["matrix"] if ARMS[row["arm"]]["hooks"]]


def approval_script(root):
    """A person-run script: E2 opt-in for the hook arms' homes only, auto approval off, sandbox
    required, each granted with its own arm's binary."""
    m = verify(root)
    track, reflex = m["route"].rsplit("/", 1)
    rows = granted(m)
    lines = ["#!/bin/sh", "set -eu",
             "printf '%%s\\n' 'Development arms: enable E2 only in %d new temporary homes and approve one learned pytest verification.'" % len(rows),
             "printf '%s\\n' 'Sandbox required; automatic approval off; declared pytest/bytecode caches only; no global changes.'"]
    caches = " ".join("--cache " + shlex.quote(c) for c in m["caches"])
    for row in rows:
        session = session_dir(root, row)
        home, checkout = session / "adage-home", session / "repo"
        p = shlex.quote(str(home / "policy.toml"))
        binary = m["binaries"][ARMS[row["arm"]]["binaries"]]["adage"]
        lines += ["test -d %s && test -f %s || { echo 'Prepare the study first.' >&2; exit 1; }" % (
                      shlex.quote(str(checkout)), shlex.quote(str(home / "index.json"))),
                  "test ! -e %s || { echo 'Grant already present; stop and review.' >&2; exit 1; }" % shlex.quote(str(home / "approvals.json")),
                  "if [ -e %s ]; then printf '%%s' %s | cmp -s - %s || { echo 'Existing policy differs; stop.' >&2; exit 1; }; "
                  "else printf '%%s' %s > %s; fi" % (p, shlex.quote(POLICY), p, shlex.quote(POLICY), p),
                  "ADAGE_HOME=%s %s review delegate %s %s --level e2 --effect verify %s --timeout-ms 30000 --repo %s --by launch-evaluation-human" % (
                      shlex.quote(str(home)), shlex.quote(binary), track, reflex, caches, shlex.quote(str(checkout)))]
    text = "\n".join(lines) + "\n"
    path = root / "approve-dev-arms.sh"
    if path.exists() and path.read_text() != text:
        raise ValueError("approval script already written with different content")
    path.write_text(text)
    return dict(path=str(path), sha256=pairs.sha(path), homes=len(rows))


def verify_grants(root):
    """Read-only: every hook arm's home holds exactly the reviewed grant and policy; A0 holds none."""
    m = verify(root)
    problems = []
    for row in m["matrix"]:
        home = session_dir(root, row) / "adage-home"
        if not ARMS[row["arm"]]["hooks"]:
            if (home / "approvals.json").exists():
                problems.append(dict(order=row["order"], problem="the no-hook arm must hold no grant"))
            continue
        try:
            store = pairs.read(home / "approvals.json")
            policy = (home / "policy.toml").read_text()
        except OSError as error:
            problems.append(dict(order=row["order"], problem=str(error)))
            continue
        grants = store.get("approvals", [])
        g = grants[0] if len(grants) == 1 else {}
        if (len(grants) != 1 or g.get("route") != m["route"] or g.get("by") != "launch-evaluation-human"
                or g.get("auto") is not False or g["key"].get("level") != "e2" or g["key"].get("effect") != "verify"
                or g["key"].get("argv_templates") != [shlex.split(m["command"])]
                or g.get("terms", {}).get("cache") != m["caches"] or g["terms"].get("timeout_ms") != 30000
                or policy != POLICY):
            problems.append(dict(order=row["order"], problem="grant or policy differs from the reviewed script"))
    return dict(passed=not problems, problems=problems, checked=len(m["matrix"]))


def hook(manifest, payload):
    m = pairs.read(manifest)
    if m["arm"] != "automatic":
        raise ValueError("the no-hook arm never runs Adage")
    if m.get("card_checks") is False:
        import os
        os.environ["ADAGE_CARD_CHECKS"] = "0"
    return pairs.hook(manifest, payload)


def requests_of(stream):
    """Distinct model requests: assistant message ids (1:1 with request ids in these streams)."""
    ids = []
    for row in stream:
        if row.get("type") == "assistant":
            mid = (row.get("message") or {}).get("id")
            if mid and mid not in ids:
                ids.append(mid)
    return ids


def pytest_requests(stream):
    return sum(1 for row in stream if row.get("type") == "assistant"
               for b in row.get("message", {}).get("content", [])
               if b.get("type") == "tool_use" and b.get("name") == "Bash" and "pytest" in (b.get("input", {}).get("command") or ""))


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
    stream = attempt("stream", lambda: canary.read_jsonl(session / "stdout.jsonl"), [])
    hooks = canary.read_jsonl(session / "production-hooks.jsonl") if m["arm"] == "automatic" else []
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
    run = attempt("process", lambda: pairs.read(session / "run.json"), {})
    rerun, wider = canary.CONDITIONS[m["condition"]][1:]
    timeline, replay = {}, {}
    if hooks:
        timeline = attempt("timeline", lambda: receipt_replay.extract(session / "production-hooks.jsonl", m["dev_arm"]), {})
        replay = attempt("replay", lambda: receipt_replay.replay(timeline, rerun, wider), {})
    usage = summary.get("usage") or {}
    result = dict(protocol=PROTOCOL, dev_arm=m["dev_arm"], condition=m["condition"], block=m["block"], task=m["task"],
        order=m["order"], provenance=provenance, quality=quality, cost_usd=summary.get("total_cost_usd"),
        requests=len(requests_of(stream)), pytest_requests=pytest_requests(stream),
        tokens=dict(input=usage.get("input_tokens"), cache_write=usage.get("cache_creation_input_tokens"),
                    cache_read=usage.get("cache_read_input_tokens"), output=usage.get("output_tokens")),
        wall_seconds=((run.get("end_monotonic_ns") or 0) - (run.get("start_monotonic_ns") or 0)) / 1e9 or None,
        timeline=timeline, replay=replay, all_journal_receipts=attempt("journal", lambda: pairs.journal(m), []) if hooks else [],
        stop_gate_outputs=[receipt_replay.hook_text(h.get("output")) for h in hooks if h.get("event") == "Stop" and h.get("output")],
        contract_emitted=any('kind=\\"execution-contract\\"' in (h.get("output") or "") for h in hooks if h.get("event") == "SessionStart"),
        final_generation=final["digest"], collection_errors=errors)
    pairs.write(session / "collection.json", result)
    return result


def live(root, wall_seconds=600):
    """Dispatch every assignment in the frozen order, one at a time, inside the shared ledger."""
    root = root.resolve(strict=True)
    m = verify(root)
    if not verify_grants(root)["passed"]:
        raise ValueError("person-issued grants are missing or differ from the reviewed script")
    canary.isolation(m)
    authority = Path(m["budget_authority"])
    for row in m["matrix"]:
        a = pairs.read(session_dir(root, row) / "manifest.json")
        pairs.verify(a)
        gate.validate_authority(a)
        gate.require_native_review(a)
        if pairs.tree_state(Path(a["execution_root"])) != a["initial_source"]:
            raise ValueError("prepared checkout changed before dispatch")
    results = []
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
                raise ValueError("study ceiling reached; stop and report")
            budget = pairs.Budget(state, gate.budget_limit(a), SESSION_CAP_USD, persist=lambda: pairs.write(state_path, state))
            attempt, reason = budget.reserve(key)
            if attempt is None:
                raise ValueError(reason)
            state["runs"][key] = dict(status="reserved", task=a["task"], arm=a["dev_arm"], condition=a["condition"], protocol=PROTOCOL)
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
                pairs.append(authority / "attempts.jsonl", dict(status="dev-arms-finished", run=key,
                    accounting=attempt, evidence=state["runs"][key], timestamp_ns=time.time_ns()))
        results.append(dict(order=row["order"], status=state["runs"][key]["status"]))
    return results


def mean(values):
    values = [v for v in values if isinstance(v, (int, float))]
    return round(statistics.mean(values), 4) if values else None


def summarize(rows, protocol):
    """Per-arm positive metrics, paired block deltas against A0, controls, and the registered
    development thresholds (`dev_thresholds` in the protocol JSON)."""
    t = protocol["dev_thresholds"]
    positives = [r for r in rows if r["condition"] == "positive" and r.get("status") != "uncollected"]
    by_arm = {arm: [r for r in positives if r["dev_arm"] == arm] for arm in ARMS}
    arms = {}
    for arm, rs in by_arm.items():
        arms[arm] = dict(n=len(rs), requests=mean([r["requests"] for r in rs]),
            requests_each=[r["requests"] for r in sorted(rs, key=lambda r: r["block"])],
            cost_usd=mean([r.get("cost_usd") for r in rs]), wall_seconds=mean([r.get("wall_seconds") for r in rs]),
            tokens={k: mean([(r.get("tokens") or {}).get(k) for r in rs]) for k in ("input", "cache_write", "cache_read", "output")},
            pytest_requests=mean([r.get("pytest_requests") for r in rs]),
            quality_failures=sum((r.get("quality") or {}).get("passed") is not True for r in rs),
            sessions_with_repeat_attempts=sum((r.get("replay") or {}).get("repeat_attempts", 0) > 0 for r in rs),
            stop_gate_runs=sum(bool(r.get("stop_gate_outputs")) for r in rs))
    blocks = {}
    for arm in ARMS:
        if arm == "A0":
            continue
        deltas = []
        for block in range(1, BLOCKS + 1):
            base = [r for r in by_arm["A0"] if r["block"] == block]
            mine = [r for r in by_arm[arm] if r["block"] == block]
            if base and mine:
                deltas.append(base[0]["requests"] - mine[0]["requests"])
        blocks[arm] = dict(request_deltas=deltas, blocks_fewer=sum(d > 0 for d in deltas))
    controls = [r for r in rows if r["condition"] != "positive" and r.get("status") != "uncollected"]
    skipped = [r["order"] for r in controls if (r.get("replay") or {}).get("necessary_work_skipped")]
    verdict = {}
    a0 = arms["A0"]
    for arm in ("A2", "A3"):
        a = arms[arm]
        ratio = (a["requests"] / a0["requests"]) if a["requests"] and a0["requests"] else None
        verdict[arm] = dict(
            requests_ratio_to_A0=round(ratio, 4) if ratio else None,
            D1_requests=bool(ratio is not None and ratio <= t["D1_requests_max_ratio_to_A0"]
                             and blocks[arm]["blocks_fewer"] >= t["D1_blocks_fewer_min"]),
            D2_quality=a["quality_failures"] <= a0["quality_failures"],
            D3_safety=not [r for r in controls if r["dev_arm"] == arm and (r.get("replay") or {}).get("necessary_work_skipped")],
            D4_repeat_sessions=a["sessions_with_repeat_attempts"] <= t["D4_sessions_with_repeat_attempts_max"])
        verdict[arm]["passed"] = all(v for k, v in verdict[arm].items() if k.startswith("D"))
    return dict(arms=arms, paired_blocks=blocks, controls=[{k: r.get(k) for k in ("order", "dev_arm", "condition", "requests", "cost_usd")} |
                dict(necessary_work_skipped=(r.get("replay") or {}).get("necessary_work_skipped"),
                     stop_gate_outputs=r.get("stop_gate_outputs")) for r in controls],
                necessary_work_skipped=skipped, verdict=verdict)


def report(root):
    root = root.resolve(strict=True)
    m = verify(root)
    rows = []
    for row in m["matrix"]:
        path = session_dir(root, row) / "collection.json"
        rows.append(pairs.read(path) if path.exists() else dict(row, dev_arm=row["arm"], status="uncollected"))
    costs = [pairs.known_cost(r.get("cost_usd")) for r in rows]
    result = dict(protocol=PROTOCOL, assignments=len(rows), collected=sum(r.get("status") != "uncollected" for r in rows),
        known_cost_usd=sum(c for c in costs if c is not None), unknown_costs=sum(c is None for c in costs),
        **summarize(rows, json.loads(canary.GATE.read_text())),
        limitations=["Development task and a guided learned route; not held-out evidence.",
                     "Model requests are distinct assistant message ids; tokens and cost are Claude Code's own accounting.",
                     "CPU runs (receipts, Stop gate) are reported separately and are never inference saved; nothing here measures GPU work."])
    pairs.write(root / "reports" / ("dev-arms-" + str(time.time_ns()) + ".json"), result)
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
    parser.add_argument("--rev7-binaries", type=Path)
    parser.add_argument("--audit-authority", type=Path, action="append", default=[])
    args = parser.parse_args()
    if args.command == "hook":
        print(hook(args.path, json.load(sys.stdin)), end="")
        sys.exit(0)
    if args.command == "freeze":
        value = freeze(args.path, args.cohort, args.cumulative, args.controls, args.binaries, args.rev7_binaries,
                       args.audit_authority)
    elif args.command == "collect":
        value = collect(args.path)
    else:
        value = {"prepare": prepare, "approval-script": approval_script, "verify-grants": verify_grants,
                 "live": live, "report": report}[args.command](args.path.resolve(strict=True))
    print(json.dumps(value, indent=2, default=str))
