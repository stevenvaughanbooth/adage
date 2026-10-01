#!/usr/bin/env python3
"""Held-out cohort runner, launch-gate-1 revision 9 (draft): Marshmallow, arms A0 / NOEXEC / BEST.

  A0      no Adage hooks;
  NOEXEC  the production hook registration with BEST's frozen binaries, learned home and card setting,
          and no grant and no E2 policy: no contract, no CPU execution;
  BEST    the development design selected by `heldout_analysis.select_best` from the development
          canary-v8 report, built from the final pre-freeze commit (which includes generic
          correctness fixes made before any held-out run), with one person-issued E2 grant.

The matrix is `heldout_analysis.matrix`: five safety controls (BEST, development task, the validated
canary suffixes) and then 20 paired blocks (four held-out tasks x five repetitions, seeded arm
order). Prompts are the frozen task material of `heldout_tasks` (results/20261001-heldout-tasks).

`freeze`, `prepare`, `approval-script`, `verify-grants`, `collect` and `report` spend nothing; `live`
is the paid entry point (probe.execute refuses nested agent sessions). No helper writes grants or
policy. The build commit and binary hashes are inputs to `freeze`, never constants here.
"""
from __future__ import annotations

import argparse
from contextlib import contextmanager
import json
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import sys
import time

import cohort_v5
import dev_arms
import heldout_analysis as analysis
import heldout_tasks
import launch_canary as canary
import real_repo_pairs as pairs
import receipt_replay

gate = pairs.substrate
HERE = Path(__file__).resolve().parent
PROTOCOL = "heldout-cohort-1"
TASK_MANIFEST = HERE / "results/20261001-heldout-tasks/manifest.json"
HOOKS = {"A0": False, "NOEXEC": True, "BEST": True}
# real_repo_pairs.hook runs adage-exec only for "automatic"; NOEXEC runs the plain adage binary.
RUNTIME_ARM = {"A0": "agent", "NOEXEC": "noexec", "BEST": "automatic"}
EVENTS = dev_arms.EVENTS
POLICY = dev_arms.POLICY
SESSION_CAP_USD = analysis.SESSION_CAP_USD
STUDY_CEILING_USD = analysis.CEILING_USD
LEDGER_FLOOR_USD = 3.0
MAX_TURNS = analysis.MAX_TURNS


def sources():
    files = [Path(__file__), Path(analysis.__file__), Path(heldout_tasks.__file__)]
    return {**dev_arms.sources(), **{str(p.resolve()): pairs.sha(p) for p in files}}


@contextmanager
def registry(root):
    """The scoring registry with the held-out tasks; the native sandbox's environment is restored."""
    before = dict(os.environ)
    try:
        modules = gate.modules(root)
        heldout_tasks.install(modules[0])
        os.environ.clear()
        os.environ.update(before)
        yield modules
    finally:
        os.environ.clear()
        os.environ.update(before)


def session_dir(root, row):
    what = row["condition"] if row["block"] is None else "b%02d-%s" % (row["block"], row["task"].split("--", 1)[1])
    return root / "sessions" / ("%02d-%s-%s" % (row["order"], row["arm"], what))


def design(dev_report_path):
    """BEST from the development report by the registered rule; None stops the held-out study."""
    report = pairs.read(dev_report_path)
    if report.get("protocol") != dev_arms.PROTOCOL:
        raise ValueError("the selection input must be a dev-arms-1 report")
    best = analysis.select_best(report)
    if best is None:
        raise ValueError("no development arm passed D1-D4; the held-out comparison is not run")
    return dict(arm=best, card_checks=dev_arms.ARMS[best]["card_checks"], label=dev_arms.ARMS[best]["label"],
                report=str(Path(dev_report_path).resolve()), report_sha256=pairs.sha(Path(dev_report_path)),
                rule="the arm passing D1-D4 with the lowest mean requests (ties: lower mean cost, then name)")


