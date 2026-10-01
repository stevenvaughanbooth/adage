#!/usr/bin/env python3
"""Click training cohort and learning step, launch-gate-1 revision 13. Only `train` spends.

The recipe is the one that produced Marshmallow's learned route (`cumulative-launch-10`): ten
instruction-guided native sessions over the five registered training tasks (two repetitions each),
one fixed workflow instruction naming the repository's test command, hooks off, and one conditioning
pass of the production learner over all ten histories, keeping every session, failures included.
Differences, all registered in r13 before any Click session:
- the ten sessions live in one cohort (session keys `guided-<repetition>-<task>`), which also holds the
  held-out study, so one native-confinement session and review covers both;
- every training prompt carries the held-out realism line (`Do not create git commits.`) between the
  task prompt and the instruction: v5's Click sessions spent many turns fighting the sandbox's `.git`
  write denial, and the held-out prompts carry the line;
- the task prompt is the held-out template (`click_tasks.base_prompt`, the first examples only).

The organic arm (natural learning, report-only) is the free replay of the five existing organic v5
Click histories (`cohorts/v5`, no new session), conditioned by the same fixed learner. Organic and
guided histories are never mixed. A paid organic top-up is not part of this plan.

Commands (from this directory):
  prepare-cohort A C    offline: the cohort (authority manifest copy, Click training plan, frozen prompts)
  prepare-native C      offline: the native-confinement recipe (creates one checkout)
  prepare-training C    offline: the ten training recipes (needs the reviewed native session)
  train C               PAID: dispatch the ten training sessions in order, one at a time; resumable
  learn C --binary B    offline: condition the fixed learner on the guided histories, replay the organic
                        arm, run the isolation audit, select the certified route by the registered rule
  status C              read-only
"""
from __future__ import annotations

import argparse
from contextlib import contextmanager
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time

import click_tasks
import cohort_v5
import real_repo_gate as gate
import real_repo_pairs as pairs
import split_audit

HERE = Path(__file__).resolve().parent
PROTOCOL = "click-guided-training-1"
GUIDED = "instruction-guided-training"
FAMILY = click_tasks.FAMILY_ID
REPO = "click"
TASKS = [FAMILY + "--" + vid for vid in click_tasks.TRAINING_IDS]
REPETITIONS = 2
COMMAND = ".venv/bin/python -m pytest tests/test_types -q"
GUIDANCE = ("\n\nRepository workflow for this instruction-guided demonstration: after implementing "
            "the code, tests, and documentation, run `" + COMMAND + "` from the repository root. Report "
            "its actual result; do not claim success if it fails.\n")
SESSION_CAP_USD = 0.60
NATIVE_CAP_USD = 0.50
LEDGER_FLOOR_USD = 3.0
ORGANIC_COHORT = "cohorts/v5"
ORGANIC_KEYS = ["%s/sessions/%s" % (ORGANIC_COHORT, t) for t in TASKS]
PROVENANCE = {"guided": "explicitly guided training", "organic": "naturally learned (organic histories only)"}


def sessions():
    """The ten guided sessions in dispatch order: repetition-major, the registered task order."""
    return [dict(key="guided-%d-%s" % (r, t.split("--", 1)[1]), task=t, repetition=r)
            for r in range(1, REPETITIONS + 1) for t in TASKS]


def training_prompt(task):
    """The held-out template for a training task, the realism line, then the workflow instruction."""
    return click_tasks.prompt(click_tasks.variant(task.split("--", 1)[1])) + GUIDANCE


def sources():
    return {str(p): gate.sha(p) for p in (Path(__file__), HERE / "click_tasks.py", HERE / "split_audit.py",
                                          HERE / "launch_gate_protocol.json")}


@contextmanager
def registry(root):
    """The scoring registry with the Click tasks; the caller's environment is restored (the sandbox's
    denial list must not see the scoring imports' cache variables)."""
    before = dict(os.environ)
    try:
        modules = gate.modules(root)
        click_tasks.install(modules[0])
        os.environ.clear()
        os.environ.update(before)
        yield modules
    finally:
        os.environ.clear()
        os.environ.update(before)


# ----------------------------------------------------------------------------------- the cohort

