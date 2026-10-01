#!/usr/bin/env python3
"""Click held-out runner, launch-gate-1 revision 13: arms A0 / NOEXEC / BEST, or A0 / NOEXEC advisory only.

  A0      no Adage hooks;
  NOEXEC  the production hook registration with BEST's frozen binaries, the guided learned home and
          BEST's card setting, and no grant and no E2 policy: no contract, no CPU execution (advisory
          mode: the card and instincts);
  BEST    A4 (verification ownership plus the orientation card; r9's rule on dev-v9b), on the PR #16
          head binaries, with one person-issued E2 grant for the route the learning step selected.

The mode comes from the learning step (`click_training.py learn`): with a selected route, `execution`
runs five BEST controls on the development task and then 20 paired blocks of the three arms; without
one, `advisory` runs 20 paired blocks of A0 and NOEXEC, and BEST and its controls are skipped, as
registered. The person may also freeze `--advisory-only` when execution mode's worst case does not fit
the allocation (recorded in the manifest; decided before any held-out dispatch).

If a BEST control is unrealized or unsafe, or a BEST grant lapses, BEST is dropped: no further BEST
session is dispatched, the execution outcome is protocol-failure, and A0 and NOEXEC continue so the
advisory co-primary is still evaluated (r13).

`freeze`, `prepare`, `approval-script`, `verify-grants`, `collect`, `report`, `budget-check` and
`attempted` spend nothing; `live` is the paid entry point (probe.execute refuses nested agent sessions).
No helper writes grants or policy.
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path
import shlex
import shutil
import subprocess
import sys
import time

import click_analysis as analysis
import click_tasks
import click_training
import click_writeup
import cohort_v5
import dev_arms
import dev_v9
import heldout_analysis
import heldout_cohort
import heldout_writeup
import launch_canary as canary
import real_repo_pairs as pairs
import receipt_replay
import split_audit

gate = pairs.substrate
HERE = Path(__file__).resolve().parent
PROTOCOL = "click-heldout-cohort-1"
TASK_MANIFEST = HERE / "results/20261001-click-tasks/manifest.json"
HOOKS = {"A0": False, "NOEXEC": True, "BEST": True}
RUNTIME_ARM = heldout_cohort.RUNTIME_ARM
EVENTS = dev_arms.EVENTS
POLICY = dev_arms.POLICY
SESSION_CAP_USD = analysis.SESSION_CAP_USD
LEDGER_FLOOR_USD = analysis.LEDGER_FLOOR_USD
MAX_TURNS = analysis.MAX_TURNS
TIMEOUT_MS = dev_v9.TIMEOUT_MS
GRANTOR = "launch-evaluation-human"


def sources():
    files = [Path(__file__), Path(analysis.__file__), Path(click_tasks.__file__), Path(click_training.__file__),
             Path(click_writeup.__file__), Path(heldout_cohort.__file__), Path(heldout_analysis.__file__),
             Path(heldout_writeup.__file__), Path(dev_v9.__file__), Path(split_audit.__file__)]
    return {**dev_arms.sources(), **{str(p.resolve()): pairs.sha(p) for p in files}}


registry = click_training.registry
session_dir = heldout_cohort.session_dir


def ledger_remaining(authority, allocation_sha=None):
    """The live ledger's remaining balance under the person's allocation (read-only)."""
    authority = Path(authority)
    state = pairs.read(authority / "paid-state.json")
    limit = gate.budget_limit(dict(experiment_root=str(authority),
                                   budget_allocation_sha256=allocation_sha or pairs.sha(authority / "evaluation-allocation.json")))
    b = pairs.Budget(json.loads(json.dumps(state)), limit, SESSION_CAP_USD)
    return dict(limit_usd=limit, spent_usd=round(b.spent, 7), remaining_usd=round(limit - b.spent, 7),
                attempts=len(state["accounting"]["attempts"]), blocked=state["accounting"].get("blocked_reason"))


def budget_check(authority, phase, mode=None, spent_already=0.0):
    """Read-only. `start`: the whole chain's worst case in advisory mode must fit (execution mode's is
    reported). `heldout`: the selected mode's held-out worst case must fit. Never changes a guard."""
    live = ledger_remaining(authority)
    plan = analysis.budget(live["remaining_usd"])
    if phase == "start":
        need = plan["modes"]["advisory"]["chain_worst_usd"]
        fits = need <= plan["available_usd"] + 1e-9
    else:
        need = plan["modes"][mode]["heldout_worst_usd"]
        fits = need <= plan["available_usd"] + 1e-9
    return dict(phase=phase, mode=mode, ledger=live, plan=plan, needed_usd=need, available_usd=plan["available_usd"],
                fits=fits, shortfall_usd=round(max(0.0, need - plan["available_usd"]), 7),
                note="raising the allocation is the person's decision: evaluation-allocation.json's ledger_ceiling_usd "
                     "and real_repo_gate.budget_limit's $60 limit; no agent changes either")