def git(*args):
    return subprocess.run(["git", *args], cwd=gate.REPO, capture_output=True, text=True)


def bind_binaries(binaries, build_commit, expected):
    """The frozen binary set: exact hashes given at freeze, built from `build_commit`, whose production
    sources must equal this checkout's (clean) production sources."""
    if set(expected) != {"adage", "adage-exec"}:
        raise ValueError("expected hashes for exactly adage and adage-exec")
    actual = {name: pairs.sha(Path(binaries) / name) for name in expected}
    if actual != expected:
        raise ValueError("binary hashes differ from the hashes given at freeze: %s" % actual)
    commit = git("rev-parse", "--verify", build_commit + "^{commit}")
    if commit.returncode:
        raise ValueError("build commit is not a commit in this repository")
    commit = commit.stdout.strip()
    if git("merge-base", "--is-ancestor", commit, "HEAD").returncode:
        raise ValueError("the runner checkout must contain the build commit")
    production = ("crates", "Cargo.toml", "Cargo.lock")
    if git("diff", "--quiet", commit, "HEAD", "--", *production).returncode:
        raise ValueError("production sources changed after the build commit; rebuild from this checkout")
    if git("status", "--porcelain", "--", *production).stdout.strip():
        raise ValueError("production sources differ from the commit; build binaries from a clean tree")
    return commit, actual


def check_heldout_controls(controls):
    """Validated held-out controls whose task material equals the committed, frozen manifest."""
    frozen = pairs.read(TASK_MANIFEST)
    validation = pairs.read(controls / "validation.json")
    m = pairs.read(controls / "manifest.json")
    if m.get("protocol") != heldout_tasks.PROTOCOL or not (validation.get("complete") and validation.get("valid")):
        raise ValueError("validated held-out task controls required (heldout_tasks.py validate)")
    for task in heldout_tasks.EVALUATION:
        for name in ("prompt.txt", "hidden.py"):
            if pairs.sha(controls / "private/tasks" / task / name) != frozen["tasks"][task]["files"][name]:
                raise ValueError("held-out task material differs from the frozen manifest: %s/%s" % (task, name))
    return m, validation


def prompts(root, rows):
    """Blocks: the frozen held-out prompt (it ends with the realism line). Controls: the development
    prompt, the same realism line, then the validated canary suffix, which alone realizes the control."""
    for row in rows:
        base = (root / "private/tasks" / row["task"] / "prompt.txt").read_text()
        if row["condition"] != "positive":
            base += heldout_tasks.REALISM + canary.CONDITIONS[row["condition"]][0]
        row["prompt"] = base
    return rows