def prepare_cohort(authority, destination):
    """The cohort: cohort_v5's copy of the authority (the person's allocation bound), with the Click
    training plan and frozen prompts written before the native recipe binds the manifest. Runs no Python
    from the authority's runtime."""
    authority, destination = Path(authority).resolve(strict=True), Path(destination).resolve()
    path = cohort_v5.prepare_cohort(authority, destination)
    m = pairs.read(path)
    plans = {p["family"]: p for p in m["families"]}
    if plans[FAMILY]["train"] != TASKS:
        raise ValueError("the authority's Click training tasks differ from the registered ones")
    click = dict(plans[FAMILY], train=list(TASKS), heldout=[])
    others = [dict(p, train=[], heldout=[]) for f, p in plans.items() if f != FAMILY]
    task_sha = {}
    for task in TASKS:
        private = destination / "private/tasks" / task
        private.mkdir(parents=True)
        (private / "prompt.txt").write_text(training_prompt(task))
        task_sha[task] = {"prompt.txt": gate.sha(private / "prompt.txt")}
    m.update(families=[click, *others], protocol=PROTOCOL, training_kind=GUIDED, workflow_guidance=GUIDANCE,
             task_sha256=task_sha, training_sessions=sessions(), source_sha256=sources(),
             budget=dict(per_session_usd=SESSION_CAP_USD, training_sessions=len(sessions()),
                         reservation_usd=round(len(sessions()) * SESSION_CAP_USD, 2), native_usd=NATIVE_CAP_USD,
                         ledger_floor_usd=LEDGER_FLOOR_USD),
             candidate_selection="r13: the lexicographically first certified verify-effect context route whose "
                                 "trigger fits the control work orders and whose command is a project pytest run; "
                                 "none means the held-out study runs advisory only",
             claims="Instruction-guided training on pinned Click; development of the learned home, not evaluation.",
             live_dispatch_enabled=False, grants_written=False, paid_calls=0)
    gate.write(path, m)
    gate.append(destination / "attempts.jsonl", dict(status="click-training-plan", protocol=PROTOCOL, paid_calls=0,
                                                     sessions=[s["key"] for s in sessions()]))
    return m


def verify_cohort(cohort):
    m = pairs.read(Path(cohort) / "manifest.json")
    if m.get("protocol") != PROTOCOL or m.get("training_kind") != GUIDED or m.get("workflow_guidance") != GUIDANCE:
        raise ValueError("not a Click training cohort of this protocol")
    if m["source_sha256"] != sources():
        raise ValueError("Click training sources changed after the cohort was prepared; prepare a new cohort")
    if [s["key"] for s in m["training_sessions"]] != [s["key"] for s in sessions()]:
        raise ValueError("training plan differs from the registered sessions")
    for task, files in m["task_sha256"].items():
        if gate.sha(Path(cohort) / "private/tasks" / task / "prompt.txt") != files["prompt.txt"]:
            raise ValueError("training prompt changed after the cohort was prepared")
    return m


def prepare_native(cohort):
    cohort = Path(cohort).resolve(strict=True)
    verify_cohort(cohort)
    if (cohort / "sessions/native-confinement/manifest.json").exists():
        return dict(native=str(cohort / "sessions/native-confinement/manifest.json"), already=True)
    return dict(native=str(cohort_v5.prepare_session(cohort)), already=False)


def prepare_training(cohort):
    """The ten guided recipes. Requires the reviewed native session; writes no grant."""
    cohort = Path(cohort).resolve(strict=True)
    m = verify_cohort(cohort)
    review = cohort / "native-confinement-reviewed.json"
    gate.require_native_review(dict(experiment_root=m["budget_authority"], cohort_root=str(cohort),
                                    native_review_path=str(review), native_review_sha256=gate.sha(review)))
    out = []
    for s in sessions():
        path = cohort / "sessions" / s["key"] / "manifest.json"
        if path.exists():
            out.append(dict(key=s["key"], manifest=str(path), already=True))
            continue
        with registry(cohort):
            value = gate.prepare_session(cohort, s["task"], cohort_v5.MODEL, setup_checkout=pairs.frozen_setup, key=s["key"])
        path = cohort_v5.finalize_recipe(value, cohort, False)
        value = pairs.read(path)
        value["budget_allocation_sha256"] = m["budget_allocation_sha256"]
        value.update(repetition=s["repetition"], session_key=s["key"])
        value["shared_sources"].update({str(p): gate.sha(p) for p in
                                        (Path(__file__), HERE / "click_tasks.py",
                                         Path(m["budget_authority"]) / "evaluation-allocation.json")})
        gate.write(path, value)
        out.append(dict(key=s["key"], manifest=str(path), already=False))
    return out