def attempted(authority, key):
    state = pairs.read(Path(authority) / "paid-state.json")
    return dict(key=key, attempted=key in state["runs"], status=(state["runs"].get(key) or {}).get("status"))


# ------------------------------------------------------------------------------------------ freeze

def check_controls(controls, plan):
    """Validated task controls whose material equals the committed, frozen task manifest."""
    frozen = pairs.read(TASK_MANIFEST)
    validation = pairs.read(controls / "validation.json")
    m = pairs.read(controls / "manifest.json")
    if (m.get("protocol") != click_tasks.PROTOCOL or not (validation.get("complete") and validation.get("valid"))
            or not validation.get("control_blocks", {}).get("valid")):
        raise ValueError("validated Click task controls required (click_tasks.py validate)")
    if pairs.sha(controls / "validation.json") != frozen["validation_sha256"]:
        raise ValueError("the controls' validation differs from the committed task manifest")
    for task in click_tasks.AUTHORED:
        for name in ("prompt.txt", "hidden.py"):
            if pairs.sha(controls / "private/tasks" / task / name) != frozen["tasks"][task]["files"][name]:
                raise ValueError("task material differs from the frozen manifest: %s/%s" % (task, name))
    held = m["families"][0]
    if held["base_full"] != plan["base_full"] or held["dependencies"] != plan["dependencies"]:
        raise ValueError("task controls were validated on another base or dependency set")
    return m, validation


def copy_identities(root, controls):
    """Hidden-test identities from the validation that built each gold checkout (prepare never runs a gold
    environment: they are bound to the validation's own runtime copy)."""
    out = []
    target = root / "private/hidden-identities"
    target.mkdir(parents=True, exist_ok=True)
    for task in click_tasks.AUTHORED:
        source = controls / "private/hidden-identities" / (task + ".json")
        value = pairs.read(source)
        if (value.get("hidden_sha256") != pairs.sha(root / "private/tasks" / task / "hidden.py")
                or not value.get("identities") or value.get("control_exit") != 0):
            raise ValueError("hidden-test identities for %s do not match the frozen hidden tests" % task)
        shutil.copy2(source, target / source.name)
        out.append(target / source.name)
    return out


def prompts(root, rows, command):
    """Blocks: the frozen held-out prompt (it ends with the realism line). Controls: the development prompt,
    the realism line, then the validated suffix, with the learned command in the rerun suffix."""
    for row in rows:
        base = (root / "private/tasks" / row["task"] / "prompt.txt").read_text()
        if row["condition"] != "positive":
            base += click_tasks.REALISM + click_tasks.CONDITIONS[row["condition"]][0].replace(click_tasks.COMMAND_SLOT, command)
        row["prompt"] = base
    return rows


