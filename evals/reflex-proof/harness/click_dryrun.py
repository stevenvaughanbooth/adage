#!/usr/bin/env python3
"""Offline dry run of the whole Click gate pipeline, on a scratch copy of the evaluation authority.

The aim: every step of `run-click-gate.sh` except the paid sessions and the person's grants works the
first time. It spends nothing and launches no Claude session.

What it never does:
- write under the real authority (it reads the real authority's v5 sessions, ledger and validation
  records, and scans it read-only in the isolation audit);
- run Python from the real authority's runtime or bases (it copies the runtime and caches into the
  scratch authority and runs only the copies, as heldout_dryrun.py does);
- write a grant or an approvals file.

What it does, all under `scratch`:
1. A scratch authority (`heldout_dryrun.scratch_authority`), plus copies of the five organic v5 Click
   session records and their ledger entries (the organic replay arm reads them).
2. The Click cohort (`click_training.prepare_cohort`), its native recipe and a synthetic review marked
   `dry_run` (never evidence: the real run needs its own reviewed native session).
3. The ten guided training recipes (`prepare_training`, ten checkouts).
4. Stand-in training histories: each guided session gets a copy of an organic v5 transcript with a
   fresh session id, and a scratch ledger entry, all marked `dry_run`. They are not guided sessions;
   they exercise the learning step's provenance checks, conditioning, audit and selection.
5. The learning step (`click_training.learn`) with the fixed learner binary.
6. Advisory mode: freeze, prepare (40 checkouts), verify-grants (no grant anywhere), report.
7. Execution mode on a simulated learning record (the learned home plus one injected certified route,
   marked `simulated`): freeze, prepare (65 checkouts), approval-script, verify-grants (fails only on
   the 25 missing person-issued BEST grants), report.

Usage: python3 click_dryrun.py <scratch dir> --binaries <dir with adage, adage-exec> [--build-commit SHA]
       [--controls DIR] [--dev-report PATH]
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import uuid

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import click_analysis  # noqa: E402
import click_gate as runner  # noqa: E402
import click_tasks  # noqa: E402
import click_training as training  # noqa: E402
import heldout_dryrun  # noqa: E402
import real_repo_pairs as pairs  # noqa: E402

AUTHORITY = heldout_dryrun.AUTHORITY
CONTROLS = Path("/private/tmp/adage-click-controls-2")
DEV_REPORT = AUTHORITY / "cohorts/launch-gate-v6/dev-v9b/reports/dev-v9-1790870521952346000.json"
EXTRA_AUDIT = [AUTHORITY, Path("/private/tmp/adage-learning-curve-v2")]
SIMULATED_ROUTE = "run-0dryrun"


def sha(path):
    return pairs.sha(Path(path))


def scratch_v5(a):
    """The five organic v5 Click session records and their ledger entries, copied into the scratch authority."""
    real = pairs.read(AUTHORITY / "paid-state.json")
    state = pairs.read(a / "paid-state.json")
    for key in training.ORGANIC_KEYS:
        target = a / key
        target.mkdir(parents=True)
        for name in ("manifest.json", "transcript-provenance.json", "native-transcript.jsonl", "settings.json"):
            if (AUTHORITY / key / name).exists():
                shutil.copy2(AUTHORITY / key / name, target / name)
        state["runs"][key] = real["runs"][key]
    pairs.write(a / "paid-state.json", state)


def scratch_review(cohort):
    native = cohort / "sessions/native-confinement/manifest.json"
    pairs.write(cohort / "native-confinement-reviewed.json",
                dict(passed=True, dry_run=True, cohort_root=str(cohort), manifest_sha256=sha(native),
                     note="SYNTHETIC: a dry run's stand-in. The real cohort needs a reviewed native session."))


def stand_in_histories(a, cohort):
    """Each guided session gets an organic v5 transcript copy with a fresh id (dry run only)."""
    state = pairs.read(a / "paid-state.json")
    sources = [AUTHORITY / key / "native-transcript.jsonl" for key in training.ORGANIC_KEYS]
    for i, s in enumerate(training.sessions()):
        session = cohort / "sessions" / s["key"]
        manifest = pairs.read(session / "manifest.json")
        source = sources[i % len(sources)]
        old = pairs.read(source.with_name("transcript-provenance.json"))["session_id"]
        new = str(uuid.uuid4())
        (session / "native-transcript.jsonl").write_text(source.read_text().replace(old, new))
        provenance = dict(session_id=new, source=str(source), destination=str(session / "native-transcript.jsonl"),
                          sha256=sha(session / "native-transcript.jsonl"), cwd=manifest["execution_root"],
                          synthetic=False, dry_run=True, stand_in="copy of an organic v5 transcript, fresh id")
        pairs.write(session / "transcript-provenance.json", provenance)
        key = str(session.relative_to(a))
        state["runs"][key] = dict(status="completed", kind=training.GUIDED, transcript=provenance, dry_run=True,
                                  result=dict(result_status="success", total_cost_usd=0.0))
    pairs.write(a / "paid-state.json", state)


def simulated_learning(learning):
    """A copy of the learned home with one injected certified context route, marked simulated."""
    record = pairs.read(learning)
    source = Path(record["learning_root"])
    target = source.parent / "guided-simulated"
    shutil.copytree(source, target, symlinks=True)
    home = target / "adage-home"
    index = pairs.read(home / "index.json")
    attempt = pairs.read(target / "attempt.json")
    sessions = attempt["count"]
    route = dict(authored=False, effect="verify", id=SIMULATED_ROUTE, slots=[], state="firm", strength="prime",
                 summary="SIMULATED for the dry run", exit=[{"when": "intent_changes"}],
                 step=dict(do="run", command=training.COMMAND),
                 trigger=dict(on="context", within=8, items=[dict(item="edit", glob="tests/**/*.py"),
                                                             dict(item="edit", glob="CHANGES.md")]),
                 evidence=dict(support=sessions, opportunities=sessions,
                               execution=dict(opportunities=sessions, sessions=sessions - 1, successful=sessions - 1)))
    track = next((r["track"] for r in index["routes"] if r["track"].endswith("/learned")), "github-com-pallets-click/learned")
    index["routes"].append(dict(repo="github.com/pallets/click", route=route, track=track, version="0.1.0"))
    pairs.write(home / "index.json", index)
    attempt["artifact_sha256"] = {str(p.relative_to(home)): sha(p) for p in
                                  [home / "index.json", *sorted((home / "tracks").rglob("*"))] if p.is_file()}
    attempt["simulated"] = True
    pairs.write(target / "attempt.json", attempt)
    chosen, verdicts = training.select_route(index)
    out = dict(record, simulated=True, learning_root=str(target), learned_home=str(home), mode="execution",
               route=chosen["route"], command=chosen["command"],
               guided=dict(record["guided"], selected=chosen, certified=verdicts),
               note="SIMULATED: one injected route, to exercise execution mode's freeze, prepare and grants")
    path = learning.parent / "learning-simulated" / "learning.json"
    path.parent.mkdir()
    pairs.write(path, out)
    (path.parent / "learning.sha256").write_text(sha(path) + "\n")
    return path


def study(cohort, name, learning, binaries, commit, digests, controls, dev_report, advisory_only=False):
    root = cohort / name
    m = runner.freeze(root, cohort, learning, controls, dev_report, binaries, commit, digests,
                      [str(p) for p in EXTRA_AUDIT], advisory_only=advisory_only, dry_run=True)
    steps = dict(root=str(root), mode=m["mode"], sessions=len(m["matrix"]), route=m["route"], command=m["command"],
                 budget={k: m["budget"][k] for k in ("sessions", "per_session_usd", "worst_case_usd")},
                 control_prompt_checks=[c["passed"] for c in m["control_prompt_checks"]])
    prepared = runner.prepare(root)
    steps["prepare"] = dict(assignments=prepared["assignments"], runtime_protected=bool(prepared["runtime"]))
    if m["mode"] == "execution":
        steps["approval_script"] = runner.approval_script(root)
    grants = runner.verify_grants(root)
    best = len(runner.granted(m))
    steps["verify_grants"] = dict(passed=grants["passed"], problems=len(grants["problems"]), best_homes=best,
                                  only_missing_best_grants=len(grants["problems"]) == best and all(
                                      "No such file" in p["problem"] for p in grants["problems"]))
    try:
        runner.live(root)
        steps["live_refused"] = False
    except ValueError as error:
        steps["live_refused"] = str(error)
    report = runner.report(root)
    steps["report"] = dict(outcomes=report["outcomes"], collected=report["collected"], assignments=report["assignments"])
    return steps


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("scratch", type=Path)
    parser.add_argument("--binaries", type=Path, required=True)
    parser.add_argument("--build-commit", default="HEAD")
    parser.add_argument("--controls", type=Path, default=CONTROLS)
    parser.add_argument("--dev-report", type=Path, default=DEV_REPORT)
    args = parser.parse_args()
    scratch = args.scratch.resolve()
    if AUTHORITY == scratch or AUTHORITY in scratch.parents:
        raise SystemExit("the dry run never writes under the real authority")
    os.environ["PYTHONDONTWRITEBYTECODE"] = "1"
    scratch.mkdir(parents=True, exist_ok=False)
    a = heldout_dryrun.scratch_authority(scratch)
    scratch_v5(a)
    cohort = a / "cohorts/click-dry"
    steps = dict(authority=str(a), cohort=str(cohort))
    m = training.prepare_cohort(a, cohort)
    steps["cohort"] = dict(protocol=m["protocol"], sessions=[s["key"] for s in m["training_sessions"]],
                           budget=m["budget"], allocation_bound=bool(m.get("budget_allocation_sha256")))
    steps["native"] = training.prepare_native(cohort)
    scratch_review(cohort)
    steps["training_recipes"] = len(training.prepare_training(cohort))
    recipe = pairs.read(cohort / "sessions" / training.sessions()[0]["key"] / "manifest.json")
    training.gate.validate_training_recipe(recipe)
    steps["training_recipe"] = dict(kind=recipe["kind"], budget_usd=recipe["budget_usd"],
                                    guided=recipe["workflow_guidance"] == training.GUIDANCE,
                                    prompt_ends_with_guidance=recipe["argv"][-1].endswith(training.GUIDANCE),
                                    realism_line=click_tasks.REALISM.strip() in recipe["argv"][-1],
                                    max_turns=recipe["max_turns"])
    steps["status_before"] = training.status(cohort)
    stand_in_histories(a, cohort)
    record = training.learn(cohort, args.binaries / "adage", [str(p) for p in EXTRA_AUDIT])
    steps["learning"] = dict(mode=record["mode"], audit_passed=record["audit_passed"],
                             guided=dict(status=record["guided"]["status"], sessions=record["guided"]["sessions"],
                                         certified=len(record["guided"]["certified"])),
                             organic=dict(status=record["organic"]["status"], sessions=record["organic"]["sessions"],
                                          certified=len(record["organic"]["certified"]),
                                          audit=record["organic"]["audit"]["passed"]))
    binaries = {n: sha(args.binaries / n) for n in ("adage", "adage-exec")}
    commit = subprocess.check_output(["git", "rev-parse", args.build_commit], cwd=HERE, text=True).strip()
    learning = cohort / "learning/learning.json"
    steps["budget_check_start"] = {k: v for k, v in runner.budget_check(a, "start").items() if k != "plan"}
    advisory_learning = learning
    if record["mode"] == "execution":  # the stand-ins certified something: also test the person's advisory choice
        steps["advisory_note"] = "learning selected a route; advisory mode is exercised with --advisory-only"
    steps["advisory"] = study(cohort, "heldout-dry-advisory", advisory_learning, args.binaries, commit, binaries,
                              args.controls, args.dev_report, advisory_only=record["mode"] == "execution")
    simulated = simulated_learning(learning)
    steps["execution"] = study(cohort, "heldout-dry-execution", simulated, args.binaries, commit, binaries,
                               args.controls, args.dev_report)
    expected = dict(advisory_sessions=steps["advisory"]["sessions"] == 40,
                    execution_sessions=steps["execution"]["sessions"] == 65,
                    advisory_grants=steps["advisory"]["verify_grants"]["passed"],
                    execution_only_missing_grants=steps["execution"]["verify_grants"]["only_missing_best_grants"],
                    execution_best_homes=steps["execution"]["verify_grants"]["best_homes"] == 25,
                    control_prompts=all(steps["execution"]["control_prompt_checks"]),
                    live_refused=bool(steps["advisory"]["live_refused"]) and bool(steps["execution"]["live_refused"]),
                    reports_incomplete=steps["advisory"]["report"]["outcomes"]["advisory"] == "incomplete"
                    and steps["execution"]["report"]["outcomes"]["execution"] == "incomplete",
                    learning_audit=steps["learning"]["audit_passed"] and steps["learning"]["organic"]["audit"])
    steps.update(expected=expected, passed=all(expected.values()), paid_calls=0, grants_written=False,
                 claude_sessions=0, analysis=click_analysis.PROTOCOL)
    pairs.write(scratch / "dry-run.json", steps)
    print(json.dumps(steps, indent=2, default=str))
    raise SystemExit(0 if steps["passed"] else 1)


if __name__ == "__main__":
    main()