def ledger(authority):
    state = pairs.read(Path(authority) / "paid-state.json")
    b = pairs.Budget(json.loads(json.dumps(state)), 60.0, 0.5)
    return state, b.spent


def train(cohort, wall_seconds=600):
    """Dispatch every guided session in order, one at a time. A session already in the ledger is never
    dispatched again (resumable; never a retry). Stops on any error and before the $3 floor."""
    cohort = Path(cohort).resolve(strict=True)
    m = verify_cohort(cohort)
    authority = Path(m["budget_authority"])
    results = []
    for s in sessions():
        path = cohort / "sessions" / s["key"] / "manifest.json"
        value = pairs.read(path)
        key = str((cohort / "sessions" / s["key"]).relative_to(authority))
        state, spent = ledger(authority)
        if key in state["runs"]:
            results.append(dict(key=s["key"], skipped="already attempted (%s); never retried" % state["runs"][key].get("status")))
            continue
        limit = gate.budget_limit(value)
        if limit - (spent + value["budget_usd"]) < LEDGER_FLOOR_USD - 1e-9:
            raise ValueError("the ledger's remaining balance would fall below $3; stop (training %s)" % s["key"])
        result = gate.run_session(path, wall_seconds=wall_seconds)
        results.append(dict(key=s["key"], status=result.get("status"),
                            cost_usd=(result.get("result") or {}).get("total_cost_usd")))
        print(json.dumps(results[-1]), flush=True)
    return results


# ------------------------------------------------------------------------------------- learning

def guided_inputs(cohort, m):
    """Every guided session with a completed, native, hook-free history, in dispatch order. Sessions that
    did not complete are listed and never retried; the learner sees every completed one."""
    authority = Path(m["budget_authority"])
    state = pairs.read(authority / "paid-state.json")
    rows, missing = [], []
    for s in sessions():
        session = cohort / "sessions" / s["key"]
        key = str(session.relative_to(authority))
        terminal = state["runs"].get(key)
        if not terminal or terminal.get("status") != "completed":
            missing.append(dict(key=s["key"], status=(terminal or {}).get("status", "not attempted")))
            continue
        manifest = pairs.read(session / "manifest.json")
        provenance = pairs.read(session / "transcript-provenance.json")
        if (manifest["kind"] != GUIDED or terminal.get("kind") != GUIDED
                or manifest["source_sha256"] != gate.sha(Path(gate.__file__))
                or any(gate.sha(Path(p)) != d for p, d in manifest["shared_sources"].items())
                or terminal["transcript"] != provenance
                or provenance["sha256"] != gate.sha(session / "native-transcript.jsonl")
                or manifest["settings_sha256"] != gate.sha(session / "settings.json")
                or Path(provenance["cwd"]).resolve() != Path(manifest["execution_root"]).resolve()
                or not pairs.read(session / "settings.json")["disableAllHooks"]
                or manifest.get("training_hooks_enabled") is not False):
            raise ValueError("native history provenance or ledger mismatch: " + s["key"])
        gate.validate_training_recipe(manifest)
        rows.append(dict(task=s["task"], session_key=s["key"], session_id=provenance["session_id"],
                         sha256=provenance["sha256"], kind=GUIDED, workflow_guidance=GUIDANCE,
                         synthetic=provenance["synthetic"], hooks_enabled=False,
                         source=str(session / "native-transcript.jsonl"), outcome=terminal.get("result", {}).get("result_status")
                         if isinstance(terminal.get("result"), dict) else None,
                         manifest_sha256=gate.sha(session / "manifest.json"),
                         provenance_sha256=gate.sha(session / "transcript-provenance.json")))
    return rows, missing


def organic_inputs(authority):
    """The five existing organic v5 Click histories (hook-free, native), in their registered task order."""
    authority = Path(authority)
    state = pairs.read(authority / "paid-state.json")
    rows = []
    for task, key in zip(TASKS, ORGANIC_KEYS):
        session = authority / key
        terminal = state["runs"].get(key) or {}
        manifest = pairs.read(session / "manifest.json")
        provenance = pairs.read(session / "transcript-provenance.json")
        if (terminal.get("status") != "completed" or manifest.get("kind") != "organic-training"
                or manifest.get("task") != task or manifest.get("training_hooks_enabled") is not False
                or terminal.get("transcript") != provenance
                or provenance["sha256"] != gate.sha(session / "native-transcript.jsonl")
                or provenance.get("synthetic") is not False):
            raise ValueError("organic v5 history provenance or ledger mismatch: " + key)
        rows.append(dict(task=task, session_key=key, session_id=provenance["session_id"], sha256=provenance["sha256"],
                         kind="organic-training", workflow_guidance=None, synthetic=False, hooks_enabled=False,
                         source=str(session / "native-transcript.jsonl"),
                         manifest_sha256=gate.sha(session / "manifest.json"),
                         provenance_sha256=gate.sha(session / "transcript-provenance.json")))
    return rows