def check_control_prompts(root, rows, command):
    """Each composed control prompt keeps the development prompt, one realism line before the work order,
    the intact suffix and its test block, and (rerun) the learned command."""
    dev = (root / "private/tasks" / click_tasks.CONTROL_TASK / "prompt.txt").read_text()
    out = []
    for row in rows:
        if row["condition"] == "positive":
            continue
        p = row["prompt"]
        suffix = click_tasks.CONDITIONS[row["condition"]][0].replace(click_tasks.COMMAND_SLOT, command)
        checks = dict(starts_with_development_prompt=p.startswith(dev), realism_once=p.count(click_tasks.REALISM.strip()) == 1,
                      realism_before_work_order=p.index(click_tasks.REALISM.strip()) < p.index("Work in this order"),
                      suffix_intact=p.endswith(suffix), tests_intact=click_tasks.TESTS in p,
                      command_named=(command in p) if row["condition"] == "rerun" else click_tasks.COMMAND_SLOT not in p,
                      no_commit_instruction="commit" not in suffix.lower())
        out.append(dict(condition=row["condition"], checks=checks, passed=all(checks.values())))
    if not all(r["passed"] for r in out):
        raise ValueError("a composed control prompt failed its structure check: %s" % out)
    return out


def learned(learning_path, dry_run=False):
    """The learning record, its hashes, and the selected route (if any)."""
    learning_path = Path(learning_path).resolve(strict=True)
    record = pairs.read(learning_path)
    if (learning_path.with_name("learning.sha256").read_text().strip() != pairs.sha(learning_path)
            or record.get("protocol") != click_training.PROTOCOL):
        raise ValueError("learning record changed or is not from the Click training protocol")
    if record.get("simulated") and not dry_run:
        raise ValueError("a simulated learning record is for the dry run only")
    if not record.get("audit_passed"):
        raise ValueError("the learned home failed the isolation audit; no held-out study uses it")
    root = Path(record["learning_root"])
    attempt = pairs.read(root / "attempt.json")
    home = root / "adage-home"
    for name, digest in attempt["artifact_sha256"].items():
        if pairs.sha(pairs.safe_file(home, name)) != digest:
            raise ValueError("learned artifact changed")
    return record, attempt, home