def freeze(root, cohort, cumulative, dev_controls, heldout_controls, dev_report, binaries, build_commit,
           binary_sha256, authorities):
    """Freeze the design, matrix, prompts, learned home, binaries and sources. Spends nothing."""
    root, cohort = Path(root).resolve(), Path(cohort).resolve(strict=True)
    cumulative = Path(cumulative).resolve(strict=True)
    dev_controls, heldout_controls = Path(dev_controls).resolve(strict=True), Path(heldout_controls).resolve(strict=True)
    if cohort not in root.parents:
        raise ValueError("the study must nest within its native-confinement cohort")
    best = design(dev_report)
    commit, digests = bind_binaries(binaries, build_commit, binary_sha256)
    parent = pairs.read(cohort / "manifest.json")
    authority = Path(parent["budget_authority"])
    record = pairs.read(cumulative / "attempt.json")
    home = cumulative / "adage-home"
    for name, digest in record["artifact_sha256"].items():
        if pairs.sha(pairs.safe_file(home, name)) != digest:
            raise ValueError("learned artifact changed")
    routes = {r["track"] + "/" + r["route"]["id"]: r for r in pairs.read(home / "index.json")["routes"]}
    if routes.get(canary.ROUTE, {}).get("route", {}).get("step", {}).get("command") != canary.COMMAND:
        raise ValueError("fixed learned verification route missing")
    guided = pairs.read(cumulative.parent / "manifest.json")
    plan = dict(guided["families"][0], train=[], heldout=list(heldout_tasks.EVALUATION))
    old, old_validation = pairs.read(dev_controls / "manifest.json"), pairs.read(dev_controls / "validation.json")
    if not (old_validation.get("complete") and old_validation.get("valid")):
        raise ValueError("validated development task controls required")
    held, _ = check_heldout_controls(heldout_controls)
    held_plan = held["families"][0]
    if held_plan["base_full"] != plan["base_full"] or held_plan["dependencies"] != plan["dependencies"]:
        raise ValueError("held-out controls were validated on another base or dependency set")
    authorities = sorted({str(authority), *(str(Path(a).resolve(strict=True)) for a in authorities)})
    audit = canary.isolation(dict(cumulative_root=str(cumulative), audit_authorities=authorities))
    root.mkdir(parents=True, exist_ok=False)
    pairs.write(root / "isolation-audit.json", audit)
    for relative in ("runtime", "cache", "bases"):
        (root / relative).symlink_to((authority / relative).resolve(strict=True), target_is_directory=True)
    (root / "private/validation").mkdir(parents=True)
    for controls in (dev_controls, heldout_controls):
        for run in sorted((controls / "private/validation").iterdir()):
            (root / "private/validation" / run.name).symlink_to(run.resolve(strict=True), target_is_directory=True)
    shutil.copy2(heldout_controls / "validation.json", root / "heldout-validation.json")
    task_sha = {}
    for task, source in [(analysis.CONTROL_TASK, dev_controls)] + [(t, heldout_controls) for t in heldout_tasks.EVALUATION]:
        shutil.copytree(source / "private/tasks" / task, root / "private/tasks" / task)
        task_sha[task] = {n: pairs.sha(root / "private/tasks" / task / n) for n in ("prompt.txt", "hidden.py")}
    if task_sha[analysis.CONTROL_TASK] != old["task_sha256"][analysis.CONTROL_TASK]:
        raise ValueError("validated development task material changed")
    rows = prompts(root, analysis.matrix(heldout_tasks.EVALUATION))
    (root / "bin").mkdir()
    bins = {}
    for name in ("adage", "adage-exec"):
        shutil.copy2(Path(binaries) / name, root / "bin" / name)
        bins[name] = str(root / "bin" / name)
    if {n: pairs.sha(Path(p)) for n, p in bins.items()} != digests:
        raise ValueError("binary copy differs from the bound hashes")
    inputs = [root / "isolation-audit.json", cumulative / "attempt.json", cohort / "manifest.json",
              dev_controls / "manifest.json", dev_controls / "validation.json", heldout_controls / "manifest.json",
              heldout_controls / "validation.json", TASK_MANIFEST, Path(best["report"]),
              *(home / n for n in record["artifact_sha256"]), *map(Path, bins.values())]
    budget = analysis.budget(len(rows))
    if not budget["fits"]:
        raise ValueError("worst case exceeds the held-out ceiling")
    m = dict(protocol=PROTOCOL, root=str(root), cohort_root=str(cohort), budget_authority=str(authority),
             cumulative_root=str(cumulative), audit_authorities=authorities, gate_protocol_sha256=pairs.sha(canary.GATE),
             budget_allocation_sha256=parent.get("budget_allocation_sha256") or pairs.sha(authority / "evaluation-allocation.json"),
             preparation_python=guided["preparation_python"], python_requested=guided["python_requested"],
             model=guided["model"], families=[plan], matrix=rows, best=best, arms=dict(HOOKS),
             route=canary.ROUTE, command=canary.COMMAND, caches=list(canary.CACHES), learned_home=str(home),
             learned_files=record["artifact_sha256"],
             learned_provenance="explicitly guided demonstrations (cumulative-launch-10); fixed route, not new learning",
             binaries=bins, binary_sha256=digests, build_commit=commit,
             runner_commit=git("rev-parse", "HEAD").stdout.strip(), task_manifest_sha256=pairs.sha(TASK_MANIFEST),
             task_sha256=task_sha, source_sha256=sources(), frozen_inputs={str(p): pairs.sha(p) for p in inputs},
             budget=dict(sessions=len(rows), per_session_usd=SESSION_CAP_USD, study_ceiling_usd=STUDY_CEILING_USD,
                         worst_case_usd=budget["worst_case_usd"], ledger_floor_usd=LEDGER_FLOOR_USD),
             stratum=analysis.STRATUM, evidence_class="held-out evaluation (launch-gate-1 revision 9)",
             grants_written=False)
    pairs.write(root / "manifest.json", m)
    (root / "manifest.sha256").write_text(pairs.sha(root / "manifest.json") + "\n")
    for source in m["source_sha256"]:
        target = root / "source" / Path(source).relative_to(gate.REPO)
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, target)
    return m