def condition(output, inputs, binary, label):
    """One conditioning pass of the production learner over exactly these histories, in a fresh home."""
    output, binary = Path(output), Path(binary).resolve(strict=True)
    kinds = {(r["kind"], r["workflow_guidance"]) for r in inputs}
    if len(kinds) > 1:
        raise ValueError("guided and organic histories must never be mixed")
    if len({r["session_id"] for r in inputs}) != len(inputs) or len({r["sha256"] for r in inputs}) != len(inputs):
        raise ValueError("duplicate training history")
    if any(r["synthetic"] is not False or r["hooks_enabled"] is not False for r in inputs):
        raise ValueError("training must be native and unexposed")
    output.mkdir(parents=True, exist_ok=False)
    history = output / "history"
    history.mkdir()
    rows = []
    for r in inputs:
        dest = history / (r["session_id"] + ".jsonl")
        shutil.copyfile(r["source"], dest)
        if gate.sha(dest) != r["sha256"]:
            raise ValueError("history copy changed")
        rows.append(dict(r, copy=str(dest)))
    executable = output / "adage"
    shutil.copy2(binary, executable)
    home = output / "adage-home"
    command = [str(executable), "condition", "--plain", "--json"]
    command += ["--transcripts", *[r["copy"] for r in rows]] if rows else ["--instincts-only"]
    record = dict(kind=label, protocol=PROTOCOL, status="started", count=len(rows), inputs=rows,
                  training_kind=rows[0]["kind"] if rows else None,
                  workflow_guidance=rows[0]["workflow_guidance"] if rows else None, command=command,
                  binary=str(binary), binary_sha256=gate.sha(executable),
                  production_source_sha256={str(p.relative_to(gate.REPO)): gate.sha(p) for p in
                                            [*sorted((gate.REPO / "crates").rglob("*")), gate.REPO / "Cargo.toml",
                                             gate.REPO / "Cargo.lock"] if p.is_file()},
                  git_head=subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=gate.REPO, text=True).strip(),
                  paid_calls=0, grants_written=False, efficacy_result=False, started_ns=time.time_ns())
    gate.write(output / "attempt.json", record)
    env = {k: v for k, v in os.environ.items() if not k.startswith("ADAGE_")}
    env.update(ADAGE_HOME=str(home), ADAGE_AUTO_LEARN="0")
    result = subprocess.run(command, cwd=output, env=env, capture_output=True, text=True, timeout=900)
    (output / "condition.stdout.json").write_text(result.stdout)
    (output / "condition.stderr.txt").write_text(result.stderr)
    record.update(exit_code=result.returncode, elapsed_ns=time.time_ns() - record["started_ns"],
                  status="completed" if result.returncode == 0 else "failed")
    if result.returncode == 0:
        index = pairs.read(home / "index.json")
        record.update(route_count=len(index["routes"]), execution_candidates=[r for r in index["routes"]
                      if r["route"].get("evidence", {}).get("execution")])
        record["artifact_sha256"] = {str(p.relative_to(home)): gate.sha(p) for p in
                                     [home / "index.json", *sorted((home / "tracks").rglob("*"))] if p.is_file()}
    if gate.sha(executable) != record["binary_sha256"] or (home / "approvals.json").exists():
        record["status"] = "invalid-binary-drift-or-grant"
    gate.write(output / "attempt.json", record)
    return record


# The static control check: does the route's trigger fire where each control needs it?
def _glob(pattern, path):
    """The runtime's glob rule: `*` and `?` stay within a segment, `**` crosses segments (`a/**/b` also
    matches `a/b`), everything else is literal."""
    import re as _re
    out, i = "", 0
    while i < len(pattern):
        if pattern.startswith("**/", i):
            out, i = out + "(?:.*/)?", i + 3
        elif pattern.startswith("**", i):
            out, i = out + ".*", i + 2
        elif pattern[i] == "*":
            out, i = out + "[^/]*", i + 1
        elif pattern[i] == "?":
            out, i = out + "[^/]", i + 1
        else:
            out, i = out + _re.escape(pattern[i]), i + 1
    return _re.fullmatch(out, path) is not None