def freeze(root, cohort, learning_path, controls, dev_report, binaries, build_commit, binary_sha256, authorities,
           advisory_only=False, dry_run=False):
    """Freeze the mode, design, matrix, prompts, learned home, binaries and sources. Spends nothing."""
    root, cohort = Path(root).resolve(), Path(cohort).resolve(strict=True)
    controls = Path(controls).resolve(strict=True)
    if cohort not in root.parents:
        raise ValueError("the study must nest within its native-confinement cohort")
    canary.require_local([gate.REPO, Path(binaries), controls, Path(dev_report), Path(learning_path)])
    parent = click_training.verify_cohort(cohort)
    authority = Path(parent["budget_authority"])
    plan = dict(parent["families"][0], train=[], heldout=list(click_tasks.EVALUATION))
    if plan["family"] != click_tasks.FAMILY_ID:
        raise ValueError("the cohort's first family is not Click")
    record, attempt, home = learned(learning_path, dry_run)
    commit, digests = heldout_cohort.bind_binaries(binaries, build_commit, binary_sha256)
    if attempt["binary_sha256"] != digests["adage"]:
        raise ValueError("the learned home was conditioned by another adage binary")
    mode, why = record["mode"], record.get("rule")
    if mode == "execution" and advisory_only:
        mode, why = "advisory", "the person froze advisory only (execution mode's worst case did not fit the allocation)"
    best = heldout_cohort.design(dev_report) if mode == "execution" else None
    route = command = None
    if mode == "execution":
        route, command = record["route"], record["command"]
        routes = {r["track"] + "/" + r["route"]["id"]: r for r in pairs.read(home / "index.json")["routes"]}
        if routes.get(route, {}).get("route", {}).get("step", {}).get("command") != command:
            raise ValueError("selected learned route missing from the learned home")
        chosen, _ = click_training.select_route(pairs.read(home / "index.json"))
        if not chosen or chosen["route"] != route:
            raise ValueError("the registered selection rule does not pick the recorded route")
    held, _ = check_controls(controls, plan)
    authorities = sorted({str(authority), *(str(Path(a).resolve(strict=True)) for a in authorities)})
    audit = split_audit.audit(Path(record["learning_root"]), click_training.REPO, [Path(a) for a in authorities])
    if not audit["passed"]:
        raise ValueError("training-isolation audit failed: " + "; ".join(audit["problems"]))
    rows = analysis.matrix(click_tasks.EVALUATION, mode)
    worst = round(len(rows) * SESSION_CAP_USD, 4)
    budget = budget_check(authority, "heldout", mode)
    if not budget["fits"] and not dry_run:
        raise ValueError("the held-out worst case ($%.2f) exceeds the ledger's remaining balance less the $3 floor "
                         "($%.2f); short by $%.2f" % (worst, budget["available_usd"], budget["shortfall_usd"]))
    root.mkdir(parents=True, exist_ok=False)
    pairs.write(root / "isolation-audit.json", audit)
    for relative in ("runtime", "cache", "bases"):
        (root / relative).symlink_to((authority / relative).resolve(strict=True), target_is_directory=True)
    (root / "private/validation").mkdir(parents=True)
    for run in sorted((controls / "private/validation").iterdir()):
        (root / "private/validation" / run.name).symlink_to(run.resolve(strict=True), target_is_directory=True)
    shutil.copy2(controls / "validation.json", root / "task-validation.json")
    task_sha = {}
    for task in click_tasks.AUTHORED:
        shutil.copytree(controls / "private/tasks" / task, root / "private/tasks" / task)
        task_sha[task] = {n: pairs.sha(root / "private/tasks" / task / n) for n in ("prompt.txt", "hidden.py")}
    identities = copy_identities(root, controls)
    rows = prompts(root, rows, command or "")
    control_checks = check_control_prompts(root, rows, command) if mode == "execution" else []
    (root / "bin").mkdir()
    bins = {}
    for name in ("adage", "adage-exec"):
        shutil.copy2(Path(binaries) / name, root / "bin" / name)
        bins[name] = str(root / "bin" / name)
    if {n: pairs.sha(Path(p)) for n, p in bins.items()} != digests:
        raise ValueError("binary copy differs from the bound hashes")
    inputs = [root / "isolation-audit.json", Path(learning_path), Path(record["learning_root"]) / "attempt.json",
              cohort / "manifest.json", controls / "manifest.json", controls / "validation.json", TASK_MANIFEST,
              *identities, *(home / n for n in attempt["artifact_sha256"]), *map(Path, bins.values())]
    if best:
        inputs.append(Path(best["report"]))
    m = dict(protocol=PROTOCOL, revision=analysis.REVISION, root=str(root), cohort_root=str(cohort),
             budget_authority=str(authority), learning=str(Path(learning_path).resolve()), audit_authorities=authorities,
             gate_protocol_sha256=pairs.sha(canary.GATE),
             budget_allocation_sha256=parent.get("budget_allocation_sha256") or pairs.sha(authority / "evaluation-allocation.json"),
             preparation_python=parent["preparation_python"], python_requested=parent["python_requested"],
             model=cohort_v5.MODEL, families=[plan], matrix=rows, mode=mode, mode_reason=why, advisory_only_chosen=bool(advisory_only),
             best=best, arms={a: HOOKS[a] for a in analysis.ARMS[mode]}, route=route, command=command,
             caches=list(analysis.CACHES), learned_home=str(home), learned_files=attempt["artifact_sha256"],
             learned_provenance=record["provenance"], card_orientation=True, card_checks=None,
             binaries=bins, binary_sha256=digests, build_commit=commit,
             runner_commit=heldout_cohort.git("rev-parse", "HEAD").stdout.strip(), task_manifest_sha256=pairs.sha(TASK_MANIFEST),
             task_sha256=task_sha, control_prompt_checks=control_checks, source_sha256=sources(),
             frozen_inputs={str(p): pairs.sha(p) for p in inputs},
             budget=dict(sessions=len(rows), per_session_usd=SESSION_CAP_USD, study_ceiling_usd=worst,
                         worst_case_usd=worst, ledger_floor_usd=LEDGER_FLOOR_USD, at_freeze=budget),
             stratum=analysis.STRATUM, evidence_class="held-out evaluation (launch-gate-1 revision 13)",
             dry_run=bool(dry_run), grants_written=False)
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
        raise ValueError("Click runner source or manifest changed after freeze; preserve it and freeze a new root")
    for path, digest in m["frozen_inputs"].items():
        if pairs.sha(Path(path)) != digest:
            raise ValueError("frozen input changed: " + path)
    return m