def verify(root):
    m = pairs.read(root / "manifest.json")
    if (m.get("protocol") != PROTOCOL or m["source_sha256"] != sources()
            or pairs.sha(root / "manifest.json") != (root / "manifest.sha256").read_text().strip()):
        raise ValueError("held-out runner source or manifest changed after freeze; preserve it and freeze a new root")
    for path, digest in m["frozen_inputs"].items():
        if pairs.sha(Path(path)) != digest:
            raise ValueError("frozen input changed: " + path)
    return m


def assignment(m, row, session, checkout, local, settings_base, hidden, review):
    """One session's settings, argv and manifest value. Pure apart from hashing existing files."""
    arm = row["arm"]
    settings, binaries, learned = dict(settings_base), {}, {}
    if HOOKS[arm]:
        binaries, learned = dict(m["binaries"]), dict(m["learned_files"])
        settings["disableAllHooks"] = False
        command = shlex.join([m["preparation_python"], "-B", str(Path(__file__).resolve()), "hook", str(session / "manifest.json")])
        settings["hooks"] = {event: [dict({"hooks": [{"type": "command", "command": command, "timeout": 90}]},
                                          **({"matcher": matcher} if matcher else {}))] for event, matcher in EVENTS}
    argv = ["claude", "--print", "--restricted", "--model", m["model"], "--max-budget-usd", str(SESSION_CAP_USD),
            "--max-turns", str(MAX_TURNS), "--output-format", "stream-json", "--verbose", "--include-hook-events",
            "--include-partial-messages", "--debug-file", str(session / "claude-debug.log"), "--no-chrome",
            "--disable-slash-commands", "--setting-sources", "", "--settings", str(session / "settings.json"),
            "--strict-mcp-config", "--mcp-config", '{"mcpServers":{}}', "--tools", "Read,Edit,Write,Glob,Grep,Bash",
            "--permission-mode", "dontAsk", "--permission-prompts", "none", row["prompt"]]
    value = dict(protocol=PROTOCOL, root=str(session), study_root=m["root"], cohort_root=m["cohort_root"],
                 experiment_root=m["budget_authority"], execution_root=str(checkout), home=str(local),
                 arm=RUNTIME_ARM[arm], heldout_arm=arm, card_checks=m["best"]["card_checks"] if HOOKS[arm] else None,
                 condition=row["condition"], block=row["block"], repetition=row["repetition"], task=row["task"],
                 order=row["order"], route=m["route"], model=m["model"], argv=argv, binaries=binaries,
                 learned_files=learned, sources=m["source_sha256"], budget_usd=SESSION_CAP_USD,
                 project_environment=str(checkout / ".venv"), expected_claude_version=pairs.probe.EXPECTED_VERSION,
                 budget_allocation_sha256=m["budget_allocation_sha256"], native_review_path=str(review),
                 hidden_identities=str(hidden), matching_command_template=m["command"])
    return settings, value