def fires_at(trigger, order):
    """Event indexes (into an edit-only work order) where the trigger fires; None when not decidable from
    edits alone (a command, failure, prompt or intent item)."""
    on = trigger.get("on")
    if on == "after_edit":
        paths = trigger.get("paths")
        paths = [paths] if isinstance(paths, str) else list(paths or [])
        return [i for i, p in enumerate(order) if any(_glob(g, p) for g in paths)]
    if on != "context":
        return None
    items = trigger.get("items") or []
    if not items or any(it.get("item") != "edit" for it in items):
        return None
    within = int(trigger.get("within", 0))
    # Every edit in the order is counted as an alphabet event (an upper bound on the runtime's gaps).
    ends = [q for q, p in enumerate(order) if _glob(items[0]["glob"], p)]
    for it in items[1:]:
        ends = [q for q, p in enumerate(order) if _glob(it["glob"], p) and any(e < q and q - e <= within for e in ends)]
    return ends


def control_fit(route):
    """Whether the controls can be realized: the trigger fires in the natural order once the new test file
    exists, and in the failed order before the implementation edit."""
    trigger = route["route"]["trigger"]
    natural = fires_at(trigger, click_tasks.NATURAL_ORDER)
    failed = fires_at(trigger, click_tasks.FAILED_ORDER)
    if natural is None or failed is None:
        return dict(fits=False, reason="the trigger is not decidable from the control edit orders")
    tests_at = click_tasks.NATURAL_ORDER.index(click_tasks.CONTROL_TEST_PATH)
    impl_at = click_tasks.FAILED_ORDER.index("src/click/types.py")
    checks = dict(natural_after_tests=any(i >= tests_at for i in natural), failed_before_implementation=any(i < impl_at for i in failed))
    return dict(fits=all(checks.values()), checks=checks, natural=natural, failed=failed)


def select_route(index):
    """The registered rule (r13): the lexicographically first certified verify-effect context route in a
    learned track whose step runs a project pytest command and whose trigger fits the control work
    orders. Returns (selected or None, every certified context candidate with its verdict)."""
    certified = sorted((r for r in index["routes"] if r["track"].endswith("/learned")
                        and r["route"].get("evidence", {}).get("execution")
                        and r["route"].get("effect") == "verify" and r["route"]["trigger"].get("on") == "context"),
                       key=lambda r: r["track"] + "/" + r["route"]["id"])
    verdicts = []
    for r in certified:
        step = r["route"].get("step", {})
        command = step.get("command") or ""
        fit = control_fit(r)
        pytest_run = step.get("do") == "run" and command.startswith(".venv/bin/python -m pytest ")
        verdicts.append(dict(route=r["track"] + "/" + r["route"]["id"], command=command, trigger=r["route"]["trigger"],
                             pytest_run=pytest_run, control_fit=fit, eligible=bool(pytest_run and fit["fits"])))
    chosen = next((v for v in verdicts if v["eligible"]), None)
    return chosen, verdicts