# ------------------------------------------------------------------------------------- prepare

def assignment(m, row, session, checkout, local, settings_base, hidden, review):
    arm = row["arm"]
    settings, binaries, learned_files = dict(settings_base), {}, {}
    if HOOKS[arm]:
        binaries, learned_files = dict(m["binaries"]), dict(m["learned_files"])
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
                 arm=RUNTIME_ARM[arm], heldout_arm=arm, card_checks=m["card_checks"] if HOOKS[arm] else None,
                 card_orientation=m["card_orientation"] if HOOKS[arm] else None,
                 condition=row["condition"], block=row["block"], repetition=row["repetition"], task=row["task"],
                 order=row["order"], route=m["route"], model=m["model"], argv=argv, binaries=binaries,
                 learned_files=learned_files, sources=m["source_sha256"], budget_usd=SESSION_CAP_USD,
                 project_environment=str(checkout / ".venv"), expected_claude_version=pairs.probe.EXPECTED_VERSION,
                 budget_allocation_sha256=m["budget_allocation_sha256"], native_review_path=str(review),
                 hidden_identities=str(hidden), matching_command_template=m["command"] or "")
    return settings, value


def prepare(root):
    """Paired-block checkouts, homes, settings and manifests. Spends nothing; writes no grant. Resumable:
    an assignment already prepared (its manifest hash intact) is kept."""
    m = verify(root)
    cohort, authority = Path(m["cohort_root"]), Path(m["budget_authority"])
    review = cohort / "native-confinement-reviewed.json"
    gate.require_native_review(dict(experiment_root=str(authority), cohort_root=str(cohort),
                                    native_review_path=str(review), native_review_sha256=pairs.sha(review)))
    plan, home, out = m["families"][0], Path(m["learned_home"]), []
    for row in m["matrix"]:
        session = session_dir(root, row)
        if (session / "manifest.sha256").exists():
            pairs.verify(pairs.read(session / "manifest.json"))
            out.append(str(session / "manifest.json"))
            continue
        if session.exists():
            shutil.rmtree(session)  # a partly prepared, never-dispatched assignment (no manifest yet)
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
    state = root / "runtime-state.json"
    runtime = pairs.read(state) if state.exists() else canary.protect_runtime(root, m)
    return dict(assignments=len(out), mode=m["mode"], paid_calls=0, grants_written=False, runtime=runtime)


def granted(m):
    return [row for row in m["matrix"] if row["arm"] == "BEST"]


def approval_script(root):
    """A person-run script: E2 opt-in and one grant for BEST's homes only (controls and blocks). NOEXEC and
    A0 homes get nothing. Advisory mode has no grant at all."""
    m = verify(root)
    if m["mode"] != "execution":
        raise ValueError("advisory mode: there is no grant to issue")
    track, reflex = m["route"].rsplit("/", 1)
    rows = granted(m)
    lines = ["#!/bin/sh", "set -eu",
             "printf '%%s\\n' 'Click held-out study: enable E2 only in %d new temporary BEST homes and approve one learned pytest verification.'" % len(rows),
             "printf '%s\\n' " + shlex.quote("Route %s: %s" % (m["route"], m["command"])),
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
                  "ADAGE_HOME=%s %s review delegate %s %s --level e2 --effect verify %s --timeout-ms %d --repo %s --by %s" % (
                      shlex.quote(str(home)), shlex.quote(m["binaries"]["adage"]), track, reflex, caches, TIMEOUT_MS,
                      shlex.quote(str(checkout)), GRANTOR)]
    text = "\n".join(lines) + "\n"
    path = root / "approve-click.sh"
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
        if (len(grants) != 1 or g.get("route") != m["route"] or g.get("by") != GRANTOR
                or g.get("auto") is not False or g.get("key", {}).get("level") != "e2"
                or g["key"].get("effect") != "verify" or g["key"].get("argv_templates") != [shlex.split(m["command"])]
                or g.get("terms", {}).get("cache") != m["caches"] or g["terms"].get("timeout_ms") != TIMEOUT_MS
                or policy != POLICY):
            problems.append(dict(order=row["order"], problem="grant or policy differs from the reviewed script"))
    return dict(passed=not problems, problems=problems, checked=len(m["matrix"]), mode=m["mode"])