def prepare(root):
    """Paired-block checkouts, homes, settings and manifests. Spends nothing; writes no grant."""
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
        with registry(root):
            pairs.prepare_hidden_expectations(root, row["task"])
        gate.create_checkout(root, plan["repo"], plan["base_full"], checkout)
        pairs.frozen_setup(root, checkout, plan, m["python_requested"])
        hidden = root / "private/hidden-identities" / (row["task"] + ".json")
        settings, value = assignment(m, row, session, checkout, local,
                                     cohort_v5.policy(authority, checkout, root / "runtime/python"), hidden, review)
        for name in value["learned_files"]:
            target = local / name
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(home / name, target)
        pairs.write(session / "settings.json", settings)
        (session / ".reflex-fixture").write_text("held-out assignment\n")
        frozen = {str(local / n): d for n, d in value["learned_files"].items()}
        frozen.update({str(p): pairs.sha(p) for p in (root / "manifest.json", review, session / "settings.json", hidden,
                                                       *map(Path, value["binaries"].values()))})
        value.update(argv=cohort_v5.startup_argv(value["argv"]), frozen=frozen, initial_source=pairs.tree_state(checkout),
                     native_review_sha256=pairs.sha(review))
        gate.validate_authority(value)
        gate.budget_limit(value)
        pairs.write(session / "manifest.json", value)
        (session / "manifest.sha256").write_text(pairs.sha(session / "manifest.json") + "\n")
        out.append(str(session / "manifest.json"))
    return dict(assignments=out, paid_calls=0, grants_written=False)


def granted(m):
    return [row for row in m["matrix"] if row["arm"] == "BEST"]


def approval_script(root):
    """A person-run script: E2 opt-in and one grant for BEST's homes only (controls and blocks).
    NOEXEC and A0 homes get nothing."""
    m = verify(root)
    track, reflex = m["route"].rsplit("/", 1)
    rows = granted(m)
    lines = ["#!/bin/sh", "set -eu",
             "printf '%%s\\n' 'Held-out study: enable E2 only in %d new temporary BEST homes and approve one learned pytest verification.'" % len(rows),
             "printf '%s\\n' 'NOEXEC and A0 homes get no grant and no policy. Sandbox required; automatic approval off; declared caches only.'"]
    caches = " ".join("--cache " + shlex.quote(c) for c in m["caches"])
    for row in rows:
        session = session_dir(root, row)
        home, checkout = session / "adage-home", session / "repo"
        p = shlex.quote(str(home / "policy.toml"))
        lines += ["test -d %s && test -f %s || { echo 'Prepare the study first.' >&2; exit 1; }" % (
                      shlex.quote(str(checkout)), shlex.quote(str(home / "index.json"))),
                  "test ! -e %s || { echo 'Grant already present; stop and review.' >&2; exit 1; }" % shlex.quote(str(home / "approvals.json")),
                  "if [ -e %s ]; then printf '%%s' %s | cmp -s - %s || { echo 'Existing policy differs; stop.' >&2; exit 1; }; "
                  "else printf '%%s' %s > %s; fi" % (p, shlex.quote(POLICY), p, shlex.quote(POLICY), p),
                  "ADAGE_HOME=%s %s review delegate %s %s --level e2 --effect verify %s --timeout-ms 30000 --repo %s --by launch-evaluation-human" % (
                      shlex.quote(str(home)), shlex.quote(m["binaries"]["adage"]), track, reflex, caches, shlex.quote(str(checkout)))]
    text = "\n".join(lines) + "\n"
    path = root / "approve-heldout.sh"
    if path.exists() and path.read_text() != text:
        raise ValueError("approval script already written with different content")
    path.write_text(text)
    return dict(path=str(path), sha256=pairs.sha(path), homes=len(rows))