def learn(cohort, binary, authorities):
    """The offline learning step: guided conditioning, the organic replay arm, the isolation audit of both
    and the route selection. Writes learning/learning.json. Spends nothing."""
    cohort = Path(cohort).resolve(strict=True)
    m = verify_cohort(cohort)
    authority = Path(m["budget_authority"])
    folder = cohort / "learning"
    if (folder / "learning.json").exists():
        return pairs.read(folder / "learning.json")
    if folder.exists():  # an interrupted pass: kept aside, never overwritten; learning is offline and repeatable
        folder.rename(folder.with_name("learning-interrupted-%d" % time.time_ns()))
    authorities = sorted({str(authority), *(str(Path(a).resolve(strict=True)) for a in authorities)})
    rows, missing = guided_inputs(cohort, m)
    if not rows:
        raise ValueError("no guided training session completed; nothing to learn from (see `status`)")
    guided = condition(folder / "guided", rows, binary, "click-guided-cumulative")
    organic = condition(folder / "organic-v5-replay", organic_inputs(authority), binary, "click-organic-replay")
    out = dict(protocol=PROTOCOL, revision=13, cohort=str(cohort), binary_sha256=gate.sha(Path(binary)),
               guided=dict(attempt=str(folder / "guided/attempt.json"), status=guided["status"], sessions=guided["count"],
                           missing=missing),
               organic=dict(attempt=str(folder / "organic-v5-replay/attempt.json"), status=organic["status"],
                            sessions=organic["count"], role="report-only: natural learning (no new sessions)"),
               paid_calls=0, grants_written=False)
    for name, record in (("guided", guided), ("organic", organic)):
        place = Path(out[name]["attempt"]).parent
        if record["status"] != "completed":
            out[name].update(certified=[], selected=None, audit=None)
            continue
        audit = split_audit.audit(place, REPO, [Path(a) for a in authorities])
        pairs.write(place / "isolation-audit.json", audit)
        chosen, verdicts = select_route(pairs.read(place / "adage-home/index.json"))
        out[name].update(audit=dict(passed=audit["passed"], problems=audit["problems"],
                                    path=str(place / "isolation-audit.json")),
                         route_count=record.get("route_count"), certified=verdicts,
                         selected=chosen if audit["passed"] else None)
    selected = out["guided"]["selected"]
    out.update(learned_home=str(folder / "guided/adage-home"), learning_root=str(folder / "guided"),
               provenance=PROVENANCE["guided"], audit_passed=bool((out["guided"].get("audit") or {}).get("passed")),
               mode="execution" if selected else "advisory",
               route=selected["route"] if selected else None, command=selected["command"] if selected else None,
               rule="r13: BEST runs only with a selected route (the lexicographically first certified verify-effect "
                    "context route with a project pytest command whose trigger fits the control work orders, in the "
                    "guided home); otherwise the study runs advisory only (A0 and NOEXEC), BEST and its controls "
                    "skipped. The organic replay is report-only.",
               expectation=EXPECTATION)
    gate.write(folder / "learning.json", out)
    (folder / "learning.sha256").write_text(gate.sha(folder / "learning.json") + "\n")
    return out


# Recorded in r13 before any Click session and before the organic replay ran.
EXPECTATION = dict(
    organic_replay="no certified route: in v5 each Click session ran a different pytest form (file, directory, "
                   "-v/-q, activated or .venv paths), so no command reaches the five-session support; the earlier "
                   "checkpoint and the release replay certified nothing",
    guided="about even odds that at least one verify route certifies: Marshmallow's first five guided sessions "
           "certified nothing and the cumulative ten certified one command (four triggers, 9/10 qualifying "
           "continuations); Click sessions interleave two docs edits and the agent's own test runs, which can "
           "break the recount",
    control_fit="if a context route over the tests and changelog edits certifies, it fits the control orders; a "
                "route needing a source edit after the changelog does not, and is passed over",
    if_none="the study runs advisory only (A0 and NOEXEC, 40 sessions), as registered")


def status(cohort):
    cohort = Path(cohort).resolve(strict=True)
    m = pairs.read(cohort / "manifest.json")
    authority = Path(m["budget_authority"])
    state = pairs.read(authority / "paid-state.json")
    rows = []
    for s in sessions():
        key = str((cohort / "sessions" / s["key"]).relative_to(authority))
        run = state["runs"].get(key) or {}
        rows.append(dict(key=s["key"], prepared=(cohort / "sessions" / s["key"] / "manifest.json").exists(),
                         status=run.get("status"), cost_usd=(run.get("result") or {}).get("total_cost_usd")))
    native = str((cohort / "sessions/native-confinement").relative_to(authority))
    return dict(cohort=str(cohort), native=(state["runs"].get(native) or {}).get("status"),
                reviewed=(cohort / "native-confinement-reviewed.json").exists(), training=rows,
                learned=(cohort / "learning/learning.json").exists())


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("command", choices=("prepare-cohort", "prepare-native", "prepare-training", "train", "learn",
                                            "status", "prompts"))
    parser.add_argument("paths", type=Path, nargs="*")
    parser.add_argument("--binary", type=Path)
    parser.add_argument("--audit-authority", type=Path, action="append", default=[])
    args = parser.parse_args()
    if args.command == "prompts":
        for t in TASKS:
            print("== " + t + "\n" + training_prompt(t))
        return
    if args.command == "prepare-cohort":
        value = prepare_cohort(*args.paths)
    elif args.command == "learn":
        if args.binary is None:
            parser.error("learn requires --binary")
        value = learn(args.paths[0], args.binary, args.audit_authority)
    else:
        value = {"prepare-native": prepare_native, "prepare-training": prepare_training, "train": train,
                 "status": status}[args.command](args.paths[0])
    print(json.dumps(value, indent=2, default=str))


if __name__ == "__main__":
    main()