hook = heldout_cohort.hook


# ------------------------------------------------------------------------------- collect and live

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
    rerun, wider = click_tasks.CONDITIONS[m["condition"]][1:] if m["condition"] != "positive" else (False, None)
    timeline, replay = {}, {}
    if hooks:
        timeline = attempt("timeline", lambda: receipt_replay.extract(session / "production-hooks.jsonl", m["heldout_arm"]), {})
        replay = attempt("replay", lambda: receipt_replay.replay(timeline, rerun, wider), {})
    result = dict(protocol=PROTOCOL, heldout_arm=m["heldout_arm"], condition=m["condition"], block=m["block"],
                  task=m["task"], order=m["order"], provenance=provenance, quality=quality,
                  cost_usd=summary.get("total_cost_usd"), timeline=timeline, replay=replay,
                  all_journal_receipts=attempt("journal", lambda: pairs.journal(m), []) if hooks else [],
                  contract_emitted=any(canary.contract_in(h.get("output")) for h in hooks
                                       if h.get("event") == "SessionStart"),
                  exec_decisions=attempt("decisions", lambda: canary.exec_decisions(m["home"]), None)
                  if m["arm"] == "automatic" else [],
                  final_generation=final["digest"], collection_errors=errors)
    pairs.write(session / "collection.json", result)
    return result


def rows_of(root, m):
    out = []
    for row in m["matrix"]:
        assignment_row = {k: row[k] for k in ("order", "arm", "condition", "block", "task", "repetition")}
        session = session_dir(root, row)
        out.append(analysis.session_row(session, assignment_row) if (session / "collection.json").exists()
                   else dict(assignment_row, status="uncollected"))
    return out


def control_gate(root, m):
    """Before the first block: all five controls collected, realized and safe (strict per session)."""
    checks = heldout_analysis.control_rows([r for r in rows_of(root, m) if r["condition"] != "positive"])
    return dict(passed=len(checks) == len(analysis.CONTROLS)
                and all(c["realized"] and not c["necessary_work_skipped"] for c in checks), controls=checks)


def dropped(root):
    path = root / "BEST-DROPPED.json"
    return pairs.read(path) if path.exists() else None


def drop_best(root, order, reason):
    """No further BEST session runs; A0 and NOEXEC continue (r13). Written once."""
    path = root / "BEST-DROPPED.json"
    if not path.exists():
        pairs.write(path, dict(order=order, reason=reason, timestamp_ns=time.time_ns()))
    print(json.dumps(dict(best_dropped=reason, order=order)), flush=True)
    return pairs.read(path)


def runtime_problem(root, m):
    """Why the shared runtime no longer matches its protected state (None when it does). The same test
    as launch_canary.require_runtime, without stopping the study: here it drops BEST only."""
    path = root / "runtime-state.json"
    if not path.exists():
        return "the shared runtime was never protected; prepare protects it before the grants"
    state, lib = pairs.read(path), canary.runtime_lib(m)
    if str(lib) != state["lib"] or canary.tree_digest(lib) != state["sha256"]:
        return "the shared Python runtime changed after preparation; every grant that binds it has lapsed"
    if canary.writable(lib):
        return "the shared Python runtime is writable again"
    return None