def verify_grants(root):
    """Read-only: BEST homes hold exactly the reviewed grant and policy; NOEXEC and A0 hold neither."""
    m = verify(root)
    problems = []
    for row in m["matrix"]:
        home = session_dir(root, row) / "adage-home"
        if row["arm"] != "BEST":
            if (home / "approvals.json").exists() or (home / "policy.toml").exists():
                problems.append(dict(order=row["order"], problem="%s must hold no grant and no policy" % row["arm"]))
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
                or g.get("auto") is not False or g.get("key", {}).get("level") != "e2"
                or g["key"].get("effect") != "verify" or g["key"].get("argv_templates") != [shlex.split(m["command"])]
                or g.get("terms", {}).get("cache") != m["caches"] or g["terms"].get("timeout_ms") != 30000
                or policy != POLICY):
            problems.append(dict(order=row["order"], problem="grant or policy differs from the reviewed script"))
    return dict(passed=not problems, problems=problems, checked=len(m["matrix"]))


def hook(manifest, payload):
    m = pairs.read(manifest)
    if m["arm"] == "agent":
        raise ValueError("A0 never runs Adage")
    if m.get("card_checks") is False:
        os.environ["ADAGE_CARD_CHECKS"] = "0"
    else:
        os.environ.pop("ADAGE_CARD_CHECKS", None)
    return pairs.hook(manifest, payload)


def collect(manifest):
    """Final source, independent hidden-test quality, hook timeline and receipt replay. Immutable once written."""
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
        except Exception as error:  # noqa: BLE001 - every failure is recorded, never hidden
            errors.append(dict(stage=label, error=str(error)))
            return fallback
    final = pairs.tree_state(checkout)
    pairs.write(session / "final-source.json", final)
    (session / "final-tracked.patch").write_bytes(subprocess.run(["git", "diff", "--binary", "HEAD"],
        cwd=checkout, capture_output=True, check=True).stdout)
    provenance = attempt("native-transcript", lambda: gate.retain_transcript(session, checkout), {})
    hooks = canary.read_jsonl(session / "production-hooks.jsonl") if m["arm"] != "agent" else []

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
        with registry(root):
            return pairs.score_with_integrity(root, m["task"], scoring, pairs.read(m["hidden_identities"]))
    quality = attempt("independent-quality", score, dict(passed=False, status="unknown"))
    summary = attempt("summary", lambda: pairs.read(session / "summary.json"), {})
    rerun, wider = canary.CONDITIONS[m["condition"]][1:] if m["condition"] != "positive" else (False, None)
    timeline, replay = {}, {}
    if hooks:
        timeline = attempt("timeline", lambda: receipt_replay.extract(session / "production-hooks.jsonl", m["heldout_arm"]), {})
        replay = attempt("replay", lambda: receipt_replay.replay(timeline, rerun, wider), {})
    result = dict(protocol=PROTOCOL, heldout_arm=m["heldout_arm"], condition=m["condition"], block=m["block"],
                  task=m["task"], order=m["order"], provenance=provenance, quality=quality,
                  cost_usd=summary.get("total_cost_usd"), timeline=timeline, replay=replay,
                  all_journal_receipts=attempt("journal", lambda: pairs.journal(m), []) if hooks else [],
                  contract_emitted=any('kind=\\"execution-contract\\"' in (h.get("output") or "") for h in hooks
                                       if h.get("event") == "SessionStart"),
                  final_generation=final["digest"], collection_errors=errors)
    pairs.write(session / "collection.json", result)
    return result


def rows_of(root, m):
    out = []
    for row in m["matrix"]:
        assignment_row = {k: row[k] for k in ("order", "arm", "condition", "block", "task", "repetition")}
        session = session_dir(root, row)
        out.append(analysis.session_row(session, assignment_row) if session.is_dir()
                   else dict(assignment_row, status="uncollected"))
    return out


def control_gate(root, m):
    """Before any held-out block: all five controls collected, realized and safe (strict per session)."""
    checks = analysis.control_rows([r for r in rows_of(root, m) if r["condition"] != "positive"])
    return dict(passed=len(checks) == len(analysis.CONTROLS)
                and all(c["realized"] and not c["necessary_work_skipped"] for c in checks), controls=checks)