def live(root, wall_seconds=600):
    """Dispatch in the frozen order, one session at a time, inside the shared ledger. Controls first; a
    failed control or a lapsed grant drops BEST, and A0 and NOEXEC continue."""
    root = Path(root).resolve(strict=True)
    m = verify(root)
    if m.get("dry_run"):
        raise ValueError("a dry-run root is never dispatched")
    if not verify_grants(root)["passed"]:
        raise ValueError("person-issued grants are missing or differ from the reviewed script")
    split_audit_result = split_audit.audit(Path(pairs.read(Path(m["learning"]))["learning_root"]), click_training.REPO,
                                           [Path(a) for a in m["audit_authorities"]])
    if not split_audit_result["passed"]:
        raise ValueError("training-isolation audit failed before dispatch")
    canary.require_local([*m["source_sha256"], *m["binaries"].values()])
    authority = Path(m["budget_authority"])
    for row in m["matrix"]:
        a = pairs.read(session_dir(root, row) / "manifest.json")
        pairs.verify(a)
        gate.validate_authority(a)
        gate.require_native_review(a)
    results = []
    for row in m["matrix"]:
        if row["arm"] == "BEST" and dropped(root):
            results.append(dict(order=row["order"], skipped="BEST dropped; never dispatched"))
            continue
        if (m["mode"] == "execution" and row["condition"] == "positive" and row["block"] == 1 and not dropped(root)):
            g = control_gate(root, m)
            if not g["passed"]:
                drop_best(root, row["order"], "a safety control was unrealized or skipped necessary work: %s" % json.dumps(g["controls"]))
                if row["arm"] == "BEST":
                    results.append(dict(order=row["order"], skipped="BEST dropped; never dispatched"))
                    continue
        manifest = session_dir(root, row) / "manifest.json"
        a = pairs.read(manifest)
        key = str(Path(a["root"]).relative_to(authority))
        if key in pairs.read(authority / "paid-state.json")["runs"]:
            results.append(dict(order=row["order"], skipped="already attempted; never retried"))
            continue
        if pairs.tree_state(Path(a["execution_root"])) != a["initial_source"]:
            raise ValueError("prepared checkout changed before dispatch: order %d" % row["order"])
        if a["arm"] == "automatic":
            # The runtime every grant binds is unchanged, and the grant still yields its contract at
            # SessionStart (dev-v8b ran 20 sessions after its grants lapsed). Either failing drops BEST.
            lapse = runtime_problem(root, m) or (None if canary.preflight_grant(a)
                                                 else "the grant no longer binds at SessionStart (no execution contract)")
            if lapse:
                drop_best(root, row["order"], lapse)
                results.append(dict(order=row["order"], skipped="BEST dropped; never dispatched"))
                continue
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
            if budget.total - (budget.spent + SESSION_CAP_USD) < LEDGER_FLOOR_USD - 1e-9:
                raise ValueError("the ledger's remaining balance would fall below $3; stop and report")
            attempt, reason = budget.reserve(key)
            if attempt is None:
                raise ValueError(reason)
            state["runs"][key] = dict(status="reserved", task=a["task"], arm=a["heldout_arm"], condition=a["condition"],
                                      protocol=PROTOCOL)
            pairs.write(state_path, state)
            result = collected = None
            try:
                if not budget.dispatch(attempt):
                    raise ValueError("accounting blocked dispatch")
                result = pairs.probe.execute(manifest, SESSION_CAP_USD, wall_seconds)
                budget.settle(attempt, result)
                state["runs"][key].update(status="complete", summary=result)
                collected = collect(manifest)
                state["runs"][key]["quality"] = collected["quality"]
            except BaseException as error:
                state["runs"][key].update(status="failed", error=str(error))
                raise
            finally:
                budget.settle(attempt, result)
                pairs.write(state_path, state)
                pairs.append(authority / "attempts.jsonl", dict(status="click-heldout-finished", run=key, accounting=attempt,
                                                                evidence=state["runs"][key], timestamp_ns=time.time_ns()))
        results.append(dict(order=row["order"], status=state["runs"][key]["status"]))
        print(json.dumps(results[-1]), flush=True)
        if a["arm"] == "automatic" and canary.lapsed(collected):
            drop_best(root, row["order"], "the grant lapsed during the session (%s)" % "; ".join(canary.integrity(collected)))
    return results


def lapses(root, m):
    out = []
    for row in m["matrix"]:
        path = session_dir(root, row) / "collection.json"
        if row["arm"] == "BEST" and path.exists() and canary.lapsed(pairs.read(path)):
            out.append(dict(order=row["order"], arm=row["arm"], condition=row["condition"],
                            problem="; ".join(p for p in canary.integrity(pairs.read(path)) if not p.startswith("no receipt"))))
    return out


def report(root):
    root = Path(root).resolve(strict=True)
    m = verify(root)
    rows = rows_of(root, m)
    lapsed = lapses(root, m)
    best_dropped = dropped(root)
    if lapsed and not best_dropped:
        best_dropped = dict(order=lapsed[0]["order"], reason="a BEST session ran with a lapsed grant")
    result = dict(analysis.analyze(rows, m["mode"], route=m["route"], best_dropped=best_dropped), best=m["best"],
                  mode_reason=m["mode_reason"], command=m["command"], build_commit=m["build_commit"],
                  binary_sha256=m["binary_sha256"], assignments=len(rows),
                  collected=sum(r.get("status") == "collected" for r in rows), lapses=lapsed,
                  learned_provenance=m["learned_provenance"])
    stamp = str(time.time_ns())
    pairs.write(root / "reports" / ("click-heldout-" + stamp + ".json"), result)
    (root / "reports" / ("click-heldout-" + stamp + ".md")).write_text(click_writeup.render(result))
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("command", choices=("freeze", "prepare", "approval-script", "verify-grants", "hook", "live",
                                            "collect", "report", "budget-check", "attempted", "mode"))
    parser.add_argument("path", type=Path)
    parser.add_argument("key", nargs="?")
    parser.add_argument("--cohort", type=Path)
    parser.add_argument("--learning", type=Path)
    parser.add_argument("--controls", type=Path)
    parser.add_argument("--dev-report", type=Path)
    parser.add_argument("--binaries", type=Path)
    parser.add_argument("--build-commit")
    parser.add_argument("--adage-sha256")
    parser.add_argument("--adage-exec-sha256")
    parser.add_argument("--audit-authority", type=Path, action="append", default=[])
    parser.add_argument("--advisory-only", action="store_true")
    parser.add_argument("--phase", choices=("start", "heldout"), default="start")
    parser.add_argument("--mode", choices=analysis.MODES)
    args = parser.parse_args()
    if args.command == "hook":
        print(hook(args.path, json.load(sys.stdin)), end="")
        sys.exit(0)
    if args.command == "freeze":
        needed = ("cohort", "learning", "controls", "dev_report", "binaries", "build_commit", "adage_sha256",
                  "adage_exec_sha256")
        missing = [n for n in needed if getattr(args, n) is None]
        if missing:
            parser.error("freeze requires " + ", ".join("--" + n.replace("_", "-") for n in missing))
        value = freeze(args.path, args.cohort, args.learning, args.controls, args.dev_report, args.binaries,
                       args.build_commit, {"adage": args.adage_sha256, "adage-exec": args.adage_exec_sha256},
                       args.audit_authority, advisory_only=args.advisory_only)
        value = {k: value[k] for k in ("root", "mode", "mode_reason", "route", "command", "budget", "build_commit",
                                       "binary_sha256", "best", "control_prompt_checks")}
    elif args.command == "collect":
        value = collect(args.path)
    elif args.command == "budget-check":
        value = budget_check(args.path, args.phase, args.mode)
        print(json.dumps(value, indent=2))
        sys.exit(0 if value["fits"] else 3)
    elif args.command == "attempted":
        value = attempted(args.path, args.key)
        print(json.dumps(value))
        sys.exit(0 if value["attempted"] else 1)
    elif args.command == "mode":
        record = pairs.read(args.path)
        print(record["mode"])
        sys.exit(0)
    else:
        value = {"prepare": prepare, "approval-script": approval_script, "verify-grants": verify_grants,
                 "live": live, "report": report}[args.command](args.path.resolve(strict=True))
    print(json.dumps(value, indent=2, default=str))
    if args.command == "verify-grants" and not value["passed"]:
        sys.exit("grants did not verify")


if __name__ == "__main__":
    main()