def live(root, wall_seconds=600):
    """Dispatch in the frozen order, one session at a time, inside the shared ledger. Controls first;
    any control failure stops the study before the first held-out block."""
    root = Path(root).resolve(strict=True)
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
        if row["condition"] == "positive" and row["block"] == 1 and not control_gate(root, m)["passed"]:
            raise ValueError("a safety control failed or was unrealized; no held-out block is dispatched")
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
            if spent + SESSION_CAP_USD > m["budget"]["study_ceiling_usd"] + 1e-9:
                raise ValueError("study ceiling reached; stop and report")
            budget = pairs.Budget(state, gate.budget_limit(a), SESSION_CAP_USD, persist=lambda: pairs.write(state_path, state))
            if budget.total - (budget.spent + SESSION_CAP_USD) < LEDGER_FLOOR_USD:
                raise ValueError("the ledger's remaining balance would fall below $3; stop and report")
            attempt, reason = budget.reserve(key)
            if attempt is None:
                raise ValueError(reason)
            state["runs"][key] = dict(status="reserved", task=a["task"], arm=a["heldout_arm"], condition=a["condition"],
                                      protocol=PROTOCOL)
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
                pairs.append(authority / "attempts.jsonl", dict(status="heldout-finished", run=key, accounting=attempt,
                                                                evidence=state["runs"][key], timestamp_ns=time.time_ns()))
        results.append(dict(order=row["order"], status=state["runs"][key]["status"]))
    return results


def report(root):
    root = Path(root).resolve(strict=True)
    m = verify(root)
    rows = rows_of(root, m)
    result = dict(analysis.analyze(rows, m["best"]["arm"], route=m["route"]), best=m["best"], build_commit=m["build_commit"],
                  binary_sha256=m["binary_sha256"], assignments=len(rows),
                  collected=sum(r.get("status") == "collected" for r in rows))
    pairs.write(root / "reports" / ("heldout-" + str(time.time_ns()) + ".json"), result)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("command", choices=("freeze", "prepare", "approval-script", "verify-grants", "hook", "live",
                                            "collect", "report"))
    parser.add_argument("path", type=Path)
    parser.add_argument("--cohort", type=Path)
    parser.add_argument("--cumulative", type=Path)
    parser.add_argument("--dev-controls", type=Path)
    parser.add_argument("--heldout-controls", type=Path)
    parser.add_argument("--dev-report", type=Path)
    parser.add_argument("--binaries", type=Path)
    parser.add_argument("--build-commit")
    parser.add_argument("--adage-sha256")
    parser.add_argument("--adage-exec-sha256")
    parser.add_argument("--audit-authority", type=Path, action="append", default=[])
    args = parser.parse_args()
    if args.command == "hook":
        print(hook(args.path, json.load(sys.stdin)), end="")
        sys.exit(0)
    if args.command == "freeze":
        needed = ("cohort", "cumulative", "dev_controls", "heldout_controls", "dev_report", "binaries", "build_commit",
                  "adage_sha256", "adage_exec_sha256")
        missing = [n for n in needed if getattr(args, n) is None]
        if missing:
            parser.error("freeze requires " + ", ".join("--" + n.replace("_", "-") for n in missing))
        value = freeze(args.path, args.cohort, args.cumulative, args.dev_controls, args.heldout_controls, args.dev_report,
                       args.binaries, args.build_commit, {"adage": args.adage_sha256, "adage-exec": args.adage_exec_sha256},
                       args.audit_authority)
    elif args.command == "collect":
        value = collect(args.path)
    else:
        value = {"prepare": prepare, "approval-script": approval_script, "verify-grants": verify_grants,
                 "live": live, "report": report}[args.command](args.path.resolve(strict=True))
    print(json.dumps(value, indent=2, default=str))


if __name__ == "__main__":
    main()
